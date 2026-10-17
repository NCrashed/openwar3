//! A still image of an MDX model in its rest pose, drawn in software:
//! orthographic projection, a depth buffer, the first layer's texture of
//! each geoset's material and simple directional lighting. For checking
//! models; it does not animate and ignores most material settings.

use ow3_formats::mdx::{Interpolation, Key, Model, Track};
use ow3_formats::mpq::ArchiveChain;

use crate::image::Image;

/// Replaceable texture 1: team colour; 2: team glow (player 0, red).
const TEAM_COLOR: &str = "ReplaceableTextures\\TeamColor\\TeamColor00.blp";
const TEAM_GLOW: &str = "ReplaceableTextures\\TeamGlow\\TeamGlow00.blp";

/// The texture of each entry of `model.textures`, or `None`. Replaceable
/// textures other than team colour and glow (trees, cliffs and so on)
/// come from the object that uses the model; `replacements` gives their
/// paths by replaceable id.
pub fn textures(
    chain: &mut ArchiveChain,
    model: &Model,
    replacements: &[(u32, String)],
) -> Vec<Option<Image>> {
    model
        .textures
        .iter()
        .map(|t| {
            let path = match t.replaceable_id {
                0 => t.path.clone(),
                1 => TEAM_COLOR.to_owned(),
                2 => TEAM_GLOW.to_owned(),
                id => replacements.iter().find(|(r, _)| *r == id)?.1.clone(),
            };
            let data = chain.read(&path)?;
            ow3_formats::blp::decode(&data).ok().map(Image::from)
        })
        .collect()
}

/// The value of a track at `frame` within the sequence `interval`: only
/// keys inside the interval count (a value does not carry over from
/// another sequence), the key at or before the frame, or a linear mix
/// with the next key (Hermite and Bezier are drawn as linear). `None`
/// when the sequence has no key.
fn track_value(track: &Track, frame: i32, interval: [i32; 2]) -> Option<Vec<f32>> {
    let keys: Vec<&Key> = track
        .keys
        .iter()
        .filter(|k| (interval[0]..=interval[1]).contains(&k.frame))
        .collect();
    let after = keys
        .iter()
        .position(|k| k.frame > frame)
        .unwrap_or(keys.len());
    let before = after.checked_sub(1).map(|i| keys[i]);
    match (before, keys.get(after)) {
        (Some(a), Some(b)) if track.interpolation != Interpolation::None => {
            let t = (frame - a.frame) as f32 / (b.frame - a.frame).max(1) as f32;
            Some(
                a.reals()
                    .iter()
                    .zip(b.reals())
                    .map(|(x, y)| x + (y - x) * t)
                    .collect(),
            )
        }
        (Some(a), _) => Some(a.reals()),
        (None, Some(b)) => Some(b.reals()),
        (None, None) => None,
    }
}

/// An animated alpha at the start of `sequence`, or `fixed` without keys.
fn alpha_at(model: &Model, tracks: &[Track], tag: &[u8; 4], fixed: f32, sequence: [i32; 2]) -> f32 {
    tracks
        .iter()
        .find(|t| &t.tag.0 == tag)
        .and_then(|t| match t.global_sequence {
            // Tracks on a global sequence are taken at its start.
            Some(g) => {
                let length = model.global_sequences.get(g as usize).copied().unwrap_or(0);
                track_value(t, 0, [0, length as i32])
            }
            None => track_value(t, sequence[0], sequence),
        })
        .map_or(fixed, |v| v[0])
}

/// The first sequence named "Stand", or the first sequence: its start is
/// the pose and visibility that are drawn.
fn stand_sequence(model: &Model) -> [i32; 2] {
    model
        .sequences
        .iter()
        .find(|s| s.name.to_ascii_lowercase().starts_with("stand"))
        .or(model.sequences.first())
        .map_or([0, 0], |s| s.interval.map(|f| f as i32))
}

/// Where the model is seen from: angles in degrees around the vertical
/// axis (0: from the front, the model's +x) and above the ground.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub yaw: f32,
    pub pitch: f32,
    pub size: u32,
}

type Vec3 = [f32; 3];

fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(a: Vec3) -> Vec3 {
    let l = dot(a, a).sqrt().max(1e-9);
    [a[0] / l, a[1] / l, a[2] / l]
}

fn sample(texture: &Image, u: f32, v: f32) -> [u8; 4] {
    let wrap = |t: f32, n: u32| ((t - t.floor()) * n as f32).min(n as f32 - 1.0) as u32;
    texture.pixel(wrap(u, texture.width), wrap(v, texture.height))
}

