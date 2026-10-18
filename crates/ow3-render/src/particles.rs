//! Particles of the `PRE2` emitters of models, after the public community
//! documentation of the MDX format: what each field means is documented
//! there, how the game moves the particles exactly is not, so this is the
//! plain reading of the fields.
//!
//! An emitter sends out `emissionRate` particles per second while it is
//! visible (`KP2V`), from a random point of a `width` by `length` area
//! round its pivot, in a random direction within `latitude` degrees of its
//! node's z axis, at `speed` give or take `variation` of it. A particle
//! falls by `gravity`, lives `lifespan` seconds and passes through three
//! segments of colour, alpha and size, the middle one reached at `time` of
//! its life. The random numbers are the viewer's own (not the game's
//! generators): the simulation does not use them.

use ow3_formats::mdx::{Model, ParticleEmitter2};

use crate::animation::{
    Mat4, Moment, Pose, Vec3, multiply, track_or, transform_direction, transform_point,
};

/// Node flag: the quads lie in the model's xy plane instead of facing the
/// camera.
pub const XY_QUAD: u32 = 0x10_0000;
/// Node flag: the emitter's particles are drawn without light.
pub const UNSHADED: u32 = 0x8000;

/// One particle, in world coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Particle {
    /// The emitter of the model it came from.
    pub emitter: usize,
    pub position: Vec3,
    pub velocity: Vec3,
    /// Seconds since its birth.
    pub age: f32,
    /// The model's x and y axes in the world when it was born, for quads
    /// in the model's xy plane.
    pub axes: [Vec3; 2],
}

/// What a particle looks like now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    /// Red, green, blue and alpha, 0 to 1.
    pub color: [f32; 4],
    /// Half the side of its quad.
    pub size: f32,
    /// The cell of the emitter's texture.
    pub cell: u32,
}

/// A small generator for the viewer's own random numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Random(u64);

