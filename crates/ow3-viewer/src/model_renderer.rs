//! Drawing the models of a map with instancing: one draw call per part of
//! each model, with the placements as instances.

use std::collections::BTreeMap;

use ow3_render::image::Image;
use wgpu::util::DeviceExt;

use crate::models::{Animation, Group, Instance, ModelVertex};
use crate::renderer::{DEPTH_FORMAT, upload_texture};

const SHADER: &str = r#"
struct Globals {
    view_projection: mat4x4<f32>,
    light: vec4<f32>,
    water: vec4<f32>,
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
};
struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) alpha: f32,
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
    out.alpha = alphas[v.alpha];
    return out;
}

fn lit(v: VertexOut) -> vec4<f32> {
    let texel = textureSample(skin, skin_sampler, v.uv);
    let n = normalize(v.normal);
    let light = 0.5 + 0.5 * max(dot(n, globals.light.xyz), 0.0);
    return vec4<f32>(texel.rgb * light, texel.a * v.alpha);
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
    instances: usize,
}

pub struct ModelRenderer {
    pipelines: BTreeMap<Kind, wgpu::RenderPipeline>,
    instance_buffers: Vec<(wgpu::Buffer, u32)>,
    parts: Vec<GpuPart>,
    animation: Animation,
    bones: wgpu::Buffer,
    alphas: wgpu::Buffer,
    animation_bind_group: wgpu::BindGroup,
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
                    10 => Uint32
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
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
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
            pipelines.insert(kind, pipeline);
        }
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("model texture"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..wgpu::SamplerDescriptor::default()
        });
        let mut bind_groups = BTreeMap::new();
        for (path, image) in textures {
            let view = upload_texture(device, queue, image);
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("model texture"),
                layout: &texture_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
            });
            bind_groups.insert(path.clone(), bind_group);
        }
        let mut instance_buffers = Vec::with_capacity(groups.len());
        let mut parts = Vec::new();
        for group in groups {
            let instances = instance_buffers.len();
            instance_buffers.push((
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("instances"),
                    contents: bytemuck::cast_slice(&group.instances),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
                group.instances.len() as u32,
            ));
            for part in &group.parts {
                let Some(bind_group) = bind_groups.get(&part.texture) else {
                    continue;
                };
                parts.push(GpuPart {
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
                    instances,
                });
            }
        }
        // Opaque parts first, then the blended kinds over them.
        parts.sort_by_key(|p| p.kind);
        let renderer = ModelRenderer {
            pipelines,
            instance_buffers,
            parts,
            animation: animation.clone(),
            bones,
            alphas,
            animation_bind_group,
        };
        renderer.update(queue, 0);
        renderer
    }

    /// Poses the models `elapsed` milliseconds into their animations.
    pub fn update(&self, queue: &wgpu::Queue, elapsed: i32) {
        let bones = self.animation.bones(elapsed);
        queue.write_buffer(&self.bones, 0, bytemuck::cast_slice(&bones));
        let alphas = self.animation.alphas(elapsed);
        queue.write_buffer(&self.alphas, 0, bytemuck::cast_slice(&alphas));
    }

    /// Draws the models into a pass whose group 0 holds the globals.
    pub fn draw(&self, pass: &mut wgpu::RenderPass) {
        pass.set_bind_group(2, &self.animation_bind_group, &[]);
        let mut current = None;
        for part in &self.parts {
            if current != Some(part.kind) {
                pass.set_pipeline(&self.pipelines[&part.kind]);
                current = Some(part.kind);
            }
            let (instances, count) = &self.instance_buffers[part.instances];
            pass.set_bind_group(1, &part.bind_group, &[]);
            pass.set_vertex_buffer(0, part.vertices.slice(..));
            pass.set_vertex_buffer(1, instances.slice(..));
            pass.set_index_buffer(part.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..part.count, 0, 0..*count);
        }
    }
}
