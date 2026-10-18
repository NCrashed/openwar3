//! `war3map.shd`: the shadow map the editor bakes from doodads and
//! cliffs, one byte per cell of 32 world units (4 x 4 per tile), rows from
//! the lower left corner of the map; 0xFF is shadow, 0x00 none.
//!
//! The file has no header: its size gives the map's width and height in
//! tiles (one less than the points of `war3map.w3e`) times 16. Layout
//! reference: the community W3X/W3M format notes (originally by Zepir,
//! revised on Hive Workshop); the row order was checked against the cliff
//! edges of `(12)IceCrown`, whose shadows lie on them.

use crate::binary::{Error, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shadows {
    pub width: u32,
    pub height: u32,
    /// Whether each cell is in shadow, row by row from the bottom.
    pub cells: Vec<bool>,
}

impl Shadows {
    /// The shadow map of a map whose terrain has `tiles_x` by `tiles_y`
    /// tiles.
    pub fn parse(data: &[u8], tiles_x: u32, tiles_y: u32) -> Result<Shadows> {
        let (width, height) = (tiles_x * 4, tiles_y * 4);
        if data.len() != (width * height) as usize {
            return Err(Error::Invalid {
                what: "shadow map size",
                offset: 0,
            });
        }
        Ok(Shadows {
            width,
            height,
            cells: data.iter().map(|&b| b != 0).collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_and_size() {
        let mut data = vec![0u8; 16];
        data[5] = 0xFF;
        let s = Shadows::parse(&data, 1, 1).unwrap();
        assert_eq!((s.width, s.height), (4, 4));
        assert!(s.cells[5] && !s.cells[4]);
        assert!(Shadows::parse(&data, 2, 1).is_err());
    }
}
