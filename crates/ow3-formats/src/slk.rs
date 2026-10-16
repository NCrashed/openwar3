//! Reader for SYLK (`.slk`) spreadsheets, which hold most Warcraft III game
//! data tables.
//!
//! Format reference: the SYLK description on Wikipedia and the Multiplan
//! file format notes it cites. A file is a list of records, one per line.
//! Each record is a type followed by `;`-separated fields, where a doubled
//! `;;` is a literal semicolon. Only the parts that affect cell values are
//! interpreted:
//!
//! - `ID` must be the first record.
//! - `B;X<cols>;Y<rows>` gives the declared size.
//! - `C` records set a cell. `X<col>` and `Y<row>` move a cursor; a missing
//!   coordinate keeps its cursor value, and `F` (format) records move the
//!   cursor too. `K<value>` is the cell value; for formulas (`E...`) it is
//!   the value computed when the file was saved.
//! - `E` ends the file.
//!
//! Interpreting the table (header row, row keys, default values) is left to
//! the caller.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// The data does not start with an `ID` record.
    NotSylk,
    InvalidUtf8 {
        line: usize,
    },
    /// A coordinate field that is not a positive integer.
    InvalidCoordinate {
        line: usize,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotSylk => write!(f, "not a SYLK file"),
            Error::InvalidUtf8 { line } => write!(f, "line {line} is not valid UTF-8"),
            Error::InvalidCoordinate { line } => {
                write!(f, "invalid cell coordinate on line {line}")
            }
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// A cell value as written in the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Text(String),
    /// A number, kept as its source text so that callers decide how to
    /// convert it.
    Number(String),
    Bool(bool),
    /// A spreadsheet error such as `#VALUE!`.
    Error(String),
}

impl Value {
    fn parse(raw: &str) -> Value {
        if let Some(quoted) = raw.strip_prefix('"') {
            return Value::Text(quoted.strip_suffix('"').unwrap_or(quoted).to_owned());
        }
        match raw {
            "TRUE" => Value::Bool(true),
            "FALSE" => Value::Bool(false),
            _ if raw.starts_with('#') => Value::Error(raw.to_owned()),
            _ if is_number(raw) => Value::Number(raw.to_owned()),
            _ => Value::Text(raw.to_owned()),
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Value::Text(text) => Some(text),
            _ => None,
        }
    }

    /// Source text of any value: the string itself, or the number, boolean
    /// or error as written.
    pub fn raw(&self) -> &str {
        match self {
            Value::Text(text) | Value::Number(text) | Value::Error(text) => text,
            Value::Bool(true) => "TRUE",
            Value::Bool(false) => "FALSE",
        }
    }
}

/// Decimal number with optional sign, fraction and exponent.
fn is_number(raw: &str) -> bool {
    let unsigned = raw.strip_prefix(['-', '+']).unwrap_or(raw);
    let (mantissa, exponent) = match unsigned.split_once(['e', 'E']) {
        Some((m, e)) => (m, Some(e.strip_prefix(['-', '+']).unwrap_or(e))),
        None => (unsigned, None),
    };
    let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    (!int.is_empty() || !frac.is_empty())
        && digits(int)
        && digits(frac)
        && exponent.is_none_or(|e| !e.is_empty() && digits(e))
}

/// Cells by 1-based (row, column).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Table {
    /// Size from the `B` record, if present. Cells may lie outside it.
    pub declared_rows: u32,
    pub declared_columns: u32,
    cells: BTreeMap<(u32, u32), Value>,
}

impl Table {
    pub fn parse(data: &[u8]) -> Result<Table> {
        let mut table = Table::default();
        let (mut row, mut column) = (1u32, 1u32);
        for (index, line) in data.split(|&b| b == b'\n').enumerate() {
            let number = index + 1;
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            let line =
                std::str::from_utf8(line).map_err(|_| Error::InvalidUtf8 { line: number })?;
            let fields = split_fields(line);
            let kind = fields.first().map(Cow::as_ref).unwrap_or("");
            if index == 0 {
                if kind != "ID" {
                    return Err(Error::NotSylk);
                }
                continue;
            }
            let coordinate = |field: &str| {
                field[1..]
                    .parse::<u32>()
                    .ok()
                    .filter(|&n| n > 0)
                    .ok_or(Error::InvalidCoordinate { line: number })
            };
            match kind {
                "E" => break,
                "B" => {
                    for field in &fields[1..] {
                        match field.as_bytes().first() {
                            Some(b'X') => table.declared_columns = coordinate(field)?,
                            Some(b'Y') => table.declared_rows = coordinate(field)?,
                            _ => {}
                        }
                    }
                }
                "C" | "F" => {
                    let mut value = None;
                    for field in &fields[1..] {
                        match field.as_bytes().first() {
                            Some(b'X') => column = coordinate(field)?,
                            Some(b'Y') => row = coordinate(field)?,
                            Some(b'K') if kind == "C" => value = Some(Value::parse(&field[1..])),
                            _ => {}
                        }
                    }
                    if let Some(value) = value {
                        table.cells.insert((row, column), value);
                    }
                }
                _ => {}
            }
        }
        Ok(table)
    }

