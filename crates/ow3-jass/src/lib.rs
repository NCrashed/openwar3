//! Parser for JASS, the scripting language of Warcraft III maps (`war3map.j`
//! with `common.j` and `Blizzard.j`) and AI scripts (`*.ai` with
//! `common.ai`).
//!
//! The syntax follows the public descriptions of the language. Parsing
//! checks syntax only; names, types and the meaning of literals are left to
//! later stages.

pub mod ast;
pub mod bytecode;
pub mod check;
pub mod lexer;
mod parser;
pub mod semantics;
pub mod vm;

use std::fmt;

/// 1-based line and column of a character in the source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Position {
    pub line: u32,
    pub column: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub position: Position,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: {}",
            self.position.line, self.position.column, self.message
        )
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// Parses a whole script.
pub fn parse(source: &str) -> Result<ast::Script> {
    parser::parse_tokens(lexer::tokenize(source)?)
}

#[cfg(test)]
mod tests;
