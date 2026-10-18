//! Poses of MDX models: animated values at a time of a sequence, bone
//! matrices and skinned vertices, after the public community
//! documentation of the format.
//!
//! An object (bone, helper, attachment, ...) moves around its pivot point:
//! its local matrix is translate(pivot + t) * rotate(r) * scale(s) *
//! translate(-pivot), and its world matrix is its parent's world matrix
//! times that. A vertex follows the bones of its matrix group, averaged
//! with equal weights.
//!
//! A track counts only keys inside the sequence (a value does not carry
//! over from another sequence); before its first key and after its last one
//! it holds that key's value; with no key in the sequence the value is the
//! default (no movement, full visibility). Tracks on a global sequence run
//! on their own clock. Hermite and Bezier rotations are drawn as spherical
//! linear interpolation.

use std::collections::BTreeMap;

use ow3_formats::mdx::{Geoset, Interpolation, Key, Model, Node, Track};

pub type Vec3 = [f32; 3];
/// Column-major: `m[column][row]`.
pub type Mat4 = [[f32; 4]; 4];

pub const IDENTITY: Mat4 = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

pub fn multiply(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut out = [[0.0; 4]; 4];
    for (c, column) in out.iter_mut().enumerate() {
        for (r, value) in column.iter_mut().enumerate() {
            *value = (0..4).map(|k| a[k][r] * b[c][k]).sum();
        }
    }
    out
}

pub fn transform_point(m: &Mat4, p: Vec3) -> Vec3 {
    let mut out = [0.0; 3];
    for (r, value) in out.iter_mut().enumerate() {
        *value = m[0][r] * p[0] + m[1][r] * p[1] + m[2][r] * p[2] + m[3][r];
    }
    out
}

pub fn transform_direction(m: &Mat4, d: Vec3) -> Vec3 {
    let mut out = [0.0; 3];
    for (r, value) in out.iter_mut().enumerate() {
        *value = m[0][r] * d[0] + m[1][r] * d[1] + m[2][r] * d[2];
    }
    out
}

fn translation(t: Vec3) -> Mat4 {
    let mut m = IDENTITY;
    m[3] = [t[0], t[1], t[2], 1.0];
    m
}

fn scaling(s: Vec3) -> Mat4 {
    let mut m = IDENTITY;
    for i in 0..3 {
        m[i][i] = s[i];
    }
    m
}

/// The rotation of a unit quaternion `[x, y, z, w]`.
fn rotation(q: [f32; 4]) -> Mat4 {
    let [x, y, z, w] = q;
    [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y + z * w),
            2.0 * (x * z - y * w),
            0.0,
        ],
        [
            2.0 * (x * y - z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z + x * w),
            0.0,
        ],
        [
            2.0 * (x * z + y * w),
            2.0 * (y * z - x * w),
            1.0 - 2.0 * (x * x + y * y),
            0.0,
        ],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

fn slerp(a: &[f32], b: &[f32], t: f32) -> Vec<f32> {
    let mut dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let b: Vec<f32> = if dot < 0.0 {
        dot = -dot;
        b.iter().map(|v| -v).collect()
    } else {
        b.to_vec()
    };
    let (wa, wb) = if dot > 0.9995 {
        (1.0 - t, t)
    } else {
        let angle = dot.clamp(-1.0, 1.0).acos();
        let s = angle.sin();
        (((1.0 - t) * angle).sin() / s, (t * angle).sin() / s)
    };
    let q: Vec<f32> = a.iter().zip(&b).map(|(x, y)| x * wa + y * wb).collect();
    let length = q.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-9);
    q.iter().map(|v| v / length).collect()
}

/// When in a model's animations: a sequence (first and last frame) and a
/// frame within it, in milliseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Moment {
    pub interval: [i32; 2],
    pub frame: i32,
    /// Milliseconds the model has been animated, for the tracks of global
    /// sequences, which loop on their own lengths whatever the sequence.
    pub global: i32,
}

