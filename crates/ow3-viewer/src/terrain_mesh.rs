//! The ground of a map as triangles: one quad per tile and ground texture
//! layer (SPEC-0012), grouped by texture so that drawing the groups in
//! increasing texture index draws every tile's layers in order.
//!
//! PROVISIONAL, until specs cover them: heights include cliff layers, so
//! cliffs show as steep slopes without cliff models; water darkens points
//! with the water flag towards blue and the map boundary is darkened.

use bytemuck::{Pod, Zeroable};
use ow3_formats::map::terrain::{Terrain, TerrainPoint};
use ow3_render::image::Image;
use ow3_render::terrain::{cell_origin, cell_size, corners, height, tile_layers};

/// Size of a tile in world units.
pub const TILE: f32 = 128.0;
const WATER: u8 = 0x4;
const BOUNDARY: u8 = 0x8;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    /// Brightness in `rgb` (the boundary is darker) and the share of water
    /// in `a`.
    pub tint: [f32; 4],
}

/// Triangles of one ground texture.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layer {
    pub texture: usize,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

/// World position of point (`x`, `y`).
fn position(terrain: &Terrain, x: u32, y: u32) -> [f32; 3] {
    let p = &terrain.points[(y * terrain.width + x) as usize];
    [
        terrain.origin_x + x as f32 * TILE,
        terrain.origin_y + y as f32 * TILE,
        height(p),
    ]
}

/// The normal at a point from the heights of its neighbours.
fn normal(terrain: &Terrain, x: u32, y: u32) -> [f32; 3] {
    let at = |x: i64, y: i64| {
        let x = x.clamp(0, i64::from(terrain.width) - 1) as u32;
        let y = y.clamp(0, i64::from(terrain.height) - 1) as u32;
        position(terrain, x, y)[2]
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
    let water = if p.flags() & WATER != 0 { 1.0 } else { 0.0 };
    [bright, bright, bright, water]
}

/// The ground mesh, one layer per ground texture that is used, in
/// increasing texture index.
pub fn build(terrain: &Terrain, textures: &[Option<Image>]) -> Vec<Layer> {
    let mut layers: Vec<Layer> = (0..textures.len())
        .map(|texture| Layer {
            texture,
            ..Layer::default()
        })
        .collect();
    let (tiles_x, tiles_y) = (terrain.width - 1, terrain.height - 1);
    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            let points = corners(terrain, tx, ty);
            for (texture, cell) in tile_layers(&points, textures) {
                let Some(atlas) = textures[texture].as_ref() else {
                    continue;
                };
                let size = cell_size(atlas);
                let (cx, cy) = cell_origin(cell, size);
                // A small inset keeps neighbouring cells from bleeding in.
                let inset = 0.5;
                let u0 = (cx as f32 + inset) / atlas.width as f32;
                let u1 = ((cx + size) as f32 - inset) / atlas.width as f32;
                let v0 = (cy as f32 + inset) / atlas.height as f32;
                let v1 = ((cy + size) as f32 - inset) / atlas.height as f32;
                let layer = &mut layers[texture];
                let base = layer.vertices.len() as u32;
                // Bottom left, bottom right, top right, top left; image row
                // 0 is the north edge of the tile.
                let corners = [
                    (tx, ty, [u0, v1], points[1]),
                    (tx + 1, ty, [u1, v1], points[0]),
                    (tx + 1, ty + 1, [u1, v0], points[2]),
                    (tx, ty + 1, [u0, v0], points[3]),
                ];
                for (x, y, uv, point) in corners {
                    layer.vertices.push(Vertex {
                        position: position(terrain, x, y),
                        normal: normal(terrain, x, y),
                        uv,
                        tint: tint(point),
                    });
                }
                layer
                    .indices
                    .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
            }
        }
    }
    layers.retain(|l| !l.indices.is_empty());
    layers
}

/// The middle of the map, on the ground.
pub fn centre(terrain: &Terrain) -> [f32; 3] {
    let (x, y) = ((terrain.width - 1) / 2, (terrain.height - 1) / 2);
    position(terrain, x, y)
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
        let layers = build(&terrain, &textures);
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
        assert!(layers[0].vertices[0].uv[0] > 0.5);
        let uv = layers[1].vertices[3].uv;
        assert!((0.25..0.5).contains(&uv[0]) && uv[1] < 0.25, "{uv:?}");
        assert_eq!(layers[0].vertices[0].normal, [0.0, 0.0, 1.0]);
    }
}
