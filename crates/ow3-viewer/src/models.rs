//! Models of the objects on the map, ready for the GPU: each model posed
//! once at the start of its Stand sequence, cut into one part per material
//! layer, and the placements of each model gathered as instances.
//!
//! PROVISIONAL: models do not animate yet; parts with a layer or geoset
//! alpha of 0 at the start of Stand are left out; texture animations,
//! particles and ribbons are not drawn.

use std::collections::BTreeMap;

use bytemuck::{Pod, Zeroable};
use ow3_formats::mdx::Model;
use ow3_formats::mpq::ArchiveChain;
use ow3_render::animation::{Moment, Pose, track_or};
use ow3_render::image::Image;

use crate::camera::Mat4;
use crate::objects::Placement;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct ModelVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

/// One material layer of one geoset.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub vertices: Vec<ModelVertex>,
    pub indices: Vec<u32>,
    pub texture: String,
    pub filter_mode: u32,
}

/// One model, posed, and where it stands.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub model: String,
    pub parts: Vec<Part>,
    pub instances: Vec<Mat4>,
}

/// The parts of a model at the start of its Stand sequence.
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
            .map(|(i, &position)| ModelVertex {
                position,
                normal: normals.get(i).copied().unwrap_or([0.0, 0.0, 1.0]),
                uv: uvs.and_then(|u| u.get(i)).copied().unwrap_or([0.0, 0.0]),
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

/// The models of `placements`, each with its instances, and the textures
/// they use by path. Models that cannot be read are left out.
pub fn load(
    chain: &mut ArchiveChain,
    placements: &[Placement],
) -> (Vec<Group>, BTreeMap<String, Image>) {
    let mut models: BTreeMap<String, Option<Model>> = BTreeMap::new();
    let mut groups: BTreeMap<GroupKey, Group> = BTreeMap::new();
    for placement in placements {
        let model = models.entry(placement.model.clone()).or_insert_with(|| {
            chain
                .read(&placement.model)
                .and_then(|d| Model::parse(&d).ok())
        });
        let Some(model) = model else {
            continue;
        };
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
                parts: parts(model, &paths),
                instances: Vec::new(),
            }
        });
        group.instances.push(placement_matrix(placement));
    }
    let groups: Vec<Group> = groups
        .into_values()
        .filter(|g| !g.parts.is_empty())
        .collect();
    let textures = textures(chain, &groups);
    (groups, textures)
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
}
