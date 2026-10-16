//! Tree-walking interpreter for checked scripts.
//!
//! Natives are provided by the embedder through [`Natives`]. Loading fails
//! when a declared native has no implementation or its implementation has
//! a different signature (SPEC-0001).
//!
//! Operations are counted with the costs of SPEC-0005, per thread.
//! Not modelled yet: sleeps and scheduling of threads.

use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use crate::Position;
use crate::ast::*;
use crate::check::{Program, Type};
use crate::semantics::{self, Fault, Value};

/// Implementation of a native. Receives the interpreter, so that it can call
/// back into scripts, and the arguments already converted to the declared
/// parameter types. Returns `None` for natives that return nothing.
/// Operations a thread may execute (SPEC-0005).
pub const OPERATION_LIMIT: u64 = 300_000;

pub type NativeFn = Rc<dyn Fn(&mut Vm, &[Value]) -> Result<Option<Value>, RuntimeError>>;

/// Native implementations by name, each with the JASS declaration it
/// implements.
#[derive(Clone, Default)]
pub struct Natives {
    entries: HashMap<String, (Signature, NativeFn)>,
    stub_missing: bool,
}

impl Natives {
    /// Registers an implementation for `declaration`, such as
    /// `native KillUnit takes unit whichUnit returns nothing`.
    pub fn register(&mut self, declaration: &str, f: NativeFn) {
        let script = crate::parse(declaration).expect("valid native declaration");
        let Some(Declaration::Native(native)) = script.declarations.into_iter().next() else {
            panic!("not a native declaration: {declaration}");
        };
        self.entries
            .insert(native.signature.name.clone(), (native.signature, f));
    }

    /// Provide a stub returning the default value for every declared native
    /// without an implementation, instead of failing to load. For tools and
    /// tests only.
    pub fn stub_missing(mut self) -> Self {
        self.stub_missing = true;
        self
    }

    pub fn contains(&self, name: &str) -> bool {
        self.entries.contains_key(name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeError {
    pub message: String,
    pub function: String,
    pub position: Position,
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "in {} at {}:{}: {}",
            self.function, self.position.line, self.position.column, self.message
        )
    }
}

impl std::error::Error for RuntimeError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadError(pub String);

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LoadError {}

enum Body<'a> {
    Native(NativeFn),
    Script(&'a Function),
}

/// Storage of one variable with its declared type. A scalar holds `None`
/// until it is first assigned.
enum Slot {
    Scalar(Option<Value>, Type),
    Array(Vec<Value>, Type),
}

type Locals<'a> = HashMap<&'a str, Slot>;

enum Flow {
    Next,
    Exit,
    Return(Option<Value>),
}

pub struct Vm<'a> {
    program: &'a Program,
    bodies: Vec<Body<'a>>,
    globals: Vec<Slot>,
    global_index: HashMap<&'a str, usize>,
    array_size: usize,
    /// Operations executed so far in all threads (SPEC-0005 costs).
    pub operations: u64,
    /// Operations executed by the current thread since it started or last
    /// resumed.
    pub thread_operations: u64,
    /// A thread stops with an error when it would execute more operations
    /// than this. The game's limit is [`OPERATION_LIMIT`].
    pub operation_limit: Option<u64>,
    /// Function and position being executed, for error messages.
    current: (String, Position),
}

