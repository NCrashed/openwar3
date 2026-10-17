//! Which cliff model stands on a tile (SPEC-0014).
//!
//! The levels of the corners BL, BR, TL, TR, lowered to at most two above
//! the lowest, give one letter each (`A` for the lowest); the name is
//! `<dir>\<dir><TL><TR><BR><BL><n>.mdx` under `Doodads\Terrain`, with the
//! directory from `TerrainArt\CliffTypes.slk` of the cliff type of the
//! tile. Tiles whose corners are all on one level have no model; the
//! ground of a tile with a model is not drawn.
//!
//! PROVISIONAL (SPEC-0014 open question 1): tiles with a ramp flag on a
//! corner get no model, and their ground is drawn as a slope.

use ow3_formats::map::terrain::{Terrain, TerrainPoint};

/// The ramp flag of a point.
const RAMP: u8 = 0x1;
/// A cliff texture that means "none".
const NO_CLIFF: u8 = 15;

/// The cliff model of a tile, before the directory and variation count
/// are known.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CliffTile {
    /// The letters of TL, TR, BR and BL, as in the model name.
    pub letters: String,
    /// The lowest level of the corners.
    pub base_level: u8,
    /// An index into `terrain.cliff_tilesets`.
    pub cliff_type: u8,
    /// The variation value; the model number is this modulo the number of
    /// variations.
    pub variation: u8,
}

fn point(terrain: &Terrain, x: i64, y: i64) -> Option<&TerrainPoint> {
    let (x, y) = (u32::try_from(x).ok()?, u32::try_from(y).ok()?);
    terrain.point(x, y)
}

/// The cliff variation value of a point: the high 3 bits of its texture
/// details byte.
fn variation(p: &TerrainPoint) -> u8 {
    p.texture_details >> 5
}

/// The cliff model of tile (`x`, `y`), whose bottom left corner is point
/// (`x`, `y`), or `None` when it has none.
pub fn cliff_tile(terrain: &Terrain, x: u32, y: u32) -> Option<CliffTile> {
    let (x, y) = (i64::from(x), i64::from(y));
    // BL, BR, TL, TR.
    let corners = [
        point(terrain, x, y)?,
        point(terrain, x + 1, y)?,
        point(terrain, x, y + 1)?,
        point(terrain, x + 1, y + 1)?,
    ];
    let levels = corners.map(TerrainPoint::layer_height);
    let lo = *levels.iter().min()?;
    if levels.iter().all(|&l| l == lo) || corners.iter().any(|p| p.flags() & RAMP != 0) {
        return None;
    }
    let letter = |l: u8| char::from(b'A' + (l - lo).min(2));
    let [bl, br, tl, tr] = levels;
    let letters = [tl, tr, br, bl].into_iter().map(letter).collect();
    let source = corners
        .iter()
        .copied()
        .find(|p| p.cliff_texture() != NO_CLIFF)
        .or_else(|| {
            (x - 2..=x + 2)
                .flat_map(|sx| (y - 2..=y + 2).map(move |sy| (sx, sy)))
                .filter_map(|(sx, sy)| point(terrain, sx, sy))
                .find(|p| p.cliff_texture() != NO_CLIFF)
        });
    let (cliff_type, variation) = source.map_or((0, 0), |p| (p.cliff_texture(), variation(p)));
    Some(CliffTile {
        letters,
        base_level: lo,
        cliff_type,
        variation,
    })
}

/// The path of variation `n` of a cliff model in directory `dir`
/// (`Cliffs` or `CityCliffs`).
pub fn model_path(dir: &str, letters: &str, n: u8) -> String {
    format!("Doodads\\Terrain\\{dir}\\{dir}{letters}{n}.mdx")
}

/// The model number for `variation` when the variations `0..count` exist
/// (at least one).
pub fn model_number(variation: u8, count: u8) -> u8 {
    variation % count.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ow3_formats::binary::RawCode;

    fn terrain(points: Vec<TerrainPoint>, width: u32) -> Terrain {
        let height = points.len() as u32 / width;
        Terrain {
            format_version: 11,
            tileset: b'L',
            custom_tilesets: false,
            ground_tilesets: vec![RawCode(*b"Ldrt")],
            cliff_tilesets: vec![RawCode(*b"CLdi"), RawCode(*b"CLgr")],
            width,
            height,
            origin_x: 0.0,
            origin_y: 0.0,
            points,
        }
    }

    fn at(level: u8, cliff: u8, variation: u8) -> TerrainPoint {
        TerrainPoint {
            ground_height: 0x2000,
            water_and_edge: 0,
            flags_and_texture: 0,
            texture_details: variation << 5,
            cliff_and_layer: (cliff << 4) | level,
        }
    }

    /// A 3 x 3 grid of points at level 2 with the middle one raised: the
    /// four tiles around it, as in the verification of SPEC-0014.
    #[test]
    fn raised_point() {
        let mut points = vec![at(2, 15, 0); 9];
        points[4] = at(3, 1, 3);
        let t = terrain(points, 3);
        let letters = |x, y| cliff_tile(&t, x, y).unwrap().letters;
        // North-east of the point it is the bottom left corner.
        assert_eq!(letters(1, 1), "AAAB");
        assert_eq!(letters(0, 0), "ABAA");
        assert_eq!(letters(1, 0), "BAAA");
        assert_eq!(letters(0, 1), "AABA");
        let tile = cliff_tile(&t, 1, 1).unwrap();
        assert_eq!(
            (tile.base_level, tile.cliff_type, tile.variation),
            (2, 1, 3)
        );
    }

    #[test]
    fn flat_ramps_and_high_corners() {
        let t = terrain(vec![at(2, 0, 0); 4], 2);
        assert_eq!(cliff_tile(&t, 0, 0), None);
        // Levels 5 and 9 over 2: lowered to two above.
        let t = terrain(vec![at(2, 0, 0), at(5, 0, 0), at(9, 0, 0), at(3, 0, 0)], 2);
        assert_eq!(cliff_tile(&t, 0, 0).unwrap().letters, "CBCA");
        let mut ramp = at(3, 0, 0);
        ramp.flags_and_texture = RAMP << 4;
        let t = terrain(vec![at(2, 0, 0), ramp, at(2, 0, 0), at(2, 0, 0)], 2);
        assert_eq!(cliff_tile(&t, 0, 0), None);
    }

    /// Without a cliff texture on the corners, the 5 x 5 points around
    /// the tile are searched, x outer and y inner; with none, type 0.
    #[test]
    fn type_from_the_neighbourhood() {
        let mut points = vec![at(2, 15, 0); 25];
        points[12] = at(3, 15, 0);
        // (0, 4) comes before (1, 0) in the search from (2, 2).
        points[4 * 5] = at(2, 1, 2);
        points[1] = at(2, 0, 5);
        let t = terrain(points, 5);
        let tile = cliff_tile(&t, 2, 2).unwrap();
        assert_eq!((tile.cliff_type, tile.variation), (1, 2));
        let t = terrain(
            vec![at(2, 15, 1), at(3, 15, 1), at(2, 15, 1), at(2, 15, 1)],
            2,
        );
        let tile = cliff_tile(&t, 0, 0).unwrap();
        assert_eq!((tile.cliff_type, tile.variation), (0, 0));
    }

    #[test]
    fn paths() {
        assert_eq!(
            model_path("Cliffs", "AAAB", model_number(5, 2)),
            "Doodads\\Terrain\\Cliffs\\CliffsAAAB1.mdx"
        );
    }
}
