//! TGA images, as the pathing textures of the game use them
//! (`PathTextures\*.tga`).
//!
//! Supports true-colour images of 24 or 32 bits per pixel, plain (type 2)
//! or run-length encoded (type 10). Layout reference: the Truevision TGA
//! file format specification, version 2.0.

use crate::binary::{Error, Reader, Result};

/// An image as RGBA rows from the bottom row up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tga {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Tga {
    pub fn parse(data: &[u8]) -> Result<Tga> {
        let mut r = Reader::new(data);
        let id_length = r.u8()?;
        let colour_map = r.u8()?;
        let kind = r.u8()?;
        let _colour_map_spec = r.bytes(5)?;
        let _origin = (r.u16()?, r.u16()?);
        let (width, height) = (u32::from(r.u16()?), u32::from(r.u16()?));
        let bits = r.u8()?;
        let descriptor = r.u8()?;
        if colour_map != 0 || !matches!(kind, 2 | 10) || !matches!(bits, 24 | 32) {
            return Err(Error::Invalid {
                what: "TGA kind",
                offset: 2,
            });
        }
        r.bytes(usize::from(id_length))?;
        let size = (bits / 8) as usize;
        let count = (width * height) as usize;
        let mut pixels = Vec::with_capacity(count * 4);
        let mut push = |p: &[u8]| {
            let alpha = if size == 4 { p[3] } else { 255 };
            pixels.extend_from_slice(&[p[2], p[1], p[0], alpha]);
        };
        let mut done = 0;
        while done < count {
            if kind == 2 {
                push(r.bytes(size)?);
                done += 1;
                continue;
            }
            let header = r.u8()?;
            let run = usize::from(header & 0x7F) + 1;
            if header & 0x80 != 0 {
                let p = r.bytes(size)?;
                for _ in 0..run {
                    push(p);
                }
            } else {
                for _ in 0..run {
                    push(r.bytes(size)?);
                }
            }
            done += run;
        }
        pixels.truncate(count * 4);
        // Bit 5 of the descriptor: rows stored from the top.
        if descriptor & 0x20 != 0 {
            let row = (width * 4) as usize;
            let flipped = pixels.chunks(row).rev().flatten().copied().collect();
            pixels = flipped;
        }
        Ok(Tga {
            width,
            height,
            rgba: pixels,
        })
    }

    /// The RGBA of pixel (`x`, `y`), `y` counted from the bottom row.
    pub fn at(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(kind: u8, w: u16, h: u16, bits: u8, descriptor: u8) -> Vec<u8> {
        let mut data = vec![0, 0, kind, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        data.extend_from_slice(&w.to_le_bytes());
        data.extend_from_slice(&h.to_le_bytes());
        data.extend_from_slice(&[bits, descriptor]);
        data
    }

    #[test]
    fn plain_bottom_up() {
        let mut data = header(2, 2, 2, 24, 0);
        // Bottom row: red, green; top row: blue, white (BGR).
        data.extend_from_slice(&[0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255]);
        let t = Tga::parse(&data).unwrap();
        assert_eq!(t.at(0, 0), [255, 0, 0, 255]);
        assert_eq!(t.at(1, 0), [0, 255, 0, 255]);
        assert_eq!(t.at(0, 1), [0, 0, 255, 255]);
    }

    #[test]
    fn run_length_top_down() {
        let mut data = header(10, 2, 2, 32, 0x20);
        // Top row: a run of two red; bottom row: two raw pixels.
        data.extend_from_slice(&[0x81, 0, 0, 255, 255]);
        data.extend_from_slice(&[0x01, 255, 0, 0, 255, 0, 255, 0, 255]);
        let t = Tga::parse(&data).unwrap();
        assert_eq!(t.at(0, 1), [255, 0, 0, 255]);
        assert_eq!(t.at(1, 1), [255, 0, 0, 255]);
        assert_eq!(t.at(0, 0), [0, 0, 255, 255]);
        assert_eq!(t.at(1, 0), [0, 255, 0, 255]);
    }
}
