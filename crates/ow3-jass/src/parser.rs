//! Recursive descent parser from tokens to the syntax tree.
//!
//! Operator precedence, lowest first: `or`, `and`, `not`, comparisons,
//! `+ -`, `* /`, unary `+ -`. Binary operators group to the left. The
//! precedence is not confirmed against the game yet (see specs/QUESTIONS.md).

use crate::ast::*;
use crate::lexer::{Keyword, Symbol, Token, TokenKind};
use crate::{Error, Position, Result};

pub fn parse_tokens(tokens: Vec<Token>) -> Result<Script> {
    let end = tokens
        .last()
        .map_or(Position { line: 1, column: 1 }, |t| t.position);
    let mut parser = Parser {
        tokens,
        index: 0,
        end,
    };
    parser.script()
}

struct Parser {
    tokens: Vec<Token>,
    index: usize,
    /// Reported for errors at the end of the input.
    end: Position,
}

impl Parser {
    fn peek(&self) -> Option<&TokenKind> {
        self.tokens.get(self.index).map(|t| &t.kind)
    }

    fn peek_at(&self, offset: usize) -> Option<&TokenKind> {
        self.tokens.get(self.index + offset).map(|t| &t.kind)
    }

    fn position(&self) -> Position {
        self.tokens.get(self.index).map_or(self.end, |t| t.position)
    }

