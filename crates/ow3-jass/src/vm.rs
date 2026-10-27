//! Interpreter for checked scripts, running compiled instructions on
//! threads that can be suspended.
//!
//! Natives are provided by the embedder through [`Natives`]. Loading fails
//! when a declared native has no implementation or its implementation has
//! a different signature (SPEC-0001).
//!
//! Operations are counted with the costs of SPEC-0005: every thread may run
//! [`OPERATION_LIMIT`] of them, and the count restarts when it resumes
//! after a sleep. A native puts the running thread to sleep with
//! [`Vm::sleep`]; the thread then stops after that native returns, and the
//! embedder resumes it later with [`Vm::resume`].

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::rc::Rc;

use crate::Position;
use crate::ast::*;
use crate::bytecode::{Code, Compiler, Op};
use crate::check::{Program, Type};
use crate::semantics::{self, Fault, Value};

/// Operations a thread may execute between sleeps (SPEC-0005).
pub const OPERATION_LIMIT: u64 = 300_000;

/// PROVISIONAL: threads started from inside other threads (by natives such
/// as `ExecuteFunc` and `TriggerExecute`) may nest this deep; starting one
/// more fails with an error in the caller. The game's limit is not known.
pub const MAX_NESTING: u32 = 100;

/// Implementation of a native. Receives the interpreter, so that it can call
/// back into scripts or put the thread to sleep, and the arguments already
/// converted to the declared parameter types. Returns `None` for natives
/// that return nothing.
pub type NativeFn = Rc<dyn Fn(&mut Vm, &[Value]) -> Result<Option<Value>, RuntimeError>>;

/// Told the name of each stub native called.
pub type StubHook = Rc<dyn Fn(&str)>;

/// Native implementations by name, each with the JASS declaration it
/// implements.
#[derive(Clone, Default)]
pub struct Natives {
    entries: HashMap<String, (Signature, NativeFn)>,
    stub_missing: bool,
    /// Told the name of each stub called, as natives are counted.
    stub_hook: Option<StubHook>,
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

    /// Calls `hook` with the name of every stub called.
    pub fn set_stub_hook(&mut self, hook: StubHook) {
        self.stub_hook = Some(hook);
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

enum Body {
    Native(NativeFn),
    Script(Rc<Code>),
}

/// Storage of one variable with its declared type. A scalar holds `None`
/// until it is first assigned.
#[derive(Clone, Debug)]
enum Slot {
    Scalar(Option<Value>, Type),
    Array(Vec<Value>, Type),
}

impl Slot {
    fn new(t: Type, array: bool) -> Slot {
        if array {
            Slot::Array(Vec::new(), t)
        } else {
            Slot::Scalar(None, t)
        }
    }
}

#[derive(Debug)]
struct Frame {
    function: usize,
    code: Rc<Code>,
    pc: usize,
    locals: Vec<Slot>,
    /// The bottom frame of a spawned thread: a single call of the thread's
    /// function, whose result ends the thread.
    entry: bool,
}

/// A script thread: its call frames, value stack and operation count. It
/// can be suspended at a sleep and resumed later.
#[derive(Debug, Default)]
pub struct Thread {
    frames: Vec<Frame>,
    stack: Vec<Value>,
    /// Operations since the thread started or last resumed.
    pub operations: u64,
}

impl Thread {
    pub fn is_finished(&self) -> bool {
        self.frames.is_empty()
    }
}

/// How a run of a thread ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The thread's function returned, with its value.
    Finished(Option<Value>),
    /// A native asked the thread to sleep for this many seconds (a real's
    /// bit pattern). Resume it with [`Vm::resume`].
    Sleep(u32),
}

pub struct Vm<'a> {
    program: &'a Program,
    bodies: Vec<Body>,
    globals: Vec<Slot>,
    array_size: usize,
    /// Operations executed so far in all threads.
    pub operations: u64,
    /// Operations of the running thread, kept here while it runs.
    thread_operations: u64,
    /// A thread stops with an error when it would execute more operations
    /// than this. The game's limit is [`OPERATION_LIMIT`].
    pub operation_limit: Option<u64>,
    /// Set by a native through [`Vm::sleep`].
    pending_sleep: Option<u32>,
    /// Threads running now, one inside the other.
    nesting: u32,
    /// Calls of stubbed natives (see [`Natives::stub_missing`]) by function
    /// index.
    stub_calls: BTreeMap<usize, u64>,
    /// Function and position being executed, for error messages.
    current: (usize, Position),
}

