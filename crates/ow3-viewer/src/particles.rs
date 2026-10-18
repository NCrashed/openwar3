//! Drawing the particles of `PRE2` emitters (`ow3_render::particles`):
//! every frame the placed models with emitters step their particles and
//! each particle becomes a textured quad, facing the camera or, for
//! emitters with the `XY_QUAD` flag, lying in the model's xy plane. The
//! quads are drawn after the models, tested against the depth but not
//! writing it, by the emitter's filter mode.
//!
//! PROVISIONAL: particles are not lit and not sorted by distance; placed
//! models play their Stand sequence from a phase of their own, units are
//! where their animations are in the simulation each frame.

use std::collections::BTreeMap;
use std::ops::Range;

use bytemuck::{Pod, Zeroable};
use ow3_render::animation::Moment;
use ow3_render::image::Image;
use ow3_render::particles::{Emissions, XY_QUAD, look};

use crate::camera::{Camera, Mat4};
use crate::models::Animation;
use crate::renderer::DEPTH_FORMAT;

const SHADER: &str = r#"
struct Globals {
    view_projection: mat4x4<f32>,
};
@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var sprite: texture_2d<f32>;
@group(1) @binding(1) var sprite_sampler: sampler;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) colour: vec4<f32>,
};
struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) colour: vec4<f32>,
};

@vertex
fn vs(v: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip = globals.view_projection * vec4<f32>(v.position, 1.0);
    out.uv = v.uv;
    out.colour = v.colour;
    return out;
}

@fragment
fn fs(v: VertexOut) -> @location(0) vec4<f32> {
    return textureSample(sprite, sprite_sampler, v.uv) * v.colour;
}
"#;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    uv: [f32; 2],
    colour: [f32; 4],
}

/// Filter modes 0 blend, 1 additive, 2 modulate, 3 modulate 2x; alpha
/// key (4) is drawn as blend.
const PIPELINES: usize = 4;

fn blend(mode: usize) -> wgpu::BlendState {
    use wgpu::{BlendComponent, BlendFactor as F, BlendOperation::Add};
    let colour = |src, dst| BlendComponent {
        src_factor: src,
        dst_factor: dst,
        operation: Add,
    };
    match mode {
        1 => wgpu::BlendState {
            color: colour(F::SrcAlpha, F::One),
            alpha: colour(F::Zero, F::One),
        },
        2 => wgpu::BlendState {
            color: colour(F::Dst, F::Zero),
            alpha: colour(F::Zero, F::One),
        },
        3 => wgpu::BlendState {
            color: colour(F::Dst, F::Src),
            alpha: colour(F::Zero, F::One),
        },
        _ => wgpu::BlendState::ALPHA_BLENDING,
    }
}

/// One placed model with emitters.
struct Placed {
    model: usize,
    matrix: Mat4,
    emissions: Emissions,
    /// Milliseconds its animation is ahead of the others.
    phase: i32,
    /// For a unit, where it is in its animations now; a placed model plays
    /// its Stand sequence.
    moment: Option<Moment>,
}

/// A unit of the simulation with emitters: its handle, model, placement
/// and where it is in its animations.
pub struct UnitEmitters {
    pub handle: u32,
    pub model: usize,
    pub matrix: Mat4,
    pub moment: Moment,
}

pub struct Particles {
    globals: wgpu::Buffer,
    globals_bind_group: wgpu::BindGroup,
    pipelines: Vec<wgpu::RenderPipeline>,
    /// A bind group for every texture, by path.
    textures: Vec<wgpu::BindGroup>,
    texture_index: BTreeMap<String, usize>,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    placed: Vec<Placed>,
    /// The units of the simulation with emitters, by handle.
    units: BTreeMap<u32, Placed>,
    vertices: wgpu::Buffer,
    capacity: usize,
    /// Pipeline, texture and vertices of each run of quads.
    draws: Vec<(usize, usize, Range<u32>)>,
    /// The time of the last step, in milliseconds.
    last: Option<i32>,
}

