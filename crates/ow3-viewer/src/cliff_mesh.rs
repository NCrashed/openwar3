//! Cliff models on their tiles (SPEC-0014), baked into one mesh per model
//! and texture.
//!
//! The model is chosen by [`ow3_render::cliffs`]; its directory and
//! texture (replaceable texture 11, `texDir\texFile.blp`) come from the
//! cliff type's row of `TerrainArt\CliffTypes.slk`.
//!
//! PROVISIONAL (SPEC-0014 open question 2): the models span x -128..0 and
//! y 0..128 and raise the corner of the letter `B` at the model's (0, 0)
//! for the bottom left corner of the name, (0, 128) for the bottom right,
//! (-128, 128) for the top right and (-128, 0) for the top left; so the
//! model point (x, y) is placed at (y, -x) from the tile's bottom left
//! corner, at the height of its lowest level, and each vertex is raised by
//! the ground heights of the corners (without layers), interpolated
//! bilinearly.

use std::collections::{BTreeMap, BTreeSet};

use ow3_formats::binary::RawCode;
use ow3_formats::map::terrain::Terrain;
use ow3_formats::mdx::Model;
use ow3_formats::mpq::ArchiveChain;
use ow3_formats::slk::Table;
use ow3_render::cliffs::{self, CliffTile};
use ow3_render::terrain::ground_height;

use crate::models::{Group, Part, parts};
use crate::terrain_mesh::TILE;

/// The replaceable texture id of cliff models.
const CLIFF_TEXTURE: u32 = 11;
const IDENTITY: crate::camera::Mat4 = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// A cliff type's model directory and texture.
#[derive(Clone, Debug, PartialEq)]
struct CliffType {
    model_dir: String,
    texture: String,
}

fn cliff_types(chain: &mut ArchiveChain) -> BTreeMap<RawCode, CliffType> {
    let mut types = BTreeMap::new();
    let Some(table) = chain
        .read("TerrainArt\\CliffTypes.slk")
        .and_then(|d| Table::parse(&d).ok())
    else {
        return types;
    };
    let column = |name: &str| table.column(name);
    let (Some(id), Some(dir), Some(tex_dir), Some(tex_file)) = (
        column("cliffID"),
        column("cliffModelDir"),
        column("texDir"),
        column("texFile"),
    ) else {
        return types;
    };
    for row in 2..=table.last_row() {
        let text = |c: u32| table.get(row, c).map(|v| v.raw().to_owned());
        let Some(Ok(code)) = text(id).map(|t| <[u8; 4]>::try_from(t.as_bytes())) else {
            continue;
        };
        types.insert(
            RawCode(code),
            CliffType {
                model_dir: text(dir).unwrap_or_default(),
                texture: format!(
                    "{}\\{}.blp",
                    text(tex_dir).unwrap_or_default(),
                    text(tex_file).unwrap_or_default()
                ),
            },
        );
    }
    types
}

/// The tiles that have a cliff model.
pub fn cliff_tiles(terrain: &Terrain) -> BTreeMap<(u32, u32), CliffTile> {
    let mut tiles = BTreeMap::new();
    for y in 0..terrain.height.saturating_sub(1) {
        for x in 0..terrain.width.saturating_sub(1) {
            if let Some(tile) = cliffs::cliff_tile(terrain, x, y) {
                tiles.insert((x, y), tile);
            }
        }
    }
    tiles
}

/// `part` moved onto tile (`x`, `y`) and appended to `into`.
fn place(into: &mut Part, part: &Part, terrain: &Terrain, x: u32, y: u32, base_level: u8) {
    let corner = |dx, dy| terrain.point(x + dx, y + dy).map_or(0.0, ground_height);
    let [bl, br, tl, tr] = [corner(0, 0), corner(1, 0), corner(0, 1), corner(1, 1)];
    let x0 = terrain.origin_x + x as f32 * TILE;
    let y0 = terrain.origin_y + y as f32 * TILE;
    let lift = (f32::from(base_level.min(14)) - 2.0) * TILE;
    let first = into.vertices.len() as u32;
    for v in &part.vertices {
        let [mx, my, mz] = v.position;
        let (u, w) = ((my / TILE).clamp(0.0, 1.0), (-mx / TILE).clamp(0.0, 1.0));
        let ground =
            bl * (1.0 - u) * (1.0 - w) + br * u * (1.0 - w) + tl * (1.0 - u) * w + tr * u * w;
        let mut vertex = *v;
        vertex.position = [x0 + my, y0 - mx, mz + lift + ground];
        let [nx, ny, nz] = v.normal;
        vertex.normal = [ny, -nx, nz];
        into.vertices.push(vertex);
    }
    into.indices.extend(part.indices.iter().map(|i| i + first));
}

