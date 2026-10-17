//! A top-down image of a map's terrain, one square per tile.
//!
//! The ground follows SPEC-0012. Each ground texture of a tileset
//! (`TerrainArt\Terrain.slk`, columns `dir` and `file`) is an atlas of
//! cells `height / 4` pixels square. In the left 4 x 4 cells, cell `n`
//! (row by row) covers the corners whose bits are set in `n`: 1 bottom
//! right, 2 bottom left, 4 top right, 8 top left; cells 0 and 15 are full.
//! Atlases twice as wide as high have 16 more full cells on the right.
//!
//! A tile is drawn from its four corner points: the lowest ground texture
//! index among them fills the tile with the cell chosen by the variation of
//! the bottom-left point (opaque for index 0, with its alpha otherwise),
//! then each higher index present covers its own corners, in increasing
//! order.
//!
//! PROVISIONAL, until a spec covers them: heights shade the tiles as if
//! lit from the north-west, water tints towards blue and the map boundary
//! is darkened, all interpolated between the points of a tile; cliffs show
//! only as steep slopes, without cliff models.

use std::collections::BTreeMap;

use ow3_formats::binary::RawCode;
use ow3_formats::map::terrain::{Terrain, TerrainPoint};
use ow3_formats::mpq::ArchiveChain;
use ow3_formats::slk::Table;

use crate::image::Image;

/// Pixels of a cell in the shipped ground texture atlases; image tiles
/// must divide it.
pub const CELL: u32 = 64;
const WATER: u8 = 0x4;
const BOUNDARY: u8 = 0x8;

/// The ground texture atlas of every ground tileset of `terrain`, in the
/// order of `terrain.ground_tilesets`; `None` where it cannot be found.
pub fn ground_textures(chain: &mut ArchiveChain, terrain: &Terrain) -> Vec<Option<Image>> {
    let paths: BTreeMap<RawCode, String> = texture_paths(chain).into_iter().collect();
    ground_tiles(chain, terrain)
        .iter()
        .map(|id| {
            let path = paths.get(id)?;
            let data = chain.read(path)?;
            ow3_formats::blp::decode(&data).ok().map(Image::from)
        })
        .collect()
}

/// The ground tiles that the texture indices of `terrain` refer to
/// (SPEC-0012, "Ground tile list"): the map's list when its custom
/// tilesets flag is set, otherwise the rows of `TerrainArt\Terrain.slk`
/// whose id starts with the tileset letter, in the order of the file.
pub fn ground_tiles(chain: &mut ArchiveChain, terrain: &Terrain) -> Vec<RawCode> {
    if terrain.custom_tilesets {
        return terrain.ground_tilesets.clone();
    }
    texture_paths(chain)
        .into_iter()
        .map(|(id, _)| id)
        .filter(|id| id.0[0] == terrain.tileset)
        .collect()
}

/// `dir\file.blp` of each ground tile type in `TerrainArt\Terrain.slk`.
/// The image of every ground tile of `TerrainArt\Terrain.slk`, in the order
/// of the file.
fn texture_paths(chain: &mut ArchiveChain) -> Vec<(RawCode, String)> {
    let mut paths = Vec::new();
    let Some(table) = chain
        .read("TerrainArt\\Terrain.slk")
        .and_then(|data| Table::parse(&data).ok())
    else {
        return paths;
    };
    let (Some(id), Some(dir), Some(file)) = (
        table.column("tileID"),
        table.column("dir"),
        table.column("file"),
    ) else {
        return paths;
    };
    for row in 2..=table.last_row() {
        let cell = |column| table.get(row, column).map(|v| v.raw().to_owned());
        if let (Some(tile), Some(dir), Some(file)) = (cell(id), cell(dir), cell(file))
            && let Ok(bytes) = <[u8; 4]>::try_from(tile.as_bytes())
        {
            paths.push((RawCode(bytes), format!("{dir}\\{file}.blp")));
        }
    }
    paths
}

/// Pixels of a cell of `atlas`: a quarter of its height (SPEC-0012).
pub fn cell_size(atlas: &Image) -> u32 {
    (atlas.height / 4).max(1)
}

/// The atlas cell a full tile uses for `variation` (SPEC-0012).
pub fn full_cell(atlas: &Image, variation: u8) -> u32 {
    let v = u32::from(variation & 0x1F);
    let wide = atlas.width >= 2 * atlas.height;
    match v {
        _ if !wide => {
            if v % 2 == 0 {
                0
            } else {
                15
            }
        }
        0..=15 => 16 + v,
        16 => 15,
        17 => 0,
        _ => v - 2,
    }
}

/// Top-left pixel of `cell` in an atlas with cells of `size` pixels:
/// cells 0..15 in the left 4 x 4 block, 16..31 in the right one.
pub fn cell_origin(cell: u32, size: u32) -> (u32, u32) {
    let block = cell / 16;
    let n = cell % 16;
    ((block * 4 + n % 4) * size, (n / 4) * size)
}

/// Height of a point in world units, layers included (SPEC-0014): layer
/// 15 counts as 14.
pub fn height(p: &TerrainPoint) -> f32 {
    ground_height(p) + (f32::from(p.layer_height().min(14)) - 2.0) * 128.0
}

