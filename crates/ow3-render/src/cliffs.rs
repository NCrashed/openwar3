//! Which cliff model stands on a tile (SPEC-0014).
//!
//! The levels of the corners BL, BR, TL, TR, lowered to at most two above
//! the lowest, give one letter each (`A` for the lowest); the name is
//! `<dir>\<dir><TL><TR><BR><BL><n>.mdx` under `Doodads\Terrain`, with the
//! directory from `TerrainArt\CliffTypes.slk` of the cliff type of the
//! tile. Tiles whose corners are all on one level have no model; the
//! ground of a tile with a model is not drawn.
//!
//! A tile with an `R` or `S` ramp corner is a ramp tile: it gets a ramp
//! model when its pattern starts one (see [`ramp_tile`]) and never a cliff
//! model; other tiles with corners on different levels get cliff models,
//! ramp points or not.

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

/// The cliff type and variation of tile (`x`, `y`) with `corners` BL, BR,
/// TL, TR: from the first corner with a cliff texture, else the first
/// point with one in the 5 x 5 square around (x, y), else type 0.
fn cliff_source(terrain: &Terrain, x: i64, y: i64, corners: &[&TerrainPoint; 4]) -> (u8, u8) {
    corners
        .iter()
        .copied()
        .find(|p| p.cliff_texture() != NO_CLIFF)
        .or_else(|| {
            (x - 2..=x + 2)
                .flat_map(|sx| (y - 2..=y + 2).map(move |sy| (sx, sy)))
                .filter_map(|(sx, sy)| point(terrain, sx, sy))
                .find(|p| p.cliff_texture() != NO_CLIFF)
        })
        .map_or((0, 0), |p| (p.cliff_texture(), variation(p)))
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
    if levels.iter().all(|&l| l == lo) || is_ramp_tile(terrain, x, y) {
        return None;
    }
    let letter = |l: u8| char::from(b'A' + (l - lo).min(2));
    let [bl, br, tl, tr] = levels;
    let letters = [tl, tr, br, bl].into_iter().map(letter).collect();
    let (cliff_type, variation) = cliff_source(terrain, x, y, &corners);
    Some(CliffTile {
        letters,
        base_level: lo,
        cliff_type,
        variation,
    })
}

/// A ramp model: it stands on a tile and the second tile next to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RampTile {
    /// The letters of the model name (TL, TR, BR, BL of the first tile,
    /// `R` written as `L`).
    pub letters: String,
    /// The tile whose bottom left corner is the model's origin: the lower
    /// left of the two.
    pub origin: (u32, u32),
    /// Tiles covered: `(1, 2)` or `(2, 1)` from `origin`.
    pub size: (u32, u32),
    /// The lowest level of the points of both tiles.
    pub base_level: u8,
    /// See [`CliffTile::cliff_type`]. PROVISIONAL (SPEC-0014 open
    /// question 1): chosen as for a cliff model on the first tile.
    pub cliff_type: u8,
}

fn is_ramp(p: &TerrainPoint) -> bool {
    p.flags() & RAMP != 0
}

/// Whether point (`x`, `y`) is "upper": the ramp points among the
/// corners of the tiles around it are on different levels, and it is on
/// the highest of them.
fn upper(terrain: &Terrain, x: i64, y: i64) -> bool {
    let Some(own) = point(terrain, x, y) else {
        return false;
    };
    let tiles_x = i64::from(terrain.width) - 1;
    let tiles_y = i64::from(terrain.height) - 1;
    let mut levels = Vec::new();
    for ty in [y - 1, y] {
        for tx in [x - 1, x] {
            if !(0..tiles_x).contains(&tx) || !(0..tiles_y).contains(&ty) {
                continue;
            }
            for (cx, cy) in [(tx, ty), (tx + 1, ty), (tx, ty + 1), (tx + 1, ty + 1)] {
                if let Some(p) = point(terrain, cx, cy).filter(|p| is_ramp(p)) {
                    levels.push(p.layer_height());
                }
            }
        }
    }
    let (Some(&lo), Some(&hi)) = (levels.iter().min(), levels.iter().max()) else {
        return false;
    };
    lo != hi && own.layer_height() == hi
}

