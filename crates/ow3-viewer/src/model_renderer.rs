//! Drawing the models of a map with instancing: one draw call per part of
//! each model, with the placements as instances.

use std::collections::BTreeMap;

use ow3_render::image::Image;
use wgpu::util::DeviceExt;

use ow3_formats::mpq::ArchiveChain;

use crate::models::{Animation, Group, Instance, ModelVertex, Part, UnitPose};
use crate::renderer::{DEPTH_FORMAT, upload_texture};
use crate::sim_units::UnitDraw;

const SHADER: &str = r#"
struct Globals {
    view_projection: mat4x4<f32>,
    // Ground light: towards the sun and its intensity, its colour and the
    // ambient intensity, the ambient colour (SPEC-0017).
    terrain_sun: vec4<f32>,
    terrain_color: vec4<f32>,
    terrain_ambient: vec4<f32>,
    // The same for models.
    unit_sun: vec4<f32>,
    unit_color: vec4<f32>,
    unit_ambient: vec4<f32>,
};
@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var skin: texture_2d<f32>;
@group(1) @binding(1) var skin_sampler: sampler;
@group(2) @binding(0) var<storage, read> bones: array<mat4x4<f32>>;
@group(2) @binding(1) var<storage, read> alphas: array<f32>;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) bone_ids: vec4<u32>,
    @location(4) weights: vec4<f32>,
    @location(5) alpha: u32,
    @location(6) m0: vec4<f32>,
    @location(7) m1: vec4<f32>,
    @location(8) m2: vec4<f32>,
    @location(9) m3: vec4<f32>,
    @location(10) bone_base: u32,
    @location(11) tint: vec3<f32>,
    @location(12) alpha_shift: u32,
};
struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) alpha: f32,
    @location(3) tint: vec3<f32>,
};

@vertex
fn vs(v: VertexIn) -> VertexOut {
    let model = mat4x4<f32>(v.m0, v.m1, v.m2, v.m3);
    // A vertex without bones keeps its rest position.
    var skinned = mat4x4<f32>(
        vec4<f32>(1.0, 0.0, 0.0, 0.0),
        vec4<f32>(0.0, 1.0, 0.0, 0.0),
        vec4<f32>(0.0, 0.0, 1.0, 0.0),
        vec4<f32>(0.0, 0.0, 0.0, 1.0),
    );
    let total = v.weights.x + v.weights.y + v.weights.z + v.weights.w;
    if total > 0.0 {
        skinned = bones[v.bone_base + v.bone_ids.x] * v.weights.x
            + bones[v.bone_base + v.bone_ids.y] * v.weights.y
            + bones[v.bone_base + v.bone_ids.z] * v.weights.z
            + bones[v.bone_base + v.bone_ids.w] * v.weights.w;
    }
    var out: VertexOut;
    out.clip = globals.view_projection * model * skinned * vec4<f32>(v.position, 1.0);
    out.normal = (model * skinned * vec4<f32>(v.normal, 0.0)).xyz;
    out.uv = v.uv;
    out.alpha = alphas[v.alpha + v.alpha_shift];
    out.tint = v.tint;
    return out;
}

// Layers with shading flag 1 are drawn without light.
override UNSHADED: bool = false;

fn lit(v: VertexOut) -> vec4<f32> {
    let texel = textureSample(skin, skin_sampler, v.uv);
    if UNSHADED {
        return vec4<f32>(texel.rgb * v.tint, texel.a * v.alpha);
    }
    let n = normalize(v.normal);
    let facing = max(dot(n, globals.unit_sun.xyz), 0.0);
    let light = min(
        globals.unit_sun.w * globals.unit_color.rgb * facing
            + globals.unit_color.w * globals.unit_ambient.rgb,
        vec3<f32>(1.0),
    );
    return vec4<f32>(texel.rgb * v.tint * light, texel.a * v.alpha);
}

@fragment
fn fs_opaque(v: VertexOut) -> @location(0) vec4<f32> {
    if v.alpha <= 0.0 {
        discard;
    }
    return vec4<f32>(lit(v).rgb, 1.0);
}

@fragment
fn fs_tested(v: VertexOut) -> @location(0) vec4<f32> {
    let c = lit(v);
    if c.a < 0.75 {
        discard;
    }
    return vec4<f32>(c.rgb, 1.0);
}

@fragment
fn fs_blended(v: VertexOut) -> @location(0) vec4<f32> {
    return lit(v);
}

@fragment
fn fs_modulated(v: VertexOut) -> @location(0) vec4<f32> {
    if v.alpha <= 0.0 {
        discard;
    }
    return textureSample(skin, skin_sampler, v.uv);
}
"#;

