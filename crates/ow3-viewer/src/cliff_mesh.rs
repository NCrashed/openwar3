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

use crate::models::{Group, Instance, Part, parts};
use crate::terrain_mesh::TILE;

/// The replaceable texture id of cliff models.
const CLIFF_TEXTURE: u32 = 11;
const IDENTITY: crate::camera::Mat4 = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// A cliff type's model directories, texture and ground.
#[derive(Clone, Debug, PartialEq)]
struct CliffType {
    model_dir: String,
    ramp_dir: String,
    texture: String,
    /// `groundTile`: the ground the corners of its cliff tiles get.
    ground: Option<RawCode>,
}

fn raw_code(text: &str) -> Option<RawCode> {
    <[u8; 4]>::try_from(text.as_bytes()).ok().map(RawCode)
}

/// The cliff texture `<dir>\<file>.blp` for tileset `tileset` (SPEC-0014
/// rule 7): the map's own copy, else the game's copy with the tileset
/// letter, else the game's plain copy.
///
/// PROVISIONAL: a lettered copy imported into the map would be read in
/// place of the game's when the texture is loaded.
fn cliff_texture(chain: &mut ArchiveChain, dir: &str, file: &str, tileset: u8) -> String {
    let plain = format!("{dir}\\{file}.blp");
    if chain.read_map_file(&plain).is_some() {
        return plain;
    }
    let lettered = format!("{dir}\\{}_{file}.blp", char::from(tileset));
    if chain.read_game_file(&lettered).is_some() {
        return lettered;
    }
    plain
}

fn cliff_types(chain: &mut ArchiveChain, tileset: u8) -> BTreeMap<RawCode, CliffType> {
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
    let ground = column("groundTile");
    for row in 2..=table.last_row() {
        let text = |c: u32| table.get(row, c).map(|v| v.raw().to_owned());
        let Some(code) = text(id).as_deref().and_then(raw_code) else {
            continue;
        };
        let texture = cliff_texture(
            chain,
            &text(tex_dir).unwrap_or_default(),
            &text(tex_file).unwrap_or_default(),
            tileset,
        );
        types.insert(
            code,
            CliffType {
                model_dir: text(dir).unwrap_or_default(),
                ramp_dir: text(ramp_dir).unwrap_or_default(),
                texture,
                ground: ground.and_then(text).as_deref().and_then(raw_code),
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
    /// The ground of a cliff model's corners (rule 8); none for ramps.
    ground: Option<RawCode>,
}

/// The ramp and cliff models of `terrain`.
fn placements(chain: &mut ArchiveChain, terrain: &Terrain) -> Vec<Placed> {
    let types = cliff_types(chain, terrain.tileset);
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
                ground: None,
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
                ground: kind.ground,
            });
        }
    }
    placed
}

/// The cliffs and ramps of a map.
pub struct Cliffs {
    /// One group per model and texture, already in world coordinates.
    pub groups: Vec<Group>,
    /// The tiles whose ground is not drawn.
    pub covered: BTreeSet<(u32, u32)>,
    /// The cliff tiles and the ground their corners get, in tile order.
    pub grounds: Vec<((u32, u32), RawCode)>,
}

/// `terrain` with the corners of cliff tiles given the ground of their
/// cliff type (SPEC-0014 rule 8), with the ground indices of `tiles`.
///
/// PROVISIONAL (SPEC-0014 open question 5): where cliff tiles of
/// different types share a point, the later tile (by rows from the
/// south) wins; a ground missing from the map's list is not added.
pub fn repaint(terrain: &Terrain, tiles: &[RawCode], grounds: &[((u32, u32), RawCode)]) -> Terrain {
    let mut painted = terrain.clone();
    for &((x, y), code) in grounds {
        let Some(index) = tiles.iter().position(|c| *c == code) else {
            continue;
        };
        for (px, py) in [(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1)] {
            let p = &mut painted.points[(py * terrain.width + px) as usize];
            p.flags_and_texture = (p.flags_and_texture & 0xF0) | index as u8;
        }
    }
    painted
}

/// The ramp and cliff models of `terrain`.
pub fn build(chain: &mut ArchiveChain, terrain: &Terrain) -> Cliffs {
    let mut models: BTreeMap<String, Option<Model>> = BTreeMap::new();
    // Parts of each model, and the baked parts, by model path and texture.
    let mut templates: BTreeMap<(String, String), Vec<Part>> = BTreeMap::new();
    let mut baked: BTreeMap<(String, String), Vec<Part>> = BTreeMap::new();
    let mut covered = BTreeSet::new();
    let mut grounds = Vec::new();
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
                    unshaded: p.unshaded,
                    // Cliff models cover the whole atlas, rock above and
                    // ground below: no repeating across its edges.
                    clamp: true,
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
        if let Some(ground) = item.ground {
            grounds.push((item.origin, ground));
        }
    }
    let groups = baked
        .into_iter()
        .map(|((model, _), parts)| Group {
            model,
            parts,
            instances: vec![Instance {
                matrix: IDENTITY,
                bone_base: 0,
                tint: [1.0; 3],
                alpha_shift: 0,
                // Cliffs are ground: they take the light of the terrain.
                terrain_light: 1,
            }],
        })
        .collect();
    Cliffs {
        groups,
        covered,
        grounds,
    }
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
        let vertex = |position| ModelVertex::fixed(position, [1.0, 0.0, 0.0], [0.0, 0.0]);
        let part = Part {
            vertices: vec![
                vertex([0.0, 0.0, 128.0]),
                vertex([0.0, 128.0, 0.0]),
                vertex([-128.0, 128.0, 0.0]),
            ],
            indices: vec![0, 1, 2],
            texture: String::new(),
            filter_mode: 0,
            unshaded: false,
            clamp: true,
        };
        let mut into = Part {
            vertices: Vec::new(),
            indices: Vec::new(),
            texture: String::new(),
            filter_mode: 0,
            unshaded: false,
            clamp: true,
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

    /// The four corners of a cliff tile take its ground; a ground missing
    /// from the map's list changes nothing.
    #[test]
    fn corners_take_the_cliff_ground() {
        let point = TerrainPoint {
            ground_height: 8192,
            water_and_edge: 0,
            flags_and_texture: 0x40,
            texture_details: 0,
            cliff_and_layer: 2,
        };
        let terrain = Terrain {
            format_version: 11,
            tileset: b'N',
            custom_tilesets: false,
            ground_tilesets: vec![RawCode(*b"Ndrt"), RawCode(*b"Nsnw")],
            cliff_tilesets: Vec::new(),
            width: 3,
            height: 2,
            origin_x: 0.0,
            origin_y: 0.0,
            points: vec![point; 6],
        };
        let painted = repaint(
            &terrain,
            &terrain.ground_tilesets,
            &[((1, 0), RawCode(*b"Nsnw"))],
        );
        let textures: Vec<u8> = painted.points.iter().map(|p| p.ground_texture()).collect();
        assert_eq!(textures, [0, 1, 1, 0, 1, 1]);
        // The flags stay.
        assert_eq!(painted.points[1].flags(), 4);
        let unchanged = repaint(
            &terrain,
            &terrain.ground_tilesets,
            &[((0, 0), RawCode(*b"Xxxx"))],
        );
        assert_eq!(unchanged.points, terrain.points);
    }
}
