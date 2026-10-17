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
}

/// One material layer of one geoset.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub vertices: Vec<ModelVertex>,
    pub indices: Vec<u32>,
    pub texture: String,
    pub filter_mode: u32,
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

/// The animated models of a scene, posed each frame.
#[derive(Clone, Debug, Default)]
pub struct Animation {
    models: Vec<Animated>,
    /// Slot 0 is always 1.
    alphas: Vec<AlphaSource>,
    bone_count: u32,
}

impl Animation {
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
            let model = &self.models[source.model].model;
            let moment = moments[source.model];
            let geoset = model
                .geoset_animations
                .iter()
                .find(|a| a.geoset_id as usize == source.geoset)
                .map_or(1.0, |a| {
                    track_or(model, &a.tracks, b"KGAO", &[a.alpha], moment)[0]
                });
            let layer = &model.materials[source.material].layers[source.layer];
            let layer = track_or(model, &layer.tracks, b"KMTA", &[layer.alpha], moment)[0];
            out.push((geoset * layer).clamp(0.0, 1.0));
        }
        out
    }
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
            });
        }
    }
    parts
}

/// What makes placements share a group: the model, its replaceable
/// texture and the player whose team colour it shows.
type GroupKey = (String, Option<(u32, String)>, u8);

/// translate(position) * rotate around z (angle) * scale.
fn placement_matrix(p: &Placement) -> Mat4 {
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
    // The index of each model in `animation.models`, or `None`.
    let mut indices: BTreeMap<String, Option<usize>> = BTreeMap::new();
    let mut groups: BTreeMap<GroupKey, Group> = BTreeMap::new();
    for placement in placements {
        let index = *indices.entry(placement.model.clone()).or_insert_with(|| {
            let model = chain
                .read(&placement.model)
                .and_then(|d| Model::parse(&d).ok())?;
            let sequence = ow3_render::model::stand_index(&model);
            let palette: Vec<u32> = Pose::new(&model, Moment::start(&model, sequence))
                .matrices
                .keys()
                .copied()
                .collect();
            let bone_base = animation.bone_count;
            animation.bone_count += (palette.len() * VARIANTS) as u32;
            animation.models.push(Animated {
                model,
                sequence,
                palette,
                bone_base,
            });
            Some(animation.models.len() - 1)
        });
        let Some(index) = index else {
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
}