/// The ramp letter of point (`x`, `y`), or `None` when it has none.
fn letter(terrain: &Terrain, x: i64, y: i64) -> Option<char> {
    let p = point(terrain, x, y)?;
    let up = upper(terrain, x, y);
    if !is_ramp(p) {
        return Some(if up { 'B' } else { 'A' });
    }
    if up {
        return Some('H');
    }
    let near: Vec<&TerrainPoint> = (y - 1..=y + 1)
        .flat_map(|ny| (x - 1..=x + 1).map(move |nx| (nx, ny)))
        .filter_map(|(nx, ny)| point(terrain, nx, ny))
        .collect();
    let all = near.len() == 9 && near.iter().all(|p| is_ramp(p));
    let ramp_levels: Vec<u8> = near
        .iter()
        .filter(|p| is_ramp(p))
        .map(|p| p.layer_height())
        .collect();
    let ramp_spread = ramp_levels.iter().max()? - ramp_levels.iter().min()?;
    if (all && ramp_spread == 0) || ramp_spread > 1 {
        return None;
    }
    let differ =
        |a: (i64, i64), b: (i64, i64)| match (point(terrain, a.0, a.1), point(terrain, b.0, b.1)) {
            (Some(a), Some(b)) => is_ramp(a) && is_ramp(b) && a.layer_height() != b.layer_height(),
            _ => false,
        };
    let vertical = differ((x, y - 1), (x, y + 1));
    let horizontal = differ((x - 1, y), (x + 1, y));
    let levels = near.iter().map(|p| p.layer_height());
    let spread = levels.clone().max()? - levels.min()?;
    match (vertical, horizontal) {
        (true, true) => (spread <= 1).then_some('S'),
        (true, false) | (false, true) => (spread <= 1).then_some('R'),
        (false, false) => Some(if all { 'S' } else { 'L' }),
    }
}

/// Whether tile (`x`, `y`) has an `R` or `S` corner.
fn is_ramp_tile(terrain: &Terrain, x: i64, y: i64) -> bool {
    [(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1)]
        .into_iter()
        .any(|(px, py)| matches!(letter(terrain, px, py), Some('R' | 'S')))
}

/// The cliff type of a ramp model on tile (`x`, `y`): of the tile and its
/// east, west and north neighbours that are ramp tiles, the first corner
/// (BL, BR, TL, TR per tile) on the highest level gives it, 15 counting
/// as 0; type 0 without such a tile.
fn ramp_cliff_type(terrain: &Terrain, x: i64, y: i64) -> u8 {
    let mut best: Option<(u8, u8)> = None;
    for (tx, ty) in [(x, y), (x + 1, y), (x - 1, y), (x, y + 1)] {
        if !is_ramp_tile(terrain, tx, ty) {
            continue;
        }
        for (px, py) in [(tx, ty), (tx + 1, ty), (tx, ty + 1), (tx + 1, ty + 1)] {
            let Some(p) = point(terrain, px, py) else {
                continue;
            };
            if best.is_none_or(|(level, _)| p.layer_height() > level) {
                let ctex = p.cliff_texture();
                best = Some((p.layer_height(), if ctex == NO_CLIFF { 0 } else { ctex }));
            }
        }
    }
    best.map_or(0, |(_, ctex)| ctex)
}

/// The patterns that start a ramp model and the offset of its second
/// tile.
const RAMP_PATTERNS: [(&str, (i64, i64)); 16] = [
    ("HRAA", (1, 0)),
    ("HRAB", (1, 0)),
    ("RHAA", (-1, 0)),
    ("RHBA", (-1, 0)),
    ("AARH", (1, 0)),
    ("BARH", (1, 0)),
    ("AAHR", (-1, 0)),
    ("ABHR", (-1, 0)),
    ("AHRA", (0, -1)),
    ("BHRA", (0, -1)),
    ("ARHA", (0, 1)),
    ("ARHB", (0, 1)),
    ("HAAR", (0, -1)),
    ("HBAR", (0, -1)),
    ("RAAH", (0, 1)),
    ("RABH", (0, 1)),
];

