//! Name resolution and type checking of a set of scripts that run together,
//! such as `common.j`, `Blizzard.j` and `war3map.j`.
//!
//! Scripts are checked in the order given, and each sees the names declared
//! by the scripts before it. Within a script a function may only call
//! functions declared above it (or itself).
//!
//! Rules that are not confirmed against the game yet are listed in
//! specs/QUESTIONS.md.

use std::collections::HashMap;
use std::fmt;

use crate::Position;
use crate::ast::*;

/// A value type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    Integer,
    Real,
    Boolean,
    String,
    Code,
    /// A handle type, by index into [`Program::handle_types`]. Index 0 is
    /// `handle` itself.
    Handle(u32),
    /// The type of the `null` literal.
    Null,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandleType {
    pub name: String,
    /// `None` only for `handle`.
    pub parent: Option<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GlobalInfo {
    pub name: String,
    pub value_type: Type,
    pub array: bool,
    pub constant: bool,
    /// Index of the script in the list given to [`check`].
    pub script: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FunctionInfo {
    pub name: String,
    pub parameters: Vec<Type>,
    pub returns: Option<Type>,
    pub native: bool,
    pub constant: bool,
    pub script: usize,
}

/// Names and types of the checked scripts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Program {
    pub handle_types: Vec<HandleType>,
    pub globals: Vec<GlobalInfo>,
    /// Natives and functions in declaration order.
    pub functions: Vec<FunctionInfo>,
    type_names: HashMap<String, Type>,
    global_names: HashMap<String, usize>,
    function_names: HashMap<String, usize>,
}

impl Program {
    pub fn type_named(&self, name: &str) -> Option<Type> {
        self.type_names.get(name).copied()
    }

    pub fn global(&self, name: &str) -> Option<&GlobalInfo> {
        self.global_names.get(name).map(|&i| &self.globals[i])
    }

    pub fn function(&self, name: &str) -> Option<&FunctionInfo> {
        self.function_names.get(name).map(|&i| &self.functions[i])
    }

    pub fn function_index(&self, name: &str) -> Option<usize> {
        self.function_names.get(name).copied()
    }

    pub fn type_name(&self, t: Type) -> &str {
        match t {
            Type::Integer => "integer",
            Type::Real => "real",
            Type::Boolean => "boolean",
            Type::String => "string",
            Type::Code => "code",
            Type::Handle(i) => &self.handle_types[i as usize].name,
            Type::Null => "null",
        }
    }

    /// Whether handle type `child` is `ancestor` or derives from it.
    pub fn derives_from(&self, child: u32, ancestor: u32) -> bool {
        let mut current = Some(child);
        while let Some(t) = current {
            if t == ancestor {
                return true;
            }
            current = self.handle_types[t as usize].parent;
        }
        false
    }

    /// Whether a value of type `from` may be stored where `to` is expected.
    pub fn assignable(&self, from: Type, to: Type) -> bool {
        match (from, to) {
            _ if from == to => true,
            (Type::Integer, Type::Real) => true,
            (Type::Null, Type::Handle(_) | Type::String | Type::Code) => true,
            (Type::Handle(child), Type::Handle(ancestor)) => self.derives_from(child, ancestor),
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub script: usize,
    pub position: Position,
    pub message: String,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "script {} {}:{}: {}",
            self.script, self.position.line, self.position.column, self.message
        )
    }
}

/// Checks `scripts` in order. Returns every problem found, or the program's
/// names and types when there is none.
pub fn check(scripts: &[&Script]) -> Result<Program, Vec<Diagnostic>> {
    let mut checker = Checker {
        program: Program::default(),
        diagnostics: Vec::new(),
        script: 0,
    };
    checker.program.handle_types.push(HandleType {
        name: "handle".into(),
        parent: None,
    });
    for (name, t) in [
        ("integer", Type::Integer),
        ("real", Type::Real),
        ("boolean", Type::Boolean),
        ("string", Type::String),
        ("code", Type::Code),
        ("handle", Type::Handle(0)),
    ] {
        checker.program.type_names.insert(name.into(), t);
    }
    for (index, script) in scripts.iter().enumerate() {
        checker.script = index;
        checker.script_declarations(script);
        for function in &script.functions {
            checker.function(function);
        }
    }
    if checker.diagnostics.is_empty() {
        Ok(checker.program)
    } else {
        Err(checker.diagnostics)
    }
}

struct Checker {
    program: Program,
    diagnostics: Vec<Diagnostic>,
    script: usize,
}

/// Names visible inside a function body besides globals.
struct Scope<'a> {
    locals: HashMap<&'a str, (Type, bool)>,
    returns: Option<Type>,
    loop_depth: usize,
    /// Position of the function's last top-level statement.
    last_statement: Option<Position>,
}

