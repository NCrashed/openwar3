//! The baked shadows of `war3map.shd` as the ground shows them (SPEC-0017,
//! "Shadows"): the map is cut into blocks of 16 x 16 cells, each block
//! has 16 x 16 samples from border to border, 512 / 15 world units
//! apart, and the shadow at a point is the bilinear interpolation of the
//! samples of its block. Inner samples take their cell, samples on a
//! block border the mean of the two cells there, samples on a corner the
//! mean of the four.
//!
//! The samples are laid out block by block in one grid, sample `(m, n)`
//! of block `(bx, by)` at `(16 bx + m, 16 by + n)`, so a renderer that
//! filters linearly between neighbouring samples of a block gets the
//! spec's interpolation (see [`sample_position`]).

use ow3_formats::map::shadows::Shadows;

/// Cells (and samples) per block side.
pub const BLOCK: u32 = 16;
/// World units per block side.
pub const BLOCK_SIZE: f32 = 512.0;

/// The samples of `shadows`, row by row from the bottom, as in the cells:
/// 0 without shadow, 1 in full shadow. Blocks cut by the edge of the map
/// count the missing cells as without shadow (PROVISIONAL: maps whose
/// size in tiles is not a multiple of 4 were not measured).
pub fn samples(shadows: &Shadows) -> (u32, u32, Vec<f32>) {
    let width = shadows.width.div_ceil(BLOCK) * BLOCK;
    let height = shadows.height.div_ceil(BLOCK) * BLOCK;
    let cell = |i: i64, j: i64| -> f32 {
        if i < 0 || j < 0 || i >= i64::from(shadows.width) || j >= i64::from(shadows.height) {
            return 0.0;
        }
        let at = j as usize * shadows.width as usize + i as usize;
        if shadows.cells[at] { 1.0 } else { 0.0 }
    };
    // The cells a sample on axis position `k` (sample index within the
    // grid) takes: its own, and the one across a block border.
    let across = |k: u32| -> [i64; 2] {
        let k = i64::from(k);
        match k % i64::from(BLOCK) {
            0 => [k - 1, k],
            15 => [k, k + 1],
            _ => [k, k],
        }
    };
    let mut out = Vec::with_capacity((width * height) as usize);
    for n in 0..height {
        let rows = across(n);
        for m in 0..width {
            let columns = across(m);
            let sum: f32 = rows
                .iter()
                .flat_map(|&j| columns.iter().map(move |&i| (i, j)))
                .map(|(i, j)| cell(i, j))
                .sum();
            out.push(sum / 4.0);
        }
    }
    (width, height, out)
}

/// Where a point `offset` world units from the map's lower left corner
/// lies in the sample grid of [`samples`], in samples (sample `k` at
/// `k`): the samples between which it is interpolated are those of its
/// block.
pub fn sample_position(offset: f32) -> f32 {
    let block = (offset / BLOCK_SIZE).floor();
    let within = (offset - block * BLOCK_SIZE) * (BLOCK - 1) as f32 / BLOCK_SIZE;
    block * BLOCK as f32 + within
}

/// The shadow at `(x, y)` world units from the lower left corner, by the
/// rule of the spec (for tests and a reference to the renderers).
pub fn shadow_at(grid: &(u32, u32, Vec<f32>), x: f32, y: f32) -> f32 {
    let (width, height, values) = grid;
    let value = |m: i64, n: i64| -> f32 {
        let m = m.clamp(0, i64::from(*width) - 1) as usize;
        let n = n.clamp(0, i64::from(*height) - 1) as usize;
        values[n * *width as usize + m]
    };
    let (px, py) = (sample_position(x), sample_position(y));
    let (m, n) = (px.floor(), py.floor());
    let (fx, fy) = (px - m, py - n);
    let (m, n) = (m as i64, n as i64);
    let low = value(m, n) * (1.0 - fx) + value(m + 1, n) * fx;
    let high = value(m, n + 1) * (1.0 - fx) + value(m + 1, n + 1) * fx;
    low * (1.0 - fy) + high * fy
}

/// The colour of shadow and how far full shadow moves a texture colour
/// towards it: `t' = t + s * a * (c - t)` before the light.
pub const SHADOW_COLOUR: [f32; 3] = [0.048, 0.048, 0.095];
pub const SHADOW_AMOUNT: f32 = 0.502;

#[cfg(test)]
mod tests {
    use super::*;

    /// A map of 2 x 1 blocks with the shadow cells `on`.
    fn grid(on: &[(u32, u32)]) -> (u32, u32, Vec<f32>) {
        let (width, height) = (32, 16);
        let mut cells = vec![false; (width * height) as usize];
        for &(i, j) in on {
            cells[(j * width + i) as usize] = true;
        }
        samples(&Shadows {
            width,
            height,
            cells,
        })
    }

    /// Verification 4: a strip at an inner column is fully dark at its
    /// sample, `k * 512 / 15` from the block's start, and gone 34 units
    /// to either side.
    #[test]
    fn inner_cells() {
        let g = grid(&(0..16).map(|j| (5, j)).collect::<Vec<_>>());
        let at = 5.0 * 512.0 / 15.0;
        assert!((shadow_at(&g, at, 200.0) - 1.0).abs() < 1e-5);
        assert!((shadow_at(&g, at + 17.0666, 200.0) - 0.5).abs() < 1e-3);
        assert!(shadow_at(&g, at + 34.2, 200.0).abs() < 1e-5);
        assert!(shadow_at(&g, at - 34.2, 200.0).abs() < 1e-5);
    }

    /// A strip at the last column of a block shows at half strength on
    /// the border; one cell on each side of it gives full darkness there.
    #[test]
    fn border_cells() {
        let strip = |i: u32| (0..16).map(move |j| (i, j)).collect::<Vec<_>>();
        let one = grid(&strip(15));
        assert!((shadow_at(&one, 512.0, 200.0) - 0.5).abs() < 1e-5);
        assert!((shadow_at(&one, 511.9, 200.0) - 0.5).abs() < 1e-2);
        let other = grid(&strip(16));
        assert!((shadow_at(&other, 512.0, 200.0) - 0.5).abs() < 1e-5);
        let both = grid(&[strip(15), strip(16)].concat());
        assert!((shadow_at(&both, 512.0, 200.0) - 1.0).abs() < 1e-5);
        assert!(shadow_at(&both, 512.0 + 34.2, 200.0).abs() < 1e-5);
        assert!(shadow_at(&both, 512.0 - 34.2, 200.0).abs() < 1e-5);
    }
}
