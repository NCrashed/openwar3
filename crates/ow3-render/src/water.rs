//! Water of a map (SPEC-0014): which tiles show it, the height of its
//! surface and its colour by depth.
//!
//! A tile shows water when one of its corners has the water flag. The
//! surface of a point is `(w - 8192) / 4 + 128 * height`, with `w` the
//! point's water level and `height` the column of `TerrainArt\Water.slk`
//! for the map's tileset. Each vertex is coloured from a table of 256
//! colours by the depth of the water above the ground there; the depths
//! come from `[Water]` of `UI\MiscData.txt` and the colours from the
//! `Smin`, `Smax`, `Dmin` and `Dmax` columns of `Water.slk`. Both files
//! are read from the game archives only.
//!
//! PROVISIONAL (SPEC-0014 open question 4): the surface shows the first
//! frame of the water texture, `cells` tiles per repeat, without
//! animation or shorelines.

use ow3_formats::map::terrain::TerrainPoint;
use ow3_formats::mpq::ArchiveChain;
use ow3_formats::slk::Table;

/// The water flag of a point.
pub const WATER: u8 = 0x4;

/// The depths of `[Water]` in `UI\MiscData.txt`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Depths {
    pub min: f32,
    pub deep: f32,
    pub max: f32,
}

/// The values shipped with the game, for when the file cannot be read.
impl Default for Depths {
    fn default() -> Depths {
        Depths {
            min: 10.0,
            deep: 64.0,
            max: 72.0,
        }
    }
}

/// The water of a tileset, from its row of `Water.slk`.
#[derive(Clone, Debug, PartialEq)]
pub struct WaterType {
    /// The `height` column.
    pub height: f32,
    /// `Smin`, `Smax`, `Dmin` and `Dmax`, each as red, green, blue and
    /// alpha.
    pub colours: [[u8; 4]; 4],
    /// The texture path without the frame number and extension.
    pub texture: String,
    /// Tiles covered by one repeat of the texture.
    pub cells: f32,
}

/// Whether a tile with these corners shows water.
pub fn has_water(corners: &[&TerrainPoint]) -> bool {
    corners.iter().any(|p| p.flags() & WATER != 0)
}

/// The height of the water surface at a point.
pub fn surface_height(p: &TerrainPoint, height: f32) -> f32 {
    (f32::from(p.water_level()) - 8192.0) / 4.0 + 128.0 * height
}

/// The colour table by depth: entries `0..k` from `Smin` to `Smax`, then
/// `k..=255` from `Dmin` to `Dmax`, where `k` is the place of `deep`
/// between `min` and `max`.
pub fn colour_table(depths: Depths, colours: &[[u8; 4]; 4]) -> Vec<[u8; 4]> {
    let [smin, smax, dmin, dmax] = colours;
    let k = ((depths.deep - depths.min) / (depths.max - depths.min) * 255.0) as usize;
    let k = k.min(255);
    let mut table = vec![[0; 4]; 256];
    let mut run = |i0: usize, i1: usize, c0: &[u8; 4], c1: &[u8; 4]| {
        for (i, entry) in table.iter_mut().enumerate().take(i1 + 1).skip(i0) {
            *entry = std::array::from_fn(|c| {
                if i1 == i0 {
                    return c0[c];
                }
                let (a, b) = (f32::from(c0[c]), f32::from(c1[c]));
                ((b - a) * (i - i0) as f32 / (i1 - i0) as f32 + a) as u8
            });
        }
    };
    run(0, k, smin, smax);
    run(k, 255, dmin, dmax);
    table
}

/// The entry of the colour table for water `depth` deep.
pub fn colour_index(depth: f32, depths: Depths) -> usize {
    if depth <= depths.min {
        0
    } else if depth >= depths.max {
        255
    } else {
        (((depth - depths.min) * 255.0 / (depths.max - depths.min)) as usize).min(255)
    }
}

