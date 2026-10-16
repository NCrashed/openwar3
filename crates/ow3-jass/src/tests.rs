use crate::ast::*;

fn script(source: &str) -> Script {
    crate::parse(source).unwrap_or_else(|e| panic!("{e}"))
}

fn error(source: &str) -> crate::Error {
    crate::parse(source).expect_err("should not parse")
}

/// The first statement of the first function.
fn first_statement(source: &str) -> StatementKind {
    script(source).functions.remove(0).body.remove(0).kind
}

/// Renders an expression with explicit parentheses to check grouping.
fn shape(e: &Expression) -> String {
    match &e.kind {
        ExpressionKind::Integer(t) | ExpressionKind::Real(t) => t.clone(),
        ExpressionKind::Variable(n) => n.clone(),
        ExpressionKind::Boolean(b) => b.to_string(),
        ExpressionKind::Unary { operator, operand } => {
            let op = match operator {
                UnaryOperator::Plus => "+",
                UnaryOperator::Minus => "-",
                UnaryOperator::Not => "not ",
            };
            format!("({op}{})", shape(operand))
        }
        ExpressionKind::Binary {
            operator,
            left,
            right,
        } => {
            let op = match operator {
                BinaryOperator::Or => "or",
                BinaryOperator::And => "and",
                BinaryOperator::Equal => "==",
                BinaryOperator::NotEqual => "!=",
                BinaryOperator::Less => "<",
                BinaryOperator::LessEqual => "<=",
                BinaryOperator::Greater => ">",
                BinaryOperator::GreaterEqual => ">=",
                BinaryOperator::Add => "+",
                BinaryOperator::Subtract => "-",
                BinaryOperator::Multiply => "*",
                BinaryOperator::Divide => "/",
            };
            format!("({} {op} {})", shape(left), shape(right))
        }
        ExpressionKind::Parenthesized(inner) => shape(inner),
        ExpressionKind::Call(call) => format!("{}(..)", call.name),
        other => format!("{other:?}"),
    }
}

fn expression_shape(expression: &str) -> String {
    let source =
        format!("function f takes nothing returns nothing\nset x = {expression}\nendfunction\n");
    match first_statement(&source) {
        StatementKind::Set { value, .. } => shape(&value),
        other => panic!("{other:?}"),
    }
}

#[test]
fn parses_declarations() {
    let s = script(
        "// header comment\n\
         type agent extends handle\n\
         type unit extends agent\n\
         globals\n\
             constant integer MAX = 12\n\
             unit array units\n\
             real speed\n\
         endglobals\n\
         constant native GetUnitX takes unit whichUnit returns real\n\
         native KillUnit takes unit whichUnit returns nothing\n\
         native GetHandleId takes handle h returns integer\n",
    );
    assert_eq!(s.declarations.len(), 6);
    let Declaration::Type(t) = &s.declarations[1] else {
        panic!()
    };
    assert_eq!((t.name.as_str(), t.parent.as_str()), ("unit", "agent"));
    let Declaration::Globals(globals) = &s.declarations[2] else {
        panic!()
    };
    assert!(globals[0].constant);
    assert!(globals[1].variable.array);
    assert_eq!(globals[2].variable.type_name, "real");
    let Declaration::Native(native) = &s.declarations[3] else {
        panic!()
    };
    assert!(native.constant);
    assert_eq!(native.signature.returns.as_deref(), Some("real"));
    assert_eq!(native.signature.parameters[0].type_name, "unit");
    let Declaration::Native(kill) = &s.declarations[4] else {
        panic!()
    };
    assert_eq!(kill.signature.returns, None);
    assert_eq!(kill.position.line, 10);
}

