//! `war3map.w3e`: the terrain grid, with one point per tile corner.
//!
//! Supports format version 11, used by every map up to 1.27. Layout
//! reference: the community W3X/W3M format notes (originally by Zepir,
//! revised on Hive Workshop).
//!
//! Each point is stored in 7 bytes. They are kept as stored and decoded by
//! accessor methods, so that nothing is lost when a field's meaning is
//! uncertain.

use crate::binary::{Error, RawCode, Reader, Result};

const MAGIC: &[u8; 4] = b"W3E!";
pub const VERSION: u32 = 11;
const POINT_SIZE: usize = 7;

#[derive(Clone, Debug, PartialEq)]
pub struct Terrain {
    pub format_version: u32,
    /// Tileset letter, such as `L` for Lordaeron Summer.
    pub tileset: u8,
    pub custom_tilesets: bool,
    /// Ground textures; a point's ground texture indexes this list.
    pub ground_tilesets: Vec<RawCode>,
    /// Cliff types; a point's cliff texture indexes this list.
    pub cliff_tilesets: Vec<RawCode>,
    /// Number of points per row: one more than the width in tiles.
    pub width: u32,
    /// Number of rows: one more than the height in tiles.
    pub height: u32,
    /// World coordinates of the first point.
    pub origin_x: f32,
    pub origin_y: f32,
    /// Row by row, starting from the bottom-left corner.
    pub points: Vec<TerrainPoint>,
}

/// One tile corner, as stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerrainPoint {
    pub ground_height: u16,
    /// Water level in the low 14 bits, plus flag bits above it.
    pub water_and_edge: u16,
    /// Flags in the high nibble, ground texture index in the low nibble.
    pub flags_and_texture: u8,
    pub texture_details: u8,
    /// Cliff texture index in the high nibble, layer height in the low one.
    pub cliff_and_layer: u8,
}

impl TerrainPoint {
    pub fn water_level(&self) -> u16 {
        self.water_and_edge & 0x3FFF
    }

    /// Bit 14 of the water field, described as the map edge boundary flag.
    pub fn edge_boundary(&self) -> bool {
        self.water_and_edge & 0x4000 != 0
    }

    pub fn ground_texture(&self) -> u8 {
        self.flags_and_texture & 0x0F
    }

    /// The four flag bits: in community notes 0x1 ramp, 0x2 blight,
    /// 0x4 water, 0x8 boundary.
    pub fn flags(&self) -> u8 {
        self.flags_and_texture >> 4
    }

    pub fn cliff_texture(&self) -> u8 {
        self.cliff_and_layer >> 4
    }

    pub fn layer_height(&self) -> u8 {
        self.cliff_and_layer & 0x0F
    }
}

impl Terrain {
    pub fn parse(data: &[u8]) -> Result<Terrain> {
        let mut r = Reader::new(data);
        if r.bytes(4)? != MAGIC {
            return Err(Error::Invalid {
                what: "terrain signature",
                offset: 0,
            });
        }
        let format_version = r.u32()?;
        if format_version != VERSION {
            return Err(Error::UnsupportedVersion(format_version));
        }
        let tileset = r.u8()?;
        let custom_tilesets = r.u32()? != 0;
        let ground_tilesets = r.list(4, Reader::raw_code)?;
        let cliff_tilesets = r.list(4, Reader::raw_code)?;
        let size_offset = r.position();
        let width = r.u32()?;
        let height = r.u32()?;
        let origin_x = r.f32()?;
        let origin_y = r.f32()?;

        let count = (width as usize)
            .checked_mul(height as usize)
            .filter(|&n| n.checked_mul(POINT_SIZE).is_some())
            .ok_or(Error::Invalid {
                what: "terrain size",
                offset: size_offset,
            })?;
        let points = r
            .bytes(count * POINT_SIZE)?
            .chunks_exact(POINT_SIZE)
            .map(|p| TerrainPoint {
                ground_height: u16::from_le_bytes([p[0], p[1]]),
                water_and_edge: u16::from_le_bytes([p[2], p[3]]),
                flags_and_texture: p[4],
                texture_details: p[5],
                cliff_and_layer: p[6],
            })
            .collect();
        r.finish()?;

        Ok(Terrain {
            format_version,
            tileset,
            custom_tilesets,
            ground_tilesets,
            cliff_tilesets,
            width,
            height,
            origin_x,
            origin_y,
            points,
        })
    }

    /// Point at column `x` and row `y`, counted from the bottom-left corner.
    pub fn point(&self, x: u32, y: u32) -> Option<&TerrainPoint> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.points
            .get(y as usize * self.width as usize + x as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terrain_file(width: u32, height: u32, points: &[[u8; 7]]) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.push(b'L');
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(b"LdrtLgrs");
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(b"CLdi");
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        out.extend_from_slice(&(-128.0f32).to_le_bytes());
        out.extend_from_slice(&(-256.0f32).to_le_bytes());
        for point in points {
            out.extend_from_slice(point);
        }
        out
    }

    #[test]
    fn parses_header_and_points() {
        let points = [
            [0x00, 0x20, 0x00, 0x1A, 0x00, 0, 0],
            [0x34, 0x12, 0xCD, 0x5B, 0x63, 0x0E, 0xA2],
            [0; 7],
            [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF],
        ];
        let terrain = Terrain::parse(&terrain_file(2, 2, &points)).unwrap();
        assert_eq!((terrain.tileset, terrain.custom_tilesets), (b'L', true));
        assert_eq!(
            terrain.ground_tilesets,
            [RawCode(*b"Ldrt"), RawCode(*b"Lgrs")]
        );
        assert_eq!(terrain.cliff_tilesets, [RawCode(*b"CLdi")]);
        assert_eq!((terrain.origin_x, terrain.origin_y), (-128.0, -256.0));

        let p = terrain.point(1, 0).unwrap();
        assert_eq!(p.ground_height, 0x1234);
        assert_eq!(p.water_level(), 0x1BCD);
        assert!(p.edge_boundary());
        assert_eq!((p.flags(), p.ground_texture()), (0x6, 0x3));
        assert_eq!(p.texture_details, 0x0E);
        assert_eq!((p.cliff_texture(), p.layer_height()), (0xA, 0x2));
        assert_eq!(terrain.point(1, 1).unwrap().ground_height, 0xFFFF);
        assert_eq!(terrain.point(2, 0), None);
    }

    #[test]
    fn rejects_bad_files() {
        let mut file = terrain_file(2, 2, &[[0; 7]; 4]);
        assert!(Terrain::parse(&file[..file.len() - 1]).is_err());
        file.push(0);
        assert!(matches!(
            Terrain::parse(&file),
            Err(Error::TrailingData { .. })
        ));
        let mut wrong_version = terrain_file(1, 1, &[[0; 7]]);
        wrong_version[4] = 12;
        assert_eq!(
            Terrain::parse(&wrong_version),
            Err(Error::UnsupportedVersion(12))
        );
        let mut wrong_magic = terrain_file(1, 1, &[[0; 7]]);
        wrong_magic[0] = b'X';
        assert!(matches!(
            Terrain::parse(&wrong_magic),
            Err(Error::Invalid { .. })
        ));
    }
}