    pub fn get(&self, row: u32, column: u32) -> Option<&Value> {
        self.cells.get(&(row, column))
    }

    /// All cells in row-major order.
    pub fn cells(&self) -> impl Iterator<Item = ((u32, u32), &Value)> {
        self.cells.iter().map(|(&at, value)| (at, value))
    }

    /// Highest row that has a cell.
    pub fn last_row(&self) -> u32 {
        self.cells.keys().next_back().map_or(0, |&(row, _)| row)
    }

    /// Cells of one row by column.
    pub fn row(&self, row: u32) -> impl Iterator<Item = (u32, &Value)> {
        self.cells
            .range((row, 0)..=(row, u32::MAX))
            .map(|(&(_, column), value)| (column, value))
    }

    /// Column of the first cell in row 1 whose text equals `name`.
    pub fn column(&self, name: &str) -> Option<u32> {
        self.row(1)
            .find(|(_, value)| value.raw() == name)
            .map(|(column, _)| column)
    }
}

/// Splits a record into fields at `;`, treating `;;` as a literal `;`.
/// Fields borrow from the line unless they contain an escaped semicolon.
fn split_fields(line: &str) -> Vec<Cow<'_, str>> {
    if !line.contains(";;") {
        return line.split(';').map(Cow::Borrowed).collect();
    }
    let mut fields = vec![String::new()];
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == ';' {
            if chars.peek() == Some(&';') {
                chars.next();
                fields.last_mut().unwrap().push(';');
            } else {
                fields.push(String::new());
            }
        } else {
            fields.last_mut().unwrap().push(ch);
        }
    }
    fields.into_iter().map(Cow::Owned).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Value {
        Value::Text(s.to_owned())
    }

    #[test]
    fn reads_cells_with_cursor_rules() {
        let data = b"ID;PWXL;N;E\r\n\
            B;Y3;X3;D0 0 2 2\r\n\
            C;Y1;X1;K\"unitID\"\r\n\
            C;X2;K\"hp\"\r\n\
            F;P38;FF3C;X3\r\n\
            C;K\"speed\"\r\n\
            C;Y2;X1;K\"hfoo\"\r\n\
            C;X2;K420\r\n\
            C;X3;K-1.5e2;EIF(TRUE,1,2)\r\n\
            C;Y3;X1;K\"hkni\"\r\n\
            C;X3;KTRUE\r\n\
            E\r\n\
            C;Y9;X9;K\"after end\"\r\n";
        let table = Table::parse(data).unwrap();
        assert_eq!((table.declared_rows, table.declared_columns), (3, 3));
        assert_eq!(table.get(1, 3), Some(&text("speed")));
        assert_eq!(table.get(2, 2), Some(&Value::Number("420".into())));
        assert_eq!(table.get(2, 3), Some(&Value::Number("-1.5e2".into())));
        assert_eq!(table.get(3, 3), Some(&Value::Bool(true)));
        assert_eq!(table.get(3, 2), None);
        assert_eq!(table.get(9, 9), None, "records after E are ignored");
        assert_eq!(table.last_row(), 3);
        assert_eq!(table.column("hp"), Some(2));
        let row: Vec<_> = table.row(2).map(|(c, v)| (c, v.raw())).collect();
        assert_eq!(row, [(1, "hfoo"), (2, "420"), (3, "-1.5e2")]);
    }

    #[test]
    fn parses_value_kinds() {
        assert_eq!(Value::parse("\"a;b\""), text("a;b"));
        assert_eq!(Value::parse("\"unterminated"), text("unterminated"));
        assert_eq!(Value::parse("#VALUE!"), Value::Error("#VALUE!".into()));
        assert_eq!(Value::parse("FALSE"), Value::Bool(false));
        assert_eq!(Value::parse(".5"), Value::Number(".5".into()));
        assert_eq!(Value::parse("1e"), text("1e"));
        assert_eq!(Value::parse("-"), text("-"));
    }

    #[test]
    fn doubled_semicolon_is_literal() {
        let table = Table::parse(b"ID;P\nC;Y1;X1;K\"a;;b\";AComment;;with semicolon\n").unwrap();
        assert_eq!(table.get(1, 1), Some(&text("a;b")));
    }

    #[test]
    fn later_cells_replace_earlier_ones() {
        let table = Table::parse(b"ID;P\nC;Y1;X1;K1\nC;Y1;X1;K2\n").unwrap();
        assert_eq!(table.get(1, 1), Some(&Value::Number("2".into())));
    }

    #[test]
    fn rejects_invalid_input() {
        assert_eq!(Table::parse(b"C;Y1;X1;K1\n"), Err(Error::NotSylk));
        assert_eq!(
            Table::parse(b"ID;P\nC;Y0;X1;K1\n"),
            Err(Error::InvalidCoordinate { line: 2 })
        );
        assert_eq!(
            Table::parse(b"ID;P\nC;Y1;X1;K\"\xFF\"\n"),
            Err(Error::InvalidUtf8 { line: 2 })
        );
    }
}
