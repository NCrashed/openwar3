//! Compilation of checked scripts to the interpreter's instructions.
//!
//! Names are resolved here: locals and parameters to slots, globals and
//! functions to indexes. Every instruction charges a fixed or a
//! value-dependent number of operations, placed so that each construct
//! costs what SPEC-0005 says on every path through it.

use std::collections::HashMap;
use std::rc::Rc;

use crate::Position;
use crate::ast::*;
use crate::check::{Program, Type};
use crate::semantics::{self, Fault, Value};

pub type Label = usize;

#[derive(Clone, Debug)]
pub enum Op {
    /// A literal or `function f`. Cost 1.
    Push(Value),
    /// Cost 1.
    LoadLocal(usize),
    /// Cost 1.
    LoadGlobal(usize),
    /// Pops the index. Cost 1.
    LoadLocalIndexed(usize),
    /// Pops the index. Cost 1.
    LoadGlobalIndexed(usize),
    /// Cost 1.
    Unary(UnaryOperator),
    /// Cost 3, plus 1 when an integer operand meets a real one.
    Binary(BinaryOperator),
    /// Pops the left operand of `and` (`and_` true) or `or`. When it
    /// decides the result: cost 3, pushes it and jumps to `end`. Otherwise
    /// cost 2, and the right operand that follows gives the result.
    ShortCircuit { and_: bool, end: Label },
    /// Converts the top value for storage as `Type`. Cost 1 for an integer
    /// becoming a real, otherwise 0.
    Coerce(Type),
    /// Pops the value. Cost 1.
    StoreLocal(usize),
    /// Pops the value. Cost 1.
    StoreGlobal(usize),
    /// Pops the value, then the index. Cost 3.
    StoreLocalIndexed(usize),
    /// Pops the value, then the index. Cost 3.
    StoreGlobalIndexed(usize),
    /// Pushing an argument. Cost 1.
    Arg,
    /// Pops the arguments and calls. Cost 1, plus the callee's own costs;
    /// a script function with arguments costs 1 more when it returns.
    Call { function: usize, arguments: usize },
    /// Discards a call result. Cost 0.
    Pop,
    /// Cost `n`.
    Charge(u8),
    /// Pops a boolean and jumps if it is false. Cost 1.
    JumpIfFalse(Label),
    /// Cost `cost`.
    Jump { target: Label, cost: u8 },
    /// Pops a boolean and leaves the loop if it is true. Cost 1.
    ExitWhen(Label),
    /// Pops the return value. Cost 2.
    Return,
    /// Leaves a function without a value: cost 2 at `endfunction`, 1 for
    /// a `return` statement.
    ReturnNothing { cost: u8 },
}

/// A compiled function: instructions with their source positions, and the
/// type of every local slot (parameters first).
#[derive(Debug)]
pub struct Code {
    pub ops: Vec<Op>,
    pub positions: Vec<Position>,
    pub slots: Vec<(Type, bool)>,
    /// Name of every local slot, for error messages.
    pub slot_names: Vec<String>,
    pub parameters: usize,
}

pub struct Compiler<'p> {
    program: &'p Program,
    globals: HashMap<&'p str, usize>,
}

#[derive(Debug)]
pub struct CompileError {
    pub position: Position,
    pub fault: Fault,
}

impl<'p> Compiler<'p> {
    pub fn new(program: &'p Program) -> Self {
        let globals = program
            .globals
            .iter()
            .enumerate()
            .map(|(i, g)| (g.name.as_str(), i))
            .collect();
        Compiler { program, globals }
    }

    pub fn global_index(&self, name: &str) -> Option<usize> {
        self.globals.get(name).copied()
    }

    /// Compiles one expression evaluated outside any function, such as a
    /// global initialiser.
    pub fn compile_expression(&self, expression: &Expression) -> Result<Code, CompileError> {
        let no_locals = HashMap::new();
        let mut f = FunctionCompiler::new(self, &no_locals);
        f.expression(expression)?;
        Ok(f.finish(Vec::new(), Vec::new(), 0))
    }