/// How a layer is drawn, by its filter mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Opaque,
    Tested,
    Blended,
    Additive,
    Modulated,
}

fn kind(filter_mode: u32) -> Kind {
    match filter_mode {
        0 => Kind::Opaque,
        1 => Kind::Tested,
        2 => Kind::Blended,
        3 | 4 => Kind::Additive,
        _ => Kind::Modulated,
    }
}

struct GpuPart {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
    bind_group: wgpu::BindGroup,
    kind: Kind,
    unshaded: bool,
    instances: usize,
}

pub struct ModelRenderer {
    /// By kind and whether the parts are unshaded.
    pipelines: BTreeMap<(Kind, bool), wgpu::RenderPipeline>,
    instance_buffers: Vec<Instances>,
    parts: Vec<GpuPart>,
    animation: Animation,
    bones: wgpu::Buffer,
    alphas: wgpu::Buffer,
    animation_bind_group: wgpu::BindGroup,
    animation_layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    /// Repeating and clamping samplers.
    samplers: [wgpu::Sampler; 2],
    /// Texture bind groups by path and clamping.
    bind_groups: BTreeMap<(String, bool), wgpu::BindGroup>,
    /// The instance buffer and model of each unit model and team.
    unit_groups: BTreeMap<(String, u8), (usize, usize)>,
    /// The matrices of units in a pose of their own (dying, swinging),
    /// after those of the variants.
    unit_bones: Vec<crate::camera::Mat4>,
    /// The alphas of effects played once, after the shared ones.
    unit_alphas: Vec<f32>,
    bone_capacity: u32,
    alpha_capacity: u32,
}

