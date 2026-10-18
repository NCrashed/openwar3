//! Models of the objects on the map, ready for the GPU: each model cut
//! into one part per material layer in its rest pose, with the bones of
//! every vertex, and the placements of each model gathered as instances.
//!
//! Each frame [`Animation`] computes the bone matrices of every model in
//! its Stand sequence and the alpha of every part (geoset and layer
//! alpha); the GPU moves the vertices. A model is posed in a few
//! variants with different phases, and its instances take them in turn,
//! so that neighbours do not move in step.
//!
//! PROVISIONAL: every model plays its first Stand sequence; a vertex
//! follows at most the first four bones of its matrix group, with equal
//! weights; texture animations, geoset colours, particles and ribbons are
//! not drawn.

use std::collections::BTreeMap;

use bytemuck::{Pod, Zeroable};
use ow3_formats::mdx::Model;
use ow3_formats::mpq::ArchiveChain;
use ow3_render::animation::{Moment, Pose, track_or};
use ow3_render::image::Image;

use crate::camera::Mat4;
use crate::objects::Placement;

/// Phases a model is posed in.
const VARIANTS: usize = 4;
/// How far apart the variants are in time, in milliseconds.
const VARIANT_SHIFT: i32 = 1_373;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct ModelVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    /// Bones within the instance's block of bone matrices, and weights.
    pub bones: [u32; 4],
    pub weights: [f32; 4],
    /// The alpha slot of the part.
    pub alpha: u32,
}

impl ModelVertex {
    /// A vertex that does not move: bone 0 of block 0 is the identity, and
    /// alpha slot 0 is always 1.
    pub fn fixed(position: [f32; 3], normal: [f32; 3], uv: [f32; 2]) -> ModelVertex {
        ModelVertex {
            position,
            normal,
            uv,
            bones: [0; 4],
            weights: [1.0, 0.0, 0.0, 0.0],
            alpha: 0,
        }
    }
}

/// One instance: its placement and the first of its bone matrices.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct Instance {
    pub matrix: Mat4,
    pub bone_base: u32,
    /// A colour the model's texels are multiplied with.
    pub tint: [f32; 3],
    /// Added to the alpha slots of its vertices, for an instance with
    /// alphas of its own (0 for the shared ones).
    pub alpha_shift: u32,
}

/// One material layer of one geoset.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub vertices: Vec<ModelVertex>,
    pub indices: Vec<u32>,
    pub texture: String,
    pub filter_mode: u32,
    /// The texture keeps its edges instead of repeating.
    pub clamp: bool,
}

/// One model, and where it stands.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub model: String,
    pub parts: Vec<Part>,
    pub instances: Vec<Instance>,
}

/// An animated model: its data, its bones and where their matrices go.
#[derive(Clone, Debug)]
struct Animated {
    model: Model,
    sequence: usize,
    /// The first sequence named Death, or the Stand sequence.
    death: usize,
    /// The height of the top of the model in its Stand pose.
    top: f32,
    /// The first sequence named Attack, if any.
    attack: Option<usize>,
    /// The first sequence named Walk, if any.
    walk: Option<usize>,
    /// The object ids of the bones, in the order of their matrices.
    palette: Vec<u32>,
    /// The first matrix of variant 0; variant `v` follows `v` blocks
    /// later.
    bone_base: u32,
}

/// What an alpha slot shows: geoset and material layer of a model.
#[derive(Clone, Copy, Debug)]
struct AlphaSource {
    model: usize,
    geoset: usize,
    material: usize,
    layer: usize,
}

/// What a unit is doing, for its animation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitPose {
    /// Its Stand sequence, in the phase of its variant.
    Stand,
    /// Milliseconds since its death.
    Death(i32),
    /// Milliseconds since the start of its swing.
    Attack(i32),
    /// Milliseconds since it started walking; the Walk sequence loops.
    Walk(i32),
    /// Stopped walking `since` milliseconds ago after walking `walked`.
    Halt { walked: i32, since: i32 },
    /// Milliseconds into its Stand sequence played once, for effects such
    /// as the confirmation of an order.
    Once(i32),
}

/// The animated models of a scene, posed each frame.
#[derive(Clone, Debug, Default)]
pub struct Animation {
    models: Vec<Animated>,
    /// The index of each model path tried, `None` if it cannot be read.
    paths: BTreeMap<String, Option<usize>>,
    /// Slot 0 is always 1.
    alphas: Vec<AlphaSource>,
    bone_count: u32,
}