impl<'a> Vm<'a> {
    /// Binds natives and runs the global initialisers in declaration order.
    pub fn load(
        program: &'a Program,
        scripts: &[&'a Script],
        natives: &Natives,
    ) -> Result<Vm<'a>, LoadError> {
        let mut script_functions: HashMap<&str, &'a Function> = HashMap::new();
        for script in scripts {
            for function in &script.functions {
                script_functions.insert(&function.signature.name, function);
            }
        }
        let mut bodies = Vec::with_capacity(program.functions.len());
        for info in &program.functions {
            if !info.native {
                bodies.push(Body::Script(script_functions[info.name.as_str()]));
                continue;
            }
            let implementation = match natives.entries.get(&info.name) {
                Some((signature, f)) => {
                    let parameters: Option<Vec<Type>> = signature
                        .parameters
                        .iter()
                        .map(|p| program.type_named(&p.type_name))
                        .collect();
                    let returns = signature.returns.as_ref().map(|t| program.type_named(t));
                    let same_returns = match returns {
                        None => info.returns.is_none(),
                        Some(r) => r.is_some() && r == info.returns,
                    };
                    if parameters.as_deref() != Some(&info.parameters[..]) || !same_returns {
                        return Err(LoadError(format!(
                            "native '{}' is implemented with a different signature",
                            info.name
                        )));
                    }
                    f.clone()
                }
                None if natives.stub_missing => {
                    let returns = info.returns;
                    Rc::new(move |_: &mut Vm, _: &[Value]| Ok(returns.map(Value::default_of)))
                        as NativeFn
                }
                None => {
                    return Err(LoadError(format!(
                        "native '{}' has no implementation",
                        info.name
                    )));
                }
            };
            bodies.push(Body::Native(implementation));
        }

        let mut vm = Vm {
            program,
            bodies,
            globals: Vec::with_capacity(program.globals.len()),
            global_index: HashMap::new(),
            array_size: 8192,
            operations: 0,
            thread_operations: 0,
            operation_limit: Some(OPERATION_LIMIT),
            current: ("<globals>".into(), Position { line: 0, column: 0 }),
        };
        let mut no_locals = Locals::new();
        for script in scripts {
            for declaration in &script.declarations {
                let Declaration::Globals(globals) = declaration else {
                    continue;
                };
                for global in globals {
                    let variable = &global.variable;
                    let t = program.global(&variable.name).expect("checked").value_type;
                    let slot = if variable.array {
                        Slot::Array(Vec::new(), t)
                    } else {
                        let value = match &variable.initial {
                            Some(initial) => Some(
                                vm.evaluate(&mut no_locals, initial)
                                    .map_err(|e| {
                                        LoadError(format!("initialising '{}': {e}", variable.name))
                                    })?
                                    .coerce(t),
                            ),
                            None => None,
                        };
                        Slot::Scalar(value, t)
                    };
                    if variable.name == "JASS_MAX_ARRAY_SIZE" {
                        if let Slot::Scalar(Some(Value::Integer(n)), _) = slot {
                            vm.array_size = n.max(0) as usize;
                        }
                    }
                    vm.global_index.insert(&variable.name, vm.globals.len());
                    vm.globals.push(slot);
                }
            }
        }
        // Global initialisers do not count toward the first thread.
        vm.thread_operations = 0;
        Ok(vm)
    }

    pub fn program(&self) -> &Program {
        self.program
    }

    /// Current value of a scalar global, or `None` if it is unknown, an
    /// array or not yet assigned.
    pub fn global(&self, name: &str) -> Option<&Value> {
        match &self.globals[*self.global_index.get(name)?] {
            Slot::Scalar(value, _) => value.as_ref(),
            Slot::Array(..) => None,
        }
    }

    /// Runs a function or native by name in a new thread.
    pub fn call_by_name(
        &mut self,
        name: &str,
        arguments: Vec<Value>,
    ) -> Result<Option<Value>, RuntimeError> {
        let index = self
            .program
            .function_index(name)
            .ok_or_else(|| self.error(format!("unknown function '{name}'")))?;
        self.run_thread(index, arguments)
    }

    /// Runs a function in a new thread with its own operation budget, as
    /// `ExecuteFunc` does (SPEC-0005). The calling thread's budget is
    /// restored afterwards, whatever the outcome.
    pub fn run_thread(
        &mut self,
        index: usize,
        arguments: Vec<Value>,
    ) -> Result<Option<Value>, RuntimeError> {
        let caller = std::mem::replace(&mut self.thread_operations, 0);
        let result = self.call(index, arguments);
        self.thread_operations = caller;
        result
    }

    /// Restarts the current thread's operation budget, as resuming after a
    /// sleep does (SPEC-0005).
    pub fn reset_thread_budget(&mut self) {
        self.thread_operations = 0;
    }

