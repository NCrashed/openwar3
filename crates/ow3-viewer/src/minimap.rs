//! The minimap in its rectangle of the console (SPEC-0028 "Minimap",
//! SPEC-0032 "The minimap").
//!
//! It shows the map's `war3mapMap.blp` stretched over the playable area,
//! and the fog of the local player over it: fogged cells at 0.74 of the
//! picture, cells never seen at 0.40 when the map's flag `0x0010` leaves
//! them partly visible, else black. Own units are white squares, the
//! units of other players squares of their colour, gold mines, neutral
//! buildings and creep camps under fog icons, and the camera a white
//! outline. Units under fog are not shown. A left click moves the camera
//! there, a right click orders the selection there.
//!
//! PROVISIONAL: the icons are `UI\MiniMap\MiniMapIcon\MinimapIconGold.blp`,
//! `MinimapIconNeutralBuilding.blp` and `UI\Minimap\MinimapIconCreepLoc.blp`
//! and the colours those of `ReplaceableTextures\TeamColor`, which the
//! spec does not name, the camp icon tinted green; all buildings are 5 pixels square (the town hall
//! was 5 x 5, a farm 6 x 5); a camp is the creeps within 512 of one
//! another; the camera outline is a rectangle as wide as the view half way
//! up and as deep as it; the edges between fog states are hard (SPEC-0032
//! open question 4).

use crate::console::Rect;
use crate::ui::{Blend, Quad};

/// The minimap's rectangle (SPEC-0028).
pub const MINIMAP: Rect = [0.009_375, 0.006_875, 0.148_75, 0.145];
/// One pixel of a 1024 x 768 window in interface units.
const PIXEL: f32 = 0.8 / 1024.0;
/// The square the picture fills, by the clicks of SPEC-0028 in a
/// 1024 x 768 window: pixels x 15.9 to 193.0 and y 586.6 to 755.6.
pub const PICTURE: Rect = [
    15.9 * PIXEL,
    (768.0 - 755.6) * PIXEL,
    193.0 * PIXEL,
    (768.0 - 586.6) * PIXEL,
];
/// What the fog leaves of the picture: fogged cells, and cells never seen
/// with the map's flag `0x0010`; without it those are black.
const FOGGED: f32 = 0.74;
const MASKED_PARTLY: f32 = 0.40;
const MAP_FLAG_MASKED_PARTLY: u32 = 0x0010;

/// Sizes in pixels: units, buildings, icons; the camera outline's width.
const UNIT: f32 = 3.0;
const BUILDING: f32 = 5.0;
const ICON: f32 = 9.0;
const LINE: f32 = 1.0;

const GOLD: &str = "UI\\MiniMap\\MiniMapIcon\\MinimapIconGold.blp";
const NEUTRAL_BUILDING: &str = "UI\\MiniMap\\MiniMapIcon\\MinimapIconNeutralBuilding.blp";
const CREEP_CAMP: &str = "UI\\Minimap\\MinimapIconCreepLoc.blp";
/// The white circle of the camp icon tinted green.
const CAMP_GREEN: [f32; 4] = [0.2, 0.9, 0.2, 1.0];

/// What a mark on the minimap stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// A unit or building of the local player, white.
    Own {
        building: bool,
    },
    /// A unit or building of player `player`, in its colour.
    Player {
        player: u8,
        building: bool,
    },
    Mine,
    NeutralBuilding,
    /// A camp of creeps under fog.
    Camp,
}

/// The minimap of a map.
#[derive(Clone, Debug, PartialEq)]
pub struct Minimap {
    /// The world area shown: left, bottom, right, top.
    area: [f32; 4],
    /// The terrain, whose cells the fog has: left, bottom, cells across.
    terrain: [f32; 3],
    /// What is left of the picture where the player never looked.
    masked: f32,
}

impl Minimap {
    /// The minimap of `map`: its playable area, the camera bounds and
    /// their margins (the terrain less the complements of `war3map.w3i`).
    pub fn new(map: &ow3_sim::map::Map) -> Minimap {
        let b = &map.world_bounds;
        let [left, right, bottom, top] =
            map.info.camera_bounds_complements.map(|c| c as f32 * 128.0);
        let partly = map.info.flags & MAP_FLAG_MASKED_PARTLY != 0;
        Minimap {
            area: [
                b.min_x + left,
                b.min_y + bottom,
                b.max_x - right,
                b.max_y - top,
            ],
            terrain: [b.min_x, b.min_y, ((b.max_x - b.min_x) / 128.0).round()],
            masked: if partly { MASKED_PARTLY } else { 0.0 },
        }
    }

