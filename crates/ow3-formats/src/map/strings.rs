//! `war3map.wts`: the map's trigger strings, and how `TRIGSTR_<number>`
//! references to them resolve. Everything here follows SPEC-0003.
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
//! Text is kept exactly as stored, including its line breaks.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    InvalidUtf8,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidUtf8 => write!(f, "trigger strings are not valid UTF-8"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

const REFERENCE_PREFIX: &str = "TRIGSTR_";

/// Natives that replace a reference passed as a string argument by the
/// entry text (SPEC-0003). Every other native and every script operation
/// sees the reference itself.
pub const RESOLVING_NATIVES: [&str; 34] = [
    "CopySaveGame",
    "DefeatConditionSetDescription",
    "DialogAddButton",
    "DialogAddQuitButton",
    "DialogSetMessage",
    "DisplayTextToPlayer",
    "DisplayTimedTextFromPlayer",
    "DisplayTimedTextToPlayer",
    "GetLocalizedString",
    "LeaderboardAddItem",
    "LeaderboardSetItemLabel",
    "LeaderboardSetLabel",
    "MultiboardSetItemIcon",
    "MultiboardSetItemValue",
    "MultiboardSetItemsIcon",
    "MultiboardSetItemsValue",
    "MultiboardSetTitleText",
    "QuestItemSetDescription",
    "QuestSetDescription",
    "QuestSetTitle",
    "RemoveSaveDirectory",
    "RenameSaveDirectory",
    "SaveGameExists",
    "SetAltMinimapIcon",
    "SetCinematicScene",
    "SetIntroShotText",
    "SetMapDescription",
    "SetMapName",
    "SetPlayerName",
    "SetTextTagText",
    "StringCase",
    "StringLength",
    "TimerDialogSetTitle",
    "TriggerRegisterPlayerChatEvent",
];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TriggerStrings {
    strings: BTreeMap<u32, String>,
    without_body: Vec<u32>,
}

impl TriggerStrings {
    /// Reads the file. Lines outside entries (comments, stray text, stray
    /// `}`) are ignored, so only invalid UTF-8 is an error.
    ///
    /// - A header is a line whose first word is `STRING`, followed by the
    ///   number. Headers with a number that is negative or does not fit in
    ///   32 bits define nothing that a reference can reach.
    /// - Comment lines may sit between the header and the `{`. If the next
    ///   other line does not start with `{`, the entry has empty text.
    /// - Text after the `{` on its line is the first line of the text. The
    ///   text ends at the first line that starts with `}`; a file that ends
    ///   first gives the text read so far.
    /// - When a number is defined more than once, the first definition is
    ///   kept, including a header without a body.
    pub fn parse(data: &[u8]) -> Result<TriggerStrings> {
        let text = std::str::from_utf8(data).map_err(|_| Error::InvalidUtf8)?;
        let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
        let mut lines = text.split_inclusive('\n').peekable();
        let mut strings = BTreeMap::new();
        let mut without_body = Vec::new();

        while let Some(line) = lines.next() {
            let Some(number) = header_number(trim_line_end(line)) else {
                continue;
            };
            while lines
                .peek()
                .is_some_and(|l| trim_line_end(l).trim_start().starts_with("//"))
            {
                lines.next();
            }
            let opening = lines
                .peek()
                .and_then(|l| l.trim_start_matches([' ', '\t']).strip_prefix('{'));
            let Some(first) = opening else {
                if let Some(number) = number {
                    without_body.push(number);
                    strings.entry(number).or_insert_with(String::new);
                }
                continue;
            };
            let mut body = first.to_owned();
            lines.next();
            if trim_line_end(&body).is_empty() {
                body.clear();
            }
            for line in lines.by_ref() {
                if line.starts_with('}') {
                    break;
                }
                body.push_str(line);
            }
            // The line break before the closing brace is not part of the text.
            body.truncate(trim_line_end(&body).len());
            if let Some(number) = number {
                if let Entry::Vacant(slot) = strings.entry(number) {
                    slot.insert(body);
                }
            }
        }
        Ok(TriggerStrings {
            strings,
            without_body,
        })
    }