    /// Calls a function or native by its index in the program, for example
    /// the one a `code` value refers to, within the current thread. For a
    /// script function this charges the cost of running it (SPEC-0005);
    /// the caller's side of a call is charged by the calling expression.
    pub fn call(
        &mut self,
        index: usize,
        arguments: Vec<Value>,
    ) -> Result<Option<Value>, RuntimeError> {
        let info = &self.program.functions[index];
        if arguments.len() != info.parameters.len() {
            return Err(self.error(format!(
                "'{}' takes {} arguments, not {}",
                info.name,
                info.parameters.len(),
                arguments.len()
            )));
        }
        let parameters = info.parameters.clone();
        let returns = info.returns;
        let arguments: Vec<Value> = arguments
            .into_iter()
            .zip(&parameters)
            .map(|(value, &t)| value.coerce(t))
            .collect();
        let function = match &self.bodies[index] {
            Body::Native(f) => {
                let f = f.clone();
                return f(self, &arguments);
            }
            Body::Script(function) => *function,
        };
        let saved = std::mem::replace(
            &mut self.current,
            (function.signature.name.clone(), function.position),
        );
        let mut locals = Locals::new();
        // One operation per parameter.
        self.charge(parameters.len() as u64)?;
        for ((parameter, value), &t) in function
            .signature
            .parameters
            .iter()
            .zip(arguments)
            .zip(&parameters)
        {
            locals.insert(&parameter.name, Slot::Scalar(Some(value), t));
        }
        for local in &function.locals {
            let t = self.program.type_named(&local.type_name).expect("checked");
            // One operation per declaration, and c(e) + 1 for an initial value.
            self.charge(1)?;
            let slot = if local.array {
                Slot::Array(Vec::new(), t)
            } else {
                let value = match &local.initial {
                    Some(initial) => {
                        let value = self.evaluate(&mut locals, initial)?;
                        self.charge_conversion(&value, t)?;
                        self.charge(1)?;
                        Some(value.coerce(t))
                    }
                    None => None,
                };
                Slot::Scalar(value, t)
            };
            locals.insert(&local.name, slot);
        }
        let result = match self.block(&mut locals, &function.body)? {
            Flow::Return(value) => value.map(|v| v.coerce(returns.expect("checked"))),
            Flow::Next | Flow::Exit => {
                // Reaching `endfunction` costs as much as a bare `return`.
                self.charge(2)?;
                None
            }
        };
        self.current = saved;
        Ok(result)
    }

    fn error(&self, message: impl Into<String>) -> RuntimeError {
        RuntimeError {
            message: message.into(),
            function: self.current.0.clone(),
            position: self.current.1,
        }
    }

    fn fault(&self, fault: Fault) -> RuntimeError {
        self.error(match fault {
            Fault::DivisionByZero => "division by zero".to_owned(),
            Fault::DivisionOverflow => "integer division overflow".to_owned(),
            Fault::InvalidLiteral(text) => format!("invalid literal {text}"),
        })
    }

    fn block(
        &mut self,
        locals: &mut Locals<'a>,
        statements: &'a [Statement],
    ) -> Result<Flow, RuntimeError> {
        for statement in statements {
            match self.statement(locals, statement)? {
                Flow::Next => {}
                flow => return Ok(flow),
            }
        }
        Ok(Flow::Next)
    }

    /// Counts `n` operations of the current thread (SPEC-0005) and stops it
    /// when that exceeds the limit.
    fn charge(&mut self, n: u64) -> Result<(), RuntimeError> {
        self.operations += n;
        self.thread_operations += n;
        if self
            .operation_limit
            .is_some_and(|limit| self.thread_operations > limit)
        {
            return Err(self.error("operation limit reached"));
        }
        Ok(())
    }

    /// One operation when an integer is used where a real is expected.
    fn charge_conversion(&mut self, value: &Value, target: Type) -> Result<(), RuntimeError> {
        if matches!(value, Value::Integer(_)) && target == Type::Real {
            self.charge(1)?;
        }
        Ok(())
    }

    fn statement(
        &mut self,
        locals: &mut Locals<'a>,
        statement: &'a Statement,
    ) -> Result<Flow, RuntimeError> {
        self.current.1 = statement.position;
        match &statement.kind {
            StatementKind::Set { name, index, value } => {
                let index = match index {
                    Some(index) => Some(self.integer(locals, index)?),
                    None => None,
                };
                let value = self.evaluate(locals, value)?;
                let target = self.variable_type(locals, name);
                self.charge_conversion(&value, target)?;
                // `set v = e` costs c(e) + 1, `set a[i] = e` c(i) + c(e) + 3.
                self.charge(if index.is_some() { 3 } else { 1 })?;
                self.store(locals, name, index, value)?;
            }
            StatementKind::Call(call) => {
                self.call_expression(locals, call)?;
            }
            StatementKind::If {
                branches,
                otherwise,
            } => return self.if_chain(locals, branches, otherwise.as_deref(), false),
            StatementKind::Loop(body) => {
                // 1 on entry, then c(S) + 1 per pass.
                self.charge(1)?;
                loop {
                    match self.block(locals, body)? {
                        Flow::Next => self.charge(1)?,
                        Flow::Exit => break,
                        flow @ Flow::Return(_) => return Ok(flow),
                    }
                }
            }
            StatementKind::ExitWhen(condition) => {
                let exit = self.boolean(locals, condition)?;
                self.charge(1)?;
                if exit {
                    return Ok(Flow::Exit);
                }
            }
            StatementKind::Return(value) => {
                let value = match value {
                    Some(value) => {
                        let value = self.evaluate(locals, value)?;
                        if let Some(returns) = self.current_returns() {
                            self.charge_conversion(&value, returns)?;
                        }
                        Some(value)
                    }
                    None => None,
                };
                self.charge(2)?;
                return Ok(Flow::Return(value));
            }
        }
        Ok(Flow::Next)
    }