impl<'a> Vm<'a> {
    /// Compiles the scripts, binds natives and runs the global
    /// initialisers in declaration order.
    pub fn load(
        program: &'a Program,
        scripts: &[&'a Script],
        natives: &Natives,
    ) -> Result<Vm<'a>, LoadError> {
        let compiler = Compiler::new(program);
        let compile_error = |e: crate::bytecode::CompileError| {
            LoadError(format!(
                "{}:{}: {}",
                e.position.line,
                e.position.column,
                fault_message(e.fault)
            ))
        };
        let mut script_functions: HashMap<&str, &Function> = HashMap::new();
        for script in scripts {
            for function in &script.functions {
                script_functions.insert(&function.signature.name, function);
            }
        }
        let mut bodies = Vec::with_capacity(program.functions.len());
        for info in &program.functions {
            if !info.native {
                let function = script_functions[info.name.as_str()];
                bodies.push(Body::Script(
                    compiler.compile_function(function).map_err(compile_error)?,
                ));
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
                    let index = bodies.len();
                    let hook = natives.stub_hook.clone();
                    let name = info.name.clone();
                    Rc::new(move |vm: &mut Vm, _: &[Value]| {
                        if let Some(hook) = &hook {
                            hook(&name);
                        }
                        *vm.stub_calls.entry(index).or_default() += 1;
                        Ok(returns.map(Value::default_of))
                    }) as NativeFn
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
            globals: program
                .globals
                .iter()
                .map(|g| Slot::new(g.value_type, g.array))
                .collect(),
            array_size: 8192,
            operations: 0,
            thread_operations: 0,
            operation_limit: None,
            pending_sleep: None,
            nesting: 0,
            stub_calls: BTreeMap::new(),
            current: (0, Position { line: 0, column: 0 }),
        };
        for script in scripts {
            for declaration in &script.declarations {
                let Declaration::Globals(globals) = declaration else {
                    continue;
                };
                for global in globals {
                    let variable = &global.variable;
                    let Some(initial) = &variable.initial else {
                        continue;
                    };
                    let index = compiler.global_index(&variable.name).expect("checked");
                    let t = program.globals[index].value_type;
                    let code = Rc::new(
                        compiler
                            .compile_expression(initial)
                            .map_err(compile_error)?,
                    );
                    let value = vm
                        .evaluate_detached(code)
                        .map_err(|e| LoadError(format!("initialising '{}': {e}", variable.name)))?
                        .coerce(t);
                    if variable.name == "JASS_MAX_ARRAY_SIZE" {
                        if let Value::Integer(n) = value {
                            vm.array_size = n.max(0) as usize;
                        }
                    }
                    vm.globals[index] = Slot::Scalar(Some(value), t);
                }
            }
        }
        // Global initialisers do not count toward any thread.
        vm.operations = 0;
        vm.operation_limit = Some(OPERATION_LIMIT);
        Ok(vm)
    }

    pub fn program(&self) -> &Program {
        self.program
    }

    /// Current value of a scalar global, or `None` if it is unknown, an
    /// array or not yet assigned.
    pub fn global(&self, name: &str) -> Option<&Value> {
        let index = self.program.globals.iter().position(|g| g.name == name)?;
        match &self.globals[index] {
            Slot::Scalar(value, _) => value.as_ref(),
            Slot::Array(..) => None,
        }
    }

    /// Asks the running thread to sleep for `duration` seconds once the
    /// calling native returns. For natives such as `TriggerSleepAction`.
    pub fn sleep(&mut self, duration: u32) {
        self.pending_sleep = Some(duration);
    }

    /// How often each stubbed native was called, by name, for finding the
    /// natives that scripts use and that are not implemented yet.
    pub fn stub_calls(&self) -> impl Iterator<Item = (&str, u64)> + '_ {
        self.stub_calls
            .iter()
            .map(|(&i, &n)| (self.program.functions[i].name.as_str(), n))
    }

