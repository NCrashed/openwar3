//! The objects placed on a map, as the editor saved them: destructables
//! and doodads of `war3map.doo`, units and items of `war3mapUnits.doo`,
//! each with the model it is drawn with.
//!
//! Model paths come from the game data: the `file` column of
//! `Units\DestructableData.slk`, `Doodads\Doodads.slk`, `Units\unitUI.slk`
//! and `Units\ItemData.slk`, with the extension `.mdx`; when a type has
//! several variations (`numVar` above 1) the variation number is appended
//! to the file name. A destructable's replaceable texture is its `texID`
//! and `texFile`; a unit has the team colour of its owner and the scale
//! `modelScale`.
//!
//! PROVISIONAL: the placement of units is taken from `war3mapUnits.doo`,
//! which the editor writes; in the game the map script creates them.
//! Random units, start locations and custom types of the map are left
//! out.

use std::collections::BTreeMap;

use ow3_formats::binary::RawCode;
use ow3_formats::map::doodads::Doodads;
use ow3_formats::map::objects::{ObjectKind, ObjectModifications, Value as ObjectValue};
use ow3_formats::map::units::PlacedUnits;
use ow3_formats::mpq::ArchiveChain;
use ow3_formats::slk::Table;

/// One object to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    pub model: String,
    /// A replaceable texture id and its path, for trees and the like.
    pub replaceable: Option<(u32, String)>,
    /// The player whose team colour the model shows.
    pub team: u8,
    pub position: [f32; 3],
    /// Facing in radians, counterclockwise from east.
    pub angle: f32,
    pub scale: [f32; 3],
}

/// What a type of object is drawn with.
#[derive(Clone, Debug, Default)]
struct Look {
    file: String,
    variations: u32,
    replaceable: Option<(u32, String)>,
    scale: f32,
    /// The selection scale of a unit (`scale` of `Units\unitUI.slk`).
    selection: f32,
}

/// How far west and south of its position a unit's model is drawn
/// (SPEC-0023, "Where models are drawn"; measured on standing footmen and
/// a peasant, the reason is open).
pub const UNIT_MODEL_SHIFT: f32 = 16.0;

/// Where and how large a doodad of `war3map.doo` is drawn (SPEC-0023,
/// "Where models are drawn"), from `Doodads\Doodads.slk`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct DoodadRule {
    /// `defScale`, `minScale`, `maxScale`.
    default: f32,
    min: f32,
    max: f32,
    /// It has a pathing texture, and so moves to the grid.
    snaps: bool,
}

impl DoodadRule {
    /// The drawn position and scale of a doodad stored at `at` with
    /// `scale`: the stored scale held within the limits, times the
    /// default; a doodad with a pathing texture moves to `64 floor(p / 64)
    /// + 32`. PROVISIONAL: measured for a 2 x 2 pathing texture only.
    fn place(&self, at: [f32; 3], scale: [f32; 3]) -> ([f32; 3], [f32; 3]) {
        let scale = scale.map(|s| s.clamp(self.min, self.max.max(self.min)) * self.default);
        let snap = |p: f32| 64.0 * (p / 64.0).floor() + 32.0;
        let at = if self.snaps {
            [snap(at[0]), snap(at[1]), at[2]]
        } else {
            at
        };
        (at, scale)
    }
}

/// How a unit type looks.
#[derive(Clone, Debug, PartialEq)]
pub struct UnitLook {
    pub model: String,
    pub scale: f32,
    /// The selection scale, which times `ScaleFactor` of `[SelectionCircle]`
    /// in `UI\MiscData.txt` gives the size of its selection circle.
    pub selection: f32,
}

/// The given columns of each row of an SLK table, by raw code.
fn table(
    chain: &mut ArchiveChain,
    path: &str,
    key: &str,
    columns: &[&str],
) -> BTreeMap<RawCode, Vec<String>> {
    let mut rows = BTreeMap::new();
    let Some(table) = chain.read(path).and_then(|d| Table::parse(&d).ok()) else {
        return rows;
    };
    let Some(key) = table.column(key) else {
        return rows;
    };
    let columns: Vec<Option<u32>> = columns.iter().map(|c| table.column(c)).collect();
    for row in 2..=table.last_row() {
        let Some(id) = table.get(row, key) else {
            continue;
        };
        let Ok(bytes) = <[u8; 4]>::try_from(id.raw().as_bytes()) else {
            continue;
        };
        let values = columns
            .iter()
            .map(|c| {
                c.and_then(|c| table.get(row, c))
                    .map_or_else(String::new, |v| v.raw().to_owned())
            })
            .collect();
        rows.insert(RawCode(bytes), values);
    }
    rows
}

fn number(text: &str, default: f32) -> f32 {
    text.parse().unwrap_or(default)
}