impl Moment {
    /// The start of sequence `index` of `model` (the rest pose without
    /// sequences).
    pub fn start(model: &Model, index: usize) -> Moment {
        let interval = model
            .sequences
            .get(index)
            .map_or([0, 0], |s| s.interval.map(|f| f as i32));
        Moment {
            interval,
            frame: interval[0],
            global: 0,
        }
    }

    /// `elapsed` milliseconds into sequence `index`, looping; global
    /// sequences are `elapsed` in too.
    pub fn at(model: &Model, index: usize, elapsed: i32) -> Moment {
        let mut moment = Moment::start(model, index);
        let length = moment.interval[1] - moment.interval[0];
        moment.frame += if length > 0 {
            elapsed.rem_euclid(length + 1)
        } else {
            0
        };
        moment.global = elapsed;
        moment
    }
}

/// The value of a track at a moment, or `None` when the sequence has no
/// key (the default applies).
pub fn track_value(model: &Model, track: &Track, moment: Moment) -> Option<Vec<f32>> {
    let (interval, frame) = match track.global_sequence {
        Some(g) => {
            let length = model.global_sequences.get(g as usize).copied().unwrap_or(0) as i32;
            // Global sequences run on their own clock, not the sequence's.
            (
                [0, length],
                if length > 0 {
                    moment.global.rem_euclid(length)
                } else {
                    0
                },
            )
        }
        None => (moment.interval, moment.frame),
    };
    let keys: Vec<&Key> = track
        .keys
        .iter()
        .filter(|k| (interval[0]..=interval[1]).contains(&k.frame))
        .collect();
    let first = keys.first()?;
    let last = keys.last()?;
    if frame <= first.frame {
        return Some(first.reals());
    }
    if frame >= last.frame {
        return Some(last.reals());
    }
    let after = keys
        .iter()
        .position(|k| k.frame > frame)
        .expect("frame before last");
    let (a, b) = (keys[after - 1], keys[after]);
    let t = (frame - a.frame) as f32 / (b.frame - a.frame) as f32;
    let (va, vb) = (a.reals(), b.reals());
    let quaternion = va.len() == 4;
    Some(match track.interpolation {
        Interpolation::None => va,
        _ if quaternion => slerp(&va, &vb, t),
        Interpolation::Linear => va.iter().zip(&vb).map(|(x, y)| x + (y - x) * t).collect(),
        Interpolation::Hermite | Interpolation::Bezier => {
            let reals = |w: &[u32]| w.iter().map(|&v| f32::from_bits(v)).collect::<Vec<f32>>();
            let out_tan = a
                .tangents
                .as_ref()
                .map(|(_, out)| reals(out))
                .unwrap_or(va.clone());
            let in_tan = b
                .tangents
                .as_ref()
                .map(|(i, _)| reals(i))
                .unwrap_or(vb.clone());
            let (t2, t3) = (t * t, t * t * t);
            let weights = if track.interpolation == Interpolation::Hermite {
                [
                    2.0 * t3 - 3.0 * t2 + 1.0,
                    t3 - 2.0 * t2 + t,
                    t3 - t2,
                    -2.0 * t3 + 3.0 * t2,
                ]
            } else {
                let u = 1.0 - t;
                [u * u * u, 3.0 * u * u * t, 3.0 * u * t2, t3]
            };
            (0..va.len())
                .map(|i| {
                    weights[0] * va[i]
                        + weights[1] * out_tan[i]
                        + weights[2] * in_tan[i]
                        + weights[3] * vb[i]
                })
                .collect()
        }
    })
}

/// A track's value, or `default` without one.
pub fn track_or(
    model: &Model,
    tracks: &[Track],
    tag: &[u8; 4],
    default: &[f32],
    moment: Moment,
) -> Vec<f32> {
    tracks
        .iter()
        .find(|t| &t.tag.0 == tag)
        .and_then(|t| track_value(model, t, moment))
        .unwrap_or_else(|| default.to_vec())
}