    /// An error that ends the running thread, at its current position, for
    /// natives that end their caller.
    pub fn fail(&self, message: impl Into<String>) -> RuntimeError {
        self.error(message)
    }

    /// A new thread that will run a function, not started yet.
    pub fn spawn(&self, index: usize, arguments: Vec<Value>) -> Thread {
        let mut thread = Thread::default();
        thread.stack.extend(arguments);
        // The first resume calls the function with the arguments on the
        // stack, from an entry frame.
        thread.frames.push(Frame {
            function: index,
            code: Rc::new(Code {
                ops: vec![Op::Call {
                    function: index,
                    arguments: self.program.functions[index].parameters.len(),
                }],
                positions: vec![Position { line: 0, column: 0 }],
                slots: Vec::new(),
                slot_names: Vec::new(),
                parameters: 0,
            }),
            pc: 0,
            locals: Vec::new(),
            entry: true,
        });
        thread
    }

    /// Runs a thread until it finishes or sleeps. A thread resumed after a
    /// sleep starts a fresh operation count (SPEC-0005).
    pub fn resume(&mut self, thread: &mut Thread) -> Result<Outcome, RuntimeError> {
        if thread.operations > 0 {
            thread.operations = 0;
        }
        self.run(thread)
    }

    /// Runs a function by name in a new thread to its end. A sleep ends it.
    pub fn call_by_name(
        &mut self,
        name: &str,
        arguments: Vec<Value>,
    ) -> Result<Option<Value>, RuntimeError> {
        let index = self
            .program
            .function_index(name)
            .ok_or_else(|| self.error(format!("unknown function '{name}'")))?;
        match self.start_thread(index, arguments)? {
            (Outcome::Finished(value), _) => Ok(value),
            (Outcome::Sleep(_), _) => Ok(None),
        }
    }

    /// Runs a function in a new thread with its own operation budget, as
    /// `ExecuteFunc` does (SPEC-0005). The running thread's budget is kept.
    /// When the new thread sleeps, it is returned so that it can be resumed.
    pub fn start_thread(
        &mut self,
        index: usize,
        arguments: Vec<Value>,
    ) -> Result<(Outcome, Thread), RuntimeError> {
        let mut thread = self.spawn(index, arguments);
        let outcome = self.run(&mut thread)?;
        Ok((outcome, thread))
    }

    /// Calls a function or native within the running thread and its budget,
    /// for callbacks run by natives. A sleep inside the callback ends the
    /// callback (SPEC-0005); use [`Vm::call_outcome`] to tell.
    pub fn call(
        &mut self,
        index: usize,
        arguments: Vec<Value>,
    ) -> Result<Option<Value>, RuntimeError> {
        match self.call_outcome(index, arguments)? {
            Outcome::Finished(value) => Ok(value),
            Outcome::Sleep(_) => Ok(None),
        }
    }

    /// Like [`Vm::call`], but tells whether the callback ended at a sleep.
    pub fn call_outcome(
        &mut self,
        index: usize,
        arguments: Vec<Value>,
    ) -> Result<Outcome, RuntimeError> {
        let mut thread = self.spawn(index, arguments);
        thread.operations = self.thread_operations;
        let outcome = self.run(&mut thread);
        self.thread_operations = thread.operations;
        outcome
    }

    /// Evaluates a compiled expression outside any thread.
    fn evaluate_detached(&mut self, code: Rc<Code>) -> Result<Value, RuntimeError> {
        let mut thread = Thread::default();
        thread.frames.push(Frame {
            function: usize::MAX,
            code,
            pc: 0,
            locals: Vec::new(),
            entry: false,
        });
        let saved = self.operation_limit.take();
        let result = self.execute(&mut thread, true);
        self.operation_limit = saved;
        result?;
        thread
            .stack
            .pop()
            .ok_or_else(|| self.error("initialiser left no value"))
    }

    /// Runs a thread with its operation count, restoring the interrupted
    /// thread's state afterwards.
    fn run(&mut self, thread: &mut Thread) -> Result<Outcome, RuntimeError> {
        if self.nesting >= MAX_NESTING {
            return Err(self.error("thread nesting limit reached"));
        }
        self.nesting += 1;
        let result = self.run_nested(thread);
        self.nesting -= 1;
        result
    }

