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
    /// 1: lit by the light of the terrain (cliffs), 0: by that of units
    /// (SPEC-0017).
    pub terrain_light: u32,
}

/// One material layer of one geoset.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub vertices: Vec<ModelVertex>,
    pub indices: Vec<u32>,
    pub texture: String,
    pub filter_mode: u32,
    /// Drawn without light (shading flag 1 of the layer), like the team
    /// colour under a skin.
    pub unshaded: bool,
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
    /// The sequence Stand Work, if any.
    work: Option<usize>,
    /// Stand Hit, played by a tree when it is hit, if any.
    hit: Option<usize>,
    /// Walk Gold and Walk Lumber, if any (SPEC-0029).
    walk_gold: Option<usize>,
    walk_lumber: Option<usize>,
    /// Stand Gold and Stand Lumber, if any (SPEC-0029).
    stand_gold: Option<usize>,
    stand_lumber: Option<usize>,
    /// Attack Gold and Attack Lumber, if any (SPEC-0029).
    attack_gold: Option<usize>,
    attack_lumber: Option<usize>,
    /// The first sequence with Portrait and without Talk in its name.
    portrait: Option<usize>,
    /// The object ids of the bones, in the order of their matrices.
    palette: Vec<u32>,
    /// The first matrix of variant 0; variant `v` follows `v` blocks
    /// later.
    bone_base: u32,
}

impl Animated {
    /// The walk with a load: Walk Gold (1) or Walk Lumber (2), else Walk.
    fn carry(&self, load: u8) -> Option<usize> {
        match load {
            1 => self.walk_gold,
            2 => self.walk_lumber,
            _ => None,
        }
        .or(self.walk)
    }

    /// A swing with a load: Attack Gold (1) or Attack Lumber (2), else
    /// Attack.
    fn load_attack(&self, load: u8) -> Option<usize> {
        match load {
            1 => self.attack_gold,
            2 => self.attack_lumber,
            _ => None,
        }
        .or(self.attack)
    }

    /// Standing with a load: Stand Gold (1) or Stand Lumber (2).
    fn load_stand(&self, load: u8) -> Option<usize> {
        match load {
            1 => self.stand_gold,
            2 => self.stand_lumber,
            _ => None,
        }
    }
}