    /// `if` with the costs of SPEC-0005. An `elseif` chain is an `if` whose
    /// `else` holds the rest of the chain; the last `elseif` (`nested`)
    /// acts as an `if` with an empty `else`.
    fn if_chain(
        &mut self,
        locals: &mut Locals<'a>,
        branches: &'a [(Expression, Vec<Statement>)],
        otherwise: Option<&'a [Statement]>,
        nested: bool,
    ) -> Result<Flow, RuntimeError> {
        let ((condition, then), rest) = branches.split_first().expect("at least one branch");
        let taken = self.boolean(locals, condition)?;
        let has_else = !rest.is_empty() || otherwise.is_some() || nested;
        let else_empty = rest.is_empty() && otherwise.is_none_or(<[Statement]>::is_empty);
        if !has_else {
            // `if c then S endif`: c(c) when S is empty, otherwise c(c) + 1
            // when false and c(c) + c(S) + 2 when true.
            if then.is_empty() {
                return Ok(Flow::Next);
            }
            self.charge(1)?;
            if !taken {
                return Ok(Flow::Next);
            }
            let flow = self.block(locals, then)?;
            if matches!(flow, Flow::Next) {
                self.charge(1)?;
            }
            return Ok(flow);
        }
        if then.is_empty() && else_empty {
            return Ok(Flow::Next);
        }
        // c(c) + c(branch) + 2 for the branch taken. The cost of an empty
        // `then` with a non-empty `else` is not measured yet (SPEC-0005).
        self.charge(1)?;
        let flow = if taken {
            self.block(locals, then)?
        } else if !rest.is_empty() {
            self.if_chain(locals, rest, otherwise, true)?
        } else {
            self.block(locals, otherwise.unwrap_or(&[]))?
        };
        if matches!(flow, Flow::Next) {
            self.charge(1)?;
        }
        Ok(flow)
    }

    /// Declared type of a variable visible here.
    fn variable_type(&mut self, locals: &mut Locals<'a>, name: &str) -> Type {
        match self.slot(locals, name) {
            Slot::Scalar(_, t) | Slot::Array(_, t) => *t,
        }
    }

    fn current_returns(&self) -> Option<Type> {
        self.program
            .function(&self.current.0)
            .and_then(|f| f.returns)
    }