    fn run_nested(&mut self, thread: &mut Thread) -> Result<Outcome, RuntimeError> {
        let saved_operations = std::mem::replace(&mut self.thread_operations, thread.operations);
        let saved_current = self.current;
        let saved_sleep = self.pending_sleep.take();
        let result = self.execute(thread, false);
        thread.operations = self.thread_operations;
        self.thread_operations = saved_operations;
        self.current = saved_current;
        self.pending_sleep = saved_sleep;
        result
    }

    fn error(&self, message: impl Into<String>) -> RuntimeError {
        let function = self
            .program
            .functions
            .get(self.current.0)
            .map_or_else(|| "<globals>".to_owned(), |f| f.name.clone());
        RuntimeError {
            message: message.into(),
            function,
            position: self.current.1,
        }
    }

    fn fault(&self, fault: Fault) -> RuntimeError {
        self.error(fault_message(fault))
    }

    /// Counts `n` operations of the running thread and stops it when that
    /// exceeds the limit.
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

    fn check_index(&self, index: i32) -> Result<usize, RuntimeError> {
        if index < 0 || index as usize >= self.array_size {
            return Err(self.error(format!("array index {index} out of range")));
        }
        Ok(index as usize)
    }

    /// The main loop. `detached` evaluates a lone expression and stops at
    /// its end instead of at a return.
    fn execute(&mut self, thread: &mut Thread, detached: bool) -> Result<Outcome, RuntimeError> {
        loop {
            let Some(frame) = thread.frames.last_mut() else {
                return Ok(Outcome::Finished(thread.stack.pop()));
            };
            if frame.pc >= frame.code.ops.len() {
                // The entry frame ends after a native thread function
                // returns; a detached expression ends with its value.
                debug_assert!(frame.entry || detached);
                let value = if frame.entry {
                    thread.stack.pop()
                } else {
                    None
                };
                thread.frames.clear();
                return Ok(Outcome::Finished(value));
            }
            let code = frame.code.clone();
            let in_entry = frame.entry;
            let pc = frame.pc;
            frame.pc += 1;
            self.current = (frame.function, code.positions[pc]);
            match &code.ops[pc] {
                Op::Push(value) => {
                    self.charge(1)?;
                    thread.stack.push(value.clone());
                }
                Op::LoadLocal(slot) => {
                    self.charge(1)?;
                    let value = read(&thread.frames.last().expect("frame").locals[*slot], None)
                        .ok_or_else(|| self.uninitialised(*slot, &code))?;
                    thread.stack.push(value);
                }
                Op::LoadGlobal(index) => {
                    self.charge(1)?;
                    let value = read(&self.globals[*index], None)
                        .ok_or_else(|| self.uninitialised_global(*index))?;
                    thread.stack.push(value);
                }
                Op::LoadLocalIndexed(slot) => {
                    let index = self.pop_index(thread)?;
                    self.charge(1)?;
                    let slot = &thread.frames.last().expect("frame").locals[*slot];
                    thread
                        .stack
                        .push(read(slot, Some(index)).expect("arrays read"));
                }
                Op::LoadGlobalIndexed(global) => {
                    let index = self.pop_index(thread)?;
                    self.charge(1)?;
                    thread
                        .stack
                        .push(read(&self.globals[*global], Some(index)).expect("arrays read"));
                }
                Op::Unary(operator) => {
                    let value = pop(thread);
                    self.charge(1)?;
                    let result = match (operator, value) {
                        (UnaryOperator::Not, Value::Boolean(b)) => Value::Boolean(!b),
                        (UnaryOperator::Minus, Value::Integer(i)) => {
                            Value::Integer(i.wrapping_neg())
                        }
                        // Flips the sign bit (SPEC-0004).
                        (UnaryOperator::Minus, Value::Real(r)) => Value::Real(r ^ 0x8000_0000),
                        (UnaryOperator::Plus, value @ (Value::Integer(_) | Value::Real(_))) => {
                            value
                        }
                        (_, value) => return Err(self.error(format!("bad operand {value:?}"))),
                    };
                    thread.stack.push(result);
                }
                Op::Binary(operator) => {
                    let r = pop(thread);
                    let l = pop(thread);
                    let mixed = matches!(
                        (&l, &r),
                        (Value::Integer(_), Value::Real(_)) | (Value::Real(_), Value::Integer(_))
                    );
                    self.charge(if mixed { 4 } else { 3 })?;
                    let value = self.binary(*operator, l, r)?;
                    thread.stack.push(value);
                }
                Op::ShortCircuit { and_, end } => {
                    let left = matches!(pop(thread), Value::Boolean(true));
                    if left != *and_ {
                        self.charge(3)?;
                        thread.stack.push(Value::Boolean(left));
                        thread.frames.last_mut().expect("frame").pc = *end;
                    } else {
                        self.charge(2)?;
                    }
                }
                Op::Coerce(t) => {
                    let value = pop(thread);
                    if matches!(value, Value::Integer(_)) && *t == Type::Real {
                        self.charge(1)?;
                    }
                    thread.stack.push(value.coerce(*t));
                }
                Op::CoerceReturn(t) => {
                    let value = match (pop(thread), t) {
                        (Value::Integer(i), Type::Real) => Value::Real(i as u32),
                        (value, t) => value.coerce(*t),
                    };
                    thread.stack.push(value);
                }
                Op::StoreLocal(slot) => {
                    let value = pop(thread);
                    self.charge(1)?;
                    let slot = &mut thread.frames.last_mut().expect("frame").locals[*slot];
                    store(slot, None, value);
                }
                Op::StoreGlobal(index) => {
                    let value = pop(thread);
                    self.charge(1)?;
                    store(&mut self.globals[*index], None, value);
                }
                Op::StoreLocalIndexed(slot) => {
                    let value = pop(thread);
                    let index = self.pop_index(thread)?;
                    self.charge(3)?;
                    let slot = &mut thread.frames.last_mut().expect("frame").locals[*slot];
                    store(slot, Some(index), value);
                }
                Op::StoreGlobalIndexed(global) => {
                    let value = pop(thread);
                    let index = self.pop_index(thread)?;
                    self.charge(3)?;
                    store(&mut self.globals[*global], Some(index), value);
                }
                Op::Arg => self.charge(1)?,
                Op::Call {
                    function,
                    arguments,
                } => {
                    // Starting a thread is not a call in its code.
                    if !in_entry {
                        self.charge(1)?;
                    }
                    let split = thread.stack.len() - arguments;
                    let arguments: Vec<Value> = thread.stack.split_off(split);
                    let info = &self.program.functions[*function];
                    if arguments.len() != info.parameters.len() {
                        return Err(self.error(format!(
                            "'{}' takes {} arguments, not {}",
                            info.name,
                            info.parameters.len(),
                            arguments.len()
                        )));
                    }
                    let arguments: Vec<Value> = arguments
                        .into_iter()
                        .zip(&info.parameters)
                        .map(|(value, &t)| value.coerce(t))
                        .collect();
                    match &self.bodies[*function] {
                        Body::Native(f) => {
                            let f = f.clone();
                            let result = f(self, &arguments)?;
                            if let Some(value) = result {
                                thread.stack.push(value);
                            }
                            if let Some(duration) = self.pending_sleep.take() {
                                return Ok(Outcome::Sleep(duration));
                            }
                        }
                        Body::Script(callee) => {
                            let callee = callee.clone();
                            // One operation per parameter.
                            self.charge(callee.parameters as u64)?;
                            let mut locals: Vec<Slot> =
                                callee.slots.iter().map(|&(t, a)| Slot::new(t, a)).collect();
                            for (slot, value) in locals.iter_mut().zip(arguments) {
                                store(slot, None, value);
                            }
                            thread.frames.push(Frame {
                                function: *function,
                                code: callee,
                                pc: 0,
                                locals,
                                entry: false,
                            });
                        }
                    }
                }
                Op::Pop => {
                    pop(thread);
                }
                Op::Charge(n) => self.charge(u64::from(*n))?,
                Op::JumpIfFalse(target) => {
                    let condition = pop(thread);
                    self.charge(1)?;
                    if condition != Value::Boolean(true) {
                        thread.frames.last_mut().expect("frame").pc = *target;
                    }
                }
                Op::Jump { target, cost } => {
                    self.charge(u64::from(*cost))?;
                    thread.frames.last_mut().expect("frame").pc = *target;
                }
                Op::ExitWhen(target) => {
                    let condition = pop(thread);
                    self.charge(1)?;
                    if condition == Value::Boolean(true) {
                        thread.frames.last_mut().expect("frame").pc = *target;
                    }
                }
                Op::Return | Op::ReturnNothing { .. } => {
                    let (value, cost) = match code.ops[pc] {
                        Op::ReturnNothing { cost } => (None, cost),
                        _ => (Some(pop(thread)), 2),
                    };
                    self.charge(u64::from(cost))?;
                    let frame = thread.frames.pop().expect("frame");
                    // Returning to the entry frame ends the thread.
                    let entry = thread.frames.last().is_some_and(|caller| caller.entry);
                    if entry {
                        thread.frames.clear();
                        return Ok(Outcome::Finished(value));
                    }
                    // A script function with arguments costs one more
                    // operation in its caller when it returns.
                    if frame.code.parameters > 0 {
                        self.charge(1)?;
                    }
                    if let Some(value) = value {
                        thread.stack.push(value);
                    }
                }
            }
        }
    }

