//! String lists of the interface definition files (`UI\FrameDef\*.fdf`):
//!
//! ```text
//! StringList {
//!     KEY    "text",
//!     OTHER  "more text",
//! }
//! ```
//!
//! Only `StringList` blocks are read; frame definitions are skipped.
//! Comments (`//` to the end of the line and `/* ... */`) are ignored.

/// The entries of every `StringList` block of `text`, in file order. A
/// key may appear more than once.
pub fn string_lists(text: &str) -> Vec<(String, String)> {
    let text = strip_comments(text);
    let mut entries = Vec::new();
    let mut rest = text.as_str();
    while let Some(at) = rest.find("StringList") {
        rest = &rest[at + "StringList".len()..];
        let Some(open) = rest.find('{') else {
            break;
        };
        rest = &rest[open + 1..];
        loop {
            rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == ',');
            if rest.is_empty() || rest.starts_with('}') {
                rest = rest.get(1..).unwrap_or("");
                break;
            }
            let key_end = rest
                .find(|c: char| c.is_whitespace() || c == '"')
                .unwrap_or(rest.len());
            let key = &rest[..key_end];
            rest = rest[key_end..].trim_start();
            let Some(body) = rest.strip_prefix('"') else {
                // Not an entry: skip the word.
                continue;
            };
            let end = body.find('"').unwrap_or(body.len());
            entries.push((key.to_owned(), body[..end].to_owned()));
            rest = body.get(end + 1..).unwrap_or("");
        }
    }
    entries
}

fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            in_string = c != '"';
            continue;
        }
        match (c, chars.peek()) {
            ('"', _) => {
                in_string = true;
                out.push(c);
            }
            ('/', Some('/')) => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            ('/', Some('*')) => {
                chars.next();
                let mut last = ' ';
                for c in chars.by_ref() {
                    if last == '*' && c == '/' {
                        break;
                    }
                    last = c;
                }
                out.push(' ');
            }
            _ => out.push(c),
        }
    }
    out
}

/// A value of a property: a word (a name, a number, a keyword) or a quoted
/// string.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Word(String),
    Str(String),
}

impl Value {
    /// The text of the value, quoted or not.
    pub fn text(&self) -> &str {
        match self {
            Value::Word(w) | Value::Str(w) => w,
        }
    }

    /// The value as a number, if it is one.
    pub fn number(&self) -> Option<f32> {
        match self {
            Value::Word(w) => w.parse().ok(),
            Value::Str(_) => None,
        }
    }
}

/// A definition of an interface file: `Frame "TYPE" "Name"`, `Texture`,
/// `String`, `Layer` and the like, with its properties (one per line,
/// the values separated by commas or spaces) and nested definitions.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Definition {
    /// `Frame`, `Texture`, `String`, `Layer`, ...
    pub kind: String,
    /// The frame type of a `Frame` (`SIMPLEFRAME`, `BACKDROP`, ...).
    pub frame_type: Option<String>,
    pub name: Option<String>,
    /// `INHERITS [WITHCHILDREN] "Base"`.
    pub inherits: Option<String>,
    pub with_children: bool,
    pub properties: Vec<(String, Vec<Value>)>,
    pub children: Vec<Definition>,
}

impl Definition {
    /// The values of the first property `key`.
    pub fn property(&self, key: &str) -> Option<&[Value]> {
        self.properties
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_slice())
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Word(String),
    Str(String),
    Open,
    Close,
    Comma,
    Line,
}

fn tokens(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            '\n' => {
                chars.next();
                tokens.push(Token::Line);
            }
            c if c.is_whitespace() => {
                chars.next();
            }
            '{' => {
                chars.next();
                tokens.push(Token::Open);
            }
            '}' => {
                chars.next();
                tokens.push(Token::Close);
            }
            ',' | ';' => {
                chars.next();
                tokens.push(Token::Comma);
            }
            '"' => {
                chars.next();
                let mut s = String::new();
                for c in chars.by_ref() {
                    if c == '"' {
                        break;
                    }
                    s.push(c);
                }
                tokens.push(Token::Str(s));
            }
            _ => {
                let mut w = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_whitespace() || matches!(c, '{' | '}' | ',' | ';' | '"') {
                        break;
                    }
                    w.push(c);
                    chars.next();
                }
                tokens.push(Token::Word(w));
            }
        }
    }
    tokens
}

/// Words that start a definition when a `{` follows its header.
const DEFINITIONS: [&str; 5] = ["Frame", "Texture", "String", "Layer", "StringList"];

/// The definitions of an interface file, in file order. `StringList`
/// blocks come as definitions without properties (see [`string_lists`]),
/// `IncludeFile` lines are left out.
pub fn definitions(text: &str) -> Vec<Definition> {
    let tokens = tokens(&strip_comments(text));
    let mut at = 0;
    let mut out = Vec::new();
    while at < tokens.len() {
        match &tokens[at] {
            Token::Word(w) if DEFINITIONS.contains(&w.as_str()) => {
                if let Some(d) = definition(&tokens, &mut at) {
                    out.push(d);
                }
            }
            _ => at += 1,
        }
    }
    out
}

