//! Values and the rules for literals and operators.
//!
//! Literals, integer operations and strings follow SPEC-0005; real
//! arithmetic, conversions and comparisons follow SPEC-0004 through
//! `ow3-math`.

use std::rc::Rc;

use crate::ast::BinaryOperator;
use crate::check::Type;

/// A runtime value. Handles are numbered by the native layer; `None` is
/// `null`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Integer(i32),
    /// The bit pattern of a real. Reals are only changed through the
    /// engine's operations of SPEC-0004, never through Rust floats.
    Real(u32),
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

    /// A real from a Rust float, for natives and tests.
    pub fn real(value: f32) -> Value {
        Value::Real(value.to_bits())
    }

    /// Initial value of an array element, and what stubs return.
    pub fn default_of(t: Type) -> Value {
        match t {
            Type::Integer => Value::Integer(0),
            Type::Real => Value::Real(0),
            Type::Boolean => Value::Boolean(false),
            Type::String => Value::String(None),
            Type::Handle(_) => Value::Handle(None),
            Type::Code => Value::Code(None),
            Type::Null => Value::Null,
        }
    }

    /// Converts a value for storage in a variable, argument or return of
    /// type `t`: integers become reals with `I2R` and `null` takes the
    /// target's form. The checker has already confirmed that the types fit.
    pub fn coerce(self, t: Type) -> Value {
        match (self, t) {
            (Value::Integer(i), Type::Real) => Value::Real(ow3_math::i2r(i)),
            (Value::Null, Type::Integer) => Value::Integer(0),
            (Value::Null, t) => Value::default_of(t),
            (value, _) => value,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    DivisionByZero,
    /// `-2147483648 / -1`. The game crashes; SPEC-0005 asks to end the
    /// thread instead.
    DivisionOverflow,
    InvalidLiteral(String),
}

/// Integer literal: decimal, octal with a leading `0`, hexadecimal with
/// `0x` or `$`. Values beyond 32 bits wrap.
pub fn integer_literal(text: &str) -> Result<i32, Fault> {
    let (digits, radix) = if let Some(hex) = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .or_else(|| text.strip_prefix('$'))
    {
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

/// Real literals follow the compiler's own rule, which differs from `S2R`
/// (SPEC-0005).
pub fn real_literal(text: &str) -> u32 {
    ow3_math::real_literal(text)
}

/// Escapes of string literals and raw codes (SPEC-0005): `\"`, `\\`, `\t`,
/// `\n` and `\r`. The lexer rejects any other escape.
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
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// Integer arithmetic wraps; division truncates toward zero (SPEC-0004).
pub fn integer_op(op: BinaryOperator, a: i32, b: i32) -> Result<i32, Fault> {
    use BinaryOperator::*;
    Ok(match op {
        Add => a.wrapping_add(b),
        Subtract => a.wrapping_sub(b),
        Multiply => a.wrapping_mul(b),
        Divide => {
            if b == 0 {
                return Err(Fault::DivisionByZero);
            }
            a.checked_div(b).ok_or(Fault::DivisionOverflow)?
        }
        _ => unreachable!("not an arithmetic operator"),
    })
}

/// Real arithmetic with the engine's operations (SPEC-0004).
pub fn real_op(op: BinaryOperator, a: u32, b: u32) -> Result<u32, Fault> {
    use BinaryOperator::*;
    Ok(match op {
        Add => ow3_math::add(a, b),
        Subtract => ow3_math::sub(a, b),
        Multiply => ow3_math::mul(a, b),
        Divide => {
            if ow3_math::is_zero(b) {
                return Err(Fault::DivisionByZero);
            }
            ow3_math::div(a, b)
        }
        _ => unreachable!("not an arithmetic operator"),
    })
}

/// Real ordering comparisons are IEEE comparisons (SPEC-0004).
pub fn real_order(op: BinaryOperator, a: u32, b: u32) -> bool {
    use BinaryOperator::*;
    let (a, b) = (f32::from_bits(a), f32::from_bits(b));
    match op {
        Less => a < b,
        LessEqual => a <= b,
        Greater => a > b,
        GreaterEqual => a >= b,
        _ => unreachable!("not an ordering operator"),
    }
}

/// `==`. Reals are equal when they differ by less than 0.001 (SPEC-0004);
/// strings compare their characters, and `null` differs from `""`
/// (SPEC-0005).
pub fn equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Real(x), Value::Real(y)) => ow3_math::jass_equal(*x, *y),
        (Value::Integer(i), Value::Real(r)) | (Value::Real(r), Value::Integer(i)) => {
            ow3_math::jass_equal(ow3_math::i2r(*i), *r)
        }
        (Value::Null, other) | (other, Value::Null) => is_null(other),
        _ => a == b,
    }
}

/// `!=`. For reals this compares bit patterns and is not the negation of
/// `==` (SPEC-0004); for everything else it is.
pub fn not_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Real(x), Value::Real(y)) => ow3_math::jass_not_equal(*x, *y),
        (Value::Integer(i), Value::Real(r)) | (Value::Real(r), Value::Integer(i)) => {
            ow3_math::jass_not_equal(ow3_math::i2r(*i), *r)
        }
        _ => !equal(a, b),
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
        assert_eq!(raw_code_literal("\\n"), Ok(10));
        assert!(raw_code_literal("abcde").is_err());
    }

    #[test]
    fn reals_and_strings() {
        assert_eq!(real_literal("5."), 5.0f32.to_bits());
        assert_eq!(real_literal(".25"), 0.25f32.to_bits());
        assert_eq!(real_literal("0.1"), 0x3DCC_CCCE);
        assert_eq!(decode_escapes(r#"a\"b\\c\nd"#), "a\"b\\c\nd");
    }

    #[test]
    fn arithmetic() {
        use BinaryOperator::*;
        assert_eq!(integer_op(Add, i32::MAX, 1), Ok(i32::MIN));
        assert_eq!(integer_op(Divide, -7, 2), Ok(-3));
        assert_eq!(integer_op(Divide, 1, 0), Err(Fault::DivisionByZero));
        assert_eq!(
            integer_op(Divide, i32::MIN, -1),
            Err(Fault::DivisionOverflow)
        );
        let (one, step) = (1.0f32.to_bits(), 0.005f32.to_bits());
        assert_eq!(real_op(Subtract, one, step), Ok(0x3F7E_B851));
        assert_eq!(
            real_op(Divide, one, (-0.0f32).to_bits()),
            Err(Fault::DivisionByZero)
        );
    }

    #[test]
    fn coercion_and_equality() {
        assert_eq!(Value::Integer(2).coerce(Type::Real), Value::real(2.0));
        assert_eq!(Value::Null.coerce(Type::Handle(3)), Value::Handle(None));
        assert_eq!(Value::Null.coerce(Type::Integer), Value::Integer(0));
        assert!(equal(&Value::Integer(1), &Value::real(1.0)));
        assert!(equal(&Value::Null, &Value::String(None)));
        assert!(!equal(&Value::Null, &Value::string("")));
        let (a, b) = (Value::real(1.0), Value::real(1.0005));
        assert!(equal(&a, &b) && not_equal(&a, &b));
        assert!(not_equal(&Value::real(0.0), &Value::real(-0.0)));
    }
}