    fn pop_index(&self, thread: &mut Thread) -> Result<usize, RuntimeError> {
        match pop(thread) {
            Value::Integer(i) => self.check_index(i),
            other => Err(self.error(format!("expected an integer index, got {other:?}"))),
        }
    }

    fn uninitialised(&self, slot: usize, code: &Code) -> RuntimeError {
        let name = &code.slot_names[slot];
        self.error(format!("read of uninitialised variable '{name}'"))
    }

    fn uninitialised_global(&self, index: usize) -> RuntimeError {
        let name = &self.program.globals[index].name;
        self.error(format!("read of uninitialised variable '{name}'"))
    }

    fn binary(&self, operator: BinaryOperator, l: Value, r: Value) -> Result<Value, RuntimeError> {
        use BinaryOperator::*;
        Ok(match (operator, l, r) {
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
        })
    }
}

fn fault_message(fault: Fault) -> String {
    match fault {
        Fault::DivisionByZero => "division by zero".to_owned(),
        Fault::DivisionOverflow => "integer division overflow".to_owned(),
        Fault::InvalidLiteral(text) => format!("invalid literal {text}"),
    }
}

fn pop(thread: &mut Thread) -> Value {
    thread
        .stack
        .pop()
        .expect("compiled code keeps the stack balanced")
}