impl Random {
    pub fn new(seed: u64) -> Random {
        Random(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    /// A number in `0..1`.
    pub fn unit(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }

    /// A number in `-1..1`.
    pub fn signed(&mut self) -> f32 {
        2.0 * self.unit() - 1.0
    }
}

/// The particles of one placed model and what is left over of its
/// emissions.
#[derive(Clone, Debug, PartialEq)]
pub struct Emissions {
    pub particles: Vec<Particle>,
    /// Fractions of particles not yet sent out, by emitter.
    owed: Vec<f32>,
    random: Random,
}

impl Emissions {
    pub fn new(model: &Model, seed: u64) -> Emissions {
        Emissions {
            particles: Vec::new(),
            owed: vec![0.0; model.particle_emitters.len()],
            random: Random::new(seed),
        }
    }

    /// Lets `dt` seconds pass for the model placed by `placement` at
    /// `moment` of its animation: old particles move and die, new ones
    /// are born.
    pub fn step(&mut self, model: &Model, placement: &Mat4, moment: Moment, dt: f32) {
        for p in &mut self.particles {
            let Some(e) = model.particle_emitters.get(p.emitter) else {
                continue;
            };
            p.age += dt;
            p.velocity[2] -= value(model, e, b"KP2G", e.gravity, moment) * dt;
            for i in 0..3 {
                p.position[i] += p.velocity[i] * dt;
            }
        }
        self.particles.retain(|p| {
            model
                .particle_emitters
                .get(p.emitter)
                .is_some_and(|e| p.age < e.lifespan)
        });
        if model.particle_emitters.is_empty() || dt <= 0.0 {
            return;
        }
        let pose = Pose::new(model, moment);
        let axes = [
            normalize(transform_direction(placement, [1.0, 0.0, 0.0])),
            normalize(transform_direction(placement, [0.0, 1.0, 0.0])),
        ];
        for (index, e) in model.particle_emitters.iter().enumerate() {
            if value(model, e, b"KP2V", 1.0, moment) <= 0.0 {
                self.owed[index] = 0.0;
                continue;
            }
            let rate = value(model, e, b"KP2E", e.emission_rate, moment);
            self.owed[index] += rate.max(0.0) * dt;
            let node = pose
                .matrices
                .get(&e.node.object_id)
                .copied()
                .unwrap_or(crate::animation::IDENTITY);
            let world = multiply(placement, &node);
            let pivot = model
                .pivots
                .get(e.node.object_id as usize)
                .copied()
                .unwrap_or([0.0; 3]);
            let speed = value(model, e, b"KP2S", e.speed, moment);
            let variation = value(model, e, b"KP2R", e.variation, moment);
            let latitude = value(model, e, b"KP2L", e.latitude, moment).to_radians();
            let width = value(model, e, b"KP2W", e.width, moment);
            let length = value(model, e, b"KP2N", e.length, moment);
            while self.owed[index] >= 1.0 {
                self.owed[index] -= 1.0;
                let r = &mut self.random;
                let start = [
                    pivot[0] + r.signed() * width / 2.0,
                    pivot[1] + r.signed() * length / 2.0,
                    pivot[2],
                ];
                // Tilted from the z axis by up to the latitude, then turned
                // round it.
                let tilt = r.signed() * latitude;
                let turn = r.unit() * std::f32::consts::TAU;
                let local = [tilt.sin() * turn.cos(), tilt.sin() * turn.sin(), tilt.cos()];
                let direction = normalize(transform_direction(&world, local));
                let speed = speed * (1.0 + r.signed() * variation);
                self.particles.push(Particle {
                    emitter: index,
                    position: transform_point(&world, start),
                    velocity: direction.map(|d| d * speed),
                    age: 0.0,
                    axes,
                });
            }
        }
    }
}

/// The value of an emitter's track, or its static value.
fn value(model: &Model, e: &ParticleEmitter2, tag: &[u8; 4], fixed: f32, moment: Moment) -> f32 {
    track_or(model, &e.tracks, tag, &[fixed], moment)[0]
}

fn normalize(v: Vec3) -> Vec3 {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 1e-9 { v.map(|c| c / l) } else { v }
}

/// The colour, size and texture cell of a particle of `e` at `age`.
pub fn look(e: &ParticleEmitter2, age: f32) -> Look {
    let life = (age / e.lifespan.max(1e-6)).clamp(0.0, 1.0);
    let middle = e.time.clamp(1e-6, 1.0 - 1e-6);
    let (from, to, f) = if life < middle {
        (0, 1, life / middle)
    } else {
        (1, 2, (life - middle) / (1.0 - middle))
    };
    let mix = |a: f32, b: f32| a + (b - a) * f;
    // The colours are stored blue, green, red.
    let [b, g, r] =
        std::array::from_fn(|i| mix(e.segment_colors[from][i], e.segment_colors[to][i]));
    let alpha = mix(
        f32::from(e.segment_alphas[from]),
        f32::from(e.segment_alphas[to]),
    ) / 255.0;
    let size = mix(e.segment_scaling[from], e.segment_scaling[to]);
    // The head cells: from the first to the last of the interval of the
    // part of the life, `repeat` times over.
    let interval = if life < middle {
        e.head_intervals
    } else {
        e.head_decay_intervals
    };
    let [first, last, repeat] = interval;
    let count = last.saturating_sub(first) + 1;
    let step = (f * repeat.max(1) as f32 * count as f32).floor() as u32;
    let cell = first + step % count;
    Look {
        color: [r, g, b, alpha],
        size,
        cell: cell.min(e.rows * e.columns - 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ow3_formats::mdx::Node;

    fn emitter() -> ParticleEmitter2 {
        ParticleEmitter2 {
            node: Node {
                name: String::new(),
                object_id: 0,
                parent_id: None,
                flags: XY_QUAD,
                tracks: Vec::new(),
            },
            speed: 30.0,
            variation: 0.0,
            latitude: 0.0,
            gravity: 0.0,
            lifespan: 4.0,
            emission_rate: 2.0,
            width: 0.0,
            length: 0.0,
            filter_mode: 1,
            rows: 1,
            columns: 1,
            head_or_tail: 0,
            tail_length: 1.0,
            time: 0.5,
            segment_colors: [[1.0; 3]; 3],
            segment_alphas: [0, 255, 0],
            segment_scaling: [30.0, 80.0, 70.0],
            head_intervals: [0, 0, 1],
            head_decay_intervals: [0, 0, 1],
            tail_intervals: [0, 0, 1],
            tail_decay_intervals: [0, 0, 1],
            texture_id: 0,
            squirt: 0,
            priority_plane: 0,
            replaceable_id: 0,
            tracks: Vec::new(),
        }
    }

    /// Two particles a second go up the z axis at their speed and die
    /// after their lifespan.
    #[test]
    fn emission_motion_and_death() {
        let model = Model {
            particle_emitters: vec![emitter()],
            pivots: vec![[0.0, 0.0, 5.0]],
            ..Model::default()
        };
        let place = crate::animation::IDENTITY;
        let moment = Moment::start(&model, 0);
        let mut e = Emissions::new(&model, 1);
        for _ in 0..101 {
            e.step(&model, &place, moment, 0.01);
        }
        // Just over a second: two particles, each 30 a second up from the
        // pivot.
        assert_eq!(e.particles.len(), 2);
        let first = e.particles[0];
        assert!((first.position[2] - (5.0 + 30.0 * first.age)).abs() < 1e-3);
        for _ in 0..1000 {
            e.step(&model, &place, moment, 0.01);
        }
        // Eight living at a time: two a second for four seconds.
        assert!(
            (7..=9).contains(&e.particles.len()),
            "{}",
            e.particles.len()
        );
        assert!(e.particles.iter().all(|p| p.age < 4.0));
    }

    /// The segments: transparent and small at birth, opaque at the middle
    /// of the life, transparent again at its end.
    #[test]
    fn segments() {
        let e = emitter();
        let born = look(&e, 0.0);
        let middle = look(&e, 2.0);
        let end = look(&e, 4.0);
        assert_eq!((born.color[3], born.size), (0.0, 30.0));
        assert_eq!((middle.color[3], middle.size), (1.0, 80.0));
        assert!(end.color[3] < 1e-6 && (end.size - 70.0).abs() < 1e-4);
    }
}
