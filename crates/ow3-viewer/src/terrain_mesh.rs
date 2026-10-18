//! The ground of a map as triangles: one quad per tile and ground texture
//! layer (SPEC-0012), grouped by texture so that drawing the groups in
//! increasing texture index draws every tile's layers in order.
//!
//! Tiles with a cliff model have no ground (SPEC-0014). The water surface
//! is a separate mesh over every tile with water, coloured per vertex by
//! the depth (SPEC-0014).
//!
//! PROVISIONAL: the map boundary is darkened, and ramp points with the
//! letter R or S are drawn half a level up to meet the ramp models
//! ([`ramp_lift`]).

use std::collections::BTreeSet;

use bytemuck::{Pod, Zeroable};
use ow3_formats::map::terrain::{Terrain, TerrainPoint};
use ow3_render::cliffs::ramp_lift;
use ow3_render::image::Image;
use ow3_render::terrain::{cell_origin, cell_size, corners, ground_height, height, tile_layers};
use ow3_render::water::{self, Depths, WaterType};

/// Size of a tile in world units.
pub const TILE: f32 = 128.0;
const BOUNDARY: u8 = 0x8;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    /// Brightness of the ground in `rgb` (the boundary is darker); the
    /// colour of the water.
    pub tint: [f32; 4],
    /// The atlas cell, a layer of the texture array (0 for water).
    pub cell: u32,
}

/// The cells of a ground texture atlas, by number (see
/// [`ow3_render::terrain::cell_origin`]), as square images.
pub fn atlas_cells(atlas: &Image) -> Vec<Image> {
    let size = cell_size(atlas);
    let count = 16 * (atlas.width / (4 * size)).max(1);
    (0..count)
        .map(|cell| {
            let (cx, cy) = cell_origin(cell, size);
            let mut image = Image::new(size, size);
            for y in 0..size {
                for x in 0..size {
                    let p = if cx + x < atlas.width && cy + y < atlas.height {
                        atlas.pixel(cx + x, cy + y)
                    } else {
                        [0; 4]
                    };
                    image.set(x, y, p);
                }
            }
            image
        })
        .collect()
}

/// Triangles of one ground texture.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layer {
    pub texture: usize,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

/// The drawn height of every point: its height, plus half a level for the
/// middle points of ramps ([`ramp_lift`]).
struct Ground<'a> {
    terrain: &'a Terrain,
    heights: Vec<f32>,
}