/// Reads a scalar (`None` if never assigned) or an array element (default
/// value if never assigned).
fn read(slot: &Slot, index: Option<usize>) -> Option<Value> {
    match slot {
        Slot::Scalar(value, _) => value.clone(),
        Slot::Array(elements, t) => Some(
            elements
                .get(index.expect("checked"))
                .cloned()
                .unwrap_or_else(|| Value::default_of(*t)),
        ),
    }
}

fn store(slot: &mut Slot, index: Option<usize>, value: Value) {
    match slot {
        Slot::Scalar(stored, t) => *stored = Some(value.coerce(*t)),
        Slot::Array(elements, t) => {
            let index = index.expect("checked");
            if elements.len() <= index {
                elements.resize(index + 1, Value::default_of(*t));
            }
            elements[index] = value.coerce(*t);
        }
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

    /// SPEC-0005: an integer returned from a real function keeps its bits;
    /// a repeated local hides the earlier one; `null` passes as 0.
    #[test]
    fn returns_locals_and_null() {
        let printed = run("
            function One takes nothing returns real
                local integer i = 1065353216
                return i
            endfunction
            function Q takes integer q returns real
                local integer q = 1
                local real q = 2.5
                return q
            endfunction
            function Z takes nothing returns integer
                if true then
                    return null
                endif
                return 5
            endfunction
            function main takes nothing returns nothing
                call Print(R2S(One()))
                call Print(R2S(Q(7)))
                call Print(I2S(Z()))
                call Print(I2S(null))
            endfunction")
        .unwrap();
        assert_eq!(printed, ["1.000", "2.500", "0", "0"]);
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
                real r = 0.0
            endglobals
            function F takes integer a returns nothing
            endfunction
            function G takes real a returns nothing
            endfunction
            function E takes nothing returns nothing
            endfunction
            function R takes nothing returns nothing
                return
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
            // Empty `then` or `else` (SPEC-0005 update).
            ("if true then\nelse\nset x = 1\nendif", 7 + 3),
            ("if false then\nelse\nset x = 1\nendif", 7 + 5),
            ("if true then\nset x = 1\nelse\nendif", 7 + 5),
            ("if false then\nset x = 1\nelse\nendif", 7 + 3),
            // Reaching `endfunction` costs 2, a bare `return` 1.
            ("call E()", 7 + 3),
            ("call R()", 7 + 2),
            // Integer to real conversion: +1 where the integer meets a real.
            ("set r = 5", 7 + 3),
            ("set r = x + x", 7 + 7),
            ("set r = r + x", 7 + 7),
            ("set r = x + r", 7 + 7),
            ("if x < r then\nendif", 7 + 6),
            ("call G(1)", 7 + 8),
        ] {
            assert_eq!(passes(extra), predicted(cost), "{extra}");
        }
    }
}

#[cfg(test)]
mod thread_tests {
    use std::cell::RefCell;

    use super::*;

    const COMMON: &str = "\
native Sleep takes real d returns nothing
native Log takes integer i returns nothing
native Spawn takes code f returns nothing
native Callback takes code f returns nothing
";

    struct Setup {
        common: Script,
        script: Script,
    }

    fn setup(source: &str) -> Setup {
        Setup {
            common: crate::parse(COMMON).unwrap(),
            script: crate::parse(source).unwrap(),
        }
    }

    /// Natives that log integers, sleep, start threads and run callbacks.
    /// Threads started by `Spawn` that go to sleep are kept in `parked`.
    fn natives(log: Rc<RefCell<Vec<i32>>>, parked: Rc<RefCell<Vec<Thread>>>) -> Natives {
        let mut natives = Natives::default();
        natives.register(
            "native Sleep takes real d returns nothing",
            Rc::new(|vm, args| {
                let Value::Real(d) = args[0] else {
                    unreachable!()
                };
                vm.sleep(d);
                Ok(None)
            }),
        );
        natives.register(
            "native Log takes integer i returns nothing",
            Rc::new(move |_, args| {
                let Value::Integer(i) = args[0] else {
                    unreachable!()
                };
                log.borrow_mut().push(i);
                Ok(None)
            }),
        );
        natives.register(
            "native Spawn takes code f returns nothing",
            Rc::new(move |vm, args| {
                let Value::Code(Some(f)) = args[0] else {
                    unreachable!()
                };
                if let (Outcome::Sleep(_), thread) = vm.start_thread(f, Vec::new())? {
                    parked.borrow_mut().push(thread);
                }
                Ok(None)
            }),
        );
        natives.register(
            "native Callback takes code f returns nothing",
            Rc::new(|vm, args| {
                let Value::Code(Some(f)) = args[0] else {
                    unreachable!()
                };
                vm.call(f, Vec::new())?;
                vm.call(f, Vec::new())?;
                Ok(None)
            }),
        );
        natives
    }

    #[test]
    fn a_thread_sleeps_and_resumes_where_it_stopped() {
        let s = setup(
            "function Main takes nothing returns integer
                local integer i = 1
                call Log(i)
                call Sleep(0.5)
                set i = i + 1
                call Log(i)
                call Sleep(1)
                return i * 10
            endfunction",
        );
        let program = crate::check::check(&[&s.common, &s.script]).unwrap();
        let log = Rc::new(RefCell::new(Vec::new()));
        let natives = natives(log.clone(), Rc::default());
        let mut vm = Vm::load(&program, &[&s.common, &s.script], &natives).unwrap();
        let main = program.function_index("Main").unwrap();
        let mut thread = vm.spawn(main, Vec::new());
        assert_eq!(
            vm.resume(&mut thread).unwrap(),
            Outcome::Sleep(0.5f32.to_bits())
        );
        assert_eq!(*log.borrow(), [1]);
        assert_eq!(
            vm.resume(&mut thread).unwrap(),
            Outcome::Sleep(1.0f32.to_bits())
        );
        assert_eq!(*log.borrow(), [1, 2]);
        assert_eq!(
            vm.resume(&mut thread).unwrap(),
            Outcome::Finished(Some(Value::Integer(20)))
        );
        assert!(thread.is_finished());
    }

    /// Verification of SPEC-0005: 10000 passes of a 19-operation loop body,
    /// then a sleep, four times, about 760000 operations in all.
    #[test]
    fn the_budget_restarts_after_each_sleep() {
        let s = setup(
            "globals
                integer g = 0
            endglobals
            function Main takes nothing returns nothing
                local integer round = 0
                local integer i
                loop
                    exitwhen round == 4
                    set i = 0
                    loop
                        exitwhen i == 10000
                        set i = i + 1
                        set g = g + 1
                    endloop
                    call Sleep(0)
                    set round = round + 1
                endloop
            endfunction",
        );
        let program = crate::check::check(&[&s.common, &s.script]).unwrap();
        let natives = natives(Rc::default(), Rc::default());
        let mut vm = Vm::load(&program, &[&s.common, &s.script], &natives).unwrap();
        let mut thread = vm.spawn(program.function_index("Main").unwrap(), Vec::new());
        let mut sleeps = 0;
        while let Outcome::Sleep(_) = vm.resume(&mut thread).unwrap() {
            sleeps += 1;
        }
        assert_eq!(sleeps, 4);
        assert_eq!(vm.global("g"), Some(&Value::Integer(40_000)));
        assert!(vm.operations > OPERATION_LIMIT * 2, "{}", vm.operations);
    }

    #[test]
    fn started_threads_sleep_on_their_own_and_callbacks_end_at_a_sleep() {
        let s = setup(
            "function Worker takes nothing returns nothing
                call Log(10)
                call Sleep(1)
                call Log(11)
            endfunction
            function Each takes nothing returns nothing
                call Log(20)
                call Sleep(1)
                call Log(21)
            endfunction
            function Main takes nothing returns nothing
                call Spawn(function Worker)
                call Log(1)
                call Callback(function Each)
                call Log(2)
            endfunction",
        );
        let program = crate::check::check(&[&s.common, &s.script]).unwrap();
        let log = Rc::new(RefCell::new(Vec::new()));
        let parked = Rc::new(RefCell::new(Vec::new()));
        let natives = natives(log.clone(), parked.clone());
        let mut vm = Vm::load(&program, &[&s.common, &s.script], &natives).unwrap();
        vm.call_by_name("Main", Vec::new()).unwrap();
        // The worker sleeps while Main goes on; each callback ends at its
        // sleep and the next one still runs (SPEC-0005).
        assert_eq!(*log.borrow(), [10, 1, 20, 20, 2]);
        let mut worker = parked.borrow_mut().pop().expect("worker sleeps");
        assert_eq!(vm.resume(&mut worker).unwrap(), Outcome::Finished(None));
        assert_eq!(*log.borrow(), [10, 1, 20, 20, 2, 11]);
    }

    /// An error in a started thread ends that thread; the caller decides
    /// what to do, as ExecuteFunc continues (SPEC-0005).
    #[test]
    fn errors_end_only_the_started_thread() {
        let s = setup(
            "function Bad takes nothing returns nothing
                call Log(1 / 0)
            endfunction
            function Main takes nothing returns nothing
                call Log(1)
            endfunction",
        );
        let program = crate::check::check(&[&s.common, &s.script]).unwrap();
        let log = Rc::new(RefCell::new(Vec::new()));
        let natives = natives(log.clone(), Rc::default());
        let mut vm = Vm::load(&program, &[&s.common, &s.script], &natives).unwrap();
        let bad = program.function_index("Bad").unwrap();
        let err = vm.start_thread(bad, Vec::new()).unwrap_err();
        assert_eq!(err.message, "division by zero");
        assert_eq!(err.function, "Bad");
        vm.call_by_name("Main", Vec::new()).unwrap();
        assert_eq!(*log.borrow(), [1]);
    }
}