/// Every object node of the model that can be a bone or a parent, by
/// object id.
fn nodes(model: &Model) -> BTreeMap<u32, &Node> {
    let mut nodes = BTreeMap::new();
    for bone in &model.bones {
        nodes.insert(bone.node.object_id, &bone.node);
    }
    for helper in &model.helpers {
        nodes.insert(helper.object_id, helper);
    }
    for emitter in &model.particle_emitters {
        nodes.insert(emitter.node.object_id, &emitter.node);
    }
    for attachment in &model.attachments {
        nodes.insert(attachment.node.object_id, &attachment.node);
    }
    nodes
}

/// The world matrices of a model's objects at a moment.
#[derive(Clone, Debug, PartialEq)]
pub struct Pose {
    pub matrices: BTreeMap<u32, Mat4>,
}

impl Pose {
    pub fn new(model: &Model, moment: Moment) -> Pose {
        let nodes = nodes(model);
        let mut matrices = BTreeMap::new();
        for &id in nodes.keys() {
            world(model, &nodes, id, moment, &mut matrices, 0);
        }
        Pose { matrices }
    }

    /// The vertices and normals of a geoset in this pose.
    pub fn skin(&self, geoset: &Geoset) -> (Vec<Vec3>, Vec<Vec3>) {
        // The bones of each matrix group.
        let mut groups = Vec::with_capacity(geoset.matrix_group_sizes.len());
        let mut next = 0usize;
        for &size in &geoset.matrix_group_sizes {
            let end = (next + size as usize).min(geoset.matrix_indices.len());
            groups.push(&geoset.matrix_indices[next..end]);
            next = end;
        }
        let blend = |v: Vec3, group: usize, point: bool| -> Vec3 {
            let bones = groups.get(group).copied().unwrap_or(&[]);
            let matrices: Vec<&Mat4> = bones.iter().filter_map(|b| self.matrices.get(b)).collect();
            if matrices.is_empty() {
                return v;
            }
            let mut sum = [0.0; 3];
            for m in &matrices {
                let p = if point {
                    transform_point(m, v)
                } else {
                    transform_direction(m, v)
                };
                for i in 0..3 {
                    sum[i] += p[i];
                }
            }
            sum.map(|s| s / matrices.len() as f32)
        };
        let group = |i: usize| geoset.vertex_groups.get(i).map_or(0, |&g| usize::from(g));
        let vertices = geoset
            .vertices
            .iter()
            .enumerate()
            .map(|(i, &v)| blend(v, group(i), true))
            .collect();
        let normals = geoset
            .normals
            .iter()
            .enumerate()
            .map(|(i, &n)| {
                let n = blend(n, group(i), false);
                let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-9);
                n.map(|c| c / length)
            })
            .collect();
        (vertices, normals)
    }
}

