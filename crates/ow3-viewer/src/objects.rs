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
pub fn placements(chain: &mut ArchiveChain) -> Vec<Placement> {
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
            },
        );
    }
    for (id, v) in table(chain, "Doodads\\Doodads.slk", "doodID", &["file", "numVar"]) {
        looks.insert(
            id,
            Look {
                file: v[0].clone(),
                variations: v[1].parse().unwrap_or(1),
                replaceable: None,
                scale: 1.0,
            },
        );
    }
    let mut unit_looks: BTreeMap<RawCode, Look> = BTreeMap::new();
    for (id, v) in table(
        chain,
        "Units\\unitUI.slk",
        "unitUIID",
        &["file", "modelScale"],
    ) {
        unit_looks.insert(
            id,
            Look {
                file: v[0].clone(),
                variations: 1,
                replaceable: None,
                scale: number(&v[1], 1.0),
            },
        );
    }
    for (id, v) in table(chain, "Units\\ItemData.slk", "itemID", &["file"]) {
        unit_looks.insert(
            id,
            Look {
                file: v[0].clone(),
                variations: 1,
                replaceable: None,
                scale: 1.0,
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
            placements.push(Placement {
                model: model_path(look, d.variation),
                replaceable: look.replaceable.clone(),
                team: 0,
                position: [d.x, d.y, d.z],
                angle: d.angle,
                scale: d.scale,
            });
        }
    }
    if let Some(units) = chain
        .read_map_file("war3mapUnits.doo")
        .and_then(|d| PlacedUnits::parse(&d).ok())
    {
        for u in &units.units {
            let Some(look) = unit_looks.get(&u.kind) else {
                continue;
            };
            let team = u8::try_from(u.owner).ok().filter(|&t| t < 16).unwrap_or(0);
            placements.push(Placement {
                model: model_path(look, u.variation),
                replaceable: None,
                team,
                position: [u.x, u.y, u.z],
                angle: u.angle,
                scale: u.scale.map(|s| s * look.scale),
            });
        }
    }
    placements
}

#[cfg(test)]
mod tests {
    use super::*;

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