/// Height of a point without its cliff layer.
pub fn ground_height(p: &TerrainPoint) -> f32 {
    (f32::from(p.ground_height) - 8192.0) / 4.0
}

/// The four corner points of tile (`x`, `y`), in the order of their bits:
/// bottom right (1), bottom left (2), top right (4), top left (8).
pub fn corners(terrain: &Terrain, x: u32, y: u32) -> [&TerrainPoint; 4] {
    let point = |x: u32, y: u32| &terrain.points[(y * terrain.width + x) as usize];
    [
        point(x + 1, y),
        point(x, y),
        point(x + 1, y + 1),
        point(x, y + 1),
    ]
}

/// The layers of a tile in drawing order (SPEC-0012): the ground texture
/// index and the atlas cell of each. Textures without an atlas are left
/// out.
pub fn tile_layers(corners: &[&TerrainPoint; 4], textures: &[Option<Image>]) -> Vec<(usize, u32)> {
    let mut present: Vec<u8> = corners.iter().map(|p| p.ground_texture()).collect();
    present.sort_unstable();
    present.dedup();
    let mut layers = Vec::with_capacity(present.len());
    for (layer, &texture) in present.iter().enumerate() {
        let Some(Some(atlas)) = textures.get(usize::from(texture)) else {
            continue;
        };
        let cell = if layer == 0 {
            full_cell(atlas, corners[1].texture_details & 0x1F)
        } else {
            corners
                .iter()
                .enumerate()
                .filter(|(_, p)| p.ground_texture() == texture)
                .map(|(bit, _)| 1 << bit)
                .sum()
        };
        layers.push((usize::from(texture), cell));
    }
    layers
}

/// The image, `tile` pixels per tile (a divisor of 64), north up.
pub fn top_down(terrain: &Terrain, textures: &[Option<Image>], tile: u32) -> Image {
    assert!(tile > 0 && CELL % tile == 0, "tile size must divide {CELL}");
    let (tiles_x, tiles_y) = (terrain.width - 1, terrain.height - 1);
    let mut image = Image::new(tiles_x * tile, tiles_y * tile);
    // Cells scaled to the tile size, made on first use.
    let mut scaled: BTreeMap<(usize, u32), Vec<[u8; 4]>> = BTreeMap::new();
    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            let (left, top) = (tx * tile, (tiles_y - 1 - ty) * tile);
            let layers = tile_layers(&corners(terrain, tx, ty), textures);
            for (layer, &(texture, cell)) in layers.iter().enumerate() {
                let Some(atlas) = textures[texture].as_ref() else {
                    continue;
                };
                let pixels = scaled.entry((texture, cell)).or_insert_with(|| {
                    let size = cell_size(atlas);
                    let (x, y) = cell_origin(cell, size);
                    atlas.scaled_square(x, y, size, tile)
                });
                for (i, &color) in pixels.iter().enumerate() {
                    let (px, py) = (left + i as u32 % tile, top + i as u32 / tile);
                    // The full tile of texture 0 is opaque; other layers,
                    // and the full tile of a higher index, blend (SPEC-0012).
                    if layer == 0 && texture == 0 {
                        image.set(px, py, [color[0], color[1], color[2], 255]);
                    } else {
                        image.blend(px, py, color);
                    }
                }
            }
        }
    }
    shade(&mut image, terrain, tile);
    image
}

/// Light, water and boundary at each point, from 0 to 1: light from the
/// slope of the neighbouring heights, lit from the north-west.
fn point_shading(terrain: &Terrain) -> Vec<[f32; 3]> {
    let (w, h) = (terrain.width as i64, terrain.height as i64);
    let at = |x: i64, y: i64| {
        let (x, y) = (x.clamp(0, w - 1), y.clamp(0, h - 1));
        height(&terrain.points[(y * w + x) as usize])
    };
    let (lx, ly, lz) = (-0.577_f32, 0.577, 0.577);
    let mut values = Vec::with_capacity(terrain.points.len());
    for y in 0..h {
        for x in 0..w {
            let dx = (at(x + 1, y) - at(x - 1, y)) / 256.0;
            let dy = (at(x, y + 1) - at(x, y - 1)) / 256.0;
            let (nx, ny, nz) = (-dx, -dy, 1.0f32);
            let length = (nx * nx + ny * ny + nz * nz).sqrt();
            let light = ((nx * lx + ny * ly + nz * lz) / length).max(0.0) / lz;
            let p = &terrain.points[(y * w + x) as usize];
            let flag = |bit: u8| if p.flags() & bit != 0 { 1.0 } else { 0.0 };
            values.push([light, flag(WATER), flag(BOUNDARY)]);
        }
    }
    values
}