    pub fn compile_function(&self, function: &Function) -> Result<Rc<Code>, CompileError> {
        let info = self
            .program
            .function(&function.signature.name)
            .expect("checked");
        let mut slots = Vec::new();
        let mut names: HashMap<&str, usize> = HashMap::new();
        for (parameter, &t) in function.signature.parameters.iter().zip(&info.parameters) {
            names.insert(&parameter.name, slots.len());
            slots.push((t, false));
        }
        for local in &function.locals {
            let t = self.program.type_named(&local.type_name).expect("checked");
            // A repeated local of the same type reuses its slot.
            if !names.contains_key(local.name.as_str()) {
                names.insert(&local.name, slots.len());
                slots.push((t, local.array));
            }
        }
        let mut f = FunctionCompiler::new(self, &names);
        f.returns = info.returns;
        f.slot_types = slots.clone();
        for local in &function.locals {
            f.position = local.position;
            // One operation per declaration, c(e) + 1 for an initial value.
            f.emit(Op::Charge(1));
            if let Some(initial) = &local.initial {
                let (t, _) = slots[names[local.name.as_str()]];
                f.expression(initial)?;
                f.emit(Op::Coerce(t));
                f.emit(Op::StoreLocal(names[local.name.as_str()]));
            }
        }
        f.block(&function.body)?;
        f.position = function.position;
        f.emit(Op::ReturnNothing { cost: 2 });
        let slot_names = {
            let mut by_slot: Vec<(usize, String)> =
                names.iter().map(|(n, &s)| (s, (*n).to_owned())).collect();
            by_slot.sort();
            by_slot.into_iter().map(|(_, n)| n).collect()
        };
        Ok(Rc::new(f.finish(
            slots,
            slot_names,
            function.signature.parameters.len(),
        )))
    }
}

struct FunctionCompiler<'c, 'p> {
    compiler: &'c Compiler<'p>,
    locals: &'c HashMap<&'c str, usize>,
    ops: Vec<Op>,
    positions: Vec<Position>,
    position: Position,
    returns: Option<Type>,
    /// Type and array flag of every local slot.
    slot_types: Vec<(Type, bool)>,
    /// Exit labels to patch for each enclosing loop.
    loop_exits: Vec<Vec<usize>>,
}

/// Placeholder target, patched once the label is known.
const UNPATCHED: Label = usize::MAX;

impl<'c, 'p> FunctionCompiler<'c, 'p> {
    fn new(compiler: &'c Compiler<'p>, locals: &'c HashMap<&'c str, usize>) -> Self {
        FunctionCompiler {
            compiler,
            locals,
            ops: Vec::new(),
            positions: Vec::new(),
            position: Position { line: 0, column: 0 },
            returns: None,
            slot_types: Vec::new(),
            loop_exits: Vec::new(),
        }
    }

    fn finish(self, slots: Vec<(Type, bool)>, slot_names: Vec<String>, parameters: usize) -> Code {
        Code {
            ops: self.ops,
            positions: self.positions,
            slots,
            slot_names,
            parameters,
        }
    }

    fn emit(&mut self, op: Op) -> usize {
        self.ops.push(op);
        self.positions.push(self.position);
        self.ops.len() - 1
    }

    fn here(&self) -> Label {
        self.ops.len()
    }

    fn patch(&mut self, at: usize, target: Label) {
        match &mut self.ops[at] {
            Op::JumpIfFalse(t) | Op::ExitWhen(t) | Op::Jump { target: t, .. } => *t = target,
            Op::ShortCircuit { end, .. } => *end = target,
            other => unreachable!("not a jump: {other:?}"),
        }
    }

    fn global(&self, name: &str) -> usize {
        self.compiler.global_index(name).expect("checked")
    }