impl Animation {
    /// The index of the model at `path`, read from `chain` and given its
    /// bone matrices the first time.
    pub fn model(&mut self, chain: &mut ArchiveChain, path: &str) -> Option<usize> {
        if let Some(index) = self.paths.get(path) {
            return *index;
        }
        let index = chain
            .read(path)
            .and_then(|d| Model::parse(&d).ok())
            .map(|model| {
                let sequence = ow3_render::model::stand_index(&model);
                let death = model
                    .sequences
                    .iter()
                    .position(|s| s.name.to_ascii_lowercase().starts_with("death"))
                    .unwrap_or(sequence);
                let walk = model
                    .sequences
                    .iter()
                    .position(|s| s.name.to_ascii_lowercase().starts_with("walk"));
                let attack = model
                    .sequences
                    .iter()
                    .position(|s| s.name.to_ascii_lowercase().starts_with("attack"));
                let top = stand_top(&model, sequence);
                let palette: Vec<u32> = Pose::new(&model, Moment::start(&model, sequence))
                    .matrices
                    .keys()
                    .copied()
                    .collect();
                let bone_base = self.bone_count.max(1);
                self.bone_count = bone_base + (palette.len() * VARIANTS) as u32;
                self.models.push(Animated {
                    model,
                    sequence,
                    death,
                    attack,
                    walk,
                    top,
                    palette,
                    bone_base,
                });
                self.models.len() - 1
            });
        self.paths.insert(path.to_owned(), index);
        index
    }

    /// The height of the top of model `index` in its Stand pose.
    pub fn model_top(&self, index: usize) -> f32 {
        self.models[index].top
    }

    /// The bones of model `index`.
    pub fn palette_len(&self, index: usize) -> u32 {
        self.models[index].palette.len() as u32
    }

    /// The first bone matrix of variant `variant` of model `index`.
    pub fn variant_base(&self, index: usize, variant: u32) -> u32 {
        let animated = &self.models[index];
        animated.bone_base + (variant % VARIANTS as u32) * animated.palette.len() as u32
    }

    /// The bone matrices of model `index` in `pose`: `elapsed`
    /// milliseconds into its Death sequence, holding the last frame, or into
    /// its Attack sequence. `None` when the unit simply stands in the phase
    /// of `variant`: for an attack past the end of the sequence and the
    /// blend after it, or a model without an Attack sequence.
    ///
    /// Changes between Stand and another sequence blend over the model's
    /// blend time (150 ms when it gives none), the bone matrices mixed
    /// linearly; `now` is the time of the Stand sequence.
    pub fn unit_pose(
        &self,
        index: usize,
        pose: UnitPose,
        now: i32,
        variant: u32,
    ) -> Option<Vec<Mat4>> {
        let animated = &self.models[index];
        let (sequence, elapsed) = match pose {
            UnitPose::Stand => return None,
            UnitPose::Death(elapsed) => (animated.death, elapsed),
            UnitPose::Attack(elapsed) => (animated.attack?, elapsed),
            UnitPose::Walk(elapsed) => (animated.walk?, elapsed),
            UnitPose::Halt { walked, .. } => (animated.walk?, walked),
            UnitPose::Once(elapsed) => (animated.sequence, elapsed),
        };
        let start = Moment::start(&animated.model, sequence);
        let length = (start.interval[1] - start.interval[0]).max(0);
        let blend = match animated.model.info.blend_time {
            0 => 150,
            time => time.min(1_000) as i32,
        };
        let stand = || self.stand_pose(index, now, variant);
        let looping = matches!(pose, UnitPose::Walk(_) | UnitPose::Halt { .. });
        let at = |frame: i32| {
            let frame = if looping {
                frame.rem_euclid(length + 1)
            } else {
                frame.clamp(0, length)
            };
            self.palette_pose(
                index,
                Moment {
                    frame: start.frame + frame,
                    ..start
                },
            )
        };
        if let UnitPose::Once(elapsed) = pose {
            return Some(at(elapsed));
        }
        if let UnitPose::Halt { walked, since } = pose {
            // Back to Stand from where the walk stopped.
            if since >= blend {
                return None;
            }
            return Some(mix(&at(walked), &stand(), since as f32 / blend as f32));
        }
        if matches!(pose, UnitPose::Attack(_)) && elapsed > length {
            // Back to Stand from the last frame of the swing.
            let after = elapsed - length;
            if after >= blend {
                return None;
            }
            return Some(mix(&at(length), &stand(), after as f32 / blend as f32));
        }
        let current = at(elapsed);
        if elapsed < blend {
            // Into the sequence from Stand.
            return Some(mix(&stand(), &current, elapsed as f32 / blend as f32));
        }
        Some(current)
    }