/// `file` without an extension, plus the variation when there are several,
/// as an `.mdx` path.
fn model_path(look: &Look, variation: i32) -> String {
    let file = look.file.trim();
    let stem = match file.rsplit_once('.') {
        Some((stem, ext)) if ext.eq_ignore_ascii_case("mdl") || ext.eq_ignore_ascii_case("mdx") => {
            stem
        }
        _ => file,
    };
    if look.variations > 1 {
        format!("{stem}{}.mdx", variation.max(0))
    } else {
        format!("{stem}.mdx")
    }
}

/// The objects of the map in `chain`.
/// With `without_units`, the units of `war3mapUnits.doo` are left out (the
/// simulation places them); its items stay.
pub fn placements(chain: &mut ArchiveChain, without_units: bool) -> Vec<Placement> {
    let mut looks: BTreeMap<RawCode, Look> = BTreeMap::new();
    for (id, v) in table(
        chain,
        "Units\\DestructableData.slk",
        "DestructableID",
        &["file", "numVar", "texID", "texFile"],
    ) {
        let replaceable = v[2]
            .parse::<u32>()
            .ok()
            .filter(|&id| id != 0 && !v[3].is_empty() && v[3] != "_")
            .map(|id| (id, format!("{}.blp", v[3])));
        looks.insert(
            id,
            Look {
                file: v[0].clone(),
                variations: v[1].parse().unwrap_or(1),
                replaceable,
                scale: 1.0,
                selection: 1.0,
            },
        );
    }
    // How doodads are scaled and whether they snap to the grid.
    let mut doodad_rules: BTreeMap<RawCode, DoodadRule> = BTreeMap::new();
    for (id, v) in table(
        chain,
        "Doodads\\Doodads.slk",
        "doodID",
        &[
            "file", "numVar", "defScale", "minScale", "maxScale", "pathTex",
        ],
    ) {
        let number = |s: &str, default: f32| s.trim().parse::<f32>().unwrap_or(default);
        let path = v[5].trim();
        doodad_rules.insert(
            id,
            DoodadRule {
                default: number(&v[2], 1.0),
                min: number(&v[3], 0.0),
                max: number(&v[4], f32::MAX),
                snaps: !path.is_empty() && path != "_" && !path.eq_ignore_ascii_case("none"),
            },
        );
        looks.insert(
            id,
            Look {
                file: v[0].clone(),
                variations: v[1].parse().unwrap_or(1),
                replaceable: None,
                scale: 1.0,
                selection: 1.0,
            },
        );
    }
    let mut unit_looks = unit_looks(chain);
    let unit_ids: std::collections::BTreeSet<RawCode> = unit_looks.keys().copied().collect();
    for (id, v) in table(chain, "Units\\ItemData.slk", "itemID", &["file"]) {
        unit_looks.insert(
            id,
            Look {
                file: v[0].clone(),
                variations: 1,
                replaceable: None,
                scale: 1.0,
                selection: 1.0,
            },
        );
    }

    let mut placements = Vec::new();
    if let Some(doodads) = chain
        .read_map_file("war3map.doo")
        .and_then(|d| Doodads::parse(&d).ok())
    {
        for d in &doodads.doodads {
            let Some(look) = looks.get(&d.kind) else {
                continue;
            };
            let (position, scale) = match doodad_rules.get(&d.kind) {
                Some(rule) => rule.place([d.x, d.y, d.z], d.scale),
                None => ([d.x, d.y, d.z], d.scale),
            };
            placements.push(Placement {
                model: model_path(look, d.variation),
                replaceable: look.replaceable.clone(),
                team: 0,
                position,
                angle: d.angle,
                scale,
            });
        }
    }
    if let Some(units) = chain
        .read_map_file("war3mapUnits.doo")
        .and_then(|d| PlacedUnits::parse(&d).ok())
    {
        for u in &units.units {
            if without_units && unit_ids.contains(&u.kind) {
                continue;
            }
            let Some(look) = unit_looks.get(&u.kind) else {
                continue;
            };
            let team = u8::try_from(u.owner).ok().filter(|&t| t < 16).unwrap_or(0);
            // Units, not items, are drawn shifted (see `UNIT_MODEL_SHIFT`).
            let shift = if unit_ids.contains(&u.kind) {
                UNIT_MODEL_SHIFT
            } else {
                0.0
            };
            placements.push(Placement {
                model: model_path(look, u.variation),
                replaceable: None,
                team,
                position: [u.x - shift, u.y - shift, u.z],
                angle: u.angle,
                scale: u.scale.map(|s| s * look.scale),
            });
        }
    }
    placements
}

