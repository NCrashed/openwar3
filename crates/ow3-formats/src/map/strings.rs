//! `war3map.wts`: the map's trigger strings. Other map files refer to them
//! as `TRIGSTR_<number>`, for example the map name in `war3map.w3i`.
//!
//! The file is UTF-8 text, optionally with a byte order mark:
//!
//! ```text
//! STRING 3
//! // optional comment lines
//! {
//! text, possibly over several lines
//! }
//! ```
//!
//! The text is kept exactly as stored, including its line breaks.

use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    InvalidUtf8,
    /// Something other than the expected structure on a 1-based line.
    Malformed {
        line: usize,
        expected: &'static str,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidUtf8 => write!(f, "trigger strings are not valid UTF-8"),
            Error::Malformed { line, expected } => write!(f, "line {line}: expected {expected}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

const REFERENCE_PREFIX: &str = "TRIGSTR_";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TriggerStrings {
    strings: BTreeMap<u32, String>,
    without_body: Vec<u32>,
}

impl TriggerStrings {
    /// When a number appears twice, the later text is kept.
    ///
    /// Some shipped maps have a `STRING <number>` header followed by a bare
    /// line instead of a braced body. Such entries are left undefined and
    /// listed by [`TriggerStrings::without_body`]; the lines up to the next
    /// header are skipped. How the game treats either case has not been
    /// checked.
    pub fn parse(data: &[u8]) -> Result<TriggerStrings> {
        let text = std::str::from_utf8(data).map_err(|_| Error::InvalidUtf8)?;
        let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
        let mut lines = text.split_inclusive('\n').enumerate().peekable();
        let mut strings = BTreeMap::new();
        let mut without_body = Vec::new();
        let mut skipping = false;

        while let Some((index, line)) = lines.next() {
            let content = trim_line_end(line);
            if content.trim().is_empty() {
                continue;
            }
            let header = content
                .strip_prefix("STRING ")
                .and_then(|n| n.trim().parse::<u32>().ok());
            let number = match header {
                Some(number) => number,
                None if skipping => continue,
                None => {
                    return Err(Error::Malformed {
                        line: index + 1,
                        expected: "STRING <number>",
                    });
                }
            };
            skipping = false;

            // Comments may sit between the header and the opening brace.
            let opening = loop {
                match lines.peek() {
                    Some((_, l)) if trim_line_end(l).starts_with("//") => {
                        lines.next();
                    }
                    other => break other.map(|&(_, l)| trim_line_end(l)),
                }
            };
            if opening != Some("{") {
                without_body.push(number);
                skipping = true;
                continue;
            }
            lines.next();

            let mut body = String::new();
            loop {
                match lines.next() {
                    Some((_, l)) if trim_line_end(l) == "}" => break,
                    Some((_, l)) => body.push_str(l),
                    None => {
                        return Err(Error::Malformed {
                            line: index + 1,
                            expected: "} closing this string",
                        });
                    }
                }
            }
            // The line break before the closing brace is not part of the text.
            body.truncate(trim_line_end(&body).len());
            strings.insert(number, body);
        }
        Ok(TriggerStrings {
            strings,
            without_body,
        })
    }

    pub fn get(&self, number: u32) -> Option<&str> {
        self.strings.get(&number).map(String::as_str)
    }

    /// Numbers whose header was not followed by a braced body.
    pub fn without_body(&self) -> &[u32] {
        &self.without_body
    }

    pub fn len(&self) -> usize {
        self.strings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }

    /// Text for a `TRIGSTR_<number>` reference, or `None` when `value` is not
    /// a reference or the number is not defined.
    pub fn resolve(&self, value: &str) -> Option<&str> {
        let digits = value.strip_prefix(REFERENCE_PREFIX)?;
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        self.get(digits.parse().ok()?)
    }
}

fn trim_line_end(line: &str) -> &str {
    let line = line.strip_suffix('\n').unwrap_or(line);
    line.strip_suffix('\r').unwrap_or(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_strings_with_comments_and_multiple_lines() {
        let data = "\u{FEFF}STRING 0\r\n{\r\nPlayer 1\r\n}\r\n\r\n\
            STRING 12\r\n// Units: h000 (Hero), Name (Name)\r\n{\r\nfirst\r\nsecond\r\n}\r\n\
            STRING 13\n{\n\n}\n";
        let strings = TriggerStrings::parse(data.as_bytes()).unwrap();
        assert_eq!(strings.len(), 3);
        assert_eq!(strings.get(0), Some("Player 1"));
        assert_eq!(strings.get(12), Some("first\r\nsecond"));
        assert_eq!(strings.get(13), Some(""));
        assert_eq!(strings.resolve("TRIGSTR_012"), Some("first\r\nsecond"));
        assert_eq!(strings.resolve("TRIGSTR_7"), None);
        assert_eq!(strings.resolve("TRIGSTR_"), None);
        assert_eq!(strings.resolve("Lost Temple"), None);
    }

    /// Shape found in a shipped map: a header followed by a bare value.
    #[test]
    fn header_without_body_is_undefined_and_skipped() {
        let data =
            "STRING 1\r\n{\r\nA\r\n}\r\n\r\nSTRING 2\r\n#VALUE!\r\n\r\nSTRING 3\r\n{\r\nC\r\n}\r\n";
        let strings = TriggerStrings::parse(data.as_bytes()).unwrap();
        assert_eq!(strings.get(1), Some("A"));
        assert_eq!(strings.get(2), None);
        assert_eq!(strings.get(3), Some("C"));
        assert_eq!(strings.without_body(), [2]);
    }

    #[test]
    fn reports_structure_errors() {
        let err = |data: &str| TriggerStrings::parse(data.as_bytes()).unwrap_err();
        assert_eq!(
            err("STRING x\n{\na\n}\n"),
            Error::Malformed {
                line: 1,
                expected: "STRING <number>"
            }
        );
        assert_eq!(
            err("STRING 1\n{\na\n"),
            Error::Malformed {
                line: 1,
                expected: "} closing this string"
            }
        );
        assert_eq!(TriggerStrings::parse(b"\xFF"), Err(Error::InvalidUtf8));
    }
}