    /// Black over the cells not visible, so that the share of the picture
    /// for their state shows: one quad a run of cells of a state in a row.
    /// `cells` are the fog cells of the terrain, rows from the bottom.
    fn fog(&self, cells: &[u8]) -> Vec<Quad> {
        use ow3_sim::game::fog;
        let [x0, y0, across] = self.terrain;
        let width = across as usize;
        let mut out = Vec::new();
        if width == 0 {
            return out;
        }
        for (j, row) in cells.chunks(width).enumerate() {
            let mut i = 0;
            while i < row.len() {
                let state = row[i];
                let start = i;
                while i < row.len() && row[i] == state {
                    i += 1;
                }
                let left = match state {
                    fog::MASKED => self.masked,
                    fog::FOGGED => FOGGED,
                    _ => continue,
                };
                let world = [
                    x0 + start as f32 * 128.0,
                    y0 + j as f32 * 128.0,
                    x0 + i as f32 * 128.0,
                    y0 + (j + 1) as f32 * 128.0,
                ];
                if let Some(rect) = self.rect_of(world) {
                    out.push(Quad {
                        rect,
                        uv: [0.0, 1.0, 0.0, 1.0],
                        texture: String::new(),
                        colour: [0.0, 0.0, 0.0, 1.0 - left],
                        blend: Blend::Alpha,
                    });
                }
            }
        }
        out
    }

    /// The interface rectangle of world rectangle `w` (left, bottom,
    /// right, top), cut to the area; `None` outside it.
    fn rect_of(&self, w: [f32; 4]) -> Option<[f32; 4]> {
        let [a0, b0, a1, b1] = self.area;
        let cut = [w[0].max(a0), w[1].max(b0), w[2].min(a1), w[3].min(b1)];
        if cut[0] >= cut[2] || cut[1] >= cut[3] {
            return None;
        }
        let lb = self.to_interface([cut[0], cut[1]])?;
        let rt = self.to_interface([cut[2], cut[3]])?;
        Some([lb[0], lb[1], rt[0], rt[1]])
    }

