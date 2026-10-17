//! A top-down image of a map's terrain, one square per tile.
//!
//! Each ground texture of a tileset (`TerrainArt\Terrain.slk`, columns
//! `dir` and `file`) is an atlas of 64 x 64 cells. Read from the textures
//! themselves: in the left 4 x 4 cells, cell `n` (row by row) covers the
//! corners whose bits are set in `n`: 1 bottom right, 2 bottom left,
//! 4 top right, 8 top left; cells 0 and 15 are fully covered. Atlases 512
//! pixels wide have 16 more full cells on the right.
//!
//! A tile is drawn from its four corner points: the lowest ground texture
//! index among them fills the tile, then each higher texture present
//! covers its own corners, in increasing order.
//!
//! PROVISIONAL, until a spec says how the game draws terrain: a full tile
//! uses the variation in the low 5 bits of its bottom-left point's texture
//! details (0..15: the right-hand cells, 16: cell 0, 17: cell 15; atlases
//! without right-hand cells use cell 0); heights shade the tiles as if lit
//! from the north-west, water tints towards blue and the map boundary is
//! darkened, all interpolated between the points of a tile; cliffs show
//! only as steep slopes, without cliff models.

use std::collections::BTreeMap;

use ow3_formats::binary::RawCode;
use ow3_formats::map::terrain::{Terrain, TerrainPoint};
use ow3_formats::mpq::ArchiveChain;
use ow3_formats::slk::Table;

use crate::image::Image;

/// Pixels of a cell in a ground texture atlas.
pub const CELL: u32 = 64;
const WATER: u8 = 0x4;
const BOUNDARY: u8 = 0x8;

/// The ground texture atlas of every ground tileset of `terrain`, in the
/// order of `terrain.ground_tilesets`; `None` where it cannot be found.
pub fn ground_textures(chain: &mut ArchiveChain, terrain: &Terrain) -> Vec<Option<Image>> {
    let paths = texture_paths(chain);
    terrain
        .ground_tilesets
        .iter()
        .map(|id| {
            let path = paths.get(id)?;
            let data = chain.read(path)?;
            ow3_formats::blp::decode(&data).ok().map(Image::from)
        })
        .collect()
}

/// `dir\file.blp` of each ground tile type in `TerrainArt\Terrain.slk`.
fn texture_paths(chain: &mut ArchiveChain) -> BTreeMap<RawCode, String> {
    let mut paths = BTreeMap::new();
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
            paths.insert(RawCode(bytes), format!("{dir}\\{file}.blp"));
        }
    }
    paths
}

/// The atlas cell a full tile uses for `variation`.
fn full_cell(atlas: &Image, variation: u8) -> u32 {
    let extended = atlas.width >= 8 * CELL;
    match variation {
        0..=15 if extended => 16 + u32::from(variation),
        17 => 15,
        _ => 0,
    }
}

/// Top-left pixel of `cell` in an atlas: cells 0..15 in the left 4 x 4
/// block, 16..31 in the right one.
fn cell_origin(cell: u32) -> (u32, u32) {
    let block = cell / 16;
    let n = cell % 16;
    ((block * 4 + n % 4) * CELL, (n / 4) * CELL)
}

/// Height of a point in world units, layers included.
fn height(p: &TerrainPoint) -> f32 {
    (f32::from(p.ground_height) - 8192.0) / 4.0 + (f32::from(p.layer_height()) - 2.0) * 128.0
}

/// The image, `tile` pixels per tile (a divisor of 64), north up.
pub fn top_down(terrain: &Terrain, textures: &[Option<Image>], tile: u32) -> Image {
    assert!(tile > 0 && CELL % tile == 0, "tile size must divide {CELL}");
    let (tiles_x, tiles_y) = (terrain.width - 1, terrain.height - 1);
    let mut image = Image::new(tiles_x * tile, tiles_y * tile);
    let point = |x: u32, y: u32| &terrain.points[(y * terrain.width + x) as usize];
    // Cells scaled to the tile size, made on first use.
    let mut scaled: BTreeMap<(usize, u32), Vec<[u8; 4]>> = BTreeMap::new();
    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            let corners = [
                point(tx + 1, ty),     // bit 1: bottom right
                point(tx, ty),         // bit 2: bottom left
                point(tx + 1, ty + 1), // bit 4: top right
                point(tx, ty + 1),     // bit 8: top left
            ];
            let (left, top) = (tx * tile, (tiles_y - 1 - ty) * tile);
            let mut present: Vec<u8> = corners.iter().map(|p| p.ground_texture()).collect();
            present.sort_unstable();
            present.dedup();
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
                let pixels = scaled
                    .entry((usize::from(texture), cell))
                    .or_insert_with(|| {
                        let (x, y) = cell_origin(cell);
                        atlas.scaled_square(x, y, CELL, tile)
                    });
                for (i, &color) in pixels.iter().enumerate() {
                    let (px, py) = (left + i as u32 % tile, top + i as u32 / tile);
                    image.blend(px, py, color);
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

    #[test]
    fn cells_of_the_atlas() {
        assert_eq!(cell_origin(0), (0, 0));
        assert_eq!(cell_origin(5), (64, 64));
        assert_eq!(cell_origin(15), (192, 192));
        assert_eq!(cell_origin(16), (256, 0));
        assert_eq!(cell_origin(31), (448, 192));
        let wide = Image::new(512, 256);
        let narrow = Image::new(256, 256);
        assert_eq!(full_cell(&wide, 0), 16);
        assert_eq!(full_cell(&wide, 16), 0);
        assert_eq!(full_cell(&wide, 17), 15);
        assert_eq!(full_cell(&narrow, 3), 0);
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
            let (x0, y0) = cell_origin(n);
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