impl Checker {
    fn report(&mut self, position: Position, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            script: self.script,
            position,
            message: message.into(),
        });
    }

    fn resolve_type(&mut self, name: &str, position: Position) -> Option<Type> {
        let found = self.program.type_named(name);
        if found.is_none() {
            self.report(position, format!("unknown type '{name}'"));
        }
        found
    }

    fn script_declarations(&mut self, script: &Script) {
        for declaration in &script.declarations {
            match declaration {
                Declaration::Type(t) => self.type_declaration(t),
                Declaration::Globals(globals) => {
                    for global in globals {
                        self.global(global);
                    }
                }
                Declaration::Native(native) => {
                    self.declare_function(
                        &native.signature,
                        true,
                        native.constant,
                        native.position,
                    );
                }
            }
        }
    }

    fn type_declaration(&mut self, t: &TypeDeclaration) {
        if self.program.type_names.contains_key(&t.name) {
            self.report(t.position, format!("type '{}' is already declared", t.name));
            return;
        }
        let parent = match self.program.type_named(&t.parent) {
            Some(Type::Handle(parent)) => parent,
            Some(_) => {
                self.report(
                    t.position,
                    format!("'{}' can only extend a handle type", t.name),
                );
                return;
            }
            None => {
                self.report(t.position, format!("unknown type '{}'", t.parent));
                return;
            }
        };
        let index = self.program.handle_types.len() as u32;
        self.program.handle_types.push(HandleType {
            name: t.name.clone(),
            parent: Some(parent),
        });
        self.program
            .type_names
            .insert(t.name.clone(), Type::Handle(index));
    }

    fn global(&mut self, global: &Global) {
        let variable = &global.variable;
        let Some(value_type) = self.resolve_type(&variable.type_name, variable.position) else {
            return;
        };
        if let Some(initial) = &variable.initial {
            let scope = Scope {
                locals: HashMap::new(),
                returns: None,
                loop_depth: 0,
                last_statement: None,
            };
            self.expect_assignable(&scope, initial, value_type, "initial value");
        }
        if self.program.global_names.contains_key(&variable.name) {
            self.report(
                variable.position,
                format!("global '{}' is already declared", variable.name),
            );
            return;
        }
        // SPEC-0005: a global may not have the name of a function.
        if self.program.function(&variable.name).is_some() {
            self.report(
                variable.position,
                format!("global '{}' has the name of a function", variable.name),
            );
            return;
        }
        self.program
            .global_names
            .insert(variable.name.clone(), self.program.globals.len());
        self.program.globals.push(GlobalInfo {
            name: variable.name.clone(),
            value_type,
            array: variable.array,
            constant: global.constant,
            script: self.script,
        });
    }

    fn declare_function(
        &mut self,
        signature: &Signature,
        native: bool,
        constant: bool,
        position: Position,
    ) {
        let mut parameters = Vec::new();
        for parameter in &signature.parameters {
            if let Some(t) = self.resolve_type(&parameter.type_name, position) {
                parameters.push(t);
            }
        }
        let returns = match &signature.returns {
            Some(name) => self.resolve_type(name, position),
            None => None,
        };
        if parameters.len() != signature.parameters.len()
            || (signature.returns.is_some() && returns.is_none())
        {
            return;
        }
        if self.program.global(&signature.name).is_some() {
            self.report(
                position,
                format!("function '{}' has the name of a global", signature.name),
            );
            return;
        }
        if let Some(existing) = self.program.function(&signature.name) {
            let kind = if existing.native {
                "native"
            } else {
                "function"
            };
            self.report(
                position,
                format!("'{}' is already declared as a {kind}", signature.name),
            );
            return;
        }
        self.program
            .function_names
            .insert(signature.name.clone(), self.program.functions.len());
        self.program.functions.push(FunctionInfo {
            name: signature.name.clone(),
            parameters,
            returns,
            native,
            constant,
            script: self.script,
        });
    }

    fn function(&mut self, function: &Function) {
        // Declared first so that the body may call the function itself.
        let before = self.diagnostics.len();
        self.declare_function(
            &function.signature,
            false,
            function.constant,
            function.position,
        );
        if self.diagnostics.len() != before {
            return;
        }
        let info = self
            .program
            .function(&function.signature.name)
            .cloned()
            .expect("declared");
        let mut scope = Scope {
            locals: HashMap::new(),
            returns: info.returns,
            loop_depth: 0,
            last_statement: function.body.last().map(|s| s.position),
        };
        for (parameter, &t) in function.signature.parameters.iter().zip(&info.parameters) {
            if scope.locals.insert(&parameter.name, (t, false)).is_some() {
                self.report(
                    function.position,
                    format!("parameter '{}' is declared twice", parameter.name),
                );
            }
        }
        for local in &function.locals {
            let Some(t) = self.resolve_type(&local.type_name, local.position) else {
                continue;
            };
            if let Some(initial) = &local.initial {
                self.expect_assignable(&scope, initial, t, "initial value");
            }
            // SPEC-0005: a local may repeat the name of a parameter or an
            // earlier local, with any type; the latest declaration is used.
            scope.locals.insert(&local.name, (t, local.array));
        }
        self.statements(&mut scope, &function.body);
    }

    fn statements(&mut self, scope: &mut Scope, statements: &[Statement]) {
        for statement in statements {
            self.statement(scope, statement);
        }
    }

    fn statement(&mut self, scope: &mut Scope, statement: &Statement) {
        let position = statement.position;
        match &statement.kind {
            StatementKind::Set { name, index, value } => {
                let Some((t, array, constant)) = self.variable(scope, name, position) else {
                    return;
                };
                if constant {
                    self.report(position, format!("'{name}' is a constant"));
                }
                match (index, array) {
                    (Some(index), true) => {
                        self.expect_type(scope, index, Type::Integer, "array index")
                    }
                    (None, false) => {}
                    (Some(_), false) => self.report(position, format!("'{name}' is not an array")),
                    (None, true) => self.report(position, format!("array '{name}' needs an index")),
                }
                self.expect_assignable(scope, value, t, "assigned value");
            }
            StatementKind::Call(call) => {
                self.call(scope, call, position);
            }
            StatementKind::If {
                branches,
                otherwise,
            } => {
                for (condition, body) in branches {
                    self.expect_type(scope, condition, Type::Boolean, "condition");
                    self.statements(scope, body);
                }
                if let Some(body) = otherwise {
                    self.statements(scope, body);
                }
            }
            StatementKind::Loop(body) => {
                scope.loop_depth += 1;
                self.statements(scope, body);
                scope.loop_depth -= 1;
            }
            StatementKind::ExitWhen(condition) => {
                if scope.loop_depth == 0 {
                    self.report(position, "'exitwhen' outside a loop");
                }
                self.expect_type(scope, condition, Type::Boolean, "condition");
            }
            StatementKind::Return(value) => match (value, scope.returns) {
                (None, None) => {}
                // SPEC-0005: `null` may be returned as 0 or 0.0, except by
                // the function's last statement.
                (Some(value), Some(Type::Integer | Type::Real))
                    if value.kind == ExpressionKind::Null
                        && scope.last_statement != Some(position) => {}
                (Some(value), Some(t)) => self.expect_assignable(scope, value, t, "returned value"),
                (Some(_), None) => self.report(position, "the function returns nothing"),
                (None, Some(t)) => {
                    let name = self.program.type_name(t).to_owned();
                    self.report(position, format!("the function must return a {name}"));
                }
            },
        }
    }

    /// Type, array flag and constant flag of a variable visible here.
    fn variable(
        &mut self,
        scope: &Scope,
        name: &str,
        position: Position,
    ) -> Option<(Type, bool, bool)> {
        if let Some(&(t, array)) = scope.locals.get(name) {
            return Some((t, array, false));
        }
        if let Some(global) = self.program.global(name) {
            return Some((global.value_type, global.array, global.constant));
        }
        self.report(position, format!("unknown variable '{name}'"));
        None
    }

    /// Checks a call and returns its result type (`None` for `nothing` or an
    /// error).
    fn call(&mut self, scope: &Scope, call: &Call, position: Position) -> Option<Type> {
        let Some(function) = self.program.function(&call.name).cloned() else {
            self.report(position, format!("unknown function '{}'", call.name));
            for argument in &call.arguments {
                self.expression(scope, argument);
            }
            return None;
        };
        if function.parameters.len() != call.arguments.len() {
            self.report(
                position,
                format!(
                    "'{}' takes {} arguments, not {}",
                    call.name,
                    function.parameters.len(),
                    call.arguments.len()
                ),
            );
        }
        for (argument, &t) in call.arguments.iter().zip(&function.parameters) {
            // SPEC-0005: `null` is accepted as an integer or real argument.
            if argument.kind == ExpressionKind::Null && matches!(t, Type::Integer | Type::Real) {
                continue;
            }
            self.expect_assignable(scope, argument, t, "argument");
        }
        function.returns
    }

    fn expect_type(&mut self, scope: &Scope, expression: &Expression, t: Type, what: &str) {
        if let Some(found) = self.expression(scope, expression) {
            if found != t {
                let (found, t) = (
                    self.program.type_name(found).to_owned(),
                    self.program.type_name(t).to_owned(),
                );
                self.report(
                    expression.position,
                    format!("{what} must be a {t}, not a {found}"),
                );
            }
        }
    }

    fn expect_assignable(&mut self, scope: &Scope, expression: &Expression, t: Type, what: &str) {
        if let Some(found) = self.expression(scope, expression) {
            if !self.program.assignable(found, t) {
                let (found, t) = (
                    self.program.type_name(found).to_owned(),
                    self.program.type_name(t).to_owned(),
                );
                self.report(
                    expression.position,
                    format!("{what} must be a {t}, not a {found}"),
                );
            }
        }
    }

    /// Type of an expression, or `None` after reporting an error.
    fn expression(&mut self, scope: &Scope, expression: &Expression) -> Option<Type> {
        let position = expression.position;
        match &expression.kind {
            ExpressionKind::Integer(_) | ExpressionKind::RawCode(_) => Some(Type::Integer),
            ExpressionKind::Real(_) => Some(Type::Real),
            ExpressionKind::String(_) => Some(Type::String),
            ExpressionKind::Boolean(_) => Some(Type::Boolean),
            ExpressionKind::Null => Some(Type::Null),
            ExpressionKind::FunctionReference(name) => {
                if self.program.function(name).is_none() {
                    self.report(position, format!("unknown function '{name}'"));
                    return None;
                }
                Some(Type::Code)
            }
            ExpressionKind::Variable(name) => {
                let (t, array, _) = self.variable(scope, name, position)?;
                if array {
                    self.report(position, format!("array '{name}' needs an index"));
                    return None;
                }
                Some(t)
            }
            ExpressionKind::Index { name, index } => {
                let (t, array, _) = self.variable(scope, name, position)?;
                self.expect_type(scope, index, Type::Integer, "array index");
                if !array {
                    self.report(position, format!("'{name}' is not an array"));
                    return None;
                }
                Some(t)
            }
            ExpressionKind::Call(call) => {
                let known = self.program.function(&call.name).is_some();
                let result = self.call(scope, call, position);
                if known && result.is_none() {
                    self.report(position, format!("'{}' returns nothing", call.name));
                }
                result
            }
            ExpressionKind::Parenthesized(inner) => self.expression(scope, inner),
            ExpressionKind::Unary { operator, operand } => {
                let t = self.expression(scope, operand)?;
                match (operator, t) {
                    (UnaryOperator::Not, Type::Boolean) => Some(Type::Boolean),
                    (UnaryOperator::Plus | UnaryOperator::Minus, Type::Integer | Type::Real) => {
                        Some(t)
                    }
                    _ => {
                        let name = self.program.type_name(t).to_owned();
                        self.report(position, format!("operator cannot be applied to a {name}"));
                        None
                    }
                }
            }
            ExpressionKind::Binary {
                operator,
                left,
                right,
            } => {
                let l = self.expression(scope, left);
                let r = self.expression(scope, right);
                let (l, r) = (l?, r?);
                let result = self.binary(*operator, l, r);
                if result.is_none() {
                    let (l, r) = (
                        self.program.type_name(l).to_owned(),
                        self.program.type_name(r).to_owned(),
                    );
                    self.report(
                        position,
                        format!("operator cannot be applied to a {l} and a {r}"),
                    );
                }
                result
            }
        }
    }

    fn binary(&self, operator: BinaryOperator, l: Type, r: Type) -> Option<Type> {
        use BinaryOperator::*;
        let numeric = |t| matches!(t, Type::Integer | Type::Real);
        match operator {
            Or | And => (l == Type::Boolean && r == Type::Boolean).then_some(Type::Boolean),
            Equal | NotEqual => self.comparable(l, r).then_some(Type::Boolean),
            Less | LessEqual | Greater | GreaterEqual => {
                (numeric(l) && numeric(r)).then_some(Type::Boolean)
            }
            Add if matches!(l, Type::String | Type::Null)
                && matches!(r, Type::String | Type::Null) =>
            {
                (l == Type::String || r == Type::String).then_some(Type::String)
            }
            Add | Subtract | Multiply | Divide if numeric(l) && numeric(r) => {
                Some(if l == Type::Integer && r == Type::Integer {
                    Type::Integer
                } else {
                    Type::Real
                })
            }
            _ => None,
        }
    }

    fn comparable(&self, l: Type, r: Type) -> bool {
        let numeric = |t| matches!(t, Type::Integer | Type::Real);
        let nullable = |t| matches!(t, Type::Handle(_) | Type::String | Type::Code | Type::Null);
        l == r
            || (numeric(l) && numeric(r))
            || (matches!(l, Type::Handle(_)) && matches!(r, Type::Handle(_)))
            || ((l == Type::Null && nullable(r)) || (r == Type::Null && nullable(l)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMON: &str = "\
type agent extends handle
type widget extends agent
type unit extends widget
type timer extends agent
native KillUnit takes unit u returns nothing
native GetUnitX takes unit u returns real
native CreateTimer takes nothing returns timer
native TimerStart takes timer t, real timeout, boolean periodic, code f returns nothing
native I2S takes integer i returns string
";

    fn check_source(source: &str) -> Result<Program, Vec<String>> {
        let common = crate::parse(COMMON).unwrap();
        let script = crate::parse(source).unwrap();
        check(&[&common, &script]).map_err(|ds| ds.into_iter().map(|d| d.message).collect())
    }

    fn errors(source: &str) -> Vec<String> {
        check_source(source).expect_err("should not check")
    }

    #[test]
    fn accepts_a_well_typed_script() {
        let program = check_source(
            "globals
                constant integer LIMIT = 10
                unit array heroes
                real speed = 1
                timer t = null
                string s = \"a\" + I2S(LIMIT)
            endglobals
            function OnTimer takes nothing returns nothing
            endfunction
            function Count takes integer n returns integer
                local integer i = 0
                local widget w = heroes[0]
                loop
                    exitwhen i >= n or w == null
                    set i = i + 1
                    set speed = speed * 2 + i
                endloop
                if GetUnitX(heroes[i]) > 0.5 and not (i == LIMIT) then
                    call KillUnit(heroes[i])
                endif
                call TimerStart(CreateTimer(), 1, false, function OnTimer)
                return Count(i - 1)
            endfunction",
        )
        .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(program.global("speed").unwrap().value_type, Type::Real);
        assert_eq!(
            program.function("Count").unwrap().returns,
            Some(Type::Integer)
        );
    }

    #[test]
    fn reports_name_errors() {
        let e = errors(
            "function A takes nothing returns nothing
                call B()
                set missing = 1
            endfunction
            function B takes nothing returns nothing
            endfunction",
        );
        assert_eq!(e, ["unknown function 'B'", "unknown variable 'missing'"]);
        let e = errors("native KillUnit takes unit u returns nothing\n");
        assert_eq!(e, ["'KillUnit' is already declared as a native"]);
        let e = errors("globals\nfoo x\nendglobals\n");
        assert_eq!(e, ["unknown type 'foo'"]);
    }

    #[test]
    fn reports_type_errors() {
        let e = errors(
            "globals
                constant integer C = 1
                unit array units
            endglobals
            function F takes unit u returns integer
                local real r = 1.5
                local widget w = u
                set C = 2
                set units = u
                set r = \"text\"
                call KillUnit(w)
                call KillUnit(u, u)
                if r then
                endif
                exitwhen true
                return r
            endfunction",
        );
        assert_eq!(
            e,
            [
                "'C' is a constant",
                "array 'units' needs an index",
                "assigned value must be a real, not a string",
                "argument must be a unit, not a widget",
                "'KillUnit' takes 1 arguments, not 2",
                "condition must be a boolean, not a real",
                "'exitwhen' outside a loop",
                "returned value must be a integer, not a real",
            ]
        );
    }

    #[test]
    fn operators_need_matching_types() {
        let e = errors(
            "function F takes unit u returns nothing
                local boolean b = 1 < \"x\"
                local string s = \"a\" + 1
                local boolean c = u == 1
                local integer i = -true
            endfunction",
        );
        assert_eq!(
            e,
            [
                "operator cannot be applied to a integer and a string",
                "operator cannot be applied to a string and a integer",
                "operator cannot be applied to a unit and a integer",
                "operator cannot be applied to a boolean",
            ]
        );
    }

    /// SPEC-0005: repeated locals of any type, `null` as an integer or real
    /// argument and in a `return` that is not the last statement.
    #[test]
    fn null_and_repeated_locals() {
        check_source(
            "function F takes integer n returns integer
                local real h
                local integer h
                local real n
                if n < 0 then
                    return null
                endif
                return I2S(null) == \"0\"
            endfunction",
        )
        .unwrap_err();
        check_source(
            "function F takes integer n returns integer
                local real h
                local integer h
                local real n = 2.5
                call I2S(null)
                if n < 0 then
                    return null
                endif
                return h
            endfunction",
        )
        .unwrap_or_else(|e| panic!("{e:?}"));
        let e = errors(
            "function F takes nothing returns integer
                local integer i = 1
                local boolean b
                set i = null
                set b = null
                return null
            endfunction",
        );
        assert_eq!(
            e,
            [
                "assigned value must be a integer, not a null",
                "assigned value must be a boolean, not a null",
                "returned value must be a integer, not a null",
            ]
        );
        let source = "function G takes nothing returns nothing\n local integer x_ = 1\nendfunction";
        assert!(crate::parse(source).is_err());
    }
}