#[test]
fn parses_functions_and_statements() {
    let s = script(
        "function Helper takes integer a, real b returns boolean\n\
             local integer i = 0\n\
             local unit array group\n\
             loop\n\
                 exitwhen i >= a\n\
                 set group[i] = null\n\
                 set i = i + 1\n\
             endloop\n\
             if a > 1 then\n\
                 return true\n\
             elseif a == 1 then\n\
                 debug call BJDebugMsg(\"one\")\n\
             else\n\
                 call DoNothing()\n\
             endif\n\
             return false\n\
         endfunction\n\
         \n\
         constant function Run takes nothing returns nothing\n\
             call TimerStart(CreateTimer(), 1.0, false, function Helper)\n\
             return\n\
         endfunction",
    );
    assert_eq!(s.functions.len(), 2);
    let helper = &s.functions[0];
    assert_eq!(helper.signature.parameters.len(), 2);
    assert_eq!(helper.locals.len(), 2);
    assert!(helper.locals[1].array);
    assert_eq!(helper.body.len(), 3);
    let StatementKind::Loop(body) = &helper.body[0].kind else {
        panic!()
    };
    assert!(matches!(
        body[1].kind,
        StatementKind::Set { index: Some(_), .. }
    ));
    let StatementKind::If {
        branches,
        otherwise,
    } = &helper.body[1].kind
    else {
        panic!()
    };
    assert_eq!(branches.len(), 2);
    assert!(branches[1].1[0].debug);
    assert!(otherwise.is_some());

    let run = &s.functions[1];
    assert!(run.constant);
    let StatementKind::Call(call) = &run.body[0].kind else {
        panic!()
    };
    assert_eq!(call.arguments.len(), 4);
    assert_eq!(
        call.arguments[3].kind,
        ExpressionKind::FunctionReference("Helper".into())
    );
    assert_eq!(run.body[1].kind, StatementKind::Return(None));
}

#[test]
fn operator_precedence_and_grouping() {
    assert_eq!(expression_shape("1 + 2 * 3"), "(1 + (2 * 3))");
    assert_eq!(expression_shape("1 - 2 - 3"), "((1 - 2) - 3)");
    assert_eq!(expression_shape("a or b and c"), "(a or (b and c))");
    assert_eq!(expression_shape("not a == b"), "(not (a == b))");
    assert_eq!(
        expression_shape("a and not b or c"),
        "((a and (not b)) or c)"
    );
    assert_eq!(expression_shape("-x * 2"), "((-x) * 2)");
    assert_eq!(expression_shape("(1 + 2) * 3"), "((1 + 2) * 3)");
    assert_eq!(expression_shape("a < b == true"), "((a < b) == true)");
    assert_eq!(expression_shape("x == not y"), "(x == (not y))");
}

#[test]
fn keeps_literals_as_written() {
    for (source, expected) in [
        ("0x1F", ExpressionKind::Integer("0x1F".into())),
        ("$ff", ExpressionKind::Integer("$ff".into())),
        ("017", ExpressionKind::Integer("017".into())),
        ("'hfoo'", ExpressionKind::RawCode("hfoo".into())),
        (".5", ExpressionKind::Real(".5".into())),
        (r#""a\"b""#, ExpressionKind::String(r#"a\"b"#.into())),
        ("null", ExpressionKind::Null),
    ] {
        let source =
            format!("function f takes nothing returns nothing\nset x = {source}\nendfunction");
        let StatementKind::Set { value, .. } = first_statement(&source) else {
            panic!()
        };
        assert_eq!(value.kind, expected);
    }
}

/// Edge cases from SPEC-0001.
#[test]
fn rejects_what_spec_0001_calls_syntax_errors() {
    let late_native = error(
        "function f takes nothing returns nothing\nendfunction\n\
         native UnitAlive takes unit u returns boolean\n",
    );
    assert_eq!(late_native.position.line, 3);
    assert!(late_native.message.contains("after the first function"));

    let keyword_parameter = error("native F takes code code returns nothing\n");
    assert!(keyword_parameter.message.contains("expected a name"));
}

#[test]
fn rejects_malformed_scripts() {
    let cases = [
        (
            "function f takes nothing returns nothing\ncall g()\n",
            "end of file",
        ),
        (
            "function f takes nothing returns nothing\ncall g()\nlocal integer i\nendfunction",
            "before the first statement",
        ),
        ("globals\nconstant integer X\nendglobals", "initial value"),
        (
            "function f takes nothing returns nothing\nset x = 1 +\nendfunction",
            "expected an expression",
        ),
        (
            "function f takes nothing returns nothing\ndebug return\nendfunction",
            "after 'debug'",
        ),
        (
            "function f takes nothing returns nothing\nif x then call g()\nendif\nendfunction",
            "end of line",
        ),
        ("set x = 1", "declaration or function"),
    ];
    for (source, message) in cases {
        let err = error(source);
        assert!(err.message.contains(message), "{source:?}: {err}");
    }
}

#[test]
fn accepts_empty_and_comment_only_scripts() {
    assert_eq!(script("").functions.len(), 0);
    assert_eq!(
        script("\r\n// only a comment\r\n\r\n").declarations.len(),
        0
    );
}
