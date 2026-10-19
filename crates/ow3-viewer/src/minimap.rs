//! The minimap in its rectangle of the console (SPEC-0028).
//!
//! PROVISIONAL: SPEC-0028 gives only the rectangle. Here it shows the
//! map's `war3mapMap.blp` stretched over the playable area (the terrain
//! less the camera bound complements of `war3map.w3i`), the units as
//! squares coloured by how the local player sees them (own green, allies
//! yellow, enemies red, as the game's default without ally colours), gold
//! mines in gold, no creeps, and the part of the ground the camera sees as
//! a dotted outline. A left click on it moves the camera there.

use crate::console::Rect;
use crate::ui::{Blend, Quad};

/// The minimap's rectangle (SPEC-0028).
pub const MINIMAP: Rect = [0.009_375, 0.006_875, 0.148_75, 0.145];

/// Sizes of the squares in interface units: units, buildings, gold mines.
const UNIT: f32 = 0.0022;
const BUILDING: f32 = 0.0040;
const MINE: f32 = 0.0055;
/// Dots along each side of the camera's outline, and their size.
const OUTLINE_DOTS: usize = 32;
const OUTLINE: f32 = 0.0011;

/// What a square on the minimap stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Own,
    Ally,
    Enemy,
    Mine,
}

/// The minimap of a map.
#[derive(Clone, Debug, PartialEq)]
pub struct Minimap {
    /// The world area shown: left, bottom, right, top.
    area: [f32; 4],
}

impl Minimap {
    /// The minimap of `map`: its playable area.
    pub fn new(map: &ow3_sim::map::Map) -> Minimap {
        let b = &map.world_bounds;
        let [left, right, bottom, top] =
            map.info.camera_bounds_complements.map(|c| c as f32 * 128.0);
        Minimap {
            area: [
                b.min_x + left,
                b.min_y + bottom,
                b.max_x - right,
                b.max_y - top,
            ],
        }
    }

    /// The world point under interface point `p`, if it is on the minimap.
    pub fn to_world(&self, p: [f32; 2]) -> Option<[f32; 2]> {
        let [x0, y0, x1, y1] = MINIMAP;
        if p[0] < x0 || p[0] > x1 || p[1] < y0 || p[1] > y1 {
            return None;
        }
        let [a0, b0, a1, b1] = self.area;
        Some([
            a0 + (p[0] - x0) / (x1 - x0) * (a1 - a0),
            b0 + (p[1] - y0) / (y1 - y0) * (b1 - b0),
        ])
    }

    /// The interface point of world point `w`, if it is in the area.
    fn to_interface(&self, w: [f32; 2]) -> Option<[f32; 2]> {
        let [a0, b0, a1, b1] = self.area;
        if w[0] < a0 || w[0] > a1 || w[1] < b0 || w[1] > b1 {
            return None;
        }
        let [x0, y0, x1, y1] = MINIMAP;
        Some([
            x0 + (w[0] - a0) / (a1 - a0) * (x1 - x0),
            y0 + (w[1] - b0) / (b1 - b0) * (y1 - y0),
        ])
    }

    /// The minimap's quads: the image, the marks (`(x, y, building, mark)`)
    /// and the outline of the ground `view` the camera sees (its corners in
    /// order round it), over the image.
    pub fn quads(
        &self,
        marks: &[([f32; 2], bool, Mark)],
        view: Option<[[f32; 2]; 4]>,
    ) -> Vec<Quad> {
        let square = |at: [f32; 2], size: f32, colour: [f32; 4]| Quad {
            rect: [
                at[0] - size / 2.0,
                at[1] - size / 2.0,
                at[0] + size / 2.0,
                at[1] + size / 2.0,
            ],
            uv: [0.0, 1.0, 0.0, 1.0],
            texture: String::new(),
            colour,
            blend: Blend::Alpha,
        };
        let mut out = vec![Quad {
            rect: MINIMAP,
            uv: [0.0, 1.0, 0.0, 1.0],
            texture: "war3mapMap.blp".to_owned(),
            colour: [1.0; 4],
            blend: Blend::Alpha,
        }];
        for &(at, building, mark) in marks {
            let Some(p) = self.to_interface(at) else {
                continue;
            };
            let (size, colour) = match mark {
                Mark::Mine => (MINE, [1.0, 0.84, 0.0, 1.0]),
                Mark::Own => (UNIT, [0.0, 1.0, 0.0, 1.0]),
                Mark::Ally => (UNIT, [1.0, 1.0, 0.0, 1.0]),
                Mark::Enemy => (UNIT, [1.0, 0.0, 0.0, 1.0]),
            };
            let size = if building && mark != Mark::Mine {
                BUILDING
            } else {
                size
            };
            out.push(square(p, size, colour));
        }
        if let Some(corners) = view {
            for i in 0..4 {
                let (a, b) = (corners[i], corners[(i + 1) % 4]);
                for k in 0..OUTLINE_DOTS {
                    let t = k as f32 / OUTLINE_DOTS as f32;
                    let w = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
                    if let Some(p) = self.to_interface(w) {
                        out.push(square(p, OUTLINE, [1.0; 4]));
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_and_interface_points_agree() {
        let m = Minimap {
            area: [-1000.0, -2000.0, 1000.0, 2000.0],
        };
        let centre = [
            (MINIMAP[0] + MINIMAP[2]) / 2.0,
            (MINIMAP[1] + MINIMAP[3]) / 2.0,
        ];
        let w = m.to_world(centre).unwrap();
        assert!(w[0].abs() < 1e-3 && w[1].abs() < 1e-3);
        let back = m.to_interface([500.0, -1000.0]).unwrap();
        let again = m.to_world(back).unwrap();
        assert!((again[0] - 500.0).abs() < 0.1 && (again[1] + 1000.0).abs() < 0.1);
        assert!(m.to_world([0.5, 0.5]).is_none());
    }
}