    /// Whether the map leaves cells never seen partly visible (its flag
    /// `0x0010`).
    pub fn masked_partly(&self) -> bool {
        self.masked > 0.0
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

    /// The minimap's quads: the picture, the fog `vision` over it (the
    /// cells of the terrain, rows from the bottom; `None` all seen), the
    /// marks at their world points and the outline of the ground `view`
    /// the camera sees (its corners: near left, near right, far right, far
    /// left).
    pub fn quads(
        &self,
        marks: &[([f32; 2], Mark)],
        view: Option<[[f32; 2]; 4]>,
        vision: Option<&[u8]>,
    ) -> Vec<Quad> {
        let square = |at: [f32; 2], size: [f32; 2], texture: &str, colour: [f32; 4]| Quad {
            rect: [
                at[0] - size[0] / 2.0,
                at[1] - size[1] / 2.0,
                at[0] + size[0] / 2.0,
                at[1] + size[1] / 2.0,
            ],
            uv: [0.0, 1.0, 0.0, 1.0],
            texture: texture.to_owned(),
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
            out.extend(self.fog(cells));
        }
        for &(at, mark) in marks {
            let Some(p) = self.to_interface(at) else {
                continue;
            };
            let size = |s: f32| [s * PIXEL, s * PIXEL];
            let side = |building: bool| size(if building { BUILDING } else { UNIT });
            out.push(match mark {
                Mark::Own { building } => square(p, side(building), "", [1.0; 4]),
                Mark::Player { player, building } => square(
                    p,
                    side(building),
                    &format!("ReplaceableTextures\\TeamColor\\TeamColor{player:02}.blp"),
                    [1.0; 4],
                ),
                Mark::Mine => square(p, size(ICON), GOLD, [1.0; 4]),
                Mark::NeutralBuilding => square(p, size(ICON), NEUTRAL_BUILDING, [1.0; 4]),
                // The icon draws its circle over the middle half.
                Mark::Camp => square(p, size(2.0 * ICON), CREEP_CAMP, CAMP_GREEN),
            });
        }
        if let Some(c) = view {
            let length =
                |a: [f32; 2], b: [f32; 2]| ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
            let width = (length(c[0], c[1]) + length(c[3], c[2])) / 2.0;
            let depth = (length(c[1], c[2]) + length(c[0], c[3])) / 2.0;
            let centre = [
                c.iter().map(|p| p[0]).sum::<f32>() / 4.0,
                c.iter().map(|p| p[1]).sum::<f32>() / 4.0,
            ];
            let [a0, b0, a1, b1] = self.area;
            let clamp = |p: [f32; 2]| [p[0].clamp(a0, a1), p[1].clamp(b0, b1)];
            let lb = self.to_interface(clamp([centre[0] - width / 2.0, centre[1] - depth / 2.0]));
            let rt = self.to_interface(clamp([centre[0] + width / 2.0, centre[1] + depth / 2.0]));
            if let (Some([l, b]), Some([r, t])) = (lb, rt) {
                let line = LINE * PIXEL;
                for rect in [
                    [l, b, r, b + line],
                    [l, t - line, r, t],
                    [l, b, l + line, t],
                    [r - line, b, r, t],
                ] {
                    out.push(Quad {
                        rect,
                        uv: [0.0, 1.0, 0.0, 1.0],
                        texture: String::new(),
                        colour: [1.0; 4],
                        blend: Blend::Alpha,
                    });
                }
            }
        }
        out
    }
}

/// The camps of creeps at `creeps`: the creeps within 512 of one another
/// chained together, each camp at the middle of its creeps.
pub fn camps(creeps: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let mut camp: Vec<usize> = (0..creeps.len()).collect();
    fn root(camp: &mut [usize], mut i: usize) -> usize {
        while camp[i] != i {
            camp[i] = camp[camp[i]];
            i = camp[i];
        }
        i
    }
    for i in 0..creeps.len() {
        for j in i + 1..creeps.len() {
            let (a, b) = (creeps[i], creeps[j]);
            if (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) <= 512.0 * 512.0 {
                let (ri, rj) = (root(&mut camp, i), root(&mut camp, j));
                camp[ri.max(rj)] = ri.min(rj);
            }
        }
    }
    let mut sums: std::collections::BTreeMap<usize, ([f32; 2], f32)> = Default::default();
    for (i, p) in creeps.iter().enumerate() {
        let r = root(&mut camp, i);
        let s = sums.entry(r).or_default();
        s.0[0] += p[0];
        s.0[1] += p[1];
        s.1 += 1.0;
    }
    sums.into_values()
        .map(|(sum, n)| [sum[0] / n, sum[1] / n])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimap(masked: f32) -> Minimap {
        Minimap {
            area: [0.0, 0.0, 512.0, 256.0],
            terrain: [0.0, 0.0, 4.0],
            masked,
        }
    }

    /// Cells not visible are covered row by row, one quad a run of a state:
    /// fogged ones leave 0.74 of the picture, those never seen 0.40 or
    /// nothing.
    #[test]
    fn the_fog_darkens_the_picture() {
        // Four cells across, two rows: masked, fogged, masked, masked;
        // then visible, masked, visible, visible.
        let cells = [0, 1, 0, 0, 2, 0, 2, 2];
        let quads = minimap(MASKED_PARTLY).quads(&[], None, Some(&cells));
        let fog: Vec<&Quad> = quads.iter().skip(1).collect();
        assert_eq!(fog.len(), 4);
        let w = (PICTURE[2] - PICTURE[0]) / 4.0;
        assert!((fog[2].rect[0] - (PICTURE[0] + 2.0 * w)).abs() < 1e-6);
        assert!((fog[2].rect[2] - PICTURE[2]).abs() < 1e-6);
        assert!((fog[0].colour[3] - 0.60).abs() < 1e-6);
        assert!((fog[1].colour[3] - 0.26).abs() < 1e-6);
        let black = minimap(0.0).quads(&[], None, Some(&cells));
        assert_eq!(black[1].colour, [0.0, 0.0, 0.0, 1.0]);
    }

    /// The picture's square maps clicks as in SPEC-0028: on IceCrown's
    /// playable area pixel (px, py) of a 1024 x 768 window is world
    /// x = 130.16 (px - 104.41), y = -136.35 (py - 669.2).
    #[test]
    fn clicks_of_the_spec() {
        let m = Minimap {
            area: [-11520.0, -11776.0, 11520.0, 11264.0],
            terrain: [-12288.0, -12288.0, 192.0],
            masked: 0.0,
        };
        for (px, py) in [(30.0f32, 600.0f32), (180.0, 740.0), (104.41, 669.2)] {
            let p = [px * PIXEL, (768.0 - py) * PIXEL];
            let [x, y] = m.to_world(p).unwrap();
            assert!((x - 130.16 * (px - 104.41)).abs() < 20.0, "{px}: {x}");
            assert!((y + 136.35 * (py - 669.2)).abs() < 20.0, "{py}: {y}");
        }
    }

    #[test]
    fn camps_of_creeps() {
        let camps = camps(&[[0.0, 0.0], [300.0, 0.0], [600.0, 0.0], [5000.0, 0.0]]);
        assert_eq!(camps, [[300.0, 0.0], [5000.0, 0.0]]);
    }

    #[test]
    fn world_and_interface_points_agree() {
        let m = Minimap {
            area: [-1000.0, -2000.0, 1000.0, 2000.0],
            terrain: [-1024.0, -2048.0, 16.0],
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