    /// The bone matrices of model `index` at `moment`, in palette order.
    fn palette_pose(&self, index: usize, moment: Moment) -> Vec<Mat4> {
        let animated = &self.models[index];
        let pose = Pose::new(&animated.model, moment);
        animated
            .palette
            .iter()
            .map(|id| {
                pose.matrices
                    .get(id)
                    .copied()
                    .unwrap_or(ow3_render::animation::IDENTITY)
            })
            .collect()
    }

    /// The Stand pose of variant `variant` of model `index` at `now`, as
    /// [`Animation::bones`] gives it.
    fn stand_pose(&self, index: usize, now: i32, variant: u32) -> Vec<Mat4> {
        let animated = &self.models[index];
        let variant = (variant % VARIANTS as u32) as i32;
        let shift = now.wrapping_add(variant * VARIANT_SHIFT);
        self.palette_pose(index, Moment::at(&animated.model, animated.sequence, shift))
    }

    /// The parts of model `index` for player `team`, with alpha slots.
    pub fn unit_parts(&mut self, index: usize, team: u8) -> Vec<Part> {
        let model = self.models[index].model.clone();
        let palette = self.models[index].palette.clone();
        let paths = ow3_render::model::texture_paths(&model, &[], team);
        animated_parts(&model, index, &palette, &paths, &mut self.alphas)
    }

    /// Bone matrices: the identity first, then every model's variants.
    pub fn bone_count(&self) -> u32 {
        self.bone_count.max(1)
    }

    /// Alpha slots, the fixed one included.
    pub fn alpha_count(&self) -> u32 {
        self.alphas.len() as u32 + 1
    }

    /// Every bone matrix `elapsed` milliseconds in.
    pub fn bones(&self, elapsed: i32) -> Vec<Mat4> {
        let mut out = Vec::with_capacity(self.bone_count() as usize);
        out.push(ow3_render::animation::IDENTITY);
        for animated in &self.models {
            for variant in 0..VARIANTS {
                let shift = elapsed.wrapping_add(variant as i32 * VARIANT_SHIFT);
                let moment = Moment::at(&animated.model, animated.sequence, shift);
                let pose = Pose::new(&animated.model, moment);
                out.extend(animated.palette.iter().map(|id| {
                    pose.matrices
                        .get(id)
                        .copied()
                        .unwrap_or(ow3_render::animation::IDENTITY)
                }));
            }
        }
        out
    }

    /// Every alpha `elapsed` milliseconds in, by the phase of variant 0.
    pub fn alphas(&self, elapsed: i32) -> Vec<f32> {
        let moments: Vec<Moment> = self
            .models
            .iter()
            .map(|a| Moment::at(&a.model, a.sequence, elapsed))
            .collect();
        let mut out = Vec::with_capacity(self.alphas.len() + 1);
        out.push(1.0);
        for source in &self.alphas {
            out.push(self.alpha(source, moments[source.model]));
        }
        out
    }

    /// The alpha of `source` at `moment` of its model.
    fn alpha(&self, source: &AlphaSource, moment: Moment) -> f32 {
        let model = &self.models[source.model].model;
        let geoset = model
            .geoset_animations
            .iter()
            .find(|a| a.geoset_id as usize == source.geoset)
            .map_or(1.0, |a| {
                track_or(model, &a.tracks, b"KGAO", &[a.alpha], moment)[0]
            });
        let layer = &model.materials[source.material].layers[source.layer];
        let layer = track_or(model, &layer.tracks, b"KMTA", &[layer.alpha], moment)[0];
        (geoset * layer).clamp(0.0, 1.0)
    }