    pub fn get(&self, number: u32) -> Option<&str> {
        self.strings.get(&number).map(String::as_str)
    }

    /// Numbers whose header was not followed by a `{`.
    pub fn without_body(&self) -> &[u32] {
        &self.without_body
    }

    pub fn len(&self) -> usize {
        self.strings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }

    /// Text of the entry that `value` refers to, or `None` when `value` is
    /// not a reference or refers to no defined entry.
    pub fn resolve(&self, value: &str) -> Option<&str> {
        match reference(value)? {
            Reference::Entry(number) => self.get(number),
            Reference::Unreachable => None,
        }
    }

    /// What a resolving native (see [`RESOLVING_NATIVES`]) works with when
    /// it receives `value`: the entry text for a reference, empty text for
    /// a reference to no defined entry, and `value` itself otherwise.
    pub fn display<'a>(&'a self, value: &'a str) -> &'a str {
        match reference(value) {
            Some(Reference::Entry(number)) => self.get(number).unwrap_or(""),
            Some(Reference::Unreachable) => "",
            None => value,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reference {
    Entry(u32),
    /// A reference that no entry can match, such as `TRIGSTR_-5`.
    Unreachable,
}

/// A reference starts with exactly `TRIGSTR_`. The number is the decimal
/// digits that follow, up to the first other character; no digits means
/// entry 0. A minus sign or a number beyond 32 bits reaches no entry.
fn reference(value: &str) -> Option<Reference> {
    let rest = value.strip_prefix(REFERENCE_PREFIX)?;
    if rest.starts_with('-') {
        return Some(Reference::Unreachable);
    }
    let digits = &rest[..rest.bytes().take_while(u8::is_ascii_digit).count()];
    if digits.is_empty() {
        return Some(Reference::Entry(0));
    }
    Some(match digits.parse::<u32>() {
        Ok(number) => Reference::Entry(number),
        Err(_) => Reference::Unreachable,
    })
}

/// `Some(Some(n))` for a header with a usable number, `Some(None)` for a
/// header whose number no reference can reach, `None` for other lines.
fn header_number(line: &str) -> Option<Option<u32>> {
    let mut words = line.split_whitespace();
    if words.next()? != "STRING" {
        return None;
    }
    Some(words.next().and_then(|n| n.parse::<u32>().ok()))
}

fn trim_line_end(line: &str) -> &str {
    let line = line.strip_suffix('\n').unwrap_or(line);
    line.strip_suffix('\r').unwrap_or(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> TriggerStrings {
        TriggerStrings::parse(text.as_bytes()).unwrap()
    }

    #[test]
    fn parses_strings_with_comments_and_multiple_lines() {
        let strings = parse(
            "\u{FEFF}STRING 0\r\n{\r\nPlayer 1\r\n}\r\n\r\n\
             STRING 12\r\n// Units: h000 (Hero), Name (Name)\r\n{\r\nfirst\r\nsecond\r\n}\r\n\
             STRING 13\n{\n\n}\n",
        );
        assert_eq!(strings.len(), 3);
        assert_eq!(strings.get(0), Some("Player 1"));
        assert_eq!(strings.get(12), Some("first\r\nsecond"));
        assert_eq!(strings.get(13), Some(""));
    }

    /// The file forms listed under "Reading war3map.wts" in SPEC-0003.
    #[test]
    fn reads_the_file_forms_of_spec_0003() {
        let strings = parse(
            "// a comment\n\
             stray text\n\
               STRING   910  \n{\nspaces around the header\n}\n\
             }\n\
             STRING 911\n{ \ntext\n}\n\
             STRING 912\n{same line\n}x ignored\n\
             STRING 913\n{\n  indented\n }not the end\n}\n\
             STRING 914\n{\nno blank line\n}\nSTRING 915\n{\nnext\n}\n\
             STRING -5\n{\nnegative\n}\n\
             STRING 4294968296\n{\ntoo large\n}\n\
             STRING 916\n{\nruns to the end of the file\n",
        );
        assert_eq!(strings.get(910), Some("spaces around the header"));
        assert_eq!(strings.get(911), Some(" \ntext"));
        assert_eq!(strings.get(912), Some("same line"));
        assert_eq!(strings.get(913), Some("  indented\n }not the end"));
        assert_eq!(strings.get(914), Some("no blank line"));
        assert_eq!(strings.get(915), Some("next"));
        assert_eq!(strings.get(916), Some("runs to the end of the file"));
        assert_eq!(strings.len(), 7);
    }

    /// Shape found in a shipped map: a header followed by a bare value.
    #[test]
    fn header_without_body_is_empty() {
        let strings = parse(
            "STRING 1\r\n{\r\nA\r\n}\r\n\r\nSTRING 2\r\n#VALUE!\r\n\r\nSTRING 3\r\n{\r\nC\r\n}\r\n",
        );
        assert_eq!(strings.get(1), Some("A"));
        assert_eq!(strings.get(2), Some(""));
        assert_eq!(strings.get(3), Some("C"));
        assert_eq!(strings.without_body(), [2]);
    }

    /// Test map W1 from the verification section of SPEC-0003.
    #[test]
    fn spec_0003_test_map_w1() {
        let strings = parse(
            "STRING 900\r\n{\r\nfirst 900\r\n}\r\n\r\n\
             STRING 900\r\n{\r\nsecond 900\r\n}\r\n\r\n\
             STRING 901\r\n#VALUE!\r\n\r\n\
             STRING 902\r\n{\r\ntext 902\r\n}\r\n\r\n\
             STRING 904\r\n{\r\nline one\r\nline two\r\n}\r\n",
        );
        assert_eq!(strings.display("TRIGSTR_900"), "first 900");
        assert_eq!(strings.display("TRIGSTR_901"), "");
        assert_eq!(strings.display("TRIGSTR_902"), "text 902");
        assert_eq!(strings.display("TRIGSTR_903"), "");
        assert_eq!(strings.display("TRIGSTR_904"), "line one\r\nline two");
        assert_eq!(strings.display("TRIGSTR_0902"), "text 902");
        assert_eq!(strings.display("[TRIGSTR_902]"), "[TRIGSTR_902]");
        assert_eq!(strings.display("Lost Temple"), "Lost Temple");
    }

    /// The reference forms of SPEC-0003.
    #[test]
    fn reference_forms_of_spec_0003() {
        let strings = parse("STRING 0\n{\nzero\n}\nSTRING 910\n{\nten\n}\n");
        for (value, shown) in [
            ("TRIGSTR_910abc", "ten"),
            ("TRIGSTR_910 ", "ten"),
            ("TRIGSTR_", "zero"),
            ("TRIGSTR_+910", "zero"),
            ("TRIGSTR_0x38e", "zero"),
            ("TRIGSTR_-5", ""),
            ("TRIGSTR_4294968296", ""),
            ("trigstr_910", "trigstr_910"),
            ("Trigstr_910", "Trigstr_910"),
            (" TRIGSTR_910", " TRIGSTR_910"),
        ] {
            assert_eq!(strings.display(value), shown, "{value:?}");
        }
        assert_eq!(strings.resolve("TRIGSTR_910"), Some("ten"));
        assert_eq!(strings.resolve("TRIGSTR_-5"), None);
        assert_eq!(strings.resolve("plain"), None);
    }

    #[test]
    fn rejects_invalid_utf8() {
        assert_eq!(TriggerStrings::parse(b"\xFF"), Err(Error::InvalidUtf8));
    }
}