impl ModelRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        globals_layout: &wgpu::BindGroupLayout,
        groups: &[Group],
        textures: &BTreeMap<String, Image>,
        animation: &Animation,
    ) -> ModelRenderer {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("models"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("model texture"),
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
        let storage = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let animation_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("animation"),
            entries: &[storage(0), storage(1)],
        });
        let bones = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bones"),
            size: u64::from(animation.bone_count()) * 64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let alphas = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("alphas"),
            size: u64::from(animation.alpha_count()) * 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let animation_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("animation"),
            layout: &animation_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: bones.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: alphas.as_entire_binding(),
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("models"),
            bind_group_layouts: &[globals_layout, &texture_layout, &animation_layout],
            push_constant_ranges: &[],
        });
        let vertex_layouts = [
            wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<ModelVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![
                    0 => Float32x3, 1 => Float32x3, 2 => Float32x2,
                    3 => Uint32x4, 4 => Float32x4, 5 => Uint32
                ],
            },
            wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Instance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![
                    6 => Float32x4, 7 => Float32x4, 8 => Float32x4, 9 => Float32x4,
                    10 => Uint32, 11 => Float32x3, 12 => Uint32
                ],
            },
        ];
        let additive = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::OVER,
        };
        let modulate = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Dst,
                dst_factor: wgpu::BlendFactor::Zero,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::OVER,
        };
        let mut pipelines = BTreeMap::new();
        for (kind, entry, blend, depth_write) in [
            (Kind::Opaque, "fs_opaque", None, true),
            (Kind::Tested, "fs_tested", None, true),
            (
                Kind::Blended,
                "fs_blended",
                Some(wgpu::BlendState::ALPHA_BLENDING),
                false,
            ),
            (Kind::Additive, "fs_blended", Some(additive), false),
            (Kind::Modulated, "fs_modulated", Some(modulate), false),
        ] {
            for unshaded in [false, true] {
                let constants = [("UNSHADED", f64::from(u8::from(unshaded)))];
                let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(entry),
                    layout: Some(&layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vs"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        buffers: &vertex_layouts,
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some(entry),
                        compilation_options: wgpu::PipelineCompilationOptions {
                            constants: &constants,
                            ..Default::default()
                        },
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    // Many model faces are meant to be seen from both sides.
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: DEPTH_FORMAT,
                        depth_write_enabled: depth_write,
                        depth_compare: wgpu::CompareFunction::LessEqual,
                        stencil: wgpu::StencilState::default(),
                        bias: wgpu::DepthBiasState::default(),
                    }),
                    multisample: wgpu::MultisampleState::default(),
                    multiview: None,
                    cache: None,
                });
                pipelines.insert((kind, unshaded), pipeline);
            }
        }
        // Textures repeat, except on parts that ask for their edges to be
        // kept (cliffs, whose atlas halves must not run into each other).
        let sampler = |address_mode| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("model texture"),
                address_mode_u: address_mode,
                address_mode_v: address_mode,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::FilterMode::Linear,
                anisotropy_clamp: 16,
                ..wgpu::SamplerDescriptor::default()
            })
        };
        let samplers = [
            sampler(wgpu::AddressMode::Repeat),
            sampler(wgpu::AddressMode::ClampToEdge),
        ];
        let mut renderer = ModelRenderer {
            pipelines,
            instance_buffers: Vec::new(),
            parts: Vec::new(),
            animation: animation.clone(),
            bones,
            alphas,
            animation_bind_group,
            animation_layout,
            texture_layout,
            samplers,
            bind_groups: BTreeMap::new(),
            unit_groups: BTreeMap::new(),
            unit_bones: Vec::new(),
            unit_alphas: Vec::new(),
            bone_capacity: animation.bone_count(),
            alpha_capacity: animation.alpha_count(),
        };
        for (path, image) in textures {
            renderer.add_texture(device, queue, path, image);
        }
        for group in groups {
            renderer.add_group(device, &group.parts, &group.instances);
        }
        renderer.update(device, queue, 0);
        renderer
    }

    /// Uploads a texture, for both samplers.
    fn add_texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        path: &str,
        image: &Image,
    ) {
        let view = upload_texture(device, queue, image);
        for (clamp, sampler) in [false, true].into_iter().zip(&self.samplers) {
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("model texture"),
                layout: &self.texture_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                ],
            });
            self.bind_groups
                .insert((path.to_owned(), clamp), bind_group);
        }
    }

    /// Adds the parts of a model with their instances; the index of its
    /// instance buffer.
    fn add_group(
        &mut self,
        device: &wgpu::Device,
        parts: &[Part],
        instances: &[Instance],
    ) -> usize {
        let index = self.instance_buffers.len();
        self.instance_buffers
            .push(Instances::new(device, instances, instances.len()));
        for part in parts {
            let Some(bind_group) = self.bind_groups.get(&(part.texture.clone(), part.clamp)) else {
                continue;
            };
            self.parts.push(GpuPart {
                vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("model vertices"),
                    contents: bytemuck::cast_slice(&part.vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
                indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("model indices"),
                    contents: bytemuck::cast_slice(&part.indices),
                    usage: wgpu::BufferUsages::INDEX,
                }),
                count: part.indices.len() as u32,
                bind_group: bind_group.clone(),
                kind: kind(part.filter_mode),
                unshaded: part.unshaded,
                instances: index,
            });
        }
        // Opaque and alpha-tested parts first, in the order of their models'
        // geosets and layers: a later layer on the same faces wins, as the
        // team colour of a rifleman's cloak over the cloak of an earlier
        // geoset. Then the blended kinds over them. The sort is stable.
        self.parts.sort_by_key(|p| match p.kind {
            Kind::Opaque | Kind::Tested => None,
            kind => Some(kind),
        });
        index
    }

    /// Places the units of the simulation for the next frame, loading the
    /// models of new kinds from `chain`.
    pub fn set_units(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        chain: &mut ArchiveChain,
        units: &[UnitDraw],
        elapsed: i32,
    ) -> Vec<UnitAnchor> {
        // Models and groups first: they may add bone matrices.
        let mut drawn = Vec::with_capacity(units.len());
        for unit in units {
            let Some(model) = self.animation.model(chain, &unit.model) else {
                continue;
            };
            let key = (unit.model.clone(), unit.team);
            let buffer = match self.unit_groups.get(&key) {
                Some(&(buffer, _)) => buffer,
                None => {
                    let parts = self.animation.unit_parts(model, unit.team);
                    for part in &parts {
                        if self
                            .bind_groups
                            .contains_key(&(part.texture.clone(), false))
                        {
                            continue;
                        }
                        if let Some(image) = chain
                            .read(&part.texture)
                            .and_then(|d| ow3_formats::blp::decode(&d).ok())
                        {
                            self.add_texture(device, queue, &part.texture, &Image::from(image));
                        }
                    }
                    let buffer = self.add_group(device, &parts, &[]);
                    self.unit_groups.insert(key, (buffer, model));
                    buffer
                }
            };
            drawn.push((unit, buffer, model));
        }
        // Dying and swinging units get their own matrices after those of
        // the variants.
        self.unit_bones.clear();
        self.unit_alphas.clear();
        let first_alpha = self.animation.alpha_count();
        let first = self.animation.bone_count();
        let mut lists: BTreeMap<usize, Vec<Instance>> = self
            .unit_groups
            .values()
            .map(|&(buffer, _)| (buffer, Vec::new()))
            .collect();
        let mut anchors = Vec::with_capacity(drawn.len());
        for (unit, buffer, model) in drawn {
            let bone_base = match self
                .animation
                .unit_pose(model, unit.pose, elapsed, unit.handle)
            {
                Some(matrices) => {
                    let base = first + self.unit_bones.len() as u32;
                    self.unit_bones.extend(matrices);
                    base
                }
                None => self.animation.variant_base(model, unit.handle),
            };
            // An effect fades on its own clock.
            let alpha_shift = match unit.pose {
                UnitPose::Once(ms) => {
                    self.animation
                        .once_alphas(model, ms)
                        .map_or(0, |(values, slot)| {
                            let shift = first_alpha + self.unit_alphas.len() as u32 - slot;
                            self.unit_alphas.extend(values);
                            shift
                        })
                }
                _ => 0,
            };
            lists.entry(buffer).or_default().push(Instance {
                matrix: unit.matrix,
                bone_base,
                tint: unit.tint,
                alpha_shift,
            });
            let m = unit.matrix;
            let top = self.animation.model_top(model);
            anchors.push(UnitAnchor {
                handle: unit.handle,
                foot: [m[3][0], m[3][1], m[3][2]],
                head: [
                    m[3][0] + m[2][0] * top,
                    m[3][1] + m[2][1] * top,
                    m[3][2] + m[2][2] * top,
                ],
                life: unit.life,
                selection: unit.selection,
                relation: unit.relation,
            });
        }
        for (buffer, instances) in lists {
            self.instance_buffers[buffer].write(device, queue, &instances);
        }
        anchors
    }

    /// Poses the models `elapsed` milliseconds into their animations.
    pub fn update(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, elapsed: i32) {
        let mut bones = self.animation.bones(elapsed);
        bones.extend_from_slice(&self.unit_bones);
        let mut alphas = self.animation.alphas(elapsed);
        alphas.extend_from_slice(&self.unit_alphas);
        let (bone_count, alpha_count) = (bones.len() as u32, alphas.len() as u32);
        if bone_count > self.bone_capacity || alpha_count > self.alpha_capacity {
            self.bone_capacity = self.bone_capacity.max(bone_count * 2);
            self.alpha_capacity = self.alpha_capacity.max(alpha_count * 2);
            self.bones = storage_buffer(device, "bones", u64::from(self.bone_capacity) * 64);
            self.alphas = storage_buffer(device, "alphas", u64::from(self.alpha_capacity) * 4);
            self.animation_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("animation"),
                layout: &self.animation_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.bones.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: self.alphas.as_entire_binding(),
                    },
                ],
            });
        }
        queue.write_buffer(&self.bones, 0, bytemuck::cast_slice(&bones));
        queue.write_buffer(&self.alphas, 0, bytemuck::cast_slice(&alphas));
    }

    /// Draws the models into a pass whose group 0 holds the globals.
    pub fn draw(&self, pass: &mut wgpu::RenderPass) {
        pass.set_bind_group(2, &self.animation_bind_group, &[]);
        let mut current = None;
        for part in &self.parts {
            let instances = &self.instance_buffers[part.instances];
            if instances.count == 0 {
                continue;
            }
            let key = (part.kind, part.unshaded);
            if current != Some(key) {
                pass.set_pipeline(&self.pipelines[&key]);
                current = Some(key);
            }
            pass.set_bind_group(1, &part.bind_group, &[]);
            pass.set_vertex_buffer(0, part.vertices.slice(..));
            pass.set_vertex_buffer(1, instances.buffer.slice(..));
            pass.set_index_buffer(part.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..part.count, 0, 0..instances.count);
        }
    }
}