/// The model drawn on a transparent square of `view.size` pixels.
pub fn render(model: &Model, textures: &[Option<Image>], view: View) -> Image {
    let (yaw, pitch) = (view.yaw.to_radians(), view.pitch.to_radians());
    let toward = [
        pitch.cos() * yaw.cos(),
        pitch.cos() * yaw.sin(),
        pitch.sin(),
    ];
    let right = normalize(cross([0.0, 0.0, 1.0], toward));
    let up = cross(toward, right);
    let light = normalize([0.4, 0.3, 0.85]);

    // The alpha of each geoset at the start of Stand; fit the visible ones
    // into the image.
    let sequence = stand_sequence(model);
    let geoset_alphas: Vec<f32> = (0..model.geosets.len())
        .map(|g| {
            model
                .geoset_animations
                .iter()
                .find(|a| a.geoset_id as usize == g)
                .map_or(1.0, |a| {
                    alpha_at(model, &a.tracks, b"KGAO", a.alpha, sequence)
                })
        })
        .collect();
    let points = model
        .geosets
        .iter()
        .zip(&geoset_alphas)
        .filter(|&(_, &a)| a > 0.0)
        .flat_map(|(g, _)| g.vertices.iter());
    let (mut min, mut max) = ([f32::MAX; 2], [f32::MIN; 2]);
    for &p in points {
        let s = [dot(p, right), dot(p, up)];
        for i in 0..2 {
            min[i] = min[i].min(s[i]);
            max[i] = max[i].max(s[i]);
        }
    }
    let size = view.size as f32;
    let span = (max[0] - min[0]).max(max[1] - min[1]).max(1e-3);
    let scale = size * 0.9 / span;
    let centre = [(min[0] + max[0]) / 2.0, (min[1] + max[1]) / 2.0];
    let project = |p: Vec3| {
        [
            size / 2.0 + (dot(p, right) - centre[0]) * scale,
            size / 2.0 - (dot(p, up) - centre[1]) * scale,
            dot(p, toward),
        ]
    };

    let mut canvas = Canvas {
        image: Image::new(view.size, view.size),
        depth: vec![f32::MIN; (view.size * view.size) as usize],
    };
    // Layers that write depth first, then blended ones over them.
    for opaque_pass in [true, false] {
        for (g, geoset) in model.geosets.iter().enumerate() {
            let geoset_alpha = geoset_alphas[g];
            if geoset_alpha <= 0.0 {
                continue;
            }
            let Some(material) = model.materials.get(geoset.material_id as usize) else {
                continue;
            };
            let screen: Vec<[f32; 3]> = geoset.vertices.iter().map(|&p| project(p)).collect();
            for layer in &material.layers {
                if (layer.filter_mode <= FILTER_TRANSPARENT) != opaque_pass {
                    continue;
                }
                let alpha =
                    alpha_at(model, &layer.tracks, b"KMTA", layer.alpha, sequence) * geoset_alpha;
                if alpha <= 0.0 {
                    continue;
                }
                let texture = textures
                    .get(layer.texture_id as usize)
                    .and_then(Option::as_ref);
                let surface = Surface {
                    geoset,
                    screen: &screen,
                    texture,
                    filter_mode: layer.filter_mode,
                    alpha,
                    light,
                };
                canvas.draw(&surface, size);
            }
        }
    }
    canvas.image
}

/// Filter modes of a layer.
const FILTER_NONE: u32 = 0;
const FILTER_TRANSPARENT: u32 = 1;
const FILTER_BLEND: u32 = 2;
const FILTER_ADDITIVE: u32 = 3;
const FILTER_ADD_ALPHA: u32 = 4;
const FILTER_MODULATE: u32 = 5;

/// One layer of a geoset, ready to draw.
struct Surface<'a> {
    geoset: &'a ow3_formats::mdx::Geoset,
    /// Screen x, y and depth of each vertex.
    screen: &'a [[f32; 3]],
    texture: Option<&'a Image>,
    filter_mode: u32,
    alpha: f32,
    light: Vec3,
}

struct Canvas {
    image: Image,
    depth: Vec<f32>,
}

