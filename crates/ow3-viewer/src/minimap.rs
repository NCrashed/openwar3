//! The minimap in its rectangle of the console (SPEC-0028 "Minimap",
//! SPEC-0032 "The minimap").
//!
//! It shows the map's `war3mapMap.blp` over the playable area, its longer
//! side across the picture and the other centred (`Minimap::square`),
//! and the fog of the local player over it: fogged cells at 0.74 of the
//! picture, cells never seen at 0.40 when the map's flag `0x0010` leaves
//! them partly visible, else black. Own units are white squares, the
//! units of other players squares of their colour, gold mines, neutral
//! buildings and creep camps under fog icons, and the camera a white
//! outline. Units under fog are not shown. A left click moves the camera
//! there, a right click orders the selection there.
//!
//! The icons of gold mines and neutral buildings come from the skins, the
//! creep camp circle is `UI\Minimap\MinimapIconCreepLoc.blp` tinted by the
//! camp's strength (SPEC-0028, "Creep camps"). Units are squares of 4/256
//! of the picture's width, buildings of 8/256. The camera is the smallest
//! rectangle round the points where the rays through the corners of the
//! world frame meet the plane of the camera target's height.
//!
//! PROVISIONAL: the colours are those of `ReplaceableTextures\TeamColor`,
//! which the spec does not name; a camp joins creeps within 840 of its
//! first one, of the measured 820 to 860; the edges between fog states are
//! hard (SPEC-0032 open question 4).

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

/// Sizes of units and buildings as shares of the picture's width
/// (SPEC-0028: 3 x 3 and about 5.5 x 6 pixels of a 177 pixel picture).
const UNIT: f32 = 4.0 / 256.0;
const BUILDING: f32 = 8.0 / 256.0;
/// Sizes in pixels: icons; the camera outline's width.
const ICON: f32 = 9.0;
const LINE: f32 = 1.0;

const GOLD: &str = "UI\\MiniMap\\MiniMapIcon\\MinimapIconGold.blp";
const NEUTRAL_BUILDING: &str = "UI\\MiniMap\\MiniMapIcon\\MinimapIconNeutralBuilding.blp";
const CREEP_CAMP: &str = "UI\\Minimap\\MinimapIconCreepLoc.blp";

/// The icons of the minimap (skins `MinimapResourceTexture` and
/// `MinimapNeutralTexture`; the camp circle is
/// `UI\Minimap\MinimapIconCreepLoc.blp`, as the skin entry
/// `MinimapCreepCampTexture` names a file the archives lack) and the look
/// of creep camps by the sum of their levels (`[Minimap]` of
/// `UI\MiscData.txt`), SPEC-0028 "Creep camps".
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub gold: String,
    pub neutral: String,
    pub camp: String,
    /// `MinimapMiddleCampThreshold`, `MinimapToughCampThreshold`.
    pub thresholds: [i32; 2],
    /// Weak, middle and tough camp colours.
    pub colours: [[f32; 4]; 3],
    /// `MinimapMiddleCampScale`, for middle and tough camps.
    pub scale: f32,
}

impl Default for Look {
    fn default() -> Look {
        Look {
            gold: GOLD.to_owned(),
            neutral: NEUTRAL_BUILDING.to_owned(),
            camp: CREEP_CAMP.to_owned(),
            thresholds: [10, 20],
            colours: [
                [0.0, 200.0 / 255.0, 0.0, 1.0],
                [1.0, 128.0 / 255.0, 0.0, 1.0],
                [220.0 / 255.0, 0.0, 0.0, 1.0],
            ],
            scale: 1.3,
        }
    }
}