    fn slot<'s>(&'s mut self, locals: &'s mut Locals<'a>, name: &str) -> &'s mut Slot {
        match locals.get_mut(name) {
            Some(slot) => slot,
            None => &mut self.globals[self.global_index[name]],
        }
    }

    fn check_index(&self, index: i32) -> Result<usize, RuntimeError> {
        if index < 0 || index as usize >= self.array_size {
            return Err(self.error(format!("array index {index} out of range")));
        }
        Ok(index as usize)
    }

    fn store(
        &mut self,
        locals: &mut Locals<'a>,
        name: &str,
        index: Option<i32>,
        value: Value,
    ) -> Result<(), RuntimeError> {
        let index = index.map(|i| self.check_index(i)).transpose()?;
        match self.slot(locals, name) {
            Slot::Scalar(slot, t) => *slot = Some(value.coerce(*t)),
            Slot::Array(elements, t) => {
                let index = index.expect("checked");
                if elements.len() <= index {
                    elements.resize(index + 1, Value::default_of(*t));
                }
                elements[index] = value.coerce(*t);
            }
        }
        Ok(())
    }

    fn read(
        &mut self,
        locals: &mut Locals<'a>,
        name: &str,
        index: Option<i32>,
    ) -> Result<Value, RuntimeError> {
        let index = index.map(|i| self.check_index(i)).transpose()?;
        match self.slot(locals, name) {
            Slot::Scalar(Some(value), _) => Ok(value.clone()),
            Slot::Scalar(None, _) => {
                Err(self.error(format!("read of uninitialised variable '{name}'")))
            }
            Slot::Array(elements, t) => {
                let index = index.expect("checked");
                Ok(elements
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| Value::default_of(*t)))
            }
        }
    }

    fn call_expression(
        &mut self,
        locals: &mut Locals<'a>,
        call: &'a Call,
    ) -> Result<Option<Value>, RuntimeError> {
        let index = self.program.function_index(&call.name).expect("checked");
        let info = &self.program.functions[index];
        let (parameters, native) = (info.parameters.clone(), info.native);
        // (c(a) + 1) per argument, then 1 for the call.
        let mut arguments = Vec::with_capacity(call.arguments.len());
        for (argument, &t) in call.arguments.iter().zip(&parameters) {
            let value = self.evaluate(locals, argument)?;
            self.charge_conversion(&value, t)?;
            self.charge(1)?;
            arguments.push(value);
        }
        self.charge(1)?;
        let position = self.current.1;
        let result = self.call(index, arguments)?;
        self.current.1 = position;
        // A script function with arguments takes one more operation to drop
        // them after it returns; a native does not.
        if !native && !parameters.is_empty() {
            self.charge(1)?;
        }
        Ok(result)
    }

    fn integer(&mut self, locals: &mut Locals<'a>, e: &'a Expression) -> Result<i32, RuntimeError> {
        match self.evaluate(locals, e)? {
            Value::Integer(i) => Ok(i),
            other => Err(self.error(format!("expected an integer, got {other:?}"))),
        }
    }

    fn boolean(
        &mut self,
        locals: &mut Locals<'a>,
        e: &'a Expression,
    ) -> Result<bool, RuntimeError> {
        match self.evaluate(locals, e)? {
            Value::Boolean(b) => Ok(b),
            other => Err(self.error(format!("expected a boolean, got {other:?}"))),
        }
    }

    fn evaluate(
        &mut self,
        locals: &mut Locals<'a>,
        e: &'a Expression,
    ) -> Result<Value, RuntimeError> {
        // Literals, variables and `function f` cost 1; `a[i]`, `-e` and
        // `not e` cost one more than their operand. Calls and binary
        // operators charge their own costs; parentheses cost nothing.
        if matches!(
            e.kind,
            ExpressionKind::Integer(_)
                | ExpressionKind::RawCode(_)
                | ExpressionKind::Real(_)
                | ExpressionKind::String(_)
                | ExpressionKind::Boolean(_)
                | ExpressionKind::Null
                | ExpressionKind::FunctionReference(_)
                | ExpressionKind::Variable(_)
        ) {
            self.charge(1)?;
        }
        Ok(match &e.kind {
            ExpressionKind::Integer(text) => {
                Value::Integer(semantics::integer_literal(text).map_err(|f| self.fault(f))?)
            }
            ExpressionKind::RawCode(text) => {
                Value::Integer(semantics::raw_code_literal(text).map_err(|f| self.fault(f))?)
            }
            ExpressionKind::Real(text) => Value::Real(semantics::real_literal(text)),
            ExpressionKind::String(text) => Value::string(&semantics::decode_escapes(text)),
            ExpressionKind::Boolean(b) => Value::Boolean(*b),
            ExpressionKind::Null => Value::Null,
            ExpressionKind::FunctionReference(name) => {
                Value::Code(Some(self.program.function_index(name).expect("checked")))
            }
            ExpressionKind::Variable(name) => self.read(locals, name, None)?,
            ExpressionKind::Index { name, index } => {
                let index = self.integer(locals, index)?;
                self.charge(1)?;
                self.read(locals, name, Some(index))?
            }
            ExpressionKind::Call(call) => self
                .call_expression(locals, call)?
                .ok_or_else(|| self.error(format!("'{}' returned no value", call.name)))?,
            ExpressionKind::Parenthesized(inner) => self.evaluate(locals, inner)?,
            ExpressionKind::Unary { operator, operand } => {
                let value = self.evaluate(locals, operand)?;
                self.charge(1)?;
                match (operator, value) {
                    (UnaryOperator::Not, Value::Boolean(b)) => Value::Boolean(!b),
                    (UnaryOperator::Minus, Value::Integer(i)) => Value::Integer(i.wrapping_neg()),
                    // Flips the sign bit (SPEC-0004).
                    (UnaryOperator::Minus, Value::Real(r)) => Value::Real(r ^ 0x8000_0000),
                    (UnaryOperator::Plus, value @ (Value::Integer(_) | Value::Real(_))) => value,
                    (_, value) => return Err(self.error(format!("bad operand {value:?}"))),
                }
            }
            ExpressionKind::Binary {
                operator,
                left,
                right,
            } => self.binary(locals, *operator, left, right)?,
        })
    }

    fn binary(
        &mut self,
        locals: &mut Locals<'a>,
        operator: BinaryOperator,
        left: &'a Expression,
        right: &'a Expression,
    ) -> Result<Value, RuntimeError> {
        use BinaryOperator::*;
        // `and` and `or` skip the right operand when the left decides. They
        // cost c(l) + 3 when the left decides, c(l) + c(r) + 2 otherwise.
        if matches!(operator, And | Or) {
            let l = self.boolean(locals, left)?;
            let decided = matches!((operator, l), (And, false) | (Or, true));
            if decided {
                self.charge(3)?;
                return Ok(Value::Boolean(l));
            }
            self.charge(2)?;
            return Ok(Value::Boolean(self.boolean(locals, right)?));
        }
        // c(l) + c(r) + 3, plus 1 for an integer operand converted to real.
        let l = self.evaluate(locals, left)?;
        let r = self.evaluate(locals, right)?;
        for (value, other) in [(&l, &r), (&r, &l)] {
            if matches!(value, Value::Integer(_)) && matches!(other, Value::Real(_)) {
                self.charge(1)?;
            }
        }
        self.charge(3)?;
        let value = match (operator, l, r) {
            (Equal, l, r) => Value::Boolean(semantics::equal(&l, &r)),
            (NotEqual, l, r) => Value::Boolean(semantics::not_equal(&l, &r)),
            (Add, Value::String(a), b @ (Value::String(_) | Value::Null))
            | (Add, b @ Value::Null, Value::String(a)) => {
                let a = a.as_deref().unwrap_or("");
                let b = match &b {
                    Value::String(Some(b)) => b,
                    _ => "",
                };
                Value::string(&format!("{a}{b}"))
            }
            (op, Value::Integer(a), Value::Integer(b)) => match op {
                Less => Value::Boolean(a < b),
                LessEqual => Value::Boolean(a <= b),
                Greater => Value::Boolean(a > b),
                GreaterEqual => Value::Boolean(a >= b),
                _ => Value::Integer(semantics::integer_op(op, a, b).map_err(|f| self.fault(f))?),
            },
            (op, l, r) => {
                let (Some(a), Some(b)) = (as_real(&l), as_real(&r)) else {
                    return Err(self.error(format!("bad operands {l:?} and {r:?}")));
                };
                match op {
                    Less | LessEqual | Greater | GreaterEqual => {
                        Value::Boolean(semantics::real_order(op, a, b))
                    }
                    _ => Value::Real(semantics::real_op(op, a, b).map_err(|f| self.fault(f))?),
                }
            }
        };
        Ok(value)
    }
}