impl<'a> Ground<'a> {
    fn new(terrain: &'a Terrain) -> Ground<'a> {
        let heights = (0..terrain.height)
            .flat_map(|y| (0..terrain.width).map(move |x| (x, y)))
            .map(|(x, y)| {
                let p = &terrain.points[(y * terrain.width + x) as usize];
                height(p) + ramp_lift(terrain, x, y)
            })
            .collect();
        Ground { terrain, heights }
    }

    /// World position of point (`x`, `y`).
    fn position(&self, x: u32, y: u32) -> [f32; 3] {
        [
            self.terrain.origin_x + x as f32 * TILE,
            self.terrain.origin_y + y as f32 * TILE,
            self.heights[(y * self.terrain.width + x) as usize],
        ]
    }

    /// The normal at a point (SPEC-0017, "Sloped ground").
    fn normal(&self, x: u32, y: u32) -> [f32; 3] {
        ground_normal(self.terrain, x, y)
    }
}

/// The normal of the ground at point (`x`, `y`) (SPEC-0017, "Sloped
/// ground"): the central differences of the raw heights of its
/// neighbours, without their cliff layers, so the ground beside a cliff
/// is lit as flat ground. Points beyond the edge of the map count as the
/// edge's (PROVISIONAL: not measured).
pub fn ground_normal(terrain: &Terrain, x: u32, y: u32) -> [f32; 3] {
    let at = |x: i64, y: i64| {
        let x = x.clamp(0, i64::from(terrain.width) - 1) as u32;
        let y = y.clamp(0, i64::from(terrain.height) - 1) as u32;
        ground_height(&terrain.points[(y * terrain.width + x) as usize])
    };
    let (x, y) = (i64::from(x), i64::from(y));
    let dx = (at(x + 1, y) - at(x - 1, y)) / (2.0 * TILE);
    let dy = (at(x, y + 1) - at(x, y - 1)) / (2.0 * TILE);
    let n = [-dx, -dy, 1.0];
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    n.map(|c| c / l)
}

fn tint(p: &TerrainPoint) -> [f32; 4] {
    let bright = if p.flags() & BOUNDARY != 0 { 0.45 } else { 1.0 };
    [bright, bright, bright, 1.0]
}

/// The water surface over every tile with water, `cells` tiles per repeat
/// of its texture.
pub fn water(terrain: &Terrain, kind: &WaterType, depths: Depths) -> Layer {
    let table = water::colour_table(depths, &kind.colours);
    let mut layer = Layer::default();
    let ground_points = Ground::new(terrain);
    let (tiles_x, tiles_y) = (terrain.width - 1, terrain.height - 1);
    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            if !water::has_water(&corners(terrain, tx, ty)) {
                continue;
            }
            let base = layer.vertices.len() as u32;
            for (x, y) in [(tx, ty), (tx + 1, ty), (tx + 1, ty + 1), (tx, ty + 1)] {
                let p = &terrain.points[(y * terrain.width + x) as usize];
                let [px, py, ground] = ground_points.position(x, y);
                let surface = water::surface_height(p, kind.height);
                let colour = table[water::colour_index(surface - ground, depths)];
                layer.vertices.push(Vertex {
                    position: [px, py, surface],
                    normal: [0.0, 0.0, 1.0],
                    uv: [x as f32 / kind.cells, -(y as f32) / kind.cells],
                    tint: colour.map(|c| f32::from(c) / 255.0),
                    cell: 0,
                });
            }
            layer
                .indices
                .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
    layer
}

/// The ground mesh, one layer per ground texture that is used, in
/// increasing texture index; tiles with a cliff model are left out.
pub fn build(
    terrain: &Terrain,
    textures: &[Option<Image>],
    cliffs: &BTreeSet<(u32, u32)>,
) -> Vec<Layer> {
    let mut layers: Vec<Layer> = (0..textures.len())
        .map(|texture| Layer {
            texture,
            ..Layer::default()
        })
        .collect();
    let ground_points = Ground::new(terrain);
    let (tiles_x, tiles_y) = (terrain.width - 1, terrain.height - 1);
    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            if cliffs.contains(&(tx, ty)) {
                continue;
            }
            let points = corners(terrain, tx, ty);
            for (texture, cell) in tile_layers(&points, textures) {
                if textures[texture].is_none() {
                    continue;
                }
                let layer = &mut layers[texture];
                let base = layer.vertices.len() as u32;
                // Bottom left, bottom right, top right, top left; image row
                // 0 is the north edge of the tile. Each cell of the atlas is
                // its own texture layer, so that its smaller copies do not
                // take in the neighbouring cells.
                let corners = [
                    (tx, ty, [0.0, 1.0], points[1]),
                    (tx + 1, ty, [1.0, 1.0], points[0]),
                    (tx + 1, ty + 1, [1.0, 0.0], points[2]),
                    (tx, ty + 1, [0.0, 0.0], points[3]),
                ];
                for (x, y, uv, point) in corners {
                    layer.vertices.push(Vertex {
                        position: ground_points.position(x, y),
                        normal: ground_points.normal(x, y),
                        uv,
                        tint: tint(point),
                        cell,
                    });
                }
                // Split from the top left to the bottom right corner (SPEC-0017).
                layer
                    .indices
                    .extend([base, base + 1, base + 3, base + 1, base + 2, base + 3]);
            }
        }
    }
    layers.retain(|l| !l.indices.is_empty());
    layers
}

