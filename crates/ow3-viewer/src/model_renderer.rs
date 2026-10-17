//! Drawing the models of a map with instancing: one draw call per part of
//! each model, with the placements as instances.

use std::collections::BTreeMap;

use ow3_render::image::Image;
use wgpu::util::DeviceExt;

use crate::models::{Group, ModelVertex};
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

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) m0: vec4<f32>,
    @location(4) m1: vec4<f32>,
    @location(5) m2: vec4<f32>,
    @location(6) m3: vec4<f32>,
};
struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
};

@vertex
fn vs(v: VertexIn) -> VertexOut {
    let model = mat4x4<f32>(v.m0, v.m1, v.m2, v.m3);
    var out: VertexOut;
    out.clip = globals.view_projection * model * vec4<f32>(v.position, 1.0);
    out.normal = (model * vec4<f32>(v.normal, 0.0)).xyz;
    out.uv = v.uv;
    return out;
}

fn lit(v: VertexOut) -> vec4<f32> {
    let texel = textureSample(skin, skin_sampler, v.uv);
    let n = normalize(v.normal);
    let light = 0.5 + 0.5 * max(dot(n, globals.light.xyz), 0.0);
    return vec4<f32>(texel.rgb * light, texel.a);
}

@fragment
fn fs_opaque(v: VertexOut) -> @location(0) vec4<f32> {
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
}

impl ModelRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        globals_layout: &wgpu::BindGroupLayout,
        groups: &[Group],
        textures: &BTreeMap<String, Image>,
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
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("models"),
            bind_group_layouts: &[globals_layout, &texture_layout],
            push_constant_ranges: &[],
        });
        let vertex_layouts = [
            wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<ModelVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2],
            },
            wgpu::VertexBufferLayout {
                array_stride: 64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![
                    3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4
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
        ModelRenderer {
            pipelines,
            instance_buffers,
            parts,
        }
    }

    /// Draws the models into a pass whose group 0 holds the globals.
    pub fn draw(&self, pass: &mut wgpu::RenderPass) {
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