    /// For a model played once (`UnitPose::Once`) `elapsed` milliseconds
    /// in: the alphas of its slots at that moment and the first of those
    /// slots, so that each instance fades on its own.
    pub fn once_alphas(&self, index: usize, elapsed: i32) -> Option<(Vec<f32>, u32)> {
        let animated = &self.models[index];
        let first = self.alphas.iter().position(|s| s.model == index)?;
        let start = Moment::start(&animated.model, animated.sequence);
        let length = (start.interval[1] - start.interval[0]).max(0);
        let moment = Moment {
            frame: start.frame + elapsed.clamp(0, length),
            ..start
        };
        let values = self.alphas[first..]
            .iter()
            .take_while(|s| s.model == index)
            .map(|s| self.alpha(s, moment))
            .collect();
        // Slot 0 is the constant 1, so source `i` has slot `i + 1`.
        Some((values, first as u32 + 1))
    }
}

/// The height of the top of `model` at the start of `sequence`: the highest
/// vertex of the geosets visible then, at least 50. (The extent in the
/// header is often much taller.)
fn stand_top(model: &Model, sequence: usize) -> f32 {
    let moment = Moment::start(model, sequence);
    let pose = Pose::new(model, moment);
    let mut top = f32::MIN;
    for (g, geoset) in model.geosets.iter().enumerate() {
        let alpha = model
            .geoset_animations
            .iter()
            .find(|a| a.geoset_id as usize == g)
            .map_or(1.0, |a| {
                track_or(model, &a.tracks, b"KGAO", &[a.alpha], moment)[0]
            });
        if alpha <= 0.0 {
            continue;
        }
        let (positions, _) = pose.skin(geoset);
        top = positions.iter().fold(top, |top, p| top.max(p[2]));
    }
    if top == f32::MIN {
        top = model.info.extent.max[2];
    }
    top.max(50.0)
}

/// `a` and `b` mixed: `t` 0 gives `a`, 1 gives `b`.
fn mix(a: &[Mat4], b: &[Mat4], t: f32) -> Vec<Mat4> {
    a.iter()
        .zip(b)
        .map(|(a, b)| {
            std::array::from_fn(|c| std::array::from_fn(|r| a[c][r] + (b[c][r] - a[c][r]) * t))
        })
        .collect()
}

/// The bones of each matrix group of a geoset, as indices into `palette`.
fn matrix_groups(geoset: &ow3_formats::mdx::Geoset, palette: &[u32]) -> Vec<([u32; 4], [f32; 4])> {
    let mut groups = Vec::with_capacity(geoset.matrix_group_sizes.len());
    let mut next = 0usize;
    for &size in &geoset.matrix_group_sizes {
        let end = (next + size as usize).min(geoset.matrix_indices.len());
        let bones: Vec<u32> = geoset.matrix_indices[next..end]
            .iter()
            .filter_map(|id| palette.binary_search(id).ok().map(|i| i as u32))
            .take(4)
            .collect();
        next = end;
        let mut indices = [0; 4];
        let mut weights = [0.0; 4];
        for (i, &bone) in bones.iter().enumerate() {
            indices[i] = bone;
            weights[i] = 1.0 / bones.len() as f32;
        }
        groups.push((indices, weights));
    }
    groups
}

/// The parts of a model in its rest pose, with bones from `palette` and
/// alpha slots added to `animation`.
fn animated_parts(
    model: &Model,
    model_index: usize,
    palette: &[u32],
    textures: &[Option<String>],
    alphas: &mut Vec<AlphaSource>,
) -> Vec<Part> {
    let mut parts = Vec::new();
    for (g, geoset) in model.geosets.iter().enumerate() {
        let Some(material) = model.materials.get(geoset.material_id as usize) else {
            continue;
        };
        let groups = matrix_groups(geoset, palette);
        let uvs = geoset.uv_sets.first();
        let vertices: Vec<ModelVertex> = geoset
            .vertices
            .iter()
            .enumerate()
            .map(|(i, &position)| {
                let group = geoset.vertex_groups.get(i).map_or(0, |&g| usize::from(g));
                // A vertex without bones stays where it is: the weights are
                // made up by the identity of the block's first bone only if
                // the model has no bones at all.
                let (bones, weights) = groups
                    .get(group)
                    .copied()
                    .filter(|(_, w)| w[0] > 0.0)
                    .unwrap_or(([0; 4], [0.0; 4]));
                ModelVertex {
                    position,
                    normal: geoset.normals.get(i).copied().unwrap_or([0.0, 0.0, 1.0]),
                    uv: uvs.and_then(|u| u.get(i)).copied().unwrap_or([0.0, 0.0]),
                    bones,
                    weights,
                    alpha: 0,
                }
            })
            .collect();
        let indices: Vec<u32> = geoset.indices.iter().map(|&i| u32::from(i)).collect();
        for (l, layer) in material.layers.iter().enumerate() {
            let Some(texture) = textures.get(layer.texture_id as usize).cloned().flatten() else {
                continue;
            };
            alphas.push(AlphaSource {
                model: model_index,
                geoset: g,
                material: geoset.material_id as usize,
                layer: l,
            });
            let slot = alphas.len() as u32;
            parts.push(Part {
                vertices: vertices
                    .iter()
                    .map(|v| ModelVertex { alpha: slot, ..*v })
                    .collect(),
                indices: indices.clone(),
                texture,
                filter_mode: layer.filter_mode,
                clamp: false,
            });
        }
    }
    parts
}