/// What an alpha slot shows: geoset and material layer of a model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    /// Milliseconds since it started working on a building; the Stand Work
    /// sequence loops.
    Work(i32),
    /// Milliseconds since it started walking with a load: 1 gold, 2
    /// lumber; Walk Gold or Walk Lumber loops.
    Carry(i32, u8),
    /// Milliseconds into standing with a load: Stand Gold or Stand Lumber
    /// loops.
    LoadStand(i32, u8),
    /// Milliseconds into a swing with a load: Attack Gold or Attack Lumber,
    /// played once.
    LoadAttack(i32, u8),
    /// Milliseconds since it was hit: Stand Hit played once.
    Hit(i32),
    /// Milliseconds into its Portrait sequence, which loops (SPEC-0028).
    Portrait(i32),
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
                    .position(|s| s.name.trim().eq_ignore_ascii_case("walk"))
                    .or_else(|| {
                        model
                            .sequences
                            .iter()
                            .position(|s| s.name.to_ascii_lowercase().starts_with("walk"))
                    });
                // Attack itself before Attack Gold and the like.
                let attack = model
                    .sequences
                    .iter()
                    .position(|s| s.name.trim().eq_ignore_ascii_case("attack"))
                    .or_else(|| {
                        model
                            .sequences
                            .iter()
                            .position(|s| s.name.to_ascii_lowercase().starts_with("attack"))
                    });
                let work = work_index(&model);
                let portrait = portrait_index(&model);
                let named = |name: &str| {
                    model
                        .sequences
                        .iter()
                        .position(|s| s.name.trim().eq_ignore_ascii_case(name))
                };
                let (walk_gold, walk_lumber) = (named("walk gold"), named("walk lumber"));
                let (stand_gold, stand_lumber) = (named("stand gold"), named("stand lumber"));
                let (attack_gold, attack_lumber) = (named("attack gold"), named("attack lumber"));
                let hit = named("stand hit");
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
                    work,
                    portrait,
                    hit,
                    walk_gold,
                    walk_lumber,
                    stand_gold,
                    stand_lumber,
                    attack_gold,
                    attack_lumber,
                    top,
                    palette,
                    bone_base,
                });
                self.models.len() - 1
            });
        self.paths.insert(path.to_owned(), index);
        index
    }

    /// The data of model `index`.
    pub fn mdx(&self, index: usize) -> &Model {
        &self.models[index].model
    }

    /// Where model `index` is `elapsed` milliseconds into its Stand
    /// sequence, its global sequences too.
    pub fn stand_moment(&self, index: usize, elapsed: i32) -> Moment {
        let animated = &self.models[index];
        Moment::at(&animated.model, animated.sequence, elapsed)
    }

    /// Where in its animations a unit of model `index` in `pose` is, as
    /// [`Animation::unit_pose`] poses it but without the blends: the
    /// sequence of the pose and the time in it, or Stand in the phase of
    /// `variant` at `now`. For the particles of its emitters.
    pub fn pose_moment(&self, index: usize, pose: UnitPose, now: i32, variant: u32) -> Moment {
        let animated = &self.models[index];
        let model = &animated.model;
        let stand = || {
            let shift = now.wrapping_add((variant % VARIANTS as u32) as i32 * VARIANT_SHIFT);
            Moment::at(model, animated.sequence, shift)
        };
        let held = |sequence: usize, elapsed: i32| {
            let mut moment = Moment::start(model, sequence);
            let length = (moment.interval[1] - moment.interval[0]).max(0);
            moment.frame += elapsed.clamp(0, length);
            moment.global = now;
            moment
        };
        match pose {
            UnitPose::Stand => stand(),
            UnitPose::Death(elapsed) => held(animated.death, elapsed),
            UnitPose::Hit(elapsed) => match animated.hit {
                Some(hit) => held(hit, elapsed),
                None => stand(),
            },
            UnitPose::Once(elapsed) => held(animated.sequence, elapsed),
            UnitPose::LoadAttack(elapsed, load) => match animated.load_attack(load) {
                Some(attack) => held(attack, elapsed),
                None => stand(),
            },
            UnitPose::Attack(elapsed) => match animated.attack {
                Some(attack) => held(attack, elapsed),
                None => stand(),
            },
            UnitPose::Walk(elapsed)
            | UnitPose::Halt {
                walked: elapsed, ..
            } => match animated.walk {
                Some(walk) => {
                    let mut moment = Moment::at(model, walk, elapsed);
                    moment.global = now;
                    moment
                }
                None => stand(),
            },
            UnitPose::Work(elapsed)
            | UnitPose::Portrait(elapsed)
            | UnitPose::Carry(elapsed, _)
            | UnitPose::LoadStand(elapsed, _) => {
                let sequence = match pose {
                    UnitPose::Work(_) => animated.work,
                    UnitPose::Carry(_, load) => animated.carry(load),
                    UnitPose::LoadStand(_, load) => animated.load_stand(load),
                    _ => animated.portrait,
                };
                match sequence {
                    Some(sequence) => {
                        let mut moment = Moment::at(model, sequence, elapsed);
                        moment.global = now;
                        moment
                    }
                    None => stand(),
                }
            }
        }
    }

    /// The index of the model at `path` if it was read.
    pub fn index_of(&self, path: &str) -> Option<usize> {
        self.paths.get(path).copied().flatten()
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
            UnitPose::Hit(elapsed) => (animated.hit?, elapsed),
            UnitPose::Attack(elapsed) => (animated.attack?, elapsed),
            UnitPose::LoadAttack(elapsed, load) => (animated.load_attack(load)?, elapsed),
            UnitPose::Walk(elapsed) => (animated.walk?, elapsed),
            UnitPose::Work(elapsed) => (animated.work?, elapsed),
            UnitPose::Portrait(elapsed) => (animated.portrait?, elapsed),
            UnitPose::Carry(elapsed, load) => (animated.carry(load)?, elapsed),
            UnitPose::LoadStand(elapsed, load) => (animated.load_stand(load)?, elapsed),
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
        let looping = matches!(
            pose,
            UnitPose::Walk(_)
                | UnitPose::Work(_)
                | UnitPose::Portrait(_)
                | UnitPose::Carry(..)
                | UnitPose::LoadStand(..)
                | UnitPose::Halt { .. }
        );
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
                    global: now,
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
        if matches!(pose, UnitPose::Attack(_) | UnitPose::LoadAttack(..)) && elapsed > length {
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
    pub fn unit_parts(
        &mut self,
        index: usize,
        team: u8,
        replaceable: Option<&(u32, String)>,
    ) -> Vec<Part> {
        let model = self.models[index].model.clone();
        let palette = self.models[index].palette.clone();
        let replacements: Vec<(u32, String)> = replaceable.into_iter().cloned().collect();
        let paths = ow3_render::model::texture_paths(&model, &replacements, team);
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

    /// The billboard data of every bone, in the order of [`Self::bones`]:
    /// the node's pivot and its mode (0 none, 1 facing the camera, 2
    /// turning about z only).
    pub fn bone_info(&self) -> Vec<[f32; 4]> {
        let mut out = Vec::with_capacity(self.bone_count() as usize);
        out.push([0.0; 4]);
        for index in 0..self.models.len() {
            let info = self.palette_info(index);
            for _ in 0..VARIANTS {
                out.extend_from_slice(&info);
            }
        }
        out
    }

    /// The billboard data of model `index`'s palette (`bone_info`).
    pub fn palette_info(&self, index: usize) -> Vec<[f32; 4]> {
        let animated = &self.models[index];
        let model = &animated.model;
        animated
            .palette
            .iter()
            .map(|&id| {
                let flags = model
                    .bones
                    .iter()
                    .map(|b| &b.node)
                    .chain(&model.helpers)
                    .find(|n| n.object_id == id)
                    .map_or(0, |n| n.flags);
                // Billboarded 0x8, locked about x, y or z 0x10, 0x20, 0x40.
                // PROVISIONAL: the locks about x and y face the camera fully.
                let mode = if flags & 0x40 != 0 {
                    2.0
                } else if flags & 0x38 != 0 {
                    1.0
                } else {
                    0.0
                };
                let p = model.pivots.get(id as usize).copied().unwrap_or([0.0; 3]);
                [p[0], p[1], p[2], mode]
            })
            .collect()
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
    pub fn alphas(&self, elapsed: i32) -> Vec<[f32; 4]> {
        let moments: Vec<Moment> = self
            .models
            .iter()
            .map(|a| Moment::at(&a.model, a.sequence, elapsed))
            .collect();
        let mut out = Vec::with_capacity(self.alphas.len() + 1);
        out.push([1.0; 4]);
        for source in &self.alphas {
            out.push(self.alpha(source, moments[source.model]));
        }
        out
    }

    /// The colour and alpha of `source` at `moment` of its model: the
    /// geoset animation's colour (`KGAC`, stored blue, green, red) and the
    /// product of its alpha (`KGAO`) and the layer's (`KMTA`).
    fn alpha(&self, source: &AlphaSource, moment: Moment) -> [f32; 4] {
        let model = &self.models[source.model].model;
        let animation = model
            .geoset_animations
            .iter()
            .find(|a| a.geoset_id as usize == source.geoset);
        let geoset = animation.map_or(1.0, |a| {
            track_or(model, &a.tracks, b"KGAO", &[a.alpha], moment)[0]
        });
        let colour = animation.map_or([1.0; 3], |a| {
            let c = track_or(model, &a.tracks, b"KGAC", &a.color, moment);
            [c[2], c[1], c[0]].map(|v| v.clamp(0.0, 1.0))
        });
        let layer = &model.materials[source.material].layers[source.layer];
        let layer = track_or(model, &layer.tracks, b"KMTA", &[layer.alpha], moment)[0];
        [
            colour[0],
            colour[1],
            colour[2],
            (geoset * layer).clamp(0.0, 1.0),
        ]
    }

    /// For a model played once (`UnitPose::Once`) `elapsed` milliseconds
    /// in: the alphas of its slots at that moment and the first of those
    /// slots, so that each instance fades on its own.
    pub fn once_alphas(&self, index: usize, elapsed: i32) -> Option<(Vec<[f32; 4]>, u32)> {
        self.sequence_alphas(index, self.models[index].sequence, elapsed, false)
    }

    /// The alphas of a unit in `pose` when they are its own: a dying unit
    /// fades by its Death sequence (and stays as its last frame shows it),
    /// an effect by its sequence; other poses share those of Stand.
    pub fn pose_alphas(&self, index: usize, pose: UnitPose) -> Option<(Vec<[f32; 4]>, u32)> {
        let animated = &self.models[index];
        // The geosets shown follow the sequence played: the sack of gold
        // and the bundle of lumber of a worker show in Walk Gold and Walk
        // Lumber only.
        match pose {
            UnitPose::Once(elapsed) => self.once_alphas(index, elapsed),
            UnitPose::Death(elapsed) => self.sequence_alphas(index, animated.death, elapsed, false),
            UnitPose::Hit(elapsed) => self.sequence_alphas(index, animated.hit?, elapsed, false),
            UnitPose::LoadAttack(elapsed, load) => {
                self.sequence_alphas(index, animated.load_attack(load)?, elapsed, false)
            }
            UnitPose::Attack(elapsed) => {
                self.sequence_alphas(index, animated.attack?, elapsed, false)
            }
            UnitPose::Walk(elapsed)
            | UnitPose::Halt {
                walked: elapsed, ..
            } => self.sequence_alphas(index, animated.walk?, elapsed, true),
            UnitPose::Carry(elapsed, load) => {
                self.sequence_alphas(index, animated.carry(load)?, elapsed, true)
            }
            UnitPose::LoadStand(elapsed, load) => {
                self.sequence_alphas(index, animated.load_stand(load)?, elapsed, true)
            }
            UnitPose::Work(elapsed) => self.sequence_alphas(index, animated.work?, elapsed, true),
            UnitPose::Portrait(elapsed) => {
                self.sequence_alphas(index, animated.portrait?, elapsed, true)
            }
            UnitPose::Stand => None,
        }
    }

    /// The alphas of model `index` `elapsed` milliseconds into `sequence`,
    /// held at its end or `looping`, and the first of its slots.
    fn sequence_alphas(
        &self,
        index: usize,
        sequence: usize,
        elapsed: i32,
        looping: bool,
    ) -> Option<(Vec<[f32; 4]>, u32)> {
        let animated = &self.models[index];
        let first = self.alphas.iter().position(|s| s.model == index)?;
        let start = Moment::start(&animated.model, sequence);
        let length = (start.interval[1] - start.interval[0]).max(0);
        let into = if looping {
            elapsed.rem_euclid(length + 1)
        } else {
            elapsed.clamp(0, length)
        };
        let moment = Moment {
            frame: start.frame + into,
            global: elapsed,
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

/// The sequence a portrait plays: the first whose name has Portrait and
/// not Talk (SPEC-0028). PROVISIONAL: how the game picks one of several
/// is not known.
fn portrait_index(model: &Model) -> Option<usize> {
    model.sequences.iter().position(|s| {
        let name = s.name.to_ascii_lowercase();
        name.contains("portrait") && !name.contains("talk")
    })
}

/// The sequence a worker plays while it builds: Stand Work, or the first
/// whose name begins with it.
fn work_index(model: &Model) -> Option<usize> {
    let names: Vec<String> = model
        .sequences
        .iter()
        .map(|s| s.name.trim().to_ascii_lowercase())
        .collect();
    names
        .iter()
        .position(|n| n == "stand work")
        .or_else(|| names.iter().position(|n| n.starts_with("stand work")))
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
            // One slot per source: parts of the same model made again (a
            // tree drawn on its own as well as in the scene) share the
            // slots, so that the alphas of a pose reach them.
            let source = AlphaSource {
                model: model_index,
                geoset: g,
                material: geoset.material_id as usize,
                layer: l,
            };
            let slot = match alphas.iter().position(|s| *s == source) {
                Some(i) => i as u32 + 1,
                None => {
                    alphas.push(source);
                    alphas.len() as u32
                }
            };
            parts.push(Part {
                vertices: vertices
                    .iter()
                    .map(|v| ModelVertex { alpha: slot, ..*v })
                    .collect(),
                indices: indices.clone(),
                texture,
                filter_mode: layer.filter_mode,
                unshaded: layer.shading_flags & 1 != 0,
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
                unshaded: layer.shading_flags & 1 != 0,
                clamp: false,
            });
        }
    }
    parts
}

/// What makes placements share a group: the model, its replaceable
/// texture and the player whose team colour it shows.
type GroupKey = (String, Option<(u32, String)>, u8);

/// What [`load`] gives.
pub type Loaded = (
    Vec<Group>,
    BTreeMap<String, Image>,
    Animation,
    Vec<Option<(usize, usize)>>,
);

/// translate(position) * rotate around z (angle) * scale.
pub fn placement_matrix(p: &Placement) -> Mat4 {
    let (s, c) = p.angle.sin_cos();
    let [sx, sy, sz] = p.scale;
    [
        [c * sx, s * sx, 0.0, 0.0],
        [-s * sy, c * sy, 0.0, 0.0],
        [0.0, 0.0, sz, 0.0],
        [p.position[0], p.position[1], p.position[2], 1.0],
    ]
}

/// The models of `placements`: their groups, textures and animation, and
/// for each placement its group and instance (`None` for a model that
/// cannot be read or has nothing to draw).
pub fn load(chain: &mut ArchiveChain, placements: &[Placement]) -> Loaded {
    let mut animation = Animation {
        bone_count: 1,
        ..Animation::default()
    };
    let mut groups: BTreeMap<GroupKey, Group> = BTreeMap::new();
    // The group and instance of each placement.
    let mut placed: Vec<Option<(GroupKey, usize)>> = Vec::with_capacity(placements.len());
    for placement in placements {
        let Some(index) = animation.model(chain, &placement.model) else {
            placed.push(None);
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
        let group = groups.entry(key.clone()).or_insert_with(|| {
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
        placed.push(Some((key, group.instances.len())));
        group.instances.push(Instance {
            matrix: placement_matrix(placement),
            bone_base: animated.bone_base + variant * animated.palette.len() as u32,
            tint: [1.0; 3],
            alpha_shift: 0,
            terrain_light: 0,
        });
    }
    let mut numbers: BTreeMap<GroupKey, usize> = BTreeMap::new();
    let groups: Vec<Group> = groups
        .into_iter()
        .filter(|(_, g)| !g.parts.is_empty())
        .enumerate()
        .map(|(i, (key, g))| {
            numbers.insert(key, i);
            g
        })
        .collect();
    let slots = placed
        .into_iter()
        .map(|p| p.and_then(|(key, i)| numbers.get(&key).map(|&g| (g, i))))
        .collect();
    let textures = textures(chain, &groups);
    (groups, textures, animation, slots)
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
        animation.unit_parts(index, 0, None);
        let (start, slot) = animation.once_alphas(index, 0).unwrap();
        let (late, same) = animation.once_alphas(index, 1200).unwrap();
        assert_eq!(slot, same);
        assert!(start.iter().all(|a| a[3] > 0.99), "{start:?}");
        assert!(late.iter().all(|a| a[3] < 0.2), "{late:?}");
    }

    /// A dying unit fades by its Death sequence and stays as its end shows
    /// it: the banshee is gone long after her death, and visible at its
    /// start. Skipped without game data.
    #[test]
    fn the_dead_keep_the_alphas_of_their_death() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b");
        let map = root.join("Maps/(2)BootyBay.w3m");
        let Ok(mut chain) = ArchiveChain::open(&root, &map) else {
            return;
        };
        let mut animation = Animation::default();
        let path = "Units\\Undead\\Banshee\\Banshee.mdx";
        let Some(index) = animation.model(&mut chain, path) else {
            return;
        };
        animation.unit_parts(index, 0, None);
        let (start, _) = animation.pose_alphas(index, UnitPose::Death(0)).unwrap();
        let (late, _) = animation
            .pose_alphas(index, UnitPose::Death(60_000))
            .unwrap();
        assert!(start.iter().any(|a| a[3] > 0.99), "{start:?}");
        assert!(late.iter().all(|a| a[3] < 0.01), "{late:?}");
        assert!(animation.pose_alphas(index, UnitPose::Stand).is_none());
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