fn storage_buffer(device: &wgpu::Device, label: &str, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size.max(64),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// The instances of one model, in a buffer that grows when needed.
struct Instances {
    buffer: wgpu::Buffer,
    count: u32,
    capacity: usize,
}

impl Instances {
    fn new(device: &wgpu::Device, instances: &[Instance], capacity: usize) -> Instances {
        let capacity = capacity.max(instances.len()).max(1);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instances"),
            size: (capacity * std::mem::size_of::<Instance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: true,
        });
        {
            let bytes: &[u8] = bytemuck::cast_slice(instances);
            buffer.slice(..).get_mapped_range_mut()[..bytes.len()].copy_from_slice(bytes);
        }
        buffer.unmap();
        Instances {
            buffer,
            count: instances.len() as u32,
            capacity,
        }
    }

    fn write(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, instances: &[Instance]) {
        if instances.len() > self.capacity {
            *self = Instances::new(device, instances, instances.len() * 2);
            return;
        }
        queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(instances));
        self.count = instances.len() as u32;
    }
}

/// Where a unit stands and how tall it is, for its health bar and for
/// picking it with the mouse.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnitAnchor {
    pub handle: u32,
    pub foot: [f32; 3],
    pub head: [f32; 3],
    pub life: Option<f32>,
    pub selection: f32,
    pub relation: Option<ow3_sim::game::Relation>,
}