impl Particles {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        animation: &Animation,
        emitters: &[(usize, Mat4)],
        images: &BTreeMap<String, Image>,
    ) -> Particles {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("particles"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("particle globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("particle texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("particle globals"),
            size: std::mem::size_of::<Mat4>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("particle globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("particles"),
            bind_group_layouts: &[&globals_layout, &texture_layout],
            push_constant_ranges: &[],
        });
        let pipelines = (0..PIPELINES)
            .map(|mode| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("particles"),
                    layout: Some(&layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vs"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        buffers: &[wgpu::VertexBufferLayout {
                            array_stride: std::mem::size_of::<Vertex>() as u64,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &wgpu::vertex_attr_array![
                                0 => Float32x3, 1 => Float32x2, 2 => Float32x4
                            ],
                        }],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some("fs"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend: Some(blend(mode)),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: DEPTH_FORMAT,
                        depth_write_enabled: false,
                        depth_compare: wgpu::CompareFunction::LessEqual,
                        stencil: wgpu::StencilState::default(),
                        bias: wgpu::DepthBiasState::default(),
                    }),
                    multisample: wgpu::MultisampleState::default(),
                    multiview: None,
                    cache: None,
                })
            })
            .collect();
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("particles"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..wgpu::SamplerDescriptor::default()
        });
        let placed = emitters
            .iter()
            .enumerate()
            .map(|(i, &(model, matrix))| Placed {
                model,
                matrix,
                emissions: Emissions::new(animation.mdx(model), i as u64 + 1),
                phase: (i as i32 * 977) % 10_000,
                moment: None,
            })
            .collect();
        let capacity = 4096;
        let mut particles = Particles {
            globals,
            globals_bind_group,
            pipelines,
            texture_layout,
            sampler,
            textures: Vec::new(),
            texture_index: BTreeMap::new(),
            placed,
            units: BTreeMap::new(),
            vertices: vertex_buffer(device, capacity),
            capacity,
            draws: Vec::new(),
            last: None,
        };
        for (path, image) in images {
            particles.add_texture(device, queue, path, image);
        }
        particles
    }

    /// Makes `image` the texture of `path`.
    fn add_texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        path: &str,
        image: &Image,
    ) {
        let view = crate::renderer::upload_texture(device, queue, image);
        self.texture_index
            .insert(path.to_owned(), self.textures.len());
        self.textures
            .push(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("particle texture"),
                layout: &self.texture_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            }));
    }

    /// The units with emitters now: new ones start without particles, the
    /// others go on from where they are, and those gone lose theirs. Their
    /// textures are read from `chain` the first time.
    pub fn set_units(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        chain: &mut ow3_formats::mpq::ArchiveChain,
        animation: &Animation,
        units: &[UnitEmitters],
    ) {
        let mut kept = BTreeMap::new();
        for u in units {
            let model = animation.mdx(u.model);
            for e in &model.particle_emitters {
                let Some(t) = model.textures.get(e.texture_id as usize) else {
                    continue;
                };
                if t.replaceable_id != 0 || self.texture_index.contains_key(&t.path) {
                    continue;
                }
                if let Some(image) = chain
                    .read(&t.path)
                    .and_then(|d| ow3_formats::blp::decode(&d).ok())
                {
                    self.add_texture(device, queue, &t.path, &Image::from(image));
                }
            }
            let mut placed = self
                .units
                .remove(&u.handle)
                .filter(|p| p.model == u.model)
                .unwrap_or_else(|| Placed {
                    model: u.model,
                    matrix: u.matrix,
                    emissions: Emissions::new(model, u64::from(u.handle)),
                    phase: 0,
                    moment: None,
                });
            placed.matrix = u.matrix;
            placed.moment = Some(u.moment);
            kept.insert(u.handle, placed);
        }
        self.units = kept;
    }

    /// Steps the particles to `elapsed` milliseconds and builds their
    /// quads for `camera`. The first call simulates the time before it, up
    /// to 10 s, so that the emitters are in full flow.
    pub fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        animation: &Animation,
        camera: &Camera,
        elapsed: i32,
    ) {
        if self.placed.is_empty() && self.units.is_empty() {
            self.draws.clear();
            return;
        }
        let from = self.last.unwrap_or((elapsed - 10_000).max(0)).min(elapsed);
        self.last = Some(elapsed);
        // Steps of at most 50 ms.
        let mut t = from;
        while t < elapsed {
            let next = (t + 50).min(elapsed);
            let dt = (next - t) as f32 / 1000.0;
            for p in self.placed.iter_mut().chain(self.units.values_mut()) {
                let moment = p
                    .moment
                    .unwrap_or_else(|| animation.stand_moment(p.model, next + p.phase));
                p.emissions
                    .step(animation.mdx(p.model), &p.matrix, moment, dt);
            }
            t = next;
        }
        // The camera's right and up for quads that face it.
        let eye = camera.eye();
        let f = normalize(sub(camera.target, eye));
        let right = normalize(cross(f, [0.0, 0.0, 1.0]));
        let up = cross(right, f);
        // (pipeline, texture) -> vertices
        let mut runs: BTreeMap<(usize, usize), Vec<Vertex>> = BTreeMap::new();
        for p in self.placed.iter().chain(self.units.values()) {
            let model = animation.mdx(p.model);
            for particle in &p.emissions.particles {
                let Some(e) = model.particle_emitters.get(particle.emitter) else {
                    continue;
                };
                let texture = model
                    .textures
                    .get(e.texture_id as usize)
                    .and_then(|t| self.texture_index.get(&t.path));
                let Some(&texture) = texture else {
                    continue;
                };
                let l = look(e, particle.age);
                if l.color[3] <= 0.0 || l.size <= 0.0 {
                    continue;
                }
                let (a, b) = if e.node.flags & XY_QUAD != 0 {
                    // Flat, the top of the texture ahead in the direction
                    // the particle moves (PROVISIONAL: the reading of the
                    // flag that turns the shore foam along the shore, as
                    // the game shows it); still ones keep the model's axes.
                    let [vx, vy, _] = particle.velocity;
                    let l = (vx * vx + vy * vy).sqrt();
                    if l > 1e-3 {
                        let b = [vx / l, vy / l, 0.0];
                        ([b[1], -b[0], 0.0], b)
                    } else {
                        (particle.axes[0], particle.axes[1])
                    }
                } else {
                    (right, up)
                };
                let columns = e.columns.max(1);
                let (cu, cv) = ((l.cell % columns) as f32, (l.cell / columns) as f32);
                let (du, dv) = (1.0 / columns as f32, 1.0 / e.rows.max(1) as f32);
                let corner = |s: f32, t: f32| -> [f32; 3] {
                    std::array::from_fn(|i| particle.position[i] + (a[i] * s + b[i] * t) * l.size)
                };
                let uv =
                    |s: f32, t: f32| [(cu + (s + 1.0) / 2.0) * du, (cv + (1.0 - t) / 2.0) * dv];
                let vertex = |s: f32, t: f32| Vertex {
                    position: corner(s, t),
                    uv: uv(s, t),
                    colour: l.color,
                };
                let mode = (e.filter_mode as usize).min(PIPELINES - 1);
                let mode = if e.filter_mode == 4 { 0 } else { mode };
                runs.entry((mode, texture)).or_default().extend([
                    vertex(-1.0, -1.0),
                    vertex(1.0, -1.0),
                    vertex(1.0, 1.0),
                    vertex(-1.0, -1.0),
                    vertex(1.0, 1.0),
                    vertex(-1.0, 1.0),
                ]);
            }
        }
        let mut vertices = Vec::new();
        self.draws.clear();
        for ((mode, texture), run) in runs {
            let start = vertices.len() as u32;
            vertices.extend(run);
            self.draws
                .push((mode, texture, start..vertices.len() as u32));
        }
        if vertices.len() > self.capacity {
            self.capacity = vertices.len() * 2;
            self.vertices = vertex_buffer(device, self.capacity);
        }
        queue.write_buffer(&self.vertices, 0, bytemuck::cast_slice(&vertices));
        queue.write_buffer(
            &self.globals,
            0,
            bytemuck::cast_slice(&[camera.view_projection()]),
        );
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass) {
        if self.draws.is_empty() {
            return;
        }
        pass.set_bind_group(0, &self.globals_bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        for (mode, texture, range) in &self.draws {
            pass.set_pipeline(&self.pipelines[*mode]);
            pass.set_bind_group(1, &self.textures[*texture], &[]);
            pass.draw(range.clone(), 0..1);
        }
    }
}

fn vertex_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("particles"),
        size: (capacity * std::mem::size_of::<Vertex>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 1e-9 { v.map(|c| c / l) } else { v }
}