/// The cliff models of `tiles`, one group per model and texture, already
/// in world coordinates, and the tiles that got a model.
pub fn build(
    chain: &mut ArchiveChain,
    terrain: &Terrain,
    tiles: &BTreeMap<(u32, u32), CliffTile>,
) -> (Vec<Group>, BTreeSet<(u32, u32)>) {
    let types = cliff_types(chain);
    let mut models: BTreeMap<String, Option<Model>> = BTreeMap::new();
    let mut counts: BTreeMap<(String, String), u8> = BTreeMap::new();
    // Baked parts by model path, texture and part index.
    let mut baked: BTreeMap<(String, String), Vec<Part>> = BTreeMap::new();
    let mut templates: BTreeMap<(String, String), Vec<Part>> = BTreeMap::new();
    let mut covered = BTreeSet::new();
    for (&(x, y), tile) in tiles {
        let Some(cliff_type) = terrain
            .cliff_tilesets
            .get(usize::from(tile.cliff_type))
            .and_then(|code| types.get(code))
        else {
            continue;
        };
        let dir = &cliff_type.model_dir;
        let count = *counts
            .entry((dir.clone(), tile.letters.clone()))
            .or_insert_with(|| {
                (0..4u8)
                    .take_while(|&n| chain.contains(&cliffs::model_path(dir, &tile.letters, n)))
                    .count() as u8
            });
        if count == 0 {
            continue;
        }
        let n = cliffs::model_number(tile.variation, count);
        let path = cliffs::model_path(dir, &tile.letters, n);
        let key = (path.clone(), cliff_type.texture.clone());
        let template = templates.entry(key.clone()).or_insert_with(|| {
            let model = models
                .entry(path.clone())
                .or_insert_with(|| chain.read(&path).and_then(|d| Model::parse(&d).ok()));
            let Some(model) = model else {
                return Vec::new();
            };
            let replacements = [(CLIFF_TEXTURE, cliff_type.texture.clone())];
            let paths = ow3_render::model::texture_paths(model, &replacements, 0);
            parts(model, &paths)
        });
        let parts = baked.entry(key).or_insert_with(|| {
            template
                .iter()
                .map(|p| Part {
                    vertices: Vec::new(),
                    indices: Vec::new(),
                    texture: p.texture.clone(),
                    filter_mode: p.filter_mode,
                })
                .collect()
        });
        if template.is_empty() {
            continue;
        }
        for (into, part) in parts.iter_mut().zip(template.iter()) {
            place(into, part, terrain, x, y, tile.base_level);
        }
        covered.insert((x, y));
    }
    let groups = baked
        .into_iter()
        .filter(|(_, parts)| !parts.is_empty())
        .map(|((model, _), parts)| Group {
            model,
            parts,
            instances: vec![IDENTITY],
        })
        .collect();
    (groups, covered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ModelVertex;
    use ow3_formats::map::terrain::TerrainPoint;

    /// The raised corner of a model (BL of the name, at the model's
    /// (0, 0)) lands on the tile's bottom left corner, one level up.
    #[test]
    fn placement_on_the_tile() {
        let point = |level: u8, h: u16| TerrainPoint {
            ground_height: h,
            water_and_edge: 0,
            flags_and_texture: 0,
            texture_details: 0,
            cliff_and_layer: level,
        };
        let terrain = Terrain {
            format_version: 11,
            tileset: b'L',
            custom_tilesets: false,
            ground_tilesets: Vec::new(),
            cliff_tilesets: Vec::new(),
            width: 2,
            height: 2,
            origin_x: 1000.0,
            origin_y: 2000.0,
            // BL 40 units above the others' ground.
            points: vec![
                point(3, 8192 + 160),
                point(2, 8192),
                point(2, 8192),
                point(2, 8192),
            ],
        };
        let vertex = |position| ModelVertex {
            position,
            normal: [1.0, 0.0, 0.0],
            uv: [0.0, 0.0],
        };
        let part = Part {
            vertices: vec![
                vertex([0.0, 0.0, 128.0]),
                vertex([0.0, 128.0, 0.0]),
                vertex([-128.0, 128.0, 0.0]),
            ],
            indices: vec![0, 1, 2],
            texture: String::new(),
            filter_mode: 0,
        };
        let mut into = Part {
            vertices: Vec::new(),
            indices: Vec::new(),
            texture: String::new(),
            filter_mode: 0,
        };
        place(&mut into, &part, &terrain, 0, 0, 2);
        let at: Vec<[f32; 3]> = into.vertices.iter().map(|v| v.position).collect();
        assert_eq!(
            at,
            [
                [1000.0, 2000.0, 168.0],
                [1128.0, 2000.0, 0.0],
                [1128.0, 2128.0, 0.0]
            ]
        );
        assert_eq!(into.vertices[0].normal, [0.0, -1.0, 0.0]);
    }
}
