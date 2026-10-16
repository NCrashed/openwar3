//! Syntax tree of a JASS script.
//!
//! Names are kept as written (JASS is case-sensitive) and literals keep
//! their source text.

use crate::Position;

#[derive(Clone, Debug, PartialEq)]
pub struct Script {
    /// Type, global and native declarations, in source order. They all
    /// precede the first function.
    pub declarations: Vec<Declaration>,
    pub functions: Vec<Function>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Declaration {
    Type(TypeDeclaration),
    /// One `globals ... endglobals` block.
    Globals(Vec<Global>),
    Native(Native),
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypeDeclaration {
    pub name: String,
    pub parent: String,
    pub position: Position,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Global {
    pub constant: bool,
    pub variable: Variable,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Variable {
    pub type_name: String,
    pub name: String,
    pub array: bool,
    pub initial: Option<Expression>,
    pub position: Position,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Native {
    pub constant: bool,
    pub signature: Signature,
    pub position: Position,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Signature {
    pub name: String,
    pub parameters: Vec<Parameter>,
    /// `None` for `returns nothing`.
    pub returns: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Parameter {
    pub type_name: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Function {
    pub constant: bool,
    pub signature: Signature,
    pub locals: Vec<Variable>,
    pub body: Vec<Statement>,
    pub position: Position,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Statement {
    pub kind: StatementKind,
    /// Prefixed with `debug`.
    pub debug: bool,
    pub position: Position,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StatementKind {
    Set {
        name: String,
        index: Option<Expression>,
        value: Expression,
    },
    Call(Call),
    If {
        /// The `if` branch followed by every `elseif` branch.
        branches: Vec<(Expression, Vec<Statement>)>,
        otherwise: Option<Vec<Statement>>,
    },
    Loop(Vec<Statement>),
    ExitWhen(Expression),
    Return(Option<Expression>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub name: String,
    pub arguments: Vec<Expression>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Expression {
    pub kind: ExpressionKind,
    pub position: Position,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExpressionKind {
    /// Decimal, octal or hexadecimal integer as written.
    Integer(String),
    /// Characters between the quotes of a raw code such as `'hfoo'`.
    RawCode(String),
    Real(String),
    /// Characters between the double quotes, escapes not decoded.
    String(String),
    Boolean(bool),
    Null,
    /// `function name`
    FunctionReference(String),
    Variable(String),
    Index {
        name: String,
        index: Box<Expression>,
    },
    Call(Call),
    Unary {
        operator: UnaryOperator,
        operand: Box<Expression>,
    },
    Binary {
        operator: BinaryOperator,
        left: Box<Expression>,
        right: Box<Expression>,
    },
    Parenthesized(Box<Expression>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOperator {
    Plus,
    Minus,
    Not,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOperator {
    Or,
    And,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Add,
    Subtract,
    Multiply,
    Divide,
}