    fn variable_type(&self, name: &str) -> Type {
        match self.locals.get(name) {
            Some(&slot) => self.slot_types[slot].0,
            None => {
                self.compiler
                    .program
                    .global(name)
                    .expect("checked")
                    .value_type
            }
        }
    }

    fn block(&mut self, statements: &[Statement]) -> Result<(), CompileError> {
        statements.iter().try_for_each(|s| self.statement(s))
    }

    fn statement(&mut self, statement: &Statement) -> Result<(), CompileError> {
        self.position = statement.position;
        match &statement.kind {
            StatementKind::Set { name, index, value } => {
                if let Some(index) = index {
                    self.expression(index)?;
                }
                self.expression(value)?;
                self.position = statement.position;
                let local = self.locals.get(name.as_str()).copied();
                let t = self.variable_type(name);
                self.emit(Op::Coerce(t));
                let op = match (local, index.is_some()) {
                    (Some(slot), false) => Op::StoreLocal(slot),
                    (Some(slot), true) => Op::StoreLocalIndexed(slot),
                    (None, false) => Op::StoreGlobal(self.global(name)),
                    (None, true) => Op::StoreGlobalIndexed(self.global(name)),
                };
                self.emit(op);
            }
            StatementKind::Call(call) => {
                let returns = self.call(call)?;
                if returns {
                    self.emit(Op::Pop);
                }
            }
            StatementKind::If {
                branches,
                otherwise,
            } => self.if_chain(branches, otherwise.as_deref(), false)?,
            StatementKind::Loop(body) => {
                // 1 on entry, then c(S) + 1 per pass.
                self.emit(Op::Charge(1));
                let start = self.here();
                self.loop_exits.push(Vec::new());
                self.block(body)?;
                self.position = statement.position;
                self.emit(Op::Jump {
                    target: start,
                    cost: 1,
                });
                let end = self.here();
                for exit in self.loop_exits.pop().expect("pushed") {
                    self.patch(exit, end);
                }
            }
            StatementKind::ExitWhen(condition) => {
                self.expression(condition)?;
                self.position = statement.position;
                let at = self.emit(Op::ExitWhen(UNPATCHED));
                self.loop_exits.last_mut().expect("checked").push(at);
            }
            StatementKind::Return(value) => {
                if let Some(value) = value {
                    self.expression(value)?;
                    self.position = statement.position;
                    self.emit(Op::Coerce(self.returns.expect("checked")));
                    self.emit(Op::Return);
                } else {
                    self.emit(Op::ReturnNothing { cost: 1 });
                }
            }
        }
        Ok(())
    }

    /// `if` with the costs of SPEC-0005. An `elseif` chain is an `if` whose
    /// `else` holds the rest of the chain; the last `elseif` (`nested`)
    /// acts as an `if` with an empty `else`.
    fn if_chain(
        &mut self,
        branches: &[(Expression, Vec<Statement>)],
        otherwise: Option<&[Statement]>,
        nested: bool,
    ) -> Result<(), CompileError> {
        let ((condition, then), rest) = branches.split_first().expect("at least one branch");
        let has_else = !rest.is_empty() || otherwise.is_some() || nested;
        let else_empty = rest.is_empty() && otherwise.is_none_or(<[Statement]>::is_empty);
        self.expression(condition)?;
        if (!has_else && then.is_empty()) || (has_else && then.is_empty() && else_empty) {
            // Only the condition is paid for.
            self.emit(Op::Pop);
            return Ok(());
        }
        let jump = self.emit(Op::JumpIfFalse(UNPATCHED));
        self.block(then)?;
        if !has_else {
            // c(c) + 1 when false, c(c) + c(S) + 2 when true.
            self.emit(Op::Charge(1));
            let end = self.here();
            self.patch(jump, end);
            return Ok(());
        }
        // c(c) + c(branch) + 2 for either branch.
        let skip = self.emit(Op::Jump {
            target: UNPATCHED,
            cost: 1,
        });
        let else_start = self.here();
        self.patch(jump, else_start);
        if !rest.is_empty() {
            self.if_chain(rest, otherwise, true)?;
        } else {
            self.block(otherwise.unwrap_or(&[]))?;
        }
        self.emit(Op::Charge(1));
        let end = self.here();
        self.patch(skip, end);
        Ok(())
    }

