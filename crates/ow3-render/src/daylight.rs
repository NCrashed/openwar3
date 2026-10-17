//! Day/night lighting (SPEC-0017): the directional light of a day/night
//! model at a time of day.
//!
//! The model has one light object. Its colour and ambient colour (stored
//! blue, green, red) come from the tracks `KLAC` and `KLBC`, or the static
//! values; its intensities from `KLAI` and `KLBI`, or the static values;
//! the direction towards the light is (0, 0, 1) turned by the rotation of
//! its node. Hour `h` is the time `333 + 2500 h` ms of the sequence.
//! Ground lit with normal `N` is the texture times
//! `min(1, I L max(0, N . D) + AI A)` per channel.

use ow3_formats::mdx::{Interpolation, Light, Model, Track};
use ow3_formats::mpq::ArchiveChain;

/// The terrain light when the script names none.
pub const DEFAULT_TERRAIN: &str =
    "Environment\\DNC\\DNCLordaeron\\DNCLordaeronTerrain\\DNCLordaeronTerrain.mdx";

/// The light at one time of day.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lighting {
    /// Towards the light.
    pub direction: [f32; 3],
    /// Red, green, blue.
    pub color: [f32; 3],
    pub intensity: f32,
    pub ambient: [f32; 3],
    pub ambient_intensity: f32,
}

impl Lighting {
    /// What a white texture becomes on ground with normal `normal`.
    pub fn shade(&self, normal: [f32; 3]) -> [f32; 3] {
        let d = self.direction;
        let facing = (normal[0] * d[0] + normal[1] * d[1] + normal[2] * d[2]).max(0.0);
        std::array::from_fn(|c| {
            (self.intensity * self.color[c] * facing + self.ambient_intensity * self.ambient[c])
                .min(1.0)
        })
    }
}

/// A day/night model.
#[derive(Clone, Debug)]
pub struct DayLight {
    model: Model,
}

impl DayLight {
    /// The model at `path`, as a script names it: `.mdl` is read as
    /// `.mdx`.
    pub fn load(chain: &mut ArchiveChain, path: &str) -> Option<DayLight> {
        let path = match path.rsplit_once('.') {
            Some((stem, ext)) if ext.eq_ignore_ascii_case("mdl") => format!("{stem}.mdx"),
            _ => path.to_owned(),
        };
        let model = Model::parse(&chain.read(&path)?).ok()?;
        model.lights.first()?;
        Some(DayLight { model })
    }

    /// The light `hour` hours into the day.
    pub fn at(&self, hour: f32) -> Lighting {
        let light: &Light = &self.model.lights[0];
        let t = 333.0 + 2500.0 * hour.rem_euclid(24.0);
        let value = |tag: &[u8; 4]| {
            light
                .tracks
                .iter()
                .chain(&light.node.tracks)
                .find(|track| &track.tag.0 == tag)
                .and_then(|track| sample(track, t))
        };
        let rgb = |bgr: Vec<f32>| [bgr[2], bgr[1], bgr[0]];
        let color = value(b"KLAC")
            .filter(|v| v.len() == 3)
            .map_or(rgb(light.color.to_vec()), rgb);
        let ambient = value(b"KLBC")
            .filter(|v| v.len() == 3)
            .map_or(rgb(light.ambient_color.to_vec()), rgb);
        let intensity = value(b"KLAI").map_or(light.intensity, |v| v[0]);
        let ambient_intensity = value(b"KLBI").map_or(light.ambient_intensity, |v| v[0]);
        let q = value(b"KGRT")
            .filter(|v| v.len() == 4)
            .unwrap_or(vec![0.0, 0.0, 0.0, 1.0]);
        Lighting {
            direction: rotate_up(&q),
            color,
            intensity,
            ambient,
            ambient_intensity,
        }
    }
}

/// The value of `track` at time `t` (SPEC-0017, "Time of day"): the first
/// key's value before it, the last one's after it, and in between the
/// interpolation of the track. Unlike unit animations, every key counts,
/// whatever sequence it lies in.
fn sample(track: &Track, t: f32) -> Option<Vec<f32>> {
    let reals = |w: &[u32]| w.iter().map(|&v| f32::from_bits(v)).collect::<Vec<f32>>();
    let keys = &track.keys;
    let first = keys.first()?;
    let last = keys.last()?;
    if t <= first.frame as f32 {
        return Some(reals(&first.value));
    }
    if t >= last.frame as f32 {
        return Some(reals(&last.value));
    }
    let after = keys.iter().position(|k| k.frame as f32 > t)?;
    let (a, b) = (&keys[after - 1], &keys[after]);
    let s = (t - a.frame as f32) / (b.frame - a.frame) as f32;
    let (v0, v1) = (reals(&a.value), reals(&b.value));
    Some(match track.interpolation {
        Interpolation::None => v0,
        Interpolation::Linear => v0.iter().zip(&v1).map(|(x, y)| x + (y - x) * s).collect(),
        Interpolation::Hermite | Interpolation::Bezier => {
            let out0 = a
                .tangents
                .as_ref()
                .map_or(v0.clone(), |(_, out)| reals(out));
            let in1 = b.tangents.as_ref().map_or(v1.clone(), |(i, _)| reals(i));
            let (s2, s3) = (s * s, s * s * s);
            let h = [
                2.0 * s3 - 3.0 * s2 + 1.0,
                -2.0 * s3 + 3.0 * s2,
                s3 - 2.0 * s2 + s,
                s3 - s2,
            ];
            (0..v0.len())
                .map(|i| h[0] * v0[i] + h[1] * v1[i] + h[2] * out0[i] + h[3] * in1[i])
                .collect()
        }
    })
}

/// (0, 0, 1) turned by the quaternion `q` (x, y, z, w).
fn rotate_up(q: &[f32]) -> [f32; 3] {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    [
        2.0 * (x * z + w * y),
        2.0 * (y * z - w * x),
        1.0 - 2.0 * (x * x + y * y),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rotation of the Lordaeron model of SPEC-0017.
    #[test]
    fn sun_direction() {
        let d = rotate_up(&[0.381_538_9, -0.215_863_96, -0.442_586_9, 0.782_270_85]);
        let expected = [-0.6755, -0.4059, 0.6157];
        for (a, b) in d.iter().zip(expected) {
            assert!((a - b).abs() < 1e-3, "{d:?}");
        }
    }
}