/// The parts of a model posed once at the start of its Stand sequence,
/// for models baked into the scene such as cliffs.
pub fn parts(model: &Model, textures: &[Option<String>]) -> Vec<Part> {
    let moment = Moment::start(model, ow3_render::model::stand_index(model));
    let pose = Pose::new(model, moment);
    let mut parts = Vec::new();
    for (g, geoset) in model.geosets.iter().enumerate() {
        let geoset_alpha = model
            .geoset_animations
            .iter()
            .find(|a| a.geoset_id as usize == g)
            .map_or(1.0, |a| {
                track_or(model, &a.tracks, b"KGAO", &[a.alpha], moment)[0]
            });
        if geoset_alpha <= 0.0 {
            continue;
        }
        let Some(material) = model.materials.get(geoset.material_id as usize) else {
            continue;
        };
        let (positions, normals) = pose.skin(geoset);
        let uvs = geoset.uv_sets.first();
        let vertices: Vec<ModelVertex> = positions
            .iter()
            .enumerate()
            .map(|(i, &position)| {
                ModelVertex::fixed(
                    position,
                    normals.get(i).copied().unwrap_or([0.0, 0.0, 1.0]),
                    uvs.and_then(|u| u.get(i)).copied().unwrap_or([0.0, 0.0]),
                )
            })
            .collect();
        let indices: Vec<u32> = geoset.indices.iter().map(|&i| u32::from(i)).collect();
        for layer in &material.layers {
            let alpha = track_or(model, &layer.tracks, b"KMTA", &[layer.alpha], moment)[0];
            let texture = textures.get(layer.texture_id as usize).cloned().flatten();
            let (Some(texture), true) = (texture, alpha > 0.0) else {
                continue;
            };
            parts.push(Part {
                vertices: vertices.clone(),
                indices: indices.clone(),
                texture,
                filter_mode: layer.filter_mode,
                clamp: false,
            });
        }
    }
    parts
}

/// What makes placements share a group: the model, its replaceable
/// texture and the player whose team colour it shows.
type GroupKey = (String, Option<(u32, String)>, u8);

/// translate(position) * rotate around z (angle) * scale.
pub(crate) fn placement_matrix(p: &Placement) -> Mat4 {
    let (s, c) = p.angle.sin_cos();
    let [sx, sy, sz] = p.scale;
    [
        [c * sx, s * sx, 0.0, 0.0],
        [-s * sy, c * sy, 0.0, 0.0],
        [0.0, 0.0, sz, 0.0],
        [p.position[0], p.position[1], p.position[2], 1.0],
    ]
}

/// The models of `placements`, each with its instances, the textures they
/// use by path, and their animation. Models that cannot be read are left
/// out.
pub fn load(
    chain: &mut ArchiveChain,
    placements: &[Placement],
) -> (Vec<Group>, BTreeMap<String, Image>, Animation) {
    let mut animation = Animation {
        bone_count: 1,
        ..Animation::default()
    };
    let mut groups: BTreeMap<GroupKey, Group> = BTreeMap::new();
    for placement in placements {
        let Some(index) = animation.model(chain, &placement.model) else {
            continue;
        };
        let animated = &animation.models[index];
        let model = &animated.model;
        // Only models with team colour or glow differ by player.
        let team_coloured = model
            .textures
            .iter()
            .any(|t| matches!(t.replaceable_id, 1 | 2));
        let team = if team_coloured { placement.team } else { 0 };
        let key = (placement.model.clone(), placement.replaceable.clone(), team);
        let group = groups.entry(key).or_insert_with(|| {
            let replacements: Vec<(u32, String)> = placement.replaceable.iter().cloned().collect();
            let paths = ow3_render::model::texture_paths(model, &replacements, team);
            Group {
                model: placement.model.clone(),
                parts: animated_parts(
                    model,
                    index,
                    &animated.palette,
                    &paths,
                    &mut animation.alphas,
                ),
                instances: Vec::new(),
            }
        });
        let animated = &animation.models[index];
        let variant = (group.instances.len() % VARIANTS) as u32;
        group.instances.push(Instance {
            matrix: placement_matrix(placement),
            bone_base: animated.bone_base + variant * animated.palette.len() as u32,
            tint: [1.0; 3],
            alpha_shift: 0,
        });
    }
    let groups: Vec<Group> = groups
        .into_values()
        .filter(|g| !g.parts.is_empty())
        .collect();
    let textures = textures(chain, &groups);
    (groups, textures, animation)
}

