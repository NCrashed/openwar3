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

#[cfg(test)]
mod tests {
    use super::*;

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