    /// Compiles a call and returns whether it leaves a value.
    fn call(&mut self, call: &Call) -> Result<bool, CompileError> {
        let position = self.position;
        let index = self
            .compiler
            .program
            .function_index(&call.name)
            .expect("checked");
        let info = &self.compiler.program.functions[index];
        let (parameters, returns) = (info.parameters.clone(), info.returns.is_some());
        // (c(a) + 1) per argument, then 1 for the call.
        for (argument, t) in call.arguments.iter().zip(parameters) {
            self.expression(argument)?;
            self.position = position;
            self.emit(Op::Coerce(t));
            self.emit(Op::Arg);
        }
        self.emit(Op::Call {
            function: index,
            arguments: call.arguments.len(),
        });
        Ok(returns)
    }

    fn expression(&mut self, e: &Expression) -> Result<(), CompileError> {
        let fault = |fault| CompileError {
            position: e.position,
            fault,
        };
        self.position = e.position;
        match &e.kind {
            ExpressionKind::Integer(text) => {
                let value = semantics::integer_literal(text).map_err(fault)?;
                self.emit(Op::Push(Value::Integer(value)));
            }
            ExpressionKind::RawCode(text) => {
                let value = semantics::raw_code_literal(text).map_err(fault)?;
                self.emit(Op::Push(Value::Integer(value)));
            }
            ExpressionKind::Real(text) => {
                self.emit(Op::Push(Value::Real(semantics::real_literal(text))));
            }
            ExpressionKind::String(text) => {
                self.emit(Op::Push(Value::string(&semantics::decode_escapes(text))));
            }
            ExpressionKind::Boolean(b) => {
                self.emit(Op::Push(Value::Boolean(*b)));
            }
            ExpressionKind::Null => {
                self.emit(Op::Push(Value::Null));
            }
            ExpressionKind::FunctionReference(name) => {
                let index = self.compiler.program.function_index(name).expect("checked");
                self.emit(Op::Push(Value::Code(Some(index))));
            }
            ExpressionKind::Variable(name) => {
                let op = match self.locals.get(name.as_str()) {
                    Some(&slot) => Op::LoadLocal(slot),
                    None => Op::LoadGlobal(self.global(name)),
                };
                self.emit(op);
            }
            ExpressionKind::Index { name, index } => {
                self.expression(index)?;
                self.position = e.position;
                let op = match self.locals.get(name.as_str()) {
                    Some(&slot) => Op::LoadLocalIndexed(slot),
                    None => Op::LoadGlobalIndexed(self.global(name)),
                };
                self.emit(op);
            }
            ExpressionKind::Call(call) => {
                self.call(call)?;
            }
            ExpressionKind::Parenthesized(inner) => self.expression(inner)?,
            ExpressionKind::Unary { operator, operand } => {
                self.expression(operand)?;
                self.position = e.position;
                self.emit(Op::Unary(*operator));
            }
            ExpressionKind::Binary {
                operator: operator @ (BinaryOperator::And | BinaryOperator::Or),
                left,
                right,
            } => {
                self.expression(left)?;
                self.position = e.position;
                let jump = self.emit(Op::ShortCircuit {
                    and_: *operator == BinaryOperator::And,
                    end: UNPATCHED,
                });
                self.expression(right)?;
                let end = self.here();
                self.patch(jump, end);
            }
            ExpressionKind::Binary {
                operator,
                left,
                right,
            } => {
                self.expression(left)?;
                self.expression(right)?;
                self.position = e.position;
                self.emit(Op::Binary(*operator));
            }
        }
        Ok(())
    }
}