/// The depths of `UI\MiscData.txt` in the game archives.
pub fn depths(chain: &mut ArchiveChain) -> Depths {
    let mut depths = Depths::default();
    let Some(data) = chain.read_game_file("UI\\MiscData.txt") else {
        return depths;
    };
    let text = String::from_utf8_lossy(&data);
    let mut in_water = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_water = line.eq_ignore_ascii_case("[Water]");
            continue;
        }
        let Some((key, value)) = line.split_once('=').filter(|_| in_water) else {
            continue;
        };
        let Ok(value) = value.trim().parse::<f32>() else {
            continue;
        };
        match key.trim() {
            "MinDepth" => depths.min = value,
            "DeepLevel" => depths.deep = value,
            "MaxDepth" => depths.max = value,
            _ => {}
        }
    }
    depths
}

/// The water of `tileset` (the tileset letter of `war3map.w3e`) from
/// `TerrainArt\Water.slk` in the game archives; its row is `<letter>Sha`.
pub fn water_type(chain: &mut ArchiveChain, tileset: u8) -> Option<WaterType> {
    let data = chain.read_game_file("TerrainArt\\Water.slk")?;
    let table = Table::parse(&data).ok()?;
    let id = table.column("waterID")?;
    let wanted = format!("{}Sha", char::from(tileset));
    let row = (2..=table.last_row()).find(|&r| {
        table
            .get(r, id)
            .is_some_and(|v| v.raw().trim_matches('"') == wanted)
    })?;
    let text = |name: &str| {
        table
            .column(name)
            .and_then(|c| table.get(row, c))
            .map(|v| v.raw().trim_matches('"').to_owned())
            .unwrap_or_default()
    };
    let number = |name: &str| text(name).parse::<f32>().unwrap_or(0.0);
    let colour = |prefix: &str| {
        ["R", "G", "B", "A"].map(|c| number(&format!("{prefix}_{c}")).clamp(0.0, 255.0) as u8)
    };
    Some(WaterType {
        height: number("height"),
        colours: [
            colour("Smin"),
            colour("Smax"),
            colour("Dmin"),
            colour("Dmax"),
        ],
        texture: text("texFile"),
        cells: number("cells").max(1.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(water: u16, flags: u8) -> TerrainPoint {
        TerrainPoint {
            ground_height: 0x2000,
            water_and_edge: water,
            flags_and_texture: flags << 4,
            texture_details: 0,
            cliff_and_layer: 2,
        }
    }

    #[test]
    fn surface_and_flags() {
        // Northrend: height -0.7, -89.6 for w = 8192.
        let p = point(8192, WATER);
        assert!((surface_height(&p, -0.7) - -89.6).abs() < 1e-4);
        assert_eq!(surface_height(&point(8592, 0), 0.0), 100.0);
        let (dry, wet) = (point(8192, 0), point(8192, WATER));
        assert!(!has_water(&[&dry, &dry, &dry, &dry]));
        assert!(has_water(&[&dry, &dry, &wet, &dry]));
    }

    #[test]
    fn colours_by_depth() {
        let depths = Depths::default();
        let colours = [
            [0, 0, 0, 0],
            [222, 111, 0, 255],
            [10, 20, 30, 40],
            [255, 255, 255, 255],
        ];
        let table = colour_table(depths, &colours);
        assert_eq!(table[0], [0, 0, 0, 0]);
        // Entry 222 ends the shallow run but begins the deep one.
        assert_eq!(table[221], [221, 110, 0, 253]);
        assert_eq!(table[222], [10, 20, 30, 40]);
        assert_eq!(table[255], [255, 255, 255, 255]);
        // trunc((255 - 10) * 11 / 33 + 10) = trunc(91.67)
        assert_eq!(table[233][0], 91);
        assert_eq!(colour_index(5.0, depths), 0);
        assert_eq!(colour_index(10.0, depths), 0);
        assert_eq!(colour_index(64.0, depths), 222);
        assert_eq!(colour_index(72.0, depths), 255);
        assert_eq!(colour_index(41.0, depths), 127);
    }
}
