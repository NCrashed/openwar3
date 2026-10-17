//! Cliff and ramp models on their tiles (SPEC-0014), baked into one mesh
//! per model and texture.
//!
//! The models are chosen by [`ow3_render::cliffs`]; their directories
//! (`cliffModelDir`, `rampModelDir`) and texture (replaceable texture 11,
//! `texDir\texFile.blp`) come from the cliff type's row of
//! `TerrainArt\CliffTypes.slk`. A ramp takes its two tiles first; the
//! other tiles get cliff models.
//!
//! PROVISIONAL (SPEC-0014 open question 2): the models span x -128..0 (or
//! -256..0) and y 0..128 (or 0..256) and raise the corner of the letter
//! `B` at the model's (0, 0) for the bottom left corner of the name,
//! (0, 128) for the bottom right, (-128, 128) for the top right and
//! (-128, 0) for the top left; so the model point (x, y) is placed at
//! (y, -x) from the bottom left corner of the lower left tile, at the
//! height of the lowest level, and each vertex is raised by the ground
//! heights of the corners of its tile (without layers), interpolated
//! bilinearly.

use std::collections::{BTreeMap, BTreeSet};

use ow3_formats::binary::RawCode;
use ow3_formats::map::terrain::Terrain;
use ow3_formats::mdx::Model;
use ow3_formats::mpq::ArchiveChain;
use ow3_formats::slk::Table;
use ow3_render::cliffs;
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

/// A cliff type's model directories and texture.
#[derive(Clone, Debug, PartialEq)]
struct CliffType {
    model_dir: String,
    ramp_dir: String,
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
    let (Some(id), Some(dir), Some(ramp_dir), Some(tex_dir), Some(tex_file)) = (
        column("cliffID"),
        column("cliffModelDir"),
        column("rampModelDir"),
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
                ramp_dir: text(ramp_dir).unwrap_or_default(),
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

/// `part` moved onto the `size` tiles from tile `origin` and appended to
/// `into`.
fn place(
    into: &mut Part,
    part: &Part,
    terrain: &Terrain,
    origin: (u32, u32),
    size: (u32, u32),
    base_level: u8,
) {
    let x0 = terrain.origin_x + origin.0 as f32 * TILE;
    let y0 = terrain.origin_y + origin.1 as f32 * TILE;
    let lift = (f32::from(base_level.min(14)) - 2.0) * TILE;
    let first = into.vertices.len() as u32;
    for v in &part.vertices {
        let [mx, my, mz] = v.position;
        let (ox, oy) = (my, -mx);
        // The tile of the vertex and its place in it.
        let tile = |o: f32, n: u32| ((o / TILE).floor().max(0.0) as u32).min(n - 1);
        let (i, j) = (tile(ox, size.0), tile(oy, size.1));
        let u = (ox / TILE - i as f32).clamp(0.0, 1.0);
        let w = (oy / TILE - j as f32).clamp(0.0, 1.0);
        let (tx, ty) = (origin.0 + i, origin.1 + j);
        let corner = |dx, dy| terrain.point(tx + dx, ty + dy).map_or(0.0, ground_height);
        let [bl, br, tl, tr] = [corner(0, 0), corner(1, 0), corner(0, 1), corner(1, 1)];
        let ground =
            bl * (1.0 - u) * (1.0 - w) + br * u * (1.0 - w) + tl * (1.0 - u) * w + tr * u * w;
        let mut vertex = *v;
        vertex.position = [x0 + ox, y0 + oy, mz + lift + ground];
        let [nx, ny, nz] = v.normal;
        vertex.normal = [ny, -nx, nz];
        into.vertices.push(vertex);
    }
    into.indices.extend(part.indices.iter().map(|i| i + first));
}

/// A model to place: its path, texture, tiles and level.
struct Placed {
    path: String,
    texture: String,
    origin: (u32, u32),
    size: (u32, u32),
    base_level: u8,
}

/// The ramp and cliff models of `terrain`.
fn placements(chain: &mut ArchiveChain, terrain: &Terrain) -> Vec<Placed> {
    let types = cliff_types(chain);
    let cliff_type = |index: u8| {
        terrain
            .cliff_tilesets
            .get(usize::from(index))
            .and_then(|code| types.get(code))
    };
    let (tiles_x, tiles_y) = (
        terrain.width.saturating_sub(1),
        terrain.height.saturating_sub(1),
    );
    let mut placed = Vec::new();
    let mut taken = BTreeSet::new();
    for y in 0..tiles_y {
        for x in 0..tiles_x {
            let Some(ramp) = cliffs::ramp_tile(terrain, x, y) else {
                continue;
            };
            let Some(kind) = cliff_type(ramp.cliff_type) else {
                continue;
            };
            let path = cliffs::model_path(&kind.ramp_dir, &ramp.letters, 0);
            if !chain.contains(&path) {
                continue;
            }
            for dy in 0..ramp.size.1 {
                for dx in 0..ramp.size.0 {
                    taken.insert((ramp.origin.0 + dx, ramp.origin.1 + dy));
                }
            }
            placed.push(Placed {
                path,
                texture: kind.texture.clone(),
                origin: ramp.origin,
                size: ramp.size,
                base_level: ramp.base_level,
            });
        }
    }
    let mut counts: BTreeMap<(String, String), u8> = BTreeMap::new();
    for y in 0..tiles_y {
        for x in 0..tiles_x {
            if taken.contains(&(x, y)) {
                continue;
            }
            let Some(tile) = cliffs::cliff_tile(terrain, x, y) else {
                continue;
            };
            let Some(kind) = cliff_type(tile.cliff_type) else {
                continue;
            };
            let dir = &kind.model_dir;
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
            placed.push(Placed {
                path: cliffs::model_path(dir, &tile.letters, n),
                texture: kind.texture.clone(),
                origin: (x, y),
                size: (1, 1),
                base_level: tile.base_level,
            });
        }
    }
    placed
}

/// The ramp and cliff models of `terrain`, one group per model and
/// texture, already in world coordinates, and the tiles they cover.
pub fn build(chain: &mut ArchiveChain, terrain: &Terrain) -> (Vec<Group>, BTreeSet<(u32, u32)>) {
    let mut models: BTreeMap<String, Option<Model>> = BTreeMap::new();
    // Parts of each model, and the baked parts, by model path and texture.
    let mut templates: BTreeMap<(String, String), Vec<Part>> = BTreeMap::new();
    let mut baked: BTreeMap<(String, String), Vec<Part>> = BTreeMap::new();
    let mut covered = BTreeSet::new();
    for item in placements(chain, terrain) {
        let key = (item.path.clone(), item.texture.clone());
        let template = templates.entry(key.clone()).or_insert_with(|| {
            let model = models
                .entry(item.path.clone())
                .or_insert_with(|| chain.read(&item.path).and_then(|d| Model::parse(&d).ok()));
            let Some(model) = model else {
                return Vec::new();
            };
            let replacements = [(CLIFF_TEXTURE, item.texture.clone())];
            let paths = ow3_render::model::texture_paths(model, &replacements, 0);
            parts(model, &paths)
        });
        if template.is_empty() {
            continue;
        }
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
        for (into, part) in parts.iter_mut().zip(template.iter()) {
            place(into, part, terrain, item.origin, item.size, item.base_level);
        }
        for dy in 0..item.size.1 {
            for dx in 0..item.size.0 {
                covered.insert((item.origin.0 + dx, item.origin.1 + dy));
            }
        }
    }
    let groups = baked
        .into_iter()
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
        place(&mut into, &part, &terrain, (0, 0), (1, 1), 2);
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
