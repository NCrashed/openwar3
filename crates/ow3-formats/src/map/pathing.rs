//! `war3map.wpm`: the pathing map, one byte of flags per cell of 32 world
//! units.
//!
//! Layout: `MP3W`, version 0, width and height in cells, then the cells
//! row by row from the lower left corner of the map. Layout reference:
//! the community W3X/W3M format notes (originally by Zepir, revised on
//! Hive Workshop); the row order was checked on test maps with walls at
//! known places.

use crate::binary::{Error, Reader, Result};

const MAGIC: &[u8; 4] = b"MP3W";
pub const VERSION: u32 = 0;

/// Not walkable.
pub const NO_WALK: u8 = 0x02;
/// Not flyable.
pub const NO_FLY: u8 = 0x04;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pathing {
    pub width: u32,
    pub height: u32,
    /// The flags of each cell, row by row from the bottom.
    pub cells: Vec<u8>,
}

impl Pathing {
    pub fn parse(data: &[u8]) -> Result<Pathing> {
        let mut r = Reader::new(data);
        if r.bytes(4)? != MAGIC {
            return Err(Error::Invalid {
                what: "pathing signature",
                offset: 0,
            });
        }
        let version = r.u32()?;
        if version != VERSION {
            return Err(Error::UnsupportedVersion(version));
        }
        let (width, height) = (r.u32()?, r.u32()?);
        let count = (width as usize)
            .checked_mul(height as usize)
            .ok_or(Error::Invalid {
                what: "pathing size",
                offset: 8,
            })?;
        let cells = r.bytes(count)?.to_vec();
        r.finish()?;
        Ok(Pathing {
            width,
            height,
            cells,
        })
    }

    /// The flags of cell (`x`, `y`), counted from the lower left corner.
    pub fn at(&self, x: u32, y: u32) -> Option<u8> {
        (x < self.width && y < self.height).then(|| self.cells[(y * self.width + x) as usize])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cells_from_the_bottom() {
        let mut data = b"MP3W".to_vec();
        for v in [0u32, 3, 2] {
            data.extend_from_slice(&v.to_le_bytes());
        }
        data.extend_from_slice(&[0, 0, 0, 0, 0xCA, 0]);
        let p = Pathing::parse(&data).unwrap();
        assert_eq!((p.width, p.height), (3, 2));
        assert_eq!(p.at(1, 1), Some(0xCA));
        assert_eq!(p.at(1, 0), Some(0));
        assert_eq!(p.at(3, 0), None);
        data.pop();
        assert!(Pathing::parse(&data).is_err());
    }
}
