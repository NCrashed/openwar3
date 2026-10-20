//! Ground heights of a map for `GetLocationZ` (SPEC-0014).
//!
//! A point of `war3map.w3e` is at `(h - 8192) / 4 + (layer - 2) * 128`,
//! with layer 15 counted as 14; these are multiples of 0.25 and exact as
//! reals. Inside a tile the height is interpolated bilinearly between its
//! corners, across cliffs and ramps too.
//!
//! PROVISIONAL: the order of the operations of the interpolation is not
//! specified; here it is `lerp(lerp(bl, br, u), lerp(tl, tr, u), v)` with
//! `lerp(a, b, t) = add(a, mul(sub(b, a), t))`. Places outside the grid
//! take the height of the nearest edge. SPEC-0014 open question 3 (small
//! raised areas) is not modelled.

use ow3_formats::map::terrain::Terrain;
use ow3_math::{add, floor, i2r, mul, r2i, sub};

/// 1 / 128, the size of a tile.
const PER_TILE: u32 = 0x3C00_0000;
const QUARTER: u32 = 0x3E80_0000;

/// The heights of the points of a map, as bit patterns of reals.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Heights {
    origin: [u32; 2],
    width: u32,
    height: u32,
    points: Vec<u32>,
    /// The cliff level of each point (SPEC-0032).
    layers: Vec<u8>,
}

fn lerp(a: u32, b: u32, t: u32) -> u32 {
    add(a, mul(sub(b, a), t))
}

impl Heights {
    pub fn new(terrain: &Terrain) -> Heights {
        let points = terrain
            .points
            .iter()
            .map(|p| {
                let ground = mul(i2r(i32::from(p.ground_height) - 8192), QUARTER);
                let layer = i32::from(p.layer_height().min(14)) - 2;
                add(ground, i2r(layer * 128))
            })
            .collect();
        Heights {
            origin: [terrain.origin_x.to_bits(), terrain.origin_y.to_bits()],
            width: terrain.width,
            height: terrain.height,
            points,
            layers: terrain.points.iter().map(|p| p.layer_height()).collect(),
        }
    }

    fn point(&self, x: u32, y: u32) -> u32 {
        self.points[(y * self.width + x) as usize]
    }

    /// The tile index along one axis and the place inside it (0..1).
    fn axis(v: u32, origin: u32, points: u32) -> (u32, u32) {
        let f = mul(sub(v, origin), PER_TILE);
        let last = points.saturating_sub(2) as i32;
        let tile = r2i(floor(f)).clamp(0, last);
        let t = sub(f, i2r(tile));
        let t = if f32::from_bits(t) < 0.0 {
            0
        } else if f32::from_bits(t) > 1.0 {
            0x3F80_0000
        } else {
            t
        };
        (tile as u32, t)
    }

    /// The ground height at (`x`, `y`), or 0 without terrain.
    pub fn at(&self, x: u32, y: u32) -> u32 {
        if self.width < 2 || self.height < 2 {
            return 0;
        }
        let (tx, u) = Heights::axis(x, self.origin[0], self.width);
        let (ty, v) = Heights::axis(y, self.origin[1], self.height);
        let bottom = lerp(self.point(tx, ty), self.point(tx + 1, ty), u);
        let top = lerp(self.point(tx, ty + 1), self.point(tx + 1, ty + 1), u);
        lerp(bottom, top, v)
    }

    /// The tiles across and up, the fog cells of SPEC-0032.
    pub fn cells(&self) -> (u32, u32) {
        (self.width.saturating_sub(1), self.height.saturating_sub(1))
    }

    /// The highest cliff level of the four corners of tile (`i`, `j`).
    pub fn cell_layer(&self, i: u32, j: u32) -> u8 {
        [(0, 0), (1, 0), (0, 1), (1, 1)]
            .into_iter()
            .map(|(a, b)| self.layers[((j + b) * self.width + i + a) as usize])
            .max()
            .unwrap_or(0)
    }

    /// The tile that (`x`, `y`) lies in, outside the map `None`.
    pub fn cell_of(&self, x: u32, y: u32) -> Option<(u32, u32)> {
        let (w, h) = self.cells();
        let index = |v: u32, origin: u32, count: u32| {
            let i = r2i(floor(mul(sub(v, origin), PER_TILE)));
            u32::try_from(i).ok().filter(|&i| i < count)
        };
        Some((index(x, self.origin[0], w)?, index(y, self.origin[1], h)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ow3_formats::binary::RawCode;
    use ow3_formats::map::terrain::TerrainPoint;

    fn terrain(points: Vec<(u16, u8)>) -> Terrain {
        Terrain {
            format_version: 11,
            tileset: b'L',
            custom_tilesets: false,
            ground_tilesets: vec![RawCode(*b"Ldrt")],
            cliff_tilesets: Vec::new(),
            width: 2,
            height: 2,
            origin_x: -128.0,
            origin_y: 0.0,
            points: points
                .into_iter()
                .map(|(h, layer)| TerrainPoint {
                    ground_height: h,
                    water_and_edge: 0,
                    flags_and_texture: 0,
                    texture_details: 0,
                    cliff_and_layer: layer,
                })
                .collect(),
        }
    }

    fn at(heights: &Heights, x: f32, y: f32) -> f32 {
        f32::from_bits(heights.at(x.to_bits(), y.to_bits()))
    }

    #[test]
    fn points_and_layers() {
        // Raw 7192 -> -250, 9192 -> 250; layer 0 -> -256, 15 like 14.
        let h = Heights::new(&terrain(vec![(7192, 2), (9192, 2), (8192, 0), (8192, 15)]));
        assert_eq!(at(&h, -128.0, 0.0), -250.0);
        assert_eq!(at(&h, 0.0, 0.0), 250.0);
        assert_eq!(at(&h, -128.0, 128.0), -256.0);
        assert_eq!(at(&h, 0.0, 128.0), 1536.0);
    }

    #[test]
    fn bilinear_and_across_levels() {
        // Corners 0, 100 (BL, BR) and 200, 400 (TL, TR).
        let h = Heights::new(&terrain(vec![(8192, 2), (8592, 2), (8992, 2), (9792, 2)]));
        assert_eq!(at(&h, -64.0, 64.0), 175.0);
        assert_eq!(at(&h, -96.0, 0.0), 25.0);
        // From layer 2 to layer 4 over one tile: 0 to 256, linearly.
        let h = Heights::new(&terrain(vec![(8192, 2), (8192, 2), (8192, 4), (8192, 4)]));
        assert_eq!(at(&h, -64.0, 32.0), 64.0);
        // Outside the grid: the nearest edge.
        assert_eq!(at(&h, -1000.0, 1000.0), 256.0);
    }
}