/// The textures that the parts of `groups` use, by path.
pub fn textures(chain: &mut ArchiveChain, groups: &[Group]) -> BTreeMap<String, Image> {
    let mut textures = BTreeMap::new();
    for part in groups.iter().flat_map(|g| &g.parts) {
        if !textures.contains_key(&part.texture)
            && let Some(image) = chain
                .read(&part.texture)
                .and_then(|d| ow3_formats::blp::decode(&d).ok())
        {
            textures.insert(part.texture.clone(), Image::from(image));
        }
    }
    textures
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Effects played once fade on their own clocks: the order
    /// confirmation is opaque at its start and gone after 1.1 s.
    /// Skipped without game data.
    #[test]
    fn effects_fade_on_their_own() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b");
        let map = root.join("Maps/(2)BootyBay.w3m");
        let Ok(mut chain) = ArchiveChain::open(&root, &map) else {
            return;
        };
        let mut animation = Animation::default();
        let path = "UI\\Feedback\\Confirmation\\Confirmation.mdx";
        let Some(index) = animation.model(&mut chain, path) else {
            return;
        };
        animation.unit_parts(index, 0);
        let (start, slot) = animation.once_alphas(index, 0).unwrap();
        let (late, same) = animation.once_alphas(index, 1200).unwrap();
        assert_eq!(slot, same);
        assert!(start.iter().all(|&a| a > 0.99), "{start:?}");
        assert!(late.iter().all(|&a| a < 0.2), "{late:?}");
    }

    /// A unit facing north (a quarter turn) at (100, 200, 10), twice as
    /// big: its forward axis (+x in the model) points north.
    #[test]
    fn placement_turns_and_scales() {
        let p = Placement {
            model: String::new(),
            replaceable: None,
            team: 0,
            position: [100.0, 200.0, 10.0],
            angle: std::f32::consts::FRAC_PI_2,
            scale: [2.0; 3],
        };
        let m = placement_matrix(&p);
        let forward = [1.0f32, 0.0, 0.0];
        let world: Vec<f32> = (0..3)
            .map(|r| m[0][r] * forward[0] + m[1][r] * forward[1] + m[2][r] * forward[2] + m[3][r])
            .collect();
        assert!(
            (world[0] - 100.0).abs() < 1e-4 && (world[1] - 202.0).abs() < 1e-4,
            "{world:?}"
        );
        assert!((world[2] - 10.0).abs() < 1e-4);
    }

    /// Matrix groups become up to four palette indices with equal weights.
    #[test]
    fn bones_of_matrix_groups() {
        let geoset = ow3_formats::mdx::Geoset {
            matrix_group_sizes: vec![1, 2],
            matrix_indices: vec![7, 3, 9],
            ..Default::default()
        };
        let groups = matrix_groups(&geoset, &[3, 7, 9]);
        assert_eq!(groups[0], ([1, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]));
        assert_eq!(groups[1], ([0, 2, 0, 0], [0.5, 0.5, 0.0, 0.0]));
    }

    /// Blending goes linearly from one pose to the other.
    #[test]
    fn mixing_poses() {
        let a = [ow3_render::animation::IDENTITY];
        let mut moved = ow3_render::animation::IDENTITY;
        moved[3] = [10.0, 0.0, 0.0, 1.0];
        let b = [moved];
        assert_eq!(mix(&a, &b, 0.0), a);
        assert_eq!(mix(&a, &b, 1.0), b);
        assert_eq!(mix(&a, &b, 0.25)[0][3], [2.5, 0.0, 0.0, 1.0]);
    }
}