/// The look of every unit type of `Units\unitUI.slk` and the map, by raw
/// code value.
pub fn unit_models(chain: &mut ArchiveChain) -> BTreeMap<i32, UnitLook> {
    unit_looks(chain)
        .into_iter()
        .map(|(id, look)| {
            let unit = UnitLook {
                model: model_path(&look, 0),
                scale: look.scale,
                selection: look.selection,
            };
            (i32::from_be_bytes(id.0), unit)
        })
        .collect()
}

/// The look of every unit type: `Units\unitUI.slk`, then the map's
/// changes to standard types and its own types (`war3map.w3u`), which
/// start from their base and may set the model (`umdl`), its scale
/// (`usca`) and the selection scale (`ussc`).
fn unit_looks(chain: &mut ArchiveChain) -> BTreeMap<RawCode, Look> {
    let mut looks: BTreeMap<RawCode, Look> = table(
        chain,
        "Units\\unitUI.slk",
        "unitUIID",
        &["file", "modelScale", "scale"],
    )
    .into_iter()
    .map(|(id, v)| {
        let look = Look {
            file: v[0].clone(),
            variations: 1,
            replaceable: None,
            scale: number(&v[1], 1.0),
            selection: number(&v[2], 1.0),
        };
        (id, look)
    })
    .collect();
    let Some(mods) = chain
        .read_map_file(ObjectKind::Units.file_name())
        .and_then(|d| ObjectModifications::parse(&d, ObjectKind::Units).ok())
    else {
        return looks;
    };
    let types = mods
        .original
        .iter()
        .map(|d| (d.base, d))
        .chain(mods.custom.iter().filter_map(|d| Some((d.id?, d))));
    for (id, d) in types {
        let Some(mut look) = looks.get(&d.base).cloned() else {
            continue;
        };
        // The last value of a field counts.
        for m in &d.modifications {
            match (&m.field.0, &m.value) {
                (b"umdl", ObjectValue::String(file)) => look.file = file.clone(),
                (b"usca", ObjectValue::Real(s) | ObjectValue::Unreal(s)) => look.scale = *s,
                (b"ussc", ObjectValue::Real(s) | ObjectValue::Unreal(s)) => look.selection = *s,
                _ => {}
            }
        }
        looks.insert(id, look);
    }
    looks
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0023, "Where models are drawn": `LPcr` (default 1, limits 0.8
    /// and 1.2) stored at 0.5, 1.0 and 2.0 is drawn at 0.8, 1.0 and 1.2;
    /// `LOca` (1.2, 1, 1.4) at 1.2, 1.2 and 1.68; `LOca`, which has a
    /// pathing texture, moves from 0, 400, -400, 256, -256 to 32, 416,
    /// -416, 288, -224.
    #[test]
    fn doodad_scale_and_grid() {
        let pcr = DoodadRule {
            default: 1.0,
            min: 0.8,
            max: 1.2,
            snaps: false,
        };
        let oca = DoodadRule {
            default: 1.2,
            min: 1.0,
            max: 1.4,
            snaps: true,
        };
        let drawn = |rule: &DoodadRule, s: f32| rule.place([0.0; 3], [s; 3]).1[0];
        let scales: Vec<f32> = [0.5, 1.0, 2.0].iter().map(|&s| drawn(&pcr, s)).collect();
        assert_eq!(scales, [0.8, 1.0, 1.2]);
        let scales: Vec<f32> = [0.5, 1.0, 2.0].iter().map(|&s| drawn(&oca, s)).collect();
        assert!((scales[0] - 1.2).abs() < 1e-5 && (scales[2] - 1.68).abs() < 1e-5);
        let moved: Vec<f32> = [0.0, 400.0, -400.0, 256.0, -256.0]
            .iter()
            .map(|&p| oca.place([p, 0.0, 0.0], [1.0; 3]).0[0])
            .collect();
        assert_eq!(moved, [32.0, 416.0, -416.0, 288.0, -224.0]);
        assert_eq!(
            pcr.place([-256.0, 5.0, 0.0], [1.0; 3]).0,
            [-256.0, 5.0, 0.0]
        );
    }

    #[test]
    fn model_paths() {
        let tree = Look {
            file: "Doodads\\Terrain\\LordaeronTree\\LordaeronTree".into(),
            variations: 10,
            ..Look::default()
        };
        assert_eq!(
            model_path(&tree, 3),
            "Doodads\\Terrain\\LordaeronTree\\LordaeronTree3.mdx"
        );
        let chest = Look {
            file: "Objects\\InventoryItems\\TreasureChest\\treasurechest.mdl".into(),
            variations: 1,
            ..Look::default()
        };
        assert_eq!(
            model_path(&chest, 0),
            "Objects\\InventoryItems\\TreasureChest\\treasurechest.mdx"
        );
    }
}
