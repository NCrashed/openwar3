//! The shoreline models the game places along the water (SPEC-0014,
//! "Shoreline models"): one at the centre of every tile with deep and
//! other corners, a corner being deep when the water over it is deeper
//! than `WavesDepth`. Which model and how it is turned follows from the
//! deep corners: a straight shore faces its deep side, an outside corner
//! (three deep) faces by its shallow corner, an inside corner (one deep)
//! faces its deep corner, but only a south east or north west one.
//!
//! PROVISIONAL (open question 8): the models stand at the height of the
//! water surface, at their own scale; a tileset with several variations
//! takes them by the tile.

use ow3_formats::map::terrain::Terrain;
use ow3_render::terrain::{corners, height};
use ow3_render::water::{Depths, WaterType, surface_height};

use crate::objects::Placement;
use crate::terrain_mesh::TILE;

/// The shoreline models of `terrain` with water of `kind`.
pub fn placements(terrain: &Terrain, kind: &WaterType, depths: Depths) -> Vec<Placement> {
    let mut out = Vec::new();
    let (tiles_x, tiles_y) = (terrain.width - 1, terrain.height - 1);
    for ty in 0..tiles_y {
        for tx in 0..tiles_x {
            // Bottom right, bottom left, top right, top left.
            let points = corners(terrain, tx, ty);
            let deep = points.map(|p| surface_height(p, kind.height) - height(p) > depths.waves);
            let [se, sw, ne, nw] = deep;
            let Some((which, degrees)) = model(sw, se, nw, ne) else {
                continue;
            };
            let (file, variations) = &kind.shores[which];
            if file.is_empty() {
                continue;
            }
            let n = (tx * 7 + ty * 13) % variations.max(&1);
            let surface = points
                .iter()
                .map(|p| surface_height(p, kind.height))
                .sum::<f32>()
                / 4.0;
            out.push(Placement {
                model: format!("{}\\{file}\\{file}{n}.mdx", kind.shore_dir),
                replaceable: None,
                team: 0,
                position: [
                    terrain.origin_x + (tx as f32 + 0.5) * TILE,
                    terrain.origin_y + (ty as f32 + 0.5) * TILE,
                    surface,
                ],
                angle: (degrees as f32).to_radians(),
                scale: [1.0; 3],
            });
        }
    }
    out
}

/// The model (0 straight, 1 outside corner, 2 inside corner) and facing
/// in degrees of a tile with these deep corners.
fn model(sw: bool, se: bool, nw: bool, ne: bool) -> Option<(usize, u32)> {
    match (sw, se, nw, ne) {
        // Straight: towards the deep side.
        (true, true, false, false) => Some((0, 270)),
        (false, false, true, true) => Some((0, 90)),
        (true, false, true, false) => Some((0, 180)),
        (false, true, false, true) => Some((0, 0)),
        // Three deep: by the shallow corner.
        (true, true, true, false) => Some((1, 270)),
        (true, true, false, true) => Some((1, 0)),
        (false, true, true, true) => Some((1, 90)),
        (true, false, true, true) => Some((1, 180)),
        // One deep: only to the south east and the north west.
        (false, true, false, false) => Some((2, 315)),
        (false, false, true, false) => Some((2, 135)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ow3_formats::binary::RawCode;
    use ow3_formats::map::terrain::TerrainPoint;

    /// A point with ground at `ground` and the surface at 0.
    fn point(ground: f32) -> TerrainPoint {
        TerrainPoint {
            ground_height: (ground * 4.0 + 8192.0) as u16,
            water_and_edge: 8192 | 0x4000,
            flags_and_texture: 0,
            texture_details: 0,
            cliff_and_layer: 2,
        }
    }

    fn kind() -> WaterType {
        WaterType {
            height: 0.0,
            colours: [[0; 4]; 4],
            texture: String::new(),
            shore_dir: "Shore".into(),
            shores: [("S".into(), 1), ("OC".into(), 1), ("IC".into(), 1)],
            frames: 1,
            rate: 0.0,
            cells: 2.0,
        }
    }

    /// A 5 x 5 grid of points with one deep point in the middle: inside
    /// corner models on its north west and south east tiles only.
    #[test]
    fn single_deep_point() {
        let points = (0..25)
            .map(|i| point(if i == 12 { -100.0 } else { 0.0 }))
            .collect();
        let terrain = Terrain {
            format_version: 11,
            tileset: b'N',
            custom_tilesets: false,
            ground_tilesets: vec![RawCode(*b"Nsnw")],
            cliff_tilesets: Vec::new(),
            width: 5,
            height: 5,
            origin_x: 0.0,
            origin_y: 0.0,
            points,
        };
        let placed = placements(&terrain, &kind(), Depths::default());
        let at: Vec<(String, [f32; 2], f32)> = placed
            .iter()
            .map(|p| {
                (
                    p.model.clone(),
                    [p.position[0], p.position[1]],
                    p.angle.to_degrees().round(),
                )
            })
            .collect();
        // The deep point (2, 2) is the south east corner of tile (1, 2)
        // and the north west corner of tile (2, 1).
        assert_eq!(
            at,
            [
                ("Shore\\IC\\IC0.mdx".to_owned(), [320.0, 192.0], 135.0),
                ("Shore\\IC\\IC0.mdx".to_owned(), [192.0, 320.0], 315.0),
            ]
        );
    }

    #[test]
    fn models_and_facings() {
        assert_eq!(model(true, true, false, false), Some((0, 270)));
        assert_eq!(model(false, true, false, true), Some((0, 0)));
        assert_eq!(model(true, true, true, false), Some((1, 270)));
        assert_eq!(model(false, true, true, true), Some((1, 90)));
        assert_eq!(model(false, false, false, true), None);
        assert_eq!(model(true, false, false, true), None);
    }
}