impl Canvas {
    fn draw(&mut self, s: &Surface, size: f32) {
        let width = self.image.width;
        let uvs = s.geoset.uv_sets.first();
        for triangle in s.geoset.indices.chunks_exact(3) {
            let index = [triangle[0], triangle[1], triangle[2]].map(usize::from);
            let [a, b, c] = index.map(|i| s.screen[i]);
            let area = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
            if area.abs() < 1e-9 {
                continue;
            }
            let lo_x = a[0].min(b[0]).min(c[0]).floor().max(0.0) as u32;
            let hi_x = a[0].max(b[0]).max(c[0]).ceil().min(size - 1.0) as u32;
            let lo_y = a[1].min(b[1]).min(c[1]).floor().max(0.0) as u32;
            let hi_y = a[1].max(b[1]).max(c[1]).ceil().min(size - 1.0) as u32;
            for y in lo_y..=hi_y {
                for x in lo_x..=hi_x {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((b[0] - px) * (c[1] - py) - (c[0] - px) * (b[1] - py)) / area;
                    let w1 = ((c[0] - px) * (a[1] - py) - (a[0] - px) * (c[1] - py)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                        continue;
                    }
                    let z = w0 * a[2] + w1 * b[2] + w2 * c[2];
                    let slot = (y * width + x) as usize;
                    // Layers of one geoset lie on the same surface.
                    if z + 1e-3 < self.depth[slot] {
                        continue;
                    }
                    let mut color = match (s.texture, uvs) {
                        (Some(texture), Some(uvs)) => {
                            let [ua, ub, uc] = index.map(|i| uvs[i]);
                            let u = w0 * ua[0] + w1 * ub[0] + w2 * uc[0];
                            let v = w0 * ua[1] + w1 * ub[1] + w2 * uc[1];
                            sample(texture, u, v)
                        }
                        _ => [160, 160, 160, 255],
                    };
                    if !s.geoset.normals.is_empty() {
                        let [na, nb, nc] = index.map(|i| s.geoset.normals[i]);
                        let n = normalize([
                            w0 * na[0] + w1 * nb[0] + w2 * nc[0],
                            w0 * na[1] + w1 * nb[1] + w2 * nc[1],
                            w0 * na[2] + w1 * nb[2] + w2 * nc[2],
                        ]);
                        let lit = 0.45 + 0.55 * dot(n, s.light).max(0.0);
                        for c in &mut color[..3] {
                            *c = (f32::from(*c) * lit).min(255.0) as u8;
                        }
                    }
                    self.put(x, y, slot, z, color, s);
                }
            }
        }
    }

    fn put(&mut self, x: u32, y: u32, slot: usize, z: f32, color: [u8; 4], s: &Surface) {
        let under = self.image.pixel(x, y);
        let a = f32::from(color[3]) / 255.0 * s.alpha;
        let channel = |i: usize, f: &dyn Fn(f32, f32) -> f32| {
            f(f32::from(color[i]), f32::from(under[i])).clamp(0.0, 255.0) as u8
        };
        let covered = |extra: f32| (f32::from(under[3]) + extra * 255.0).min(255.0) as u8;
        match s.filter_mode {
            FILTER_NONE => {
                self.image.set(x, y, [color[0], color[1], color[2], 255]);
                self.depth[slot] = z;
            }
            FILTER_TRANSPARENT => {
                if a >= 0.75 {
                    self.image.set(x, y, [color[0], color[1], color[2], 255]);
                    self.depth[slot] = z;
                }
            }
            FILTER_BLEND => {
                let mix = |s: f32, d: f32| s * a + d * (1.0 - a);
                let out = [
                    channel(0, &mix),
                    channel(1, &mix),
                    channel(2, &mix),
                    covered(a),
                ];
                self.image.set(x, y, out);
            }
            FILTER_ADDITIVE | FILTER_ADD_ALPHA => {
                let weight = if s.filter_mode == FILTER_ADDITIVE {
                    s.alpha
                } else {
                    a
                };
                let add = |s: f32, d: f32| d + s * weight;
                let out = [
                    channel(0, &add),
                    channel(1, &add),
                    channel(2, &add),
                    covered(weight),
                ];
                self.image.set(x, y, out);
            }
            _ => {
                let factor = if s.filter_mode == FILTER_MODULATE {
                    1.0
                } else {
                    2.0
                };
                let modulate = |s: f32, d: f32| d * s / 255.0 * factor;
                let out = [
                    channel(0, &modulate),
                    channel(1, &modulate),
                    channel(2, &modulate),
                    under[3],
                ];
                self.image.set(x, y, out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ow3_formats::mdx::{Geoset, Layer, Material};

    /// A triangle without a texture, seen from the front, covers the
    /// middle of the image and leaves the corners empty.
    #[test]
    fn draws_a_triangle() {
        let geoset = Geoset {
            vertices: vec![[0.0, -1.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 2.0]],
            indices: vec![0, 1, 2],
            face_groups: vec![3],
            ..Geoset::default()
        };
        let model = Model {
            materials: vec![Material {
                priority_plane: 0,
                flags: 0,
                layers: vec![Layer {
                    filter_mode: 0,
                    shading_flags: 0,
                    texture_id: 0,
                    texture_animation_id: None,
                    coord_id: 0,
                    alpha: 1.0,
                    tracks: Vec::new(),
                }],
            }],
            geosets: vec![geoset],
            ..Model::default()
        };
        let view = View {
            yaw: 0.0,
            pitch: 0.0,
            size: 64,
        };
        let image = render(&model, &[], view);
        assert_eq!(image.pixel(32, 40)[3], 255);
        assert_eq!(image.pixel(1, 1)[3], 0);
        assert_eq!(image.pixel(62, 1)[3], 0);
    }
}