/// The height of the ground at (`x`, `y`), interpolated between the
/// corners of its tile; the nearest edge outside the map.
pub fn ground_z(terrain: &Terrain, x: f32, y: f32) -> f32 {
    if terrain.width < 2 || terrain.height < 2 {
        return 0.0;
    }
    let fx = ((x - terrain.origin_x) / TILE).clamp(0.0, (terrain.width - 1) as f32);
    let fy = ((y - terrain.origin_y) / TILE).clamp(0.0, (terrain.height - 1) as f32);
    let (tx, ty) = (
        (fx as u32).min(terrain.width - 2),
        (fy as u32).min(terrain.height - 2),
    );
    let (u, v) = (fx - tx as f32, fy - ty as f32);
    let h = |x: u32, y: u32| height(&terrain.points[(y * terrain.width + x) as usize]);
    let bottom = h(tx, ty) + (h(tx + 1, ty) - h(tx, ty)) * u;
    let top = h(tx, ty + 1) + (h(tx + 1, ty + 1) - h(tx, ty + 1)) * u;
    bottom + (top - bottom) * v
}

/// The middle of the map, on the ground.
pub fn centre(terrain: &Terrain) -> [f32; 3] {
    let (x, y) = ((terrain.width - 1) / 2, (terrain.height - 1) / 2);
    Ground::new(terrain).position(x, y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ow3_formats::binary::RawCode;

    fn point(texture: u8) -> TerrainPoint {
        TerrainPoint {
            ground_height: 0x2000,
            water_and_edge: 0,
            flags_and_texture: texture,
            texture_details: 0,
            cliff_and_layer: 2,
        }
    }

    /// A tile with texture 1 in one corner: a full quad of texture 0, then
    /// a quad of texture 1, both over the same square.
    #[test]
    fn one_quad_per_layer() {
        let terrain = Terrain {
            format_version: 11,
            tileset: b'L',
            custom_tilesets: false,
            ground_tilesets: vec![RawCode(*b"Ldrt"), RawCode(*b"Lgrs")],
            cliff_tilesets: Vec::new(),
            width: 2,
            height: 2,
            origin_x: -128.0,
            origin_y: 0.0,
            points: vec![point(0), point(1), point(0), point(0)],
        };
        let textures = [Some(Image::new(512, 256)), Some(Image::new(256, 256))];
        let layers = build(&terrain, &textures, &BTreeSet::new());
        assert_eq!(layers.len(), 2);
        assert_eq!((layers[0].texture, layers[1].texture), (0, 1));
        for layer in &layers {
            assert_eq!(layer.indices.len(), 6);
            let corners: Vec<[f32; 3]> = layer.vertices.iter().map(|v| v.position).collect();
            assert_eq!(
                corners,
                [
                    [-128.0, 0.0, 0.0],
                    [0.0, 0.0, 0.0],
                    [0.0, 128.0, 0.0],
                    [-128.0, 128.0, 0.0]
                ]
            );
        }
        // Texture 0 fills the tile with its variation cell (16, the first
        // right-hand cell), texture 1 uses cell 1 (bottom right corner).
        assert_eq!(layers[0].vertices[0].cell, 16);
        assert_eq!(layers[1].vertices[0].cell, 1);
        let uvs: Vec<[f32; 2]> = layers[1].vertices.iter().map(|v| v.uv).collect();
        assert_eq!(uvs, [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]);
        assert_eq!(layers[0].vertices[0].normal, [0.0, 0.0, 1.0]);
    }

    /// Cells are cut from the atlas in the numbering of `cell_origin`.
    #[test]
    fn cells_of_an_atlas() {
        let mut atlas = Image::new(512, 256);
        // Cell 1 is the second in the top row of the left half; cell 16
        // the first of the right half.
        atlas.set(64, 0, [1, 2, 3, 4]);
        atlas.set(256, 0, [5, 6, 7, 8]);
        let cells = atlas_cells(&atlas);
        assert_eq!(cells.len(), 32);
        assert_eq!(cells[1].pixel(0, 0), [1, 2, 3, 4]);
        assert_eq!(cells[16].pixel(0, 0), [5, 6, 7, 8]);
    }
}
