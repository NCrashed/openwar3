//! Values and the rules for literals and operators.
//!
//! PROVISIONAL: no spec covers these rules yet. They follow the most common
//! reading of the language and are kept in this module so that they can be
//! replaced when the specs arrive. Each open point is listed in
//! specs/QUESTIONS.md under "JASS evaluation".

use std::rc::Rc;

use crate::check::Type;

/// A runtime value. Handles are numbered by the native layer; `None` is
/// `null`.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Integer(i32),
    Real(f32),
    Boolean(bool),
    String(Option<Rc<str>>),
    Handle(Option<u32>),
    /// Index of a function in [`crate::check::Program::functions`].
    Code(Option<usize>),
    /// The `null` literal before it is stored somewhere with a type.
    Null,
}

impl Value {
    pub fn string(text: &str) -> Value {
        Value::String(Some(Rc::from(text)))
    }

    /// Initial value of an array element, and what stubs return.
    pub fn default_of(t: Type) -> Value {
        match t {
            Type::Integer => Value::Integer(0),
            Type::Real => Value::Real(0.0),
            Type::Boolean => Value::Boolean(false),
            Type::String => Value::String(None),
            Type::Handle(_) => Value::Handle(None),
            Type::Code => Value::Code(None),
            Type::Null => Value::Null,
        }
    }

    /// Converts a value for storage in a variable, argument or return of
    /// type `t`: integers become reals and `null` takes the target's form.
    /// The checker has already confirmed that the types fit.
    pub fn coerce(self, t: Type) -> Value {
        match (self, t) {
            (Value::Integer(i), Type::Real) => Value::Real(i as f32),
            (Value::Null, Type::Integer) => Value::Integer(0),
            (Value::Null, t) => Value::default_of(t),
            (value, _) => value,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    DivisionByZero,
    InvalidLiteral(String),
}

/// Integer literal: decimal, octal with a leading `0`, hexadecimal with
/// `0x` or `$`. Values beyond 32 bits wrap.
pub fn integer_literal(text: &str) -> Result<i32, Fault> {
    let (digits, radix) =
        if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
            (hex, 16)
        } else if let Some(hex) = text.strip_prefix('$') {
            (hex, 16)
        } else if text.len() > 1 && text.starts_with('0') {
            (&text[1..], 8)
        } else {
            (text, 10)
        };
    let mut value: u32 = 0;
    for ch in digits.chars() {
        let digit = ch
            .to_digit(radix)
            .ok_or_else(|| Fault::InvalidLiteral(text.to_owned()))?;
        value = value.wrapping_mul(radix).wrapping_add(digit);
    }
    Ok(value as i32)
}

/// Raw code such as `'hfoo'`: the characters as base-256 digits, first
/// character most significant.
pub fn raw_code_literal(text: &str) -> Result<i32, Fault> {
    let decoded = decode_escapes(text);
    if decoded.is_empty() || decoded.len() > 4 {
        return Err(Fault::InvalidLiteral(text.to_owned()));
    }
    Ok(decoded
        .bytes()
        .fold(0u32, |acc, b| (acc << 8) | u32::from(b)) as i32)
}

pub fn real_literal(text: &str) -> Result<f32, Fault> {
    text.parse::<f32>()
        .map_err(|_| Fault::InvalidLiteral(text.to_owned()))
}

/// String literal escapes: `\"`, `\\`, `\n`, `\r`, `\t`, `\b`, `\f`. Any
/// other escaped character stands for itself.
pub fn decode_escapes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('b') => out.push('\u{8}'),
            Some('f') => out.push('\u{c}'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// Integer arithmetic wraps; division truncates toward zero and fails on
/// zero.
pub fn integer_op(op: crate::ast::BinaryOperator, a: i32, b: i32) -> Result<i32, Fault> {
    use crate::ast::BinaryOperator::*;
    Ok(match op {
        Add => a.wrapping_add(b),
        Subtract => a.wrapping_sub(b),
        Multiply => a.wrapping_mul(b),
        Divide => {
            if b == 0 {
                return Err(Fault::DivisionByZero);
            }
            a.wrapping_div(b)
        }
        _ => unreachable!("not an arithmetic operator"),
    })
}

/// Real arithmetic in IEEE single precision. SPEC-0002 notes that the game's
/// float addition differs from IEEE; the exact rule is still open.
pub fn real_op(op: crate::ast::BinaryOperator, a: f32, b: f32) -> Result<f32, Fault> {
    use crate::ast::BinaryOperator::*;
    Ok(match op {
        Add => a + b,
        Subtract => a - b,
        Multiply => a * b,
        Divide => {
            if b == 0.0 {
                return Err(Fault::DivisionByZero);
            }
            a / b
        }
        _ => unreachable!("not an arithmetic operator"),
    })
}

/// `==` on two values of comparable types. Reals compare exactly, strings
/// by their characters.
pub fn equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Integer(x), Value::Real(y)) | (Value::Real(y), Value::Integer(x)) => {
            (*x as f32) == *y
        }
        (Value::Null, other) | (other, Value::Null) => is_null(other),
        _ => a == b,
    }
}

fn is_null(value: &Value) -> bool {
    matches!(
        value,
        Value::Null | Value::String(None) | Value::Handle(None) | Value::Code(None)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_literals() {
        assert_eq!(integer_literal("42"), Ok(42));
        assert_eq!(integer_literal("0"), Ok(0));
        assert_eq!(integer_literal("017"), Ok(15));
        assert_eq!(integer_literal("0x1F"), Ok(31));
        assert_eq!(integer_literal("$ff"), Ok(255));
        assert_eq!(integer_literal("4294967295"), Ok(-1));
        assert!(integer_literal("09").is_err());
    }

    #[test]
    fn raw_codes() {
        assert_eq!(raw_code_literal("hfoo"), Ok(0x6866_6F6F));
        assert_eq!(raw_code_literal("A"), Ok(65));
        assert!(raw_code_literal("abcde").is_err());
    }

    #[test]
    fn reals_and_strings() {
        assert_eq!(real_literal("1."), Ok(1.0));
        assert_eq!(real_literal(".5"), Ok(0.5));
        assert_eq!(decode_escapes(r#"a\"b\\c\nd"#), "a\"b\\c\nd");
    }

    #[test]
    fn arithmetic() {
        use crate::ast::BinaryOperator::*;
        assert_eq!(integer_op(Add, i32::MAX, 1), Ok(i32::MIN));
        assert_eq!(integer_op(Divide, -7, 2), Ok(-3));
        assert_eq!(integer_op(Divide, 1, 0), Err(Fault::DivisionByZero));
        assert_eq!(real_op(Divide, 1.0, 4.0), Ok(0.25));
    }

    #[test]
    fn coercion_and_equality() {
        assert_eq!(Value::Integer(2).coerce(Type::Real), Value::Real(2.0));
        assert_eq!(Value::Null.coerce(Type::Handle(3)), Value::Handle(None));
        assert_eq!(Value::Null.coerce(Type::Integer), Value::Integer(0));
        assert!(equal(&Value::Integer(1), &Value::Real(1.0)));
        assert!(equal(&Value::Null, &Value::String(None)));
        assert!(!equal(&Value::Null, &Value::string("")));
    }
}
