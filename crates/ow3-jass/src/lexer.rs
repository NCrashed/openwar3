//! Splits JASS source into tokens.
//!
//! Line breaks are tokens, because JASS statements end at the end of a line.
//! Comments run from `//` to the end of the line. Literals keep their source
//! text; converting them to values is left to later stages.

use crate::{Error, Position, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Identifier(String),
    Keyword(Keyword),
    /// Decimal, octal (leading `0`) or hexadecimal (`0x`, `$`) integer, as
    /// written.
    Integer(String),
    /// Characters between the quotes of a raw code such as `'hfoo'`.
    RawCode(String),
    /// Number with a decimal point, as written.
    Real(String),
    /// Characters between the double quotes, escapes not decoded.
    String(String),
    Symbol(Symbol),
    Newline,
}

macro_rules! keywords {
    ($($variant:ident = $text:literal,)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Keyword {
            $($variant,)*
        }

        impl Keyword {
            pub fn from_text(text: &str) -> Option<Keyword> {
                match text {
                    $($text => Some(Keyword::$variant),)*
                    _ => None,
                }
            }

            pub fn text(self) -> &'static str {
                match self {
                    $(Keyword::$variant => $text,)*
                }
            }
        }
    };
}

keywords! {
    Type = "type",
    Extends = "extends",
    Globals = "globals",
    EndGlobals = "endglobals",
    Constant = "constant",
    Native = "native",
    Function = "function",
    Takes = "takes",
    Returns = "returns",
    EndFunction = "endfunction",
    Nothing = "nothing",
    Local = "local",
    Array = "array",
    Set = "set",
    Call = "call",
    If = "if",
    Then = "then",
    Else = "else",
    ElseIf = "elseif",
    EndIf = "endif",
    Loop = "loop",
    EndLoop = "endloop",
    ExitWhen = "exitwhen",
    Return = "return",
    Debug = "debug",
    And = "and",
    Or = "or",
    Not = "not",
    True = "true",
    False = "false",
    Null = "null",
    // Built-in type names. They name types only; SPEC-0001 notes that
    // `code` cannot be used as a parameter name.
    Integer = "integer",
    Real = "real",
    Boolean = "boolean",
    String = "string",
    Handle = "handle",
    Code = "code",
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Symbol {
    Assign,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Plus,
    Minus,
    Star,
    Slash,
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    Comma,
}

impl Symbol {
    pub fn text(self) -> &'static str {
        match self {
            Symbol::Assign => "=",
            Symbol::Equal => "==",
            Symbol::NotEqual => "!=",
            Symbol::Less => "<",
            Symbol::LessEqual => "<=",
            Symbol::Greater => ">",
            Symbol::GreaterEqual => ">=",
            Symbol::Plus => "+",
            Symbol::Minus => "-",
            Symbol::Star => "*",
            Symbol::Slash => "/",
            Symbol::LeftParen => "(",
            Symbol::RightParen => ")",
            Symbol::LeftBracket => "[",
            Symbol::RightBracket => "]",
            Symbol::Comma => ",",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub position: Position,
}

pub fn tokenize(source: &str) -> Result<Vec<Token>> {
    let mut lexer = Lexer {
        chars: source.chars().collect(),
        index: 0,
        line: 1,
        column: 1,
    };
    let mut tokens = Vec::new();
    while let Some(token) = lexer.next_token()? {
        tokens.push(token);
    }
    Ok(tokens)
}

struct Lexer {
    chars: Vec<char>,
    index: usize,
    line: u32,
    column: u32,
}

impl Lexer {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.index).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.index + offset).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.index += 1;
        if ch == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(ch)
    }

    fn position(&self) -> Position {
        Position {
            line: self.line,
            column: self.column,
        }
    }

    fn error(&self, position: Position, message: impl Into<String>) -> Error {
        Error {
            position,
            message: message.into(),
        }
    }

    fn take_while(&mut self, mut keep: impl FnMut(char) -> bool) -> String {
        let mut text = String::new();
        while let Some(ch) = self.peek().filter(|&c| keep(c)) {
            text.push(ch);
            self.bump();
        }
        text
    }

    fn next_token(&mut self) -> Result<Option<Token>> {
        loop {
            match self.peek() {
                Some(' ' | '\t' | '\u{FEFF}') => {
                    self.bump();
                }
                Some('/') if self.peek_at(1) == Some('/') => {
                    while self.peek().is_some_and(|c| c != '\n' && c != '\r') {
                        self.bump();
                    }
                }
                _ => break,
            }
        }
        let position = self.position();
        let Some(ch) = self.peek() else {
            return Ok(None);
        };
        let kind = match ch {
            '\r' | '\n' => {
                self.bump();
                if ch == '\r' && self.peek() == Some('\n') {
                    self.bump();
                }
                TokenKind::Newline
            }
            'a'..='z' | 'A'..='Z' => {
                let word = self.take_while(|c| c.is_ascii_alphanumeric() || c == '_');
                match Keyword::from_text(&word) {
                    Some(keyword) => TokenKind::Keyword(keyword),
                    None => TokenKind::Identifier(word),
                }
            }
            '0'..='9' | '.' | '$' => self.number(position)?,
            '"' => self.string(position)?,
            '\'' => self.raw_code(position)?,
            _ => TokenKind::Symbol(self.symbol(position)?),
        };
        Ok(Some(Token { kind, position }))
    }

    fn number(&mut self, position: Position) -> Result<TokenKind> {
        let hex_prefix = match (self.peek(), self.peek_at(1)) {
            (Some('$'), _) => 1,
            (Some('0'), Some('x' | 'X')) => 2,
            _ => 0,
        };
        if hex_prefix > 0 {
            let mut text: String = (0..hex_prefix).filter_map(|_| self.bump()).collect();
            let digits = self.take_while(|c| c.is_ascii_hexdigit());
            if digits.is_empty() {
                return Err(self.error(position, "hexadecimal number without digits"));
            }
            text.push_str(&digits);
            return Ok(TokenKind::Integer(text));
        }
        let mut text = self.take_while(|c| c.is_ascii_digit());
        if self.peek() == Some('.') {
            text.push('.');
            self.bump();
            text.push_str(&self.take_while(|c| c.is_ascii_digit()));
            if text == "." {
                return Err(self.error(position, "unexpected '.'"));
            }
            return Ok(TokenKind::Real(text));
        }
        Ok(TokenKind::Integer(text))
    }

    fn string(&mut self, position: Position) -> Result<TokenKind> {
        self.bump();
        let mut text = String::new();
        loop {
            match self.bump() {
                Some('"') => return Ok(TokenKind::String(text)),
                Some('\\') => {
                    text.push('\\');
                    match self.bump() {
                        Some(escaped) => text.push(escaped),
                        None => break,
                    }
                }
                Some(ch) => text.push(ch),
                None => break,
            }
        }
        Err(self.error(position, "unterminated string"))
    }

    fn raw_code(&mut self, position: Position) -> Result<TokenKind> {
        self.bump();
        let mut text = String::new();
        loop {
            match self.bump() {
                Some('\'') => break,
                Some('\\') => {
                    text.push('\\');
                    if let Some(escaped) = self.bump() {
                        text.push(escaped);
                    }
                }
                Some('\n' | '\r') | None => {
                    return Err(self.error(position, "unterminated raw code"));
                }
                Some(ch) => text.push(ch),
            }
        }
        if text.is_empty() {
            return Err(self.error(position, "empty raw code"));
        }
        Ok(TokenKind::RawCode(text))
    }

    fn symbol(&mut self, position: Position) -> Result<Symbol> {
        let first = self.bump().expect("caller checked");
        let followed_by_equals = self.peek() == Some('=');
        let symbol = match first {
            '=' if followed_by_equals => Symbol::Equal,
            '!' if followed_by_equals => Symbol::NotEqual,
            '<' if followed_by_equals => Symbol::LessEqual,
            '>' if followed_by_equals => Symbol::GreaterEqual,
            '=' => Symbol::Assign,
            '<' => Symbol::Less,
            '>' => Symbol::Greater,
            '+' => Symbol::Plus,
            '-' => Symbol::Minus,
            '*' => Symbol::Star,
            '/' => Symbol::Slash,
            '(' => Symbol::LeftParen,
            ')' => Symbol::RightParen,
            '[' => Symbol::LeftBracket,
            ']' => Symbol::RightBracket,
            ',' => Symbol::Comma,
            other => return Err(self.error(position, format!("unexpected character {other:?}"))),
        };
        if matches!(
            symbol,
            Symbol::Equal | Symbol::NotEqual | Symbol::LessEqual | Symbol::GreaterEqual
        ) {
            self.bump();
        }
        Ok(symbol)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(source: &str) -> Vec<TokenKind> {
        tokenize(source)
            .unwrap()
            .into_iter()
            .map(|t| t.kind)
            .collect()
    }

    fn ident(s: &str) -> TokenKind {
        TokenKind::Identifier(s.to_owned())
    }

    #[test]
    fn splits_words_numbers_and_symbols() {
        use Symbol::*;
        assert_eq!(
            kinds("set x[i] = a<=b != 0x1F+$ff*017/.5-2. // comment\r\nendloop"),
            [
                TokenKind::Keyword(Keyword::Set),
                ident("x"),
                TokenKind::Symbol(LeftBracket),
                ident("i"),
                TokenKind::Symbol(RightBracket),
                TokenKind::Symbol(Assign),
                ident("a"),
                TokenKind::Symbol(LessEqual),
                ident("b"),
                TokenKind::Symbol(NotEqual),
                TokenKind::Integer("0x1F".into()),
                TokenKind::Symbol(Plus),
                TokenKind::Integer("$ff".into()),
                TokenKind::Symbol(Star),
                TokenKind::Integer("017".into()),
                TokenKind::Symbol(Slash),
                TokenKind::Real(".5".into()),
                TokenKind::Symbol(Minus),
                TokenKind::Real("2.".into()),
                TokenKind::Newline,
                TokenKind::Keyword(Keyword::EndLoop),
            ]
        );
    }

    #[test]
    fn keeps_literal_text() {
        assert_eq!(
            kinds(r#""a \"q\" \\ b" 'hfoo' 'A'"#),
            [
                TokenKind::String(r#"a \"q\" \\ b"#.into()),
                TokenKind::RawCode("hfoo".into()),
                TokenKind::RawCode("A".into()),
            ]
        );
        assert_eq!(
            kinds("\"two\nlines\""),
            [TokenKind::String("two\nlines".into())]
        );
    }

    #[test]
    fn tracks_positions() {
        let tokens = tokenize("a\n  b").unwrap();
        assert_eq!(tokens[2].position, Position { line: 2, column: 3 });
    }

    #[test]
    fn reports_errors_with_positions() {
        let err = tokenize("x = \"open").unwrap_err();
        assert_eq!((err.position.line, err.position.column), (1, 5));
        assert!(tokenize("a ? b").is_err());
        assert!(tokenize("''").is_err());
        assert!(tokenize("0x").is_err());
    }
}
