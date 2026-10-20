//! The minimap in its rectangle of the console (SPEC-0028, SPEC-0032).
//!
//! It shows the map's `war3mapMap.blp` over the whole terrain of
//! `war3map.w3e`, darkened where the local player has never looked: to
//! 0.55 of the picture when the map's flag `0x0010` leaves such areas
//! partly visible, else black; fogged cells look like visible ones
//! (SPEC-0032, "The minimap").
//!
//! PROVISIONAL: the marks are not specified. Here the units are squares
//! coloured by how the local player sees them (own green, allies yellow,
//! enemies red, as the game's default without ally colours), gold mines
//! in gold, no creeps, and the part of the ground the camera sees a
//! dotted outline. The edges between fog states are hard (SPEC-0032 open
//! question 4). A left click on it moves the camera there.

use crate::console::Rect;
use crate::ui::{Blend, Quad};

/// The minimap's rectangle (SPEC-0028).
pub const MINIMAP: Rect = [0.009_375, 0.006_875, 0.148_75, 0.145];
/// The square the picture fills: pixels (17, 584) to (191, 758) of a
/// 1024 x 768 window (SPEC-0032), in interface units.
pub const PICTURE: Rect = [
    17.0 / 1024.0 * 0.8,
    (768.0 - 758.0) / 768.0 * 0.6,
    191.0 / 1024.0 * 0.8,
    (768.0 - 584.0) / 768.0 * 0.6,
];
/// What the fog leaves of the picture where the player never looked,
/// with the map's flag `0x0010` and without it.
const MASKED_PARTLY: f32 = 0.55;
const MAP_FLAG_MASKED_PARTLY: u32 = 0x0010;

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
    /// What is left of the picture where the player never looked.
    masked: f32,
}

impl Minimap {
    /// The minimap of `map`: its whole terrain.
    pub fn new(map: &ow3_sim::map::Map) -> Minimap {
        let b = &map.world_bounds;
        let partly = map.info.flags & MAP_FLAG_MASKED_PARTLY != 0;
        Minimap {
            area: [b.min_x, b.min_y, b.max_x, b.max_y],
            masked: if partly { MASKED_PARTLY } else { 0.0 },
        }
    }

    /// Black over the cells never seen, so that `masked` of the picture
    /// shows: one quad a run of such cells in a row. `cells` are the fog
    /// cells of the terrain, `width` across, rows from the bottom.
    fn fog(&self, cells: &[u8], width: usize) -> Vec<Quad> {
        let mut out = Vec::new();
        if width == 0 {
            return out;
        }
        let height = cells.len() / width;
        let [x0, y0, x1, y1] = PICTURE;
        let (w, h) = ((x1 - x0) / width as f32, (y1 - y0) / height as f32);
        for (j, row) in cells.chunks(width).enumerate() {
            let mut i = 0;
            while i < width {
                if row[i] != ow3_sim::game::fog::MASKED {
                    i += 1;
                    continue;
                }
                let start = i;
                while i < width && row[i] == ow3_sim::game::fog::MASKED {
                    i += 1;
                }
                out.push(Quad {
                    rect: [
                        x0 + start as f32 * w,
                        y0 + j as f32 * h,
                        x0 + i as f32 * w,
                        y0 + (j + 1) as f32 * h,
                    ],
                    uv: [0.0, 1.0, 0.0, 1.0],
                    texture: String::new(),
                    colour: [0.0, 0.0, 0.0, 1.0 - self.masked],
                    blend: Blend::Alpha,
                });
            }
        }
        out
    }

    /// The world point under interface point `p`, if it is on the minimap.
    pub fn to_world(&self, p: [f32; 2]) -> Option<[f32; 2]> {
        let [x0, y0, x1, y1] = PICTURE;
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
        let [x0, y0, x1, y1] = PICTURE;
        Some([
            x0 + (w[0] - a0) / (a1 - a0) * (x1 - x0),
            y0 + (w[1] - b0) / (b1 - b0) * (y1 - y0),
        ])
    }

    /// The minimap's quads: the image, the marks (`(x, y, building, mark)`)
    /// and the outline of the ground `view` the camera sees (its corners in
    /// order round it), over the image darkened by the fog `vision` (the
    /// cells of the terrain, rows from the bottom; `None` all seen).
    pub fn quads(
        &self,
        marks: &[([f32; 2], bool, Mark)],
        view: Option<[[f32; 2]; 4]>,
        vision: Option<&[u8]>,
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
            rect: PICTURE,
            uv: [0.0, 1.0, 0.0, 1.0],
            texture: "war3mapMap.blp".to_owned(),
            colour: [1.0; 4],
            blend: Blend::Alpha,
        }];
        if let Some(cells) = vision {
            let across = ((self.area[2] - self.area[0]) / 128.0).round() as usize;
            out.extend(self.fog(cells, across));
        }
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

    /// Cells never seen are covered row by row, one quad a run, leaving
    /// 0.55 of the picture or none; fogged cells are left as they are.
    #[test]
    fn the_fog_darkens_cells_never_seen() {
        let mut m = Minimap {
            area: [0.0, 0.0, 512.0, 256.0],
            masked: MASKED_PARTLY,
        };
        // Four cells across, two rows: masked, fogged, masked, masked;
        // then visible, masked, visible, visible.
        let cells = [0, 1, 0, 0, 2, 0, 2, 2];
        let quads = m.quads(&[], None, Some(&cells));
        let fog: Vec<&Quad> = quads.iter().skip(1).collect();
        assert_eq!(fog.len(), 3);
        let w = (PICTURE[2] - PICTURE[0]) / 4.0;
        assert!((fog[1].rect[0] - (PICTURE[0] + 2.0 * w)).abs() < 1e-6);
        assert!((fog[1].rect[2] - PICTURE[2]).abs() < 1e-6);
        assert!((fog[0].colour[3] - 0.45).abs() < 1e-6);
        m.masked = 0.0;
        assert_eq!(
            m.quads(&[], None, Some(&cells))[1].colour,
            [0.0, 0.0, 0.0, 1.0]
        );
    }

    #[test]
    fn world_and_interface_points_agree() {
        let m = Minimap {
            area: [-1000.0, -2000.0, 1000.0, 2000.0],
            masked: 0.0,
        };
        let centre = [
            (PICTURE[0] + PICTURE[2]) / 2.0,
            (PICTURE[1] + PICTURE[3]) / 2.0,
        ];
        let w = m.to_world(centre).unwrap();
        assert!(w[0].abs() < 1e-3 && w[1].abs() < 1e-3);
        let back = m.to_interface([500.0, -1000.0]).unwrap();
        let again = m.to_world(back).unwrap();
        assert!((again[0] - 500.0).abs() < 0.1 && (again[1] + 1000.0).abs() < 0.1);
        assert!(m.to_world([0.5, 0.5]).is_none());
    }
}