impl Look {
    /// The look the game files give: `skin` finds a skin entry, `misc`
    /// is the text of `UI\MiscData.txt`. What is missing keeps its
    /// default.
    pub fn load(skin: impl Fn(&str) -> Option<String>, misc: &str) -> Look {
        let mut look = Look::default();
        if let Some(t) = skin("MinimapResourceTexture") {
            look.gold = t;
        }
        if let Some(t) = skin("MinimapNeutralTexture") {
            look.neutral = t;
        }
        let mut inside = false;
        for line in misc.lines().map(str::trim) {
            if line.starts_with('[') {
                inside = line.eq_ignore_ascii_case("[Minimap]");
                continue;
            }
            let Some((key, value)) = line.split_once('=').filter(|_| inside) else {
                continue;
            };
            let numbers: Vec<f32> = value
                .split(',')
                .filter_map(|v| v.trim().parse().ok())
                .collect();
            // Colours are alpha, red, green, blue of 255.
            let colour = || {
                (numbers.len() == 4)
                    .then(|| [numbers[1], numbers[2], numbers[3], numbers[0]].map(|c| c / 255.0))
            };
            match key.trim() {
                "MinimapMiddleCampThreshold" => {
                    look.thresholds[0] = numbers.first().map_or(10, |&v| v as i32);
                }
                "MinimapToughCampThreshold" => {
                    look.thresholds[1] = numbers.first().map_or(20, |&v| v as i32);
                }
                "MinimapMiddleCampScale" => look.scale = numbers.first().copied().unwrap_or(1.3),
                "MinimapWeakCampColor" => look.colours[0] = colour().unwrap_or(look.colours[0]),
                "MinimapMiddleCampColor" => look.colours[1] = colour().unwrap_or(look.colours[1]),
                "MinimapToughCampColor" => look.colours[2] = colour().unwrap_or(look.colours[2]),
                _ => {}
            }
        }
        look
    }

    /// The colour and scale of a camp whose levels sum to `levels`; a
    /// creep made later by a script (`None`) shows as a weak camp.
    fn camp(&self, levels: Option<i32>) -> ([f32; 4], f32) {
        match levels {
            Some(l) if l >= self.thresholds[1] => (self.colours[2], self.scale),
            Some(l) if l >= self.thresholds[0] => (self.colours[1], self.scale),
            _ => (self.colours[0], 1.0),
        }
    }
}

/// What a mark on the minimap stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// A unit or building of the local player, white.
    Own {
        building: bool,
    },
    /// A unit or building of player `player`, in its colour, or with the
    /// ally colours by how the local player sees it.
    Player {
        player: u8,
        building: bool,
        relation: Option<ow3_sim::game::Relation>,
    },
    Mine,
    NeutralBuilding,
    /// A camp of creeps under fog: the sum of its levels, `None` for a
    /// creep made later by a script.
    Camp {
        levels: Option<i32>,
    },
}

/// The five buttons beside the minimap, from the top (SPEC-0028).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Signal,
    Terrain,
    AllyColours,
    CreepCamps,
    Formation,
}

/// The buttons' rectangles, in the order of [`Button`] (SPEC-0028, "Main
/// frames").
pub const BUTTONS: [(Button, Rect); 5] = [
    (Button::Signal, [0.154_375, 0.123_125, 0.1775, 0.143_75]),
    (Button::Terrain, [0.154_375, 0.101_25, 0.1775, 0.121_875]),
    (Button::AllyColours, [0.154_375, 0.079_375, 0.1775, 0.1]),
    (Button::CreepCamps, [0.154_375, 0.056_875, 0.1775, 0.0775]),
    (Button::Formation, [0.155, 0.029_375, 0.178_125, 0.05]),
];

/// Seconds a signal shows on the minimap.
const SIGNAL_SECONDS: f32 = 3.0;
/// The ally colours: enemies red, allies teal (`TeamColor00` and `02`).
const ENEMY_COLOUR: u8 = 0;
const ALLY_COLOUR: u8 = 2;