/// The world matrix of object `id`, computed once with its parents.
fn world(
    model: &Model,
    nodes: &BTreeMap<u32, &Node>,
    id: u32,
    moment: Moment,
    matrices: &mut BTreeMap<u32, Mat4>,
    depth: usize,
) -> Mat4 {
    if let Some(m) = matrices.get(&id) {
        return *m;
    }
    let Some(node) = nodes.get(&id) else {
        return IDENTITY;
    };
    let pivot = model.pivots.get(id as usize).copied().unwrap_or([0.0; 3]);
    let t = track_or(model, &node.tracks, b"KGTR", &[0.0; 3], moment);
    let r = track_or(model, &node.tracks, b"KGRT", &[0.0, 0.0, 0.0, 1.0], moment);
    let s = track_or(model, &node.tracks, b"KGSC", &[1.0; 3], moment);
    let local = multiply(
        &multiply(
            &translation([pivot[0] + t[0], pivot[1] + t[1], pivot[2] + t[2]]),
            &rotation([r[0], r[1], r[2], r[3]]),
        ),
        &multiply(
            &scaling([s[0], s[1], s[2]]),
            &translation(pivot.map(|p| -p)),
        ),
    );
    // A parent cycle in a broken file stops after a few hundred levels.
    let parent = match node.parent_id {
        Some(p) if depth < 256 && p != id => world(model, nodes, p, moment, matrices, depth + 1),
        _ => IDENTITY,
    };
    let m = multiply(&parent, &local);
    matrices.insert(id, m);
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use ow3_formats::binary::RawCode;
    use ow3_formats::mdx::Bone;

    fn key(frame: i32, value: &[f32]) -> Key {
        Key {
            frame,
            value: value.iter().map(|v| v.to_bits()).collect(),
            tangents: None,
        }
    }

    fn track(tag: &[u8; 4], keys: Vec<Key>) -> Track {
        Track {
            tag: RawCode(*tag),
            interpolation: Interpolation::Linear,
            global_sequence: None,
            keys,
        }
    }

    /// A track of a global sequence runs on its own clock: 1200 ms into a
    /// sequence of 1000 ms it is at 1200 of its 1500, not at 199.
    #[test]
    fn global_sequences_keep_their_own_time() {
        let model = Model {
            global_sequences: vec![1500],
            ..Model::default()
        };
        let mut t = track(
            b"KGTR",
            vec![key(0, &[0.0; 3]), key(1500, &[15.0, 0.0, 0.0])],
        );
        t.global_sequence = Some(0);
        let at = Moment {
            interval: [0, 1000],
            frame: 199,
            global: 1200,
        };
        assert_eq!(track_value(&model, &t, at).unwrap(), [12.0, 0.0, 0.0]);
    }

    fn moment(frame: i32) -> Moment {
        Moment {
            interval: [0, 1000],
            frame,
            global: frame,
        }
    }

    #[test]
    fn tracks_use_keys_of_the_sequence() {
        let model = Model::default();
        let t = track(
            b"KGTR",
            vec![
                key(0, &[0.0; 3]),
                key(1000, &[10.0, 0.0, 0.0]),
                key(2000, &[99.0; 3]),
            ],
        );
        assert_eq!(
            track_value(&model, &t, moment(500)).unwrap(),
            [5.0, 0.0, 0.0]
        );
        assert_eq!(
            track_value(&model, &t, moment(1000)).unwrap(),
            [10.0, 0.0, 0.0]
        );
        let other = Moment {
            interval: [3000, 4000],
            frame: 3500,
            global: 500,
        };
        assert_eq!(track_value(&model, &t, other), None);
    }

    /// A child bone turned 90 degrees around z about its pivot at (1, 0, 0),
    /// below a parent moved up by 5.
    #[test]
    fn bones_turn_around_their_pivots() {
        let half = std::f32::consts::FRAC_1_SQRT_2;
        let node = |id, parent, tracks| Node {
            name: String::new(),
            object_id: id,
            parent_id: parent,
            flags: 0,
            tracks,
        };
        let bone = |node| Bone {
            node,
            geoset_id: None,
            geoset_animation_id: None,
        };
        let model = Model {
            bones: vec![
                bone(node(
                    0,
                    None,
                    vec![track(b"KGTR", vec![key(0, &[0.0, 0.0, 5.0])])],
                )),
                bone(node(
                    1,
                    Some(0),
                    vec![track(b"KGRT", vec![key(0, &[0.0, 0.0, half, half])])],
                )),
            ],
            pivots: vec![[0.0; 3], [1.0, 0.0, 0.0]],
            ..Model::default()
        };
        let pose = Pose::new(&model, moment(0));
        let p = transform_point(&pose.matrices[&1], [2.0, 0.0, 0.0]);
        for (got, want) in p.iter().zip([1.0, 1.0, 5.0]) {
            assert!((got - want).abs() < 1e-5, "{p:?}");
        }
    }
}