/// The ramp model that tile (`x`, `y`) starts, or `None` (SPEC-0014).
pub fn ramp_tile(terrain: &Terrain, x: u32, y: u32) -> Option<RampTile> {
    let (x, y) = (i64::from(x), i64::from(y));
    let corners = [
        point(terrain, x, y)?,
        point(terrain, x + 1, y)?,
        point(terrain, x, y + 1)?,
        point(terrain, x + 1, y + 1)?,
    ];
    if !corners.iter().any(|p| is_ramp(p)) {
        return None;
    }
    let pattern: String = [(x, y + 1), (x + 1, y + 1), (x + 1, y), (x, y)]
        .into_iter()
        .map(|(px, py)| letter(terrain, px, py))
        .collect::<Option<_>>()?;
    let &(_, (dx, dy)) = RAMP_PATTERNS.iter().find(|(p, _)| *p == pattern)?;
    let (sx, sy) = (x + dx, y + dy);
    let tiles_x = i64::from(terrain.width) - 1;
    let tiles_y = i64::from(terrain.height) - 1;
    if !(0..tiles_x).contains(&sx) || !(0..tiles_y).contains(&sy) {
        return None;
    }
    let (ox, oy) = (x.min(sx), y.min(sy));
    let size = (1 + dx.unsigned_abs() as u32, 1 + dy.unsigned_abs() as u32);
    let base_level = (ox..=ox + i64::from(size.0))
        .flat_map(|px| (oy..=oy + i64::from(size.1)).map(move |py| (px, py)))
        .filter_map(|(px, py)| point(terrain, px, py))
        .map(TerrainPoint::layer_height)
        .min()?;
    let cliff_type = ramp_cliff_type(terrain, x, y);
    Some(RampTile {
        letters: pattern.replace('R', "L"),
        origin: (ox as u32, oy as u32),
        size,
        base_level,
        cliff_type,
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
    }

    fn ramp(level: u8) -> TerrainPoint {
        let mut p = at(level, 0, 0);
        p.flags_and_texture = RAMP << 4;
        p
    }

    /// A ramp point that is `H` or `L` does not stop a cliff model; a tile
    /// with an `R` corner gets none (SPEC-0014, "What each tile gets").
    #[test]
    fn ramp_corners() {
        // A lone ramp point one level up: letter L, an ordinary cliff.
        let t = terrain(vec![at(2, 0, 0), ramp(3), at(2, 0, 0), at(2, 0, 0)], 2);
        assert_eq!(cliff_tile(&t, 0, 0).unwrap().letters, "AABA");
        // A column of ramp points from level 2 to 3: the middle one is R.
        let mut points = vec![at(2, 0, 0); 9];
        points[1] = ramp(2);
        points[4] = ramp(2);
        points[7] = ramp(3);
        let t = terrain(points, 3);
        assert_eq!(letter(&t, 1, 1), Some('R'));
        assert_eq!(cliff_tile(&t, 0, 1), None);
        assert!(is_ramp_tile(&t, 0, 1));
    }

    /// The cliff type of a ramp: the first highest corner of the tile and
    /// its east, west and north ramp neighbours; 15 counts as 0.
    #[test]
    fn ramp_cliff_types() {
        let mut points = vec![at(2, 15, 0); 9];
        points[1] = ramp(2);
        points[4] = ramp(2);
        let mut top = ramp(3);
        top.cliff_and_layer = (1 << 4) | 3;
        points[7] = top;
        let t = terrain(points, 3);
        assert_eq!(ramp_cliff_type(&t, 0, 1), 1);
        // No ramp tile around: type 0.
        let t = terrain(vec![at(2, 15, 0); 4], 2);
        assert_eq!(ramp_cliff_type(&t, 0, 0), 0);
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