/// What the buttons set (SPEC-0028, "Minimap"): none of it is an action
/// of the game.
///
/// PROVISIONAL: the textures of the buttons in each state are not
/// specified (here those of `UI\Widgets\Console\Human`); ally colours has
/// three modes, the second and third alike on the minimap (the third was
/// not studied); creep camps hides the camp marks when off, which the
/// spec could not see with the creeps visible; formation is only shown,
/// as orders keep their formation; a signal is a blinking square at its
/// point for 3 seconds, its look and sound not specified.
#[derive(Clone, Debug, PartialEq)]
pub struct Toggles {
    pub terrain: bool,
    pub ally_colours: u8,
    pub creep_camps: bool,
    pub formation: bool,
    /// The signal button waits for its point.
    pub signal: bool,
    /// Signals given, with the game time they were given at.
    signals: Vec<([f32; 2], f32)>,
}

impl Default for Toggles {
    fn default() -> Toggles {
        Toggles {
            terrain: true,
            ally_colours: 0,
            creep_camps: true,
            formation: true,
            signal: false,
            signals: Vec::new(),
        }
    }
}

impl Toggles {
    /// A click on `button`.
    pub fn press(&mut self, button: Button) {
        match button {
            Button::Signal => self.signal = !self.signal,
            Button::Terrain => self.terrain = !self.terrain,
            Button::AllyColours => self.ally_colours = (self.ally_colours + 1) % 3,
            Button::CreepCamps => self.creep_camps = !self.creep_camps,
            Button::Formation => self.formation = !self.formation,
        }
    }

    /// A signal at world point `at`, game time `now`; the button no longer
    /// waits.
    pub fn signal_at(&mut self, at: [f32; 2], now: f32) {
        self.signal = false;
        self.signals.retain(|&(_, t)| now - t < SIGNAL_SECONDS);
        self.signals.push((at, now));
    }

    /// The texture of `button` in its state.
    fn texture(&self, button: Button) -> String {
        let state = match button {
            Button::Signal if self.signal => "minimap-ping-active-down",
            Button::Signal => "minimap-ping-active",
            Button::Terrain if self.terrain => "minimap-terrain-active",
            Button::Terrain => "minimap-terrain-inactive",
            Button::AllyColours => match self.ally_colours {
                0 => "minimap-ally-off",
                1 => "minimap-ally-active",
                _ => "minimap-ally-inactive",
            },
            Button::CreepCamps if self.creep_camps => "minimap-creep-active",
            Button::CreepCamps => "minimap-creep-inactive",
            Button::Formation if self.formation => "formation-on",
            Button::Formation => "formation-off",
        };
        format!("UI\\Widgets\\Console\\Human\\human-{state}.blp")
    }
}