/// Height shading, water and the boundary over the drawn tiles, each
/// interpolated between the corners of the tile.
fn shade(image: &mut Image, terrain: &Terrain, tile: u32) {
    let values = point_shading(terrain);
    let (tiles_x, tiles_y) = (terrain.width - 1, terrain.height - 1);
    let value = |x: u32, y: u32| values[(y * terrain.width + x) as usize];
    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            let (bl, br) = (value(tx, ty), value(tx + 1, ty));
            let (tl, tr) = (value(tx, ty + 1), value(tx + 1, ty + 1));
            let (left, top) = (tx * tile, (tiles_y - 1 - ty) * tile);
            for oy in 0..tile {
                let v = 1.0 - (oy as f32 + 0.5) / tile as f32;
                for ox in 0..tile {
                    let u = (ox as f32 + 0.5) / tile as f32;
                    let mix = |i: usize| {
                        let bottom = bl[i] * (1.0 - u) + br[i] * u;
                        let upper = tl[i] * (1.0 - u) + tr[i] * u;
                        bottom * (1.0 - v) + upper * v
                    };
                    let (light, water, boundary) = (mix(0), mix(1), mix(2));
                    let factor = (0.55 + 0.45 * light) * (1.0 - 0.55 * boundary);
                    let (px, py) = (left + ox, top + oy);
                    let [r, g, b, _] = image.pixel(px, py);
                    // Water: towards a dark blue, by how wet the place is.
                    let wet = |c: u8, w: f32| {
                        let lit = (f32::from(c) * factor).min(255.0);
                        (lit * (1.0 - 0.7 * water) + w * 0.7 * water) as u8
                    };
                    image.set(px, py, [wet(r, 20.0), wet(g, 60.0), wet(b, 110.0), 255]);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0012: cell positions and the variation table.
    #[test]
    fn cells_of_the_atlas() {
        assert_eq!(cell_origin(0, 64), (0, 0));
        assert_eq!(cell_origin(5, 64), (64, 64));
        assert_eq!(cell_origin(15, 64), (192, 192));
        assert_eq!(cell_origin(16, 64), (256, 0));
        assert_eq!(cell_origin(31, 32), (224, 96));
        let wide = Image::new(512, 256);
        let narrow = Image::new(256, 256);
        assert_eq!(cell_size(&wide), 64);
        // The table of SPEC-0012.
        let cells: Vec<u32> = (0..32).map(|v| full_cell(&wide, v)).collect();
        let mut expected: Vec<u32> = (16..32).collect();
        expected.extend([15, 0]);
        expected.extend(16..30);
        assert_eq!(cells, expected);
        for v in 0..32u8 {
            assert_eq!(full_cell(&narrow, v), if v % 2 == 0 { 0 } else { 15 });
        }
    }
}

#[cfg(test)]
mod drawing_tests {
    use super::*;
    use ow3_formats::binary::RawCode;

    /// An atlas of one colour whose cells cover the corners of their
    /// index bits, as the game's atlases do.
    fn atlas(color: [u8; 3]) -> Image {
        let mut image = Image::new(4 * CELL, 4 * CELL);
        for n in 0..16u32 {
            let (x0, y0) = cell_origin(n, CELL);
            for y in 0..CELL {
                for x in 0..CELL {
                    let right = x >= CELL / 2;
                    let bottom = y >= CELL / 2;
                    let bit = match (bottom, right) {
                        (true, true) => 1,
                        (true, false) => 2,
                        (false, true) => 4,
                        (false, false) => 8,
                    };
                    let covered = n == 0 || n == 15 || n & bit != 0;
                    let alpha = if covered { 255 } else { 0 };
                    image.set(x0 + x, y0 + y, [color[0], color[1], color[2], alpha]);
                }
            }
        }
        image
    }

    fn point(texture: u8) -> TerrainPoint {
        TerrainPoint {
            ground_height: 0x2000,
            water_and_edge: 0,
            flags_and_texture: texture,
            texture_details: 0,
            cliff_and_layer: 2,
        }
    }

    /// One tile: texture 1 at the bottom-right corner only, texture 0 at
    /// the others. Texture 0 fills the tile, texture 1 covers the
    /// bottom-right quarter.
    #[test]
    fn higher_textures_cover_their_corners() {
        let terrain = Terrain {
            format_version: 11,
            tileset: b'L',
            custom_tilesets: false,
            ground_tilesets: vec![RawCode(*b"Ldrt"), RawCode(*b"Lgrs")],
            cliff_tilesets: Vec::new(),
            width: 2,
            height: 2,
            origin_x: 0.0,
            origin_y: 0.0,
            // Bottom row first: bottom left, bottom right, top left, top
            // right.
            points: vec![point(0), point(1), point(0), point(0)],
        };
        let textures = [Some(atlas([200, 0, 0])), Some(atlas([0, 0, 200]))];
        let image = top_down(&terrain, &textures, CELL);
        assert_eq!((image.width, image.height), (CELL, CELL));
        let quarter = |x: u32, y: u32| image.pixel(x * CELL / 2 + 8, y * CELL / 2 + 8);
        assert_eq!(quarter(1, 1), [0, 0, 200, 255], "bottom right");
        for (x, y) in [(0, 0), (1, 0), (0, 1)] {
            assert_eq!(quarter(x, y), [200, 0, 0, 255], "quarter {x}, {y}");
        }
    }
}