/// The definition whose header starts at `at`, leaving `at` after it.
fn definition(tokens: &[Token], at: &mut usize) -> Option<Definition> {
    let Token::Word(kind) = &tokens[*at] else {
        return None;
    };
    let mut d = Definition {
        kind: kind.clone(),
        ..Definition::default()
    };
    *at += 1;
    let mut strings = Vec::new();
    let mut base_next = false;
    while *at < tokens.len() {
        match &tokens[*at] {
            Token::Open => break,
            // The base follows INHERITS [WITHCHILDREN].
            Token::Str(s) if base_next => {
                d.inherits = Some(s.clone());
                base_next = false;
            }
            Token::Str(s) => strings.push(s.clone()),
            Token::Word(w) if w == "INHERITS" => base_next = true,
            Token::Word(w) if w == "WITHCHILDREN" => d.with_children = true,
            Token::Line => {}
            _ => return None,
        }
        *at += 1;
    }
    *at += 1;
    match strings.len() {
        0 => {}
        1 => d.name = strings.pop(),
        _ => {
            d.name = strings.pop();
            d.frame_type = strings.pop();
        }
    }
    if d.kind == "StringList" {
        // Skip to the matching brace.
        while *at < tokens.len() && tokens[*at] != Token::Close {
            *at += 1;
        }
        *at += 1;
        return Some(d);
    }
    body(tokens, at, &mut d);
    Some(d)
}

/// The properties and nested definitions up to the closing brace.
fn body(tokens: &[Token], at: &mut usize, d: &mut Definition) {
    while *at < tokens.len() {
        match &tokens[*at] {
            Token::Close => {
                *at += 1;
                return;
            }
            Token::Line | Token::Comma => *at += 1,
            Token::Word(w) if DEFINITIONS.contains(&w.as_str()) && opens(tokens, *at) => {
                let start = *at;
                match definition(tokens, at) {
                    Some(child) => d.children.push(child),
                    None => *at = start + 1,
                }
            }
            Token::Word(key) => {
                let key = key.clone();
                *at += 1;
                let mut values = Vec::new();
                while *at < tokens.len() {
                    match &tokens[*at] {
                        Token::Word(w) => values.push(Value::Word(w.clone())),
                        Token::Str(s) => values.push(Value::Str(s.clone())),
                        Token::Comma => {}
                        _ => break,
                    }
                    *at += 1;
                }
                d.properties.push((key, values));
            }
            _ => *at += 1,
        }
    }
}

/// Whether the definition header at `at` opens a block: a `{` comes
/// before any comma or closing brace.
fn opens(tokens: &[Token], at: usize) -> bool {
    tokens[at + 1..]
        .iter()
        .find(|t| matches!(t, Token::Open | Token::Comma | Token::Close))
        .is_some_and(|t| *t == Token::Open)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_definitions() {
        let text = "// a comment\n\
            Texture \"Icon\" {\n    Width 0.016,\n}\n\
            Frame \"SIMPLEFRAME\" \"Bar\" {\n\
                DecorateFileNames,\n\
                Width 0.338,\n\
                Texture INHERITS \"Icon\" {\n\
                    Anchor TOPLEFT, 0.0, -0.003125,\n\
                    File \"GoldIcon\",\n\
                }\n\
                Frame \"SIMPLEBUTTON\" \"Quests\" INHERITS WITHCHILDREN \"Template\" {\n\
                    SetPoint TOPLEFT,\"Bar\",TOPLEFT,0.002,0,\n\
                    NormalText \"TextTemplate\" \"KEY_QUESTS\",\n\
                    ButtonPushedTextOffset 0.001 -0.001,\n\
                }\n\
            }\n\
            StringList { KEY \"text\", }\n";
        let d = definitions(text);
        assert_eq!(d.len(), 3);
        assert_eq!(d[0].name.as_deref(), Some("Icon"));
        assert_eq!(d[0].property("Width").unwrap()[0].number(), Some(0.016));
        let bar = &d[1];
        assert_eq!(bar.frame_type.as_deref(), Some("SIMPLEFRAME"));
        assert_eq!(bar.name.as_deref(), Some("Bar"));
        assert_eq!(bar.property("DecorateFileNames"), Some(&[][..]));
        assert_eq!(bar.children.len(), 2);
        let icon = &bar.children[0];
        assert_eq!(
            (icon.name.as_deref(), icon.inherits.as_deref()),
            (None, Some("Icon"))
        );
        let anchor: Vec<&str> = icon
            .property("Anchor")
            .unwrap()
            .iter()
            .map(Value::text)
            .collect();
        assert_eq!(anchor, ["TOPLEFT", "0.0", "-0.003125"]);
        let quests = &bar.children[1];
        assert_eq!(quests.inherits.as_deref(), Some("Template"));
        assert!(quests.with_children);
        assert_eq!(
            quests.property("SetPoint").unwrap()[1],
            Value::Str("Bar".to_owned())
        );
        assert_eq!(quests.property("ButtonPushedTextOffset").unwrap().len(), 2);
        assert_eq!(d[2].kind, "StringList");
    }

    #[test]
    fn reads_string_lists() {
        let text = "/* header\n StringList { NOT \"this\" } */\n\
            Frame \"TEXT\" \"X\" { Width 0.1, }\n\
            StringList {\n\
                ACCEPT   \"Accept\",  // a comment\n\
                HIGH_VOL \"\",\n\
                URL      \"http://x\",\n\
                HIGH_VOL \"High\",\n\
            }\n";
        assert_eq!(
            string_lists(text),
            [
                ("ACCEPT".to_owned(), "Accept".to_owned()),
                ("HIGH_VOL".to_owned(), String::new()),
                ("URL".to_owned(), "http://x".to_owned()),
                ("HIGH_VOL".to_owned(), "High".to_owned()),
            ]
        );
    }
}