/// The minimap button at interface point `p`.
pub fn button_at(p: [f32; 2]) -> Option<Button> {
    BUTTONS
        .iter()
        .find(|(_, r)| p[0] >= r[0] && p[0] <= r[2] && p[1] >= r[1] && p[1] <= r[3])
        .map(|&(b, _)| b)
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
    /// Its icons and camp colours.
    pub look: Look,
    /// What its buttons set.
    pub toggles: Toggles,
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
            toggles: Toggles::default(),
            look: Look::default(),
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

    /// The world point under interface point `p`, or under the nearest
    /// point of the minimap when `p` is off it.
    pub fn to_world_clamped(&self, p: [f32; 2]) -> [f32; 2] {
        let [x0, y0, x1, y1] = PICTURE;
        let inside = [p[0].clamp(x0, x1), p[1].clamp(y0, y1)];
        self.to_world(inside).unwrap_or([0.0; 2])
    }

    /// The world point under interface point `p`, if it is on the minimap.
    pub fn to_world(&self, p: [f32; 2]) -> Option<[f32; 2]> {
        let [x0, y0, x1, y1] = PICTURE;
        if p[0] < x0 || p[0] > x1 || p[1] < y0 || p[1] > y1 {
            return None;
        }
        let [a0, b0, a1, b1] = self.square();
        let [c0, d0, c1, d1] = self.area;
        Some([
            (a0 + (p[0] - x0) / (x1 - x0) * (a1 - a0)).clamp(c0, c1),
            (b0 + (p[1] - y0) / (y1 - y0) * (b1 - b0)).clamp(d0, d1),
        ])
    }

    /// The square the picture shows: the playable area, its longer side
    /// across the whole picture and the other centred, as the map's
    /// `war3mapMap.blp` draws a map that is not square (Echo Isles: 14848 x
    /// 10752, black bands above and below).
    fn square(&self) -> [f32; 4] {
        let [a0, b0, a1, b1] = self.area;
        let side = (a1 - a0).max(b1 - b0);
        let (cx, cy) = ((a0 + a1) / 2.0, (b0 + b1) / 2.0);
        [
            cx - side / 2.0,
            cy - side / 2.0,
            cx + side / 2.0,
            cy + side / 2.0,
        ]
    }

    /// The interface point of world point `w`, if it is in the area.
    fn to_interface(&self, w: [f32; 2]) -> Option<[f32; 2]> {
        let [a0, b0, a1, b1] = self.area;
        if w[0] < a0 || w[0] > a1 || w[1] < b0 || w[1] > b1 {
            return None;
        }
        let [x0, y0, x1, y1] = PICTURE;
        let [a0, b0, a1, b1] = self.square();
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
        now: f32,
    ) -> Vec<Quad> {
        use ow3_sim::game::Relation;
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
        let toggles = &self.toggles;
        // Without terrain, black instead of the picture.
        let mut out = vec![Quad {
            rect: PICTURE,
            uv: [0.0, 1.0, 0.0, 1.0],
            texture: if toggles.terrain {
                "war3mapMap.blp".to_owned()
            } else {
                String::new()
            },
            colour: if toggles.terrain {
                [1.0; 4]
            } else {
                [0.0, 0.0, 0.0, 1.0]
            },
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
            let side = |building: bool| {
                let s = (PICTURE[2] - PICTURE[0]) * if building { BUILDING } else { UNIT };
                [s, s]
            };
            let team =
                |player: u8| format!("ReplaceableTextures\\TeamColor\\TeamColor{player:02}.blp");
            out.push(match mark {
                Mark::Own { building } => square(p, side(building), "", [1.0; 4]),
                Mark::Player {
                    player,
                    building,
                    relation,
                } => {
                    let colour = match relation {
                        _ if toggles.ally_colours == 0 => player,
                        // The neutral players keep their colours.
                        _ if i32::from(player) >= ow3_sim::game::CREEPS => player,
                        Some(Relation::Friend) => ALLY_COLOUR,
                        _ => ENEMY_COLOUR,
                    };
                    square(p, side(building), &team(colour), [1.0; 4])
                }
                Mark::Mine => square(p, size(ICON), &self.look.gold, [1.0; 4]),
                Mark::NeutralBuilding => square(p, size(ICON), &self.look.neutral, [1.0; 4]),
                Mark::Camp { .. } if !toggles.creep_camps => continue,
                // The icon draws its circle over the middle half.
                Mark::Camp { levels } => {
                    let (colour, scale) = self.look.camp(levels);
                    square(p, size(2.0 * ICON * scale), &self.look.camp, colour)
                }
            });
        }
        // Signals blink at their points.
        for &(at, t) in &toggles.signals {
            let age = now - t;
            if (0.0..SIGNAL_SECONDS).contains(&age)
                && (age * 4.0) as i32 % 2 == 0
                && let Some(p) = self.to_interface(at)
            {
                let s = ICON * PIXEL;
                out.push(square(p, [s, s], "", [1.0, 1.0, 0.0, 1.0]));
            }
        }
        // The buttons.
        for &(button, rect) in &BUTTONS {
            out.push(Quad {
                rect,
                uv: [0.0, 1.0, 0.0, 1.0],
                texture: toggles.texture(button),
                colour: [1.0; 4],
                blend: Blend::Alpha,
            });
        }
        // The camera: the smallest rectangle round the four points.
        if let Some(c) = view {
            let low = |i: usize| c.iter().map(|p| p[i]).fold(f32::INFINITY, f32::min);
            let high = |i: usize| c.iter().map(|p| p[i]).fold(f32::NEG_INFINITY, f32::max);
            let [a0, b0, a1, b1] = self.area;
            let clamp = |p: [f32; 2]| [p[0].clamp(a0, a1), p[1].clamp(b0, b1)];
            let lb = self.to_interface(clamp([low(0), low(1)]));
            let rt = self.to_interface(clamp([high(0), high(1)]));
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

/// How near a creep stands to the first creep of a camp to join it
/// (SPEC-0028, "Creep camps": 820 joined, 860 did not).
const CAMP_REACH: f32 = 840.0;

/// The camps of the creeps at `creeps`, in the order they were made: the
/// index of each creep's camp. A creep joins the first camp whose first
/// creep is within about 840 of it, else starts a camp.
pub fn camps(creeps: &[[f32; 2]]) -> Vec<usize> {
    let mut firsts: Vec<[f32; 2]> = Vec::new();
    creeps
        .iter()
        .map(|p| {
            let near = |f: &[f32; 2]| {
                (p[0] - f[0]).powi(2) + (p[1] - f[1]).powi(2) < CAMP_REACH * CAMP_REACH
            };
            firsts.iter().position(near).unwrap_or_else(|| {
                firsts.push(*p);
                firsts.len() - 1
            })
        })
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
            toggles: Toggles::default(),
            look: Look::default(),
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
        let quads = minimap(MASKED_PARTLY).quads(&[], None, Some(&cells), 0.0);
        // The fog comes after the picture; the five buttons last.
        let fog: Vec<&Quad> = quads.iter().skip(1).take(quads.len() - 6).collect();
        assert_eq!(fog.len(), 4);
        let w = (PICTURE[2] - PICTURE[0]) / 4.0;
        assert!((fog[2].rect[0] - (PICTURE[0] + 2.0 * w)).abs() < 1e-6);
        assert!((fog[2].rect[2] - PICTURE[2]).abs() < 1e-6);
        assert!((fog[0].colour[3] - 0.60).abs() < 1e-6);
        assert!((fog[1].colour[3] - 0.26).abs() < 1e-6);
        let black = minimap(0.0).quads(&[], None, Some(&cells), 0.0);
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
            toggles: Toggles::default(),
            look: Look::default(),
        };
        for (px, py) in [(30.0f32, 600.0f32), (180.0, 740.0), (104.41, 669.2)] {
            let p = [px * PIXEL, (768.0 - py) * PIXEL];
            let [x, y] = m.to_world(p).unwrap();
            assert!((x - 130.16 * (px - 104.41)).abs() < 20.0, "{px}: {x}");
            assert!((y + 136.35 * (py - 669.2)).abs() < 20.0, "{py}: {y}");
        }
    }

    /// The buttons (SPEC-0028): terrain turns the picture black, ally
    /// colours paint enemies red and allies teal, creep camps hides the
    /// camps; each button sits in its rectangle.
    #[test]
    fn the_buttons() {
        use ow3_sim::game::Relation;
        for &(button, r) in &BUTTONS {
            let centre = [(r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0];
            assert_eq!(button_at(centre), Some(button));
        }
        assert_eq!(button_at([0.5, 0.5]), None);
        let mut m = minimap(0.0);
        let enemy = Mark::Player {
            player: 1,
            building: false,
            relation: Some(Relation::Enemy),
        };
        let ally = Mark::Player {
            player: 3,
            building: false,
            relation: Some(Relation::Friend),
        };
        let marks = [
            ([100.0, 100.0], enemy),
            ([200.0, 100.0], ally),
            ([300.0, 100.0], Mark::Camp { levels: Some(3) }),
        ];
        let textures = |m: &Minimap| -> Vec<String> {
            m.quads(&marks, None, None, 0.0)
                .into_iter()
                .take(4)
                .map(|q| q.texture)
                .collect()
        };
        let plain = textures(&m);
        assert_eq!(plain[0], "war3mapMap.blp");
        assert!(plain[1].ends_with("TeamColor01.blp") && plain[2].ends_with("TeamColor03.blp"));
        assert_eq!(plain[3], CREEP_CAMP);
        let camp = &m.quads(&marks, None, None, 0.0)[3];
        assert_eq!(camp.colour, Look::default().colours[0]);
        m.toggles.press(Button::Terrain);
        m.toggles.press(Button::AllyColours);
        m.toggles.press(Button::CreepCamps);
        let pressed = textures(&m);
        assert_eq!(pressed[0], "");
        assert!(pressed[1].ends_with("TeamColor00.blp") && pressed[2].ends_with("TeamColor02.blp"));
        assert_ne!(pressed[3], CREEP_CAMP);
        // Three modes of ally colours, then the first again.
        m.toggles.press(Button::AllyColours);
        m.toggles.press(Button::AllyColours);
        assert_eq!(m.toggles.ally_colours, 0);
    }

    /// Camp colours and sizes by the sum of levels, as `[Minimap]` of
    /// `UI\MiscData.txt` gives them (SPEC-0028): one creep of level 10
    /// and two of 5 make a middle camp, four of 5 and two of 10 a tough
    /// one; a creep made later is weak whatever its level.
    #[test]
    fn camp_looks() {
        let misc = "[Other]\nMinimapToughCampThreshold=99\n[Minimap]\n\
            MinimapMiddleCampThreshold=10\nMinimapToughCampThreshold=20\n\
            MinimapMiddleCampScale=1.3\nMinimapWeakCampColor=255,0,200,0\n\
            MinimapMiddleCampColor=255,255,128,0\nMinimapToughCampColor=255,220,0,0\n";
        let look = Look::load(
            |k| (k == "MinimapResourceTexture").then(|| "gold.blp".into()),
            misc,
        );
        assert_eq!(look.gold, "gold.blp");
        assert_eq!(look.camp, CREEP_CAMP);
        assert_eq!(look.thresholds, [10, 20]);
        assert_eq!(look.camp(Some(9)), ([0.0, 200.0 / 255.0, 0.0, 1.0], 1.0));
        assert_eq!(look.camp(Some(10)), ([1.0, 128.0 / 255.0, 0.0, 1.0], 1.3));
        assert_eq!(look.camp(Some(20)).0, [220.0 / 255.0, 0.0, 0.0, 1.0]);
        assert_eq!(look.camp(None), look.camp(Some(0)));
    }

    #[test]
    fn camps_of_creeps() {
        // Pairs 820 apart join, 860 apart do not; of three in a row 600
        // apart the third is a camp of its own.
        assert_eq!(
            camps(&[[0.0, 0.0], [820.0, 0.0], [5000.0, 0.0], [5860.0, 0.0]]),
            [0, 0, 1, 2]
        );
        assert_eq!(camps(&[[0.0, 0.0], [600.0, 0.0], [1200.0, 0.0]]), [0, 0, 1]);
    }

    #[test]
    fn world_and_interface_points_agree() {
        let m = Minimap {
            area: [-1000.0, -2000.0, 1000.0, 2000.0],
            terrain: [-1024.0, -2048.0, 16.0],
            masked: 0.0,
            toggles: Toggles::default(),
            look: Look::default(),
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
        // Off the minimap, the nearest point of it: here its upper right
        // corner.
        assert_eq!(m.to_world_clamped([0.5, 0.5]), [1000.0, 2000.0]);
    }
}