    fn advance(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.index).cloned();
        self.index += 1;
        token
    }

    fn error<T>(&self, message: impl Into<String>) -> Result<T> {
        Err(Error {
            position: self.position(),
            message: message.into(),
        })
    }

    fn found(&self) -> String {
        match self.peek() {
            None => "end of file".to_owned(),
            Some(TokenKind::Newline) => "end of line".to_owned(),
            Some(TokenKind::Identifier(name)) => format!("'{name}'"),
            Some(TokenKind::Keyword(k)) => format!("'{}'", k.text()),
            Some(TokenKind::Symbol(s)) => format!("'{}'", s.text()),
            Some(TokenKind::Integer(t) | TokenKind::Real(t)) => format!("number {t}"),
            Some(TokenKind::RawCode(t)) => format!("raw code '{t}'"),
            Some(TokenKind::String(_)) => "string".to_owned(),
        }
    }

    fn at_keyword(&self, keyword: Keyword) -> bool {
        self.peek() == Some(&TokenKind::Keyword(keyword))
    }

    fn at_symbol(&self, symbol: Symbol) -> bool {
        self.peek() == Some(&TokenKind::Symbol(symbol))
    }

    fn eat_keyword(&mut self, keyword: Keyword) -> bool {
        let found = self.at_keyword(keyword);
        if found {
            self.index += 1;
        }
        found
    }

    fn eat_symbol(&mut self, symbol: Symbol) -> bool {
        let found = self.at_symbol(symbol);
        if found {
            self.index += 1;
        }
        found
    }

    fn expect_keyword(&mut self, keyword: Keyword) -> Result<()> {
        if self.eat_keyword(keyword) {
            Ok(())
        } else {
            self.error(format!(
                "expected '{}', found {}",
                keyword.text(),
                self.found()
            ))
        }
    }

    fn expect_symbol(&mut self, symbol: Symbol) -> Result<()> {
        if self.eat_symbol(symbol) {
            Ok(())
        } else {
            self.error(format!(
                "expected '{}', found {}",
                symbol.text(),
                self.found()
            ))
        }
    }

    fn skip_newlines(&mut self) {
        while self.peek() == Some(&TokenKind::Newline) {
            self.index += 1;
        }
    }

    /// End of a line: a line break or the end of the input.
    fn expect_line_end(&mut self) -> Result<()> {
        match self.peek() {
            None => Ok(()),
            Some(TokenKind::Newline) => {
                self.skip_newlines();
                Ok(())
            }
            _ => self.error(format!("expected end of line, found {}", self.found())),
        }
    }

    fn name(&mut self) -> Result<String> {
        match self.peek() {
            Some(TokenKind::Identifier(name)) => {
                let name = name.clone();
                self.index += 1;
                Ok(name)
            }
            _ => self.error(format!("expected a name, found {}", self.found())),
        }
    }

    fn type_name(&mut self) -> Result<String> {
        let builtin = match self.peek() {
            Some(TokenKind::Keyword(
                k @ (Keyword::Integer
                | Keyword::Real
                | Keyword::Boolean
                | Keyword::String
                | Keyword::Handle
                | Keyword::Code),
            )) => Some(k.text()),
            _ => None,
        };
        match builtin {
            Some(text) => {
                self.index += 1;
                Ok(text.to_owned())
            }
            None => self
                .name()
                .or_else(|_| self.error(format!("expected a type, found {}", self.found()))),
        }
    }

    fn script(&mut self) -> Result<Script> {
        let mut declarations = Vec::new();
        let mut functions = Vec::new();
        self.skip_newlines();
        while self.peek().is_some() {
            let constant = self.at_keyword(Keyword::Constant);
            let next = if constant {
                self.peek_at(1)
            } else {
                self.peek()
            };
            match next {
                Some(TokenKind::Keyword(Keyword::Function)) => functions.push(self.function()?),
                Some(TokenKind::Keyword(
                    keyword @ (Keyword::Type | Keyword::Globals | Keyword::Native),
                )) if !functions.is_empty() => {
                    let keyword = keyword.text();
                    return self.error(format!(
                        "'{keyword}' declaration after the first function; declarations must come first"
                    ));
                }
                Some(TokenKind::Keyword(Keyword::Native)) => {
                    declarations.push(Declaration::Native(self.native()?));
                }
                Some(TokenKind::Keyword(Keyword::Type)) if !constant => {
                    declarations.push(Declaration::Type(self.type_declaration()?));
                }
                Some(TokenKind::Keyword(Keyword::Globals)) if !constant => {
                    declarations.push(Declaration::Globals(self.globals()?));
                }
                _ => {
                    return self.error(format!(
                        "expected a declaration or function, found {}",
                        self.found()
                    ));
                }
            }
        }
        Ok(Script {
            declarations,
            functions,
        })
    }

    fn type_declaration(&mut self) -> Result<TypeDeclaration> {
        let position = self.position();
        self.expect_keyword(Keyword::Type)?;
        let name = self.name()?;
        self.expect_keyword(Keyword::Extends)?;
        let parent = self.type_name()?;
        self.expect_line_end()?;
        Ok(TypeDeclaration {
            name,
            parent,
            position,
        })
    }

    fn globals(&mut self) -> Result<Vec<Global>> {
        self.expect_keyword(Keyword::Globals)?;
        self.expect_line_end()?;
        let mut globals = Vec::new();
        while !self.eat_keyword(Keyword::EndGlobals) {
            if self.peek().is_none() {
                return self.error("expected 'endglobals', found end of file");
            }
            let constant = self.eat_keyword(Keyword::Constant);
            let variable = self.variable()?;
            if constant && (variable.array || variable.initial.is_none()) {
                return Err(Error {
                    position: variable.position,
                    message: "a constant global needs an initial value and cannot be an array"
                        .into(),
                });
            }
            self.expect_line_end()?;
            globals.push(Global { constant, variable });
        }
        self.expect_line_end()?;
        Ok(globals)
    }

    /// `type name [= value]` or `type array name`.
    fn variable(&mut self) -> Result<Variable> {
        let position = self.position();
        let type_name = self.type_name()?;
        let array = self.eat_keyword(Keyword::Array);
        let name = self.name()?;
        let initial = if !array && self.eat_symbol(Symbol::Assign) {
            Some(self.expression()?)
        } else {
            None
        };
        Ok(Variable {
            type_name,
            name,
            array,
            initial,
            position,
        })
    }

    fn native(&mut self) -> Result<Native> {
        let position = self.position();
        let constant = self.eat_keyword(Keyword::Constant);
        self.expect_keyword(Keyword::Native)?;
        let signature = self.signature()?;
        self.expect_line_end()?;
        Ok(Native {
            constant,
            signature,
            position,
        })
    }

    fn signature(&mut self) -> Result<Signature> {
        let name = self.name()?;
        self.expect_keyword(Keyword::Takes)?;
        let mut parameters = Vec::new();
        if !self.eat_keyword(Keyword::Nothing) {
            loop {
                let type_name = self.type_name()?;
                let name = self.name()?;
                parameters.push(Parameter { type_name, name });
                if !self.eat_symbol(Symbol::Comma) {
                    break;
                }
            }
        }
        self.expect_keyword(Keyword::Returns)?;
        let returns = if self.eat_keyword(Keyword::Nothing) {
            None
        } else {
            Some(self.type_name()?)
        };
        Ok(Signature {
            name,
            parameters,
            returns,
        })
    }

    fn function(&mut self) -> Result<Function> {
        let position = self.position();
        let constant = self.eat_keyword(Keyword::Constant);
        self.expect_keyword(Keyword::Function)?;
        let signature = self.signature()?;
        self.expect_line_end()?;
        let mut locals = Vec::new();
        while self.eat_keyword(Keyword::Local) {
            locals.push(self.variable()?);
            self.expect_line_end()?;
        }
        let body = self.statements(&[Keyword::EndFunction])?;
        self.expect_keyword(Keyword::EndFunction)?;
        self.expect_line_end()?;
        Ok(Function {
            constant,
            signature,
            locals,
            body,
            position,
        })
    }

    /// Statements up to, not including, one of the `terminators`.
    fn statements(&mut self, terminators: &[Keyword]) -> Result<Vec<Statement>> {
        let mut statements = Vec::new();
        loop {
            match self.peek() {
                Some(TokenKind::Keyword(k)) if terminators.contains(k) => return Ok(statements),
                Some(TokenKind::Keyword(Keyword::Local)) => {
                    return self.error("'local' declarations must come before the first statement");
                }
                None => {
                    let expected: Vec<_> = terminators
                        .iter()
                        .map(|k| format!("'{}'", k.text()))
                        .collect();
                    return self.error(format!(
                        "expected {}, found end of file",
                        expected.join(" or ")
                    ));
                }
                _ => {
                    statements.push(self.statement()?);
                    self.expect_line_end()?;
                }
            }
        }
    }

    fn statement(&mut self) -> Result<Statement> {
        let position = self.position();
        let debug = self.eat_keyword(Keyword::Debug);
        let kind = match self.advance().map(|t| t.kind) {
            Some(TokenKind::Keyword(Keyword::Set)) => {
                let name = self.name()?;
                let index = if self.eat_symbol(Symbol::LeftBracket) {
                    let index = self.expression()?;
                    self.expect_symbol(Symbol::RightBracket)?;
                    Some(index)
                } else {
                    None
                };
                self.expect_symbol(Symbol::Assign)?;
                StatementKind::Set {
                    name,
                    index,
                    value: self.expression()?,
                }
            }
            Some(TokenKind::Keyword(Keyword::Call)) => {
                let name = self.name()?;
                StatementKind::Call(self.call_arguments(name)?)
            }
            Some(TokenKind::Keyword(Keyword::If)) => self.if_statement()?,
            Some(TokenKind::Keyword(Keyword::Loop)) => {
                self.expect_line_end()?;
                let body = self.statements(&[Keyword::EndLoop])?;
                self.expect_keyword(Keyword::EndLoop)?;
                StatementKind::Loop(body)
            }
            Some(TokenKind::Keyword(Keyword::ExitWhen)) if !debug => {
                StatementKind::ExitWhen(self.expression()?)
            }
            Some(TokenKind::Keyword(Keyword::Return)) if !debug => {
                let value = match self.peek() {
                    None | Some(TokenKind::Newline) => None,
                    _ => Some(self.expression()?),
                };
                StatementKind::Return(value)
            }
            _ => {
                self.index -= 1;
                let expected = if debug {
                    "'set', 'call', 'if' or 'loop' after 'debug'"
                } else {
                    "a statement"
                };
                return self.error(format!("expected {expected}, found {}", self.found()));
            }
        };
        Ok(Statement {
            kind,
            debug,
            position,
        })
    }

    /// After `if`: condition, branches and `endif`.
    fn if_statement(&mut self) -> Result<StatementKind> {
        let mut branches = Vec::new();
        let mut otherwise = None;
        loop {
            let condition = self.expression()?;
            self.expect_keyword(Keyword::Then)?;
            self.expect_line_end()?;
            let body = self.statements(&[Keyword::ElseIf, Keyword::Else, Keyword::EndIf])?;
            branches.push((condition, body));
            if self.eat_keyword(Keyword::ElseIf) {
                continue;
            }
            if self.eat_keyword(Keyword::Else) {
                self.expect_line_end()?;
                otherwise = Some(self.statements(&[Keyword::EndIf])?);
            }
            self.expect_keyword(Keyword::EndIf)?;
            return Ok(StatementKind::If {
                branches,
                otherwise,
            });
        }
    }

    fn call_arguments(&mut self, name: String) -> Result<Call> {
        self.expect_symbol(Symbol::LeftParen)?;
        let mut arguments = Vec::new();
        if !self.eat_symbol(Symbol::RightParen) {
            loop {
                arguments.push(self.expression()?);
                if self.eat_symbol(Symbol::RightParen) {
                    break;
                }
                self.expect_symbol(Symbol::Comma)?;
            }
        }
        Ok(Call { name, arguments })
    }

    fn expression(&mut self) -> Result<Expression> {
        self.binary_level(0)
    }

    /// Binary operators of precedence `level` and above.
    fn binary_level(&mut self, level: usize) -> Result<Expression> {
        let operators: &[(Option<Keyword>, Option<Symbol>, BinaryOperator)] = match level {
            0 => &[(Some(Keyword::Or), None, BinaryOperator::Or)],
            1 => &[(Some(Keyword::And), None, BinaryOperator::And)],
            2 => return self.not_level(),
            3 => &[
                (None, Some(Symbol::Equal), BinaryOperator::Equal),
                (None, Some(Symbol::NotEqual), BinaryOperator::NotEqual),
                (None, Some(Symbol::Less), BinaryOperator::Less),
                (None, Some(Symbol::LessEqual), BinaryOperator::LessEqual),
                (None, Some(Symbol::Greater), BinaryOperator::Greater),
                (
                    None,
                    Some(Symbol::GreaterEqual),
                    BinaryOperator::GreaterEqual,
                ),
            ],
            4 => &[
                (None, Some(Symbol::Plus), BinaryOperator::Add),
                (None, Some(Symbol::Minus), BinaryOperator::Subtract),
            ],
            5 => &[
                (None, Some(Symbol::Star), BinaryOperator::Multiply),
                (None, Some(Symbol::Slash), BinaryOperator::Divide),
            ],
            _ => return self.unary(),
        };
        let mut left = self.binary_level(level + 1)?;
        'outer: loop {
            for &(keyword, symbol, operator) in operators {
                let matched = match (keyword, symbol) {
                    (Some(k), _) => self.at_keyword(k),
                    (_, Some(s)) => self.at_symbol(s),
                    _ => false,
                };
                if matched {
                    self.index += 1;
                    let right = self.binary_level(level + 1)?;
                    let position = left.position;
                    left = Expression {
                        kind: ExpressionKind::Binary {
                            operator,
                            left: Box::new(left),
                            right: Box::new(right),
                        },
                        position,
                    };
                    continue 'outer;
                }
            }
            return Ok(left);
        }
    }

    /// `not` binds looser than comparisons: `not a == b` is `not (a == b)`.
    fn not_level(&mut self) -> Result<Expression> {
        let position = self.position();
        if self.eat_keyword(Keyword::Not) {
            let operand = self.not_level()?;
            return Ok(Expression {
                kind: ExpressionKind::Unary {
                    operator: UnaryOperator::Not,
                    operand: Box::new(operand),
                },
                position,
            });
        }
        self.binary_level(3)
    }

    fn unary(&mut self) -> Result<Expression> {
        let position = self.position();
        let operator = if self.eat_symbol(Symbol::Minus) {
            Some(UnaryOperator::Minus)
        } else if self.eat_symbol(Symbol::Plus) {
            Some(UnaryOperator::Plus)
        } else if self.eat_keyword(Keyword::Not) {
            // `not` inside an arithmetic or comparison operand, as in `a == not b`.
            Some(UnaryOperator::Not)
        } else {
            None
        };
        match operator {
            Some(operator) => Ok(Expression {
                kind: ExpressionKind::Unary {
                    operator,
                    operand: Box::new(self.unary()?),
                },
                position,
            }),
            None => self.primary(),
        }
    }

    fn primary(&mut self) -> Result<Expression> {
        let position = self.position();
        let kind = match self.advance().map(|t| t.kind) {
            Some(TokenKind::Integer(text)) => ExpressionKind::Integer(text),
            Some(TokenKind::RawCode(text)) => ExpressionKind::RawCode(text),
            Some(TokenKind::Real(text)) => ExpressionKind::Real(text),
            Some(TokenKind::String(text)) => ExpressionKind::String(text),
            Some(TokenKind::Keyword(Keyword::True)) => ExpressionKind::Boolean(true),
            Some(TokenKind::Keyword(Keyword::False)) => ExpressionKind::Boolean(false),
            Some(TokenKind::Keyword(Keyword::Null)) => ExpressionKind::Null,
            Some(TokenKind::Keyword(Keyword::Function)) => {
                ExpressionKind::FunctionReference(self.name()?)
            }
            Some(TokenKind::Identifier(name)) => {
                if self.at_symbol(Symbol::LeftParen) {
                    ExpressionKind::Call(self.call_arguments(name)?)
                } else if self.eat_symbol(Symbol::LeftBracket) {
                    let index = self.expression()?;
                    self.expect_symbol(Symbol::RightBracket)?;
                    ExpressionKind::Index {
                        name,
                        index: Box::new(index),
                    }
                } else {
                    ExpressionKind::Variable(name)
                }
            }
            Some(TokenKind::Symbol(Symbol::LeftParen)) => {
                let inner = self.expression()?;
                self.expect_symbol(Symbol::RightParen)?;
                ExpressionKind::Parenthesized(Box::new(inner))
            }
            _ => {
                self.index -= 1;
                return self.error(format!("expected an expression, found {}", self.found()));
            }
        };
        Ok(Expression { kind, position })
    }
}
