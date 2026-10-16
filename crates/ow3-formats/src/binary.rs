//! Little-endian reader shared by the binary map file formats.

use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// The data ended in the middle of a value starting at `offset`.
    UnexpectedEnd {
        offset: usize,
    },
    /// A string starting at `offset` is not valid UTF-8.
    InvalidUtf8 {
        offset: usize,
    },
    /// Bytes left over after the last field, starting at `offset`.
    TrailingData {
        offset: usize,
    },
    UnsupportedVersion(u32),
    /// A field holds a value the format does not allow.
    Invalid {
        what: &'static str,
        offset: usize,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::UnexpectedEnd { offset } => write!(f, "data ends inside a value at {offset:#x}"),
            Error::InvalidUtf8 { offset } => write!(f, "string at {offset:#x} is not valid UTF-8"),
            Error::TrailingData { offset } => {
                write!(f, "unexpected data after the end at {offset:#x}")
            }
            Error::UnsupportedVersion(v) => write!(f, "unsupported format version {v}"),
            Error::Invalid { what, offset } => write!(f, "invalid {what} at {offset:#x}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// Four-character code identifying a unit, ability, upgrade and so on, such
/// as `hfoo`. Stored as its bytes in reading order.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RawCode(pub [u8; 4]);

impl fmt::Display for RawCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for &b in &self.0 {
            if b.is_ascii_graphic() {
                write!(f, "{}", b as char)?;
            } else {
                write!(f, "\\x{b:02x}")?;
            }
        }
        Ok(())
    }
}

impl fmt::Debug for RawCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RawCode({self})")
    }
}

impl From<&[u8; 4]> for RawCode {
    fn from(bytes: &[u8; 4]) -> Self {
        RawCode(*bytes)
    }
}

pub struct Reader<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Reader { data, position: 0 }
    }

    pub fn position(&self) -> usize {
        self.position
    }

    pub fn bytes(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self
            .position
            .checked_add(len)
            .filter(|&end| end <= self.data.len())
            .ok_or(Error::UnexpectedEnd {
                offset: self.position,
            })?;
        let bytes = &self.data[self.position..end];
        self.position = end;
        Ok(bytes)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.bytes(N)?.try_into().expect("length checked"))
    }

    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.array::<1>()?[0])
    }

    pub fn u32(&mut self) -> Result<u32> {
        self.array().map(u32::from_le_bytes)
    }

    pub fn i32(&mut self) -> Result<i32> {
        self.array().map(i32::from_le_bytes)
    }

    /// Reads the bit pattern as stored; no arithmetic is done on it here.
    pub fn f32(&mut self) -> Result<f32> {
        self.array().map(f32::from_le_bytes)
    }

    pub fn raw_code(&mut self) -> Result<RawCode> {
        self.array().map(RawCode)
    }

    /// A NUL-terminated UTF-8 string.
    pub fn string(&mut self) -> Result<String> {
        let start = self.position;
        let len = self.data[start..]
            .iter()
            .position(|&b| b == 0)
            .ok_or(Error::UnexpectedEnd { offset: start })?;
        let bytes = self.bytes(len)?;
        self.position += 1;
        String::from_utf8(bytes.to_vec()).map_err(|_| Error::InvalidUtf8 { offset: start })
    }

    /// A count followed by that many items. The count is checked against the
    /// remaining data so that a corrupt count cannot cause a huge allocation.
    pub fn list<T>(
        &mut self,
        min_item_size: usize,
        mut item: impl FnMut(&mut Self) -> Result<T>,
    ) -> Result<Vec<T>> {
        let offset = self.position;
        let count = self.u32()? as usize;
        let remaining = self.data.len() - self.position;
        if count.saturating_mul(min_item_size.max(1)) > remaining {
            return Err(Error::Invalid {
                what: "item count",
                offset,
            });
        }
        (0..count).map(|_| item(self)).collect()
    }

    pub fn finish(self) -> Result<()> {
        if self.position == self.data.len() {
            Ok(())
        } else {
            Err(Error::TrailingData {
                offset: self.position,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_values_in_order() {
        let mut data = vec![7u8];
        data.extend_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
        data.extend_from_slice(&(-2i32).to_le_bytes());
        data.extend_from_slice(&1.5f32.to_le_bytes());
        data.extend_from_slice(b"hfoo");
        data.extend_from_slice(b"name\0");
        let mut r = Reader::new(&data);
        assert_eq!(r.u8().unwrap(), 7);
        assert_eq!(r.u32().unwrap(), 0xDEAD_BEEF);
        assert_eq!(r.i32().unwrap(), -2);
        assert_eq!(r.f32().unwrap(), 1.5);
        assert_eq!(r.raw_code().unwrap(), RawCode(*b"hfoo"));
        assert_eq!(r.string().unwrap(), "name");
        r.finish().unwrap();
    }

    #[test]
    fn reports_truncation_and_leftovers() {
        assert_eq!(
            Reader::new(&[1, 2]).u32(),
            Err(Error::UnexpectedEnd { offset: 0 })
        );
        assert_eq!(
            Reader::new(b"abc").string(),
            Err(Error::UnexpectedEnd { offset: 0 })
        );
        let mut r = Reader::new(&[1, 2]);
        r.u8().unwrap();
        assert_eq!(r.finish(), Err(Error::TrailingData { offset: 1 }));
    }

    #[test]
    fn rejects_counts_larger_than_the_data() {
        let data = 1000u32.to_le_bytes();
        let result = Reader::new(&data).list(4, |r| r.u32());
        assert_eq!(
            result,
            Err(Error::Invalid {
                what: "item count",
                offset: 0
            })
        );
    }

    #[test]
    fn raw_codes_display_as_text() {
        assert_eq!(RawCode(*b"hfoo").to_string(), "hfoo");
        assert_eq!(RawCode([b'A', 0, b'b', b'c']).to_string(), "A\\x00bc");
    }
}