/// Real operand as a bit pattern; integers are promoted with `I2R`.
fn as_real(value: &Value) -> Option<u32> {
    match value {
        Value::Integer(i) => Some(ow3_math::i2r(*i)),
        Value::Real(r) => Some(*r),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    const COMMON: &str = "\
type agent extends handle
type timer extends agent
globals
    constant integer JASS_MAX_ARRAY_SIZE = 16
endglobals
native Print takes string s returns nothing
native I2S takes integer i returns string
native R2S takes real r returns string
native CreateTimer takes nothing returns timer
native Run takes code f returns nothing
";

    /// Runs `main` of `source` and returns what it printed.
    fn run(source: &str) -> Result<Vec<String>, String> {
        let common = crate::parse(COMMON).unwrap();
        let script = crate::parse(source).map_err(|e| e.to_string())?;
        let program = crate::check::check(&[&common, &script]).map_err(|d| format!("{d:?}"))?;
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut natives = Natives::default();
        let sink = output.clone();
        natives.register(
            "native Print takes string s returns nothing",
            Rc::new(move |_, args| {
                let Value::String(s) = &args[0] else {
                    unreachable!()
                };
                sink.borrow_mut()
                    .push(s.as_deref().unwrap_or("<null>").to_owned());
                Ok(None)
            }),
        );
        natives.register(
            "native I2S takes integer i returns string",
            Rc::new(|_, args| {
                let Value::Integer(i) = args[0] else {
                    unreachable!()
                };
                Ok(Some(Value::string(&i.to_string())))
            }),
        );
        natives.register(
            "native R2S takes real r returns string",
            Rc::new(|_, args| {
                let Value::Real(r) = args[0] else {
                    unreachable!()
                };
                Ok(Some(Value::string(&format!("{:.3}", f32::from_bits(r)))))
            }),
        );
        natives.register(
            "native CreateTimer takes nothing returns timer",
            Rc::new(|_, _| Ok(Some(Value::Handle(Some(7))))),
        );
        natives.register(
            "native Run takes code f returns nothing",
            Rc::new(|vm, args| {
                let Value::Code(Some(f)) = args[0] else {
                    unreachable!()
                };
                vm.call(f, Vec::new())
            }),
        );
        let mut vm =
            Vm::load(&program, &[&common, &script], &natives).map_err(|e| e.to_string())?;
        vm.operation_limit = Some(10_000);
        vm.call_by_name("main", Vec::new())
            .map_err(|e| e.to_string())?;
        let printed = output.borrow().clone();
        Ok(printed)
    }

    #[test]
    fn runs_control_flow_and_arithmetic() {
        let printed = run("
            globals
                integer total = 0
                real array weights
            endglobals
            function Fib takes integer n returns integer
                if n < 2 then
                    return n
                endif
                return Fib(n - 1) + Fib(n - 2)
            endfunction
            function main takes nothing returns nothing
                local integer i = 0
                loop
                    exitwhen i > 4
                    set total = total + i
                    set weights[i] = i / 2
                    set i = i + 1
                endloop
                call Print(I2S(total) + \" \" + I2S(Fib(10)))
                call Print(R2S(weights[3]) + \" \" + R2S(weights[9]) + \" \" + R2S(7 / 2.0))
                if total == 10 and not (i != 5) then
                    call Print(\"branch\")
                elseif true then
                    call Print(\"wrong\")
                endif
            endfunction")
        .unwrap();
        assert_eq!(printed, ["10 55", "1.000 0.000 3.500", "branch"]);
    }

    #[test]
    fn calls_back_through_code_values() {
        let printed = run("
            function Callback takes nothing returns nothing
                call Print(\"called back\")
            endfunction
            function main takes nothing returns nothing
                local timer t = CreateTimer()
                call Run(function Callback)
                if t != null and CreateTimer() == t then
                    call Print(\"same handle\")
                endif
            endfunction")
        .unwrap();
        assert_eq!(printed, ["called back", "same handle"]);
    }

    #[test]
    fn reports_runtime_errors() {
        let err = run("
            function main takes nothing returns nothing
                local integer x
                call Print(I2S(x))
            endfunction")
        .unwrap_err();
        assert!(err.contains("uninitialised variable 'x'"), "{err}");
        let err = run("
            function main takes nothing returns nothing
                local integer array a
                set a[16] = 1
            endfunction")
        .unwrap_err();
        assert!(err.contains("index 16 out of range"), "{err}");
        let err = run("
            function main takes nothing returns nothing
                call Print(I2S(1 / 0))
            endfunction")
        .unwrap_err();
        assert!(err.contains("division by zero"), "{err}");
        let err = run("
            function main takes nothing returns nothing
                loop
                endloop
            endfunction")
        .unwrap_err();
        assert!(err.contains("operation limit"), "{err}");
    }

    #[test]
    /// The precedence and short-circuit table of SPEC-0005's verification.
    fn spec_0005_precedence_and_short_circuit() {
        let cases = [
            ("T(\"a\") and F(\"b\") or T(\"c\")", "a b c", true),
            ("F(\"a\") and T(\"b\") or T(\"c\")", "a", false),
            ("T(\"a\") or F(\"b\") and F(\"c\")", "a c", false),
            ("F(\"a\") or T(\"b\") and F(\"c\")", "a b c", false),
            ("(F(\"a\") and F(\"b\")) or T(\"c\")", "a c", true),
            ("F(\"a\") and (F(\"b\") or T(\"c\"))", "a", false),
            (
                "T(\"a\") or F(\"b\") and F(\"c\") or T(\"d\")",
                "a c d",
                true,
            ),
            ("F(\"a\") and T(\"b\") or T(\"c\") and T(\"d\")", "a", false),
            ("not F(\"a\") and F(\"b\")", "a b", false),
            ("not T(\"a\") or T(\"b\")", "a b", true),
            ("not true and false", "", false),
            ("true or true and false", "", false),
            ("false and false or true", "", false),
        ];
        for (expression, evaluated, result) in cases {
            let printed = run(&format!(
                "function T takes string s returns boolean
                    call Print(s)
                    return true
                endfunction
                function F takes string s returns boolean
                    call Print(s)
                    return false
                endfunction
                function main takes nothing returns nothing
                    if {expression} then
                        call Print(\"=true\")
                    else
                        call Print(\"=false\")
                    endif
                endfunction"
            ))
            .unwrap();
            let expected: Vec<String> = evaluated
                .split_whitespace()
                .map(str::to_owned)
                .chain([format!("={result}")])
                .collect();
            assert_eq!(printed, expected, "{expression}");
        }
    }

    /// The literal values of SPEC-0005's verification.
    #[test]
    fn spec_0005_literals() {
        let printed = run("
            function main takes nothing returns nothing
                call Print(I2S(0x10) + \" \" + I2S(0xFF) + \" \" + I2S(0xffffffff))
                call Print(I2S($10) + \" \" + I2S($ff) + \" \" + I2S(0X1f))
                call Print(I2S(010) + \" \" + I2S(017) + \" \" + I2S(4294967296))
                call Print(I2S(2147483647 + 1) + \" \" + I2S('a') + \" \" + I2S('abcd'))
                call Print(I2S('hfoo') + \" \" + I2S('\\n'))
                call Print(R2S(5.) + \" \" + R2S(.25))
            endfunction")
        .unwrap();
        assert_eq!(
            printed,
            [
                "16 255 -1",
                "16 255 31",
                "8 15 0",
                "-2147483648 97 1633837924",
                "1751543663 10",
                "5.000 0.250"
            ]
        );
        for invalid in ["08", "'ab'", "\"\\q\""] {
            let err = run(&format!(
                "function main takes nothing returns nothing\ncall Print(I2S({invalid}))\nendfunction"
            ))
            .unwrap_err();
            assert!(!err.is_empty(), "{invalid} should not load");
        }
    }

    #[test]
    fn integer_division_overflow_ends_the_thread() {
        let err = run("
            function main takes nothing returns nothing
                call Print(I2S((-2147483647 - 1) / -1))
            endfunction")
        .unwrap_err();
        assert!(err.contains("integer division overflow"), "{err}");
    }

    #[test]
    fn loading_needs_every_native_with_its_signature() {
        let common = crate::parse(COMMON).unwrap();
        let program = crate::check::check(&[&common]).unwrap();
        let err = Vm::load(&program, &[&common], &Natives::default())
            .err()
            .unwrap();
        assert_eq!(err.0, "native 'Print' has no implementation");

        let mut natives = Natives::default().stub_missing();
        natives.register(
            "native Print takes integer s returns nothing",
            Rc::new(|_, _| Ok(None)),
        );
        let err = Vm::load(&program, &[&common], &natives).err().unwrap();
        assert_eq!(
            err.0,
            "native 'Print' is implemented with a different signature"
        );

        let natives = Natives::default().stub_missing();
        let mut vm = Vm::load(&program, &[&common], &natives).unwrap();
        assert_eq!(
            vm.call_by_name("I2S", vec![Value::Integer(1)]).unwrap(),
            Some(Value::String(None))
        );
    }
}

#[cfg(test)]
mod operation_cost_tests {
    use super::*;

    const COMMON: &str = "\
type agent extends handle
type timer extends agent
native CreateTimer takes nothing returns timer
";

    /// Runs `Body` in a fresh thread until the operation limit stops it and
    /// returns how many passes stored to `g`, as in SPEC-0005's
    /// verification.
    fn passes(extra: &str) -> i32 {
        let source = format!(
            "globals
                integer g = 0
                integer x = 0
            endglobals
            function F takes integer a returns nothing
            endfunction
            function Body takes nothing returns nothing
                local integer lx = 0
                set g = 0
                loop
                    set g = g + 1
                    {extra}
                endloop
            endfunction"
        );
        let common = crate::parse(COMMON).unwrap();
        let script = crate::parse(&source).unwrap();
        let program = crate::check::check(&[&common, &script]).unwrap();
        let natives = Natives::default().stub_missing();
        let mut vm = Vm::load(&program, &[&common, &script], &natives).unwrap();
        let err = vm.call_by_name("Body", Vec::new()).unwrap_err();
        assert_eq!(err.message, "operation limit reached");
        let Some(Value::Integer(g)) = vm.global("g") else {
            panic!("g is not an integer")
        };
        *g
    }

    /// `N = floor(299988 / C) + 1` passes for a loop body of cost `C`.
    fn predicted(cost: i32) -> i32 {
        299_988 / cost + 1
    }

    #[test]
    fn spec_0005_loop_measurements() {
        assert_eq!(passes(""), 42_856);
        assert_eq!(predicted(7), 42_856);
        assert_eq!(passes("set x = 1"), 33_333);
        assert_eq!(passes("call F(1)"), 21_428);
        assert_eq!(passes("call CreateTimer()"), 37_499);
    }

    #[test]
    fn statement_and_expression_costs() {
        // Each body adds a statement of known cost to the plain loop (7).
        for (extra, cost) in [
            ("set x = (1 + 2) * 3", 7 + 10),
            ("set x = -x", 7 + 3),
            ("if true then\nset x = 1\nendif", 7 + 5),
            ("if false then\nset x = 1\nendif", 7 + 2),
            ("if false then\nendif", 7 + 1),
            ("if true then\nset x = 1\nelse\nset x = 2\nendif", 7 + 5),
            ("if false then\nset x = 1\nelse\nset x = 2\nendif", 7 + 5),
            (
                "if false then\nset x = 1\nelseif true then\nset x = 2\nendif",
                7 + 8,
            ),
            (
                "if false then\nset x = 1\nelseif false then\nset x = 2\nendif",
                7 + 6,
            ),
            ("exitwhen false", 7 + 2),
            ("if true and false then\nendif", 7 + 4),
            ("if false and true then\nendif", 7 + 4),
        ] {
            assert_eq!(passes(extra), predicted(cost), "{extra}");
        }
    }
}
