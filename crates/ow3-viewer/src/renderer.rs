//! Drawing the ground with wgpu, to a window's surface or to an image.

use bytemuck::{Pod, Zeroable};
use ow3_render::image::Image;
use wgpu::util::DeviceExt;

use crate::camera::{Camera, Mat4};
use crate::model_renderer::{ModelRenderer, UnitAnchor};
use crate::overlay::{Bar, Overlay};
use crate::scene::Scene;
use crate::terrain_mesh::{Layer, Vertex, atlas_cells};

pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Colour of the sky behind the map.
const CLEAR: wgpu::Color = wgpu::Color {
    r: 0.02,
    g: 0.02,
    b: 0.03,
    a: 1.0,
};

const SHADER: &str = r#"
struct Globals {
    view_projection: mat4x4<f32>,
    light: vec4<f32>,
    water: vec4<f32>,
};
@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var atlas: texture_2d_array<f32>;
@group(1) @binding(1) var atlas_sampler: sampler;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) tint: vec4<f32>,
    @location(4) cell: u32,
};
struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) tint: vec4<f32>,
    @location(3) @interpolate(flat) cell: u32,
};

@vertex
fn vs(v: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip = globals.view_projection * vec4<f32>(v.position, 1.0);
    out.normal = v.normal;
    out.uv = v.uv;
    out.tint = v.tint;
    out.cell = v.cell;
    return out;
}

@fragment
fn fs(v: VertexOut) -> @location(0) vec4<f32> {
    let texel = textureSample(atlas, atlas_sampler, v.uv, v.cell);
    let light = 0.55 + 0.45 * max(dot(normalize(v.normal), globals.light.xyz), 0.0) / globals.light.z;
    return vec4<f32>(texel.rgb * light * v.tint.rgb, texel.a);
}

@fragment
fn fs_water(v: VertexOut) -> @location(0) vec4<f32> {
    return textureSample(atlas, atlas_sampler, v.uv, v.cell) * v.tint;
}
"#;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Globals {
    view_projection: Mat4,
    light: [f32; 4],
    water: [f32; 4],
}

struct GpuLayer {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
    bind_group: wgpu::BindGroup,
    /// Texture 0: only full tiles, drawn opaque.
    opaque: bool,
}

/// The ground on the GPU, ready to draw.
pub struct Renderer {
    opaque: wgpu::RenderPipeline,
    blended: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    globals_bind_group: wgpu::BindGroup,
    layers: Vec<GpuLayer>,
    models: ModelRenderer,
    water_pipeline: wgpu::RenderPipeline,
    water: Option<GpuLayer>,
    overlay: Overlay,
    /// The units placed last, for health bars and picking.
    anchors: Vec<UnitAnchor>,
    /// The unit picked with the mouse.
    pub selected: Option<u32>,
}

/// The image and its smaller copies, each half the size of the one before.
fn mip_chain(image: &Image) -> Vec<Image> {
    let mut levels = vec![image.clone()];
    while let Some(last) = levels.last() {
        if last.width == 1 && last.height == 1 {
            break;
        }
        let (w, h) = ((last.width / 2).max(1), (last.height / 2).max(1));
        let mut next = Image::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let mut sum = [0u32; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let p = last.pixel(
                        (2 * x + dx).min(last.width - 1),
                        (2 * y + dy).min(last.height - 1),
                    );
                    for c in 0..4 {
                        sum[c] += u32::from(p[c]);
                    }
                }
                next.set(x, y, sum.map(|s| (s / 4) as u8));
            }
        }
        levels.push(next);
    }
    levels
}

pub(crate) fn upload_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    image: &Image,
) -> wgpu::TextureView {
    let levels = mip_chain(image);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ground atlas"),
        size: wgpu::Extent3d {
            width: image.width,
            height: image.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (level, mip) in levels.iter().enumerate() {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: level as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &mip.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * mip.width),
                rows_per_image: Some(mip.height),
            },
            wgpu::Extent3d {
                width: mip.width,
                height: mip.height,
                depth_or_array_layers: 1,
            },
        );
    }
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// Images of one size as the layers of a texture array, each with its own
/// smaller copies.
pub(crate) fn upload_layers(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    images: &[Image],
) -> wgpu::TextureView {
    let (width, height) = images.first().map_or((1, 1), |i| (i.width, i.height));
    let chains: Vec<Vec<Image>> = images.iter().map(mip_chain).collect();
    let levels = chains.first().map_or(1, Vec::len) as u32;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ground cells"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: images.len().max(1) as u32,
        },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (layer, chain) in chains.iter().enumerate() {
        for (level, mip) in chain.iter().enumerate() {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: level as u32,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: layer as u32,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &mip.rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * mip.width),
                    rows_per_image: Some(mip.height),
                },
                wgpu::Extent3d {
                    width: mip.width,
                    height: mip.height,
                    depth_or_array_layers: 1,
                },
            );
        }
    }
    texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..wgpu::TextureViewDescriptor::default()
    })
}

pub fn depth_view(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

impl Renderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        scene: &Scene,
    ) -> Renderer {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ground"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("atlas"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
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
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ground"),
            bind_group_layouts: &[&globals_layout, &texture_layout],
            push_constant_ranges: &[],
        });
        // The full tiles of texture 0 are drawn opaque, everything else with
        // its alpha (SPEC-0012).
        let pipeline = |blend: Option<wgpu::BlendState>, entry: &str, depth_write: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("ground"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![
                            0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32x4,
                            4 => Uint32
                        ],
                    }],
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
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: depth_write,
                    // Later layers lie on the same triangles as earlier ones.
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let opaque = pipeline(None, "fs", true);
        let blended = pipeline(Some(wgpu::BlendState::ALPHA_BLENDING), "fs", true);
        // Water is see-through and drawn last, without hiding what lies
        // below it.
        let water_pipeline = pipeline(Some(wgpu::BlendState::ALPHA_BLENDING), "fs_water", false);
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("atlas"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            anisotropy_clamp: 16,
            ..wgpu::SamplerDescriptor::default()
        });
        let mut views = Vec::with_capacity(scene.textures.len());
        for texture in &scene.textures {
            views.push(
                texture
                    .as_ref()
                    .map(|image| upload_layers(device, queue, &atlas_cells(image))),
            );
        }
        let gpu_layer = |layer: &Layer, view: &wgpu::TextureView, sampler: &wgpu::Sampler| {
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("atlas"),
                layout: &texture_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                ],
            });
            GpuLayer {
                vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("ground vertices"),
                    contents: bytemuck::cast_slice(&layer.vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
                indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("ground indices"),
                    contents: bytemuck::cast_slice(&layer.indices),
                    usage: wgpu::BufferUsages::INDEX,
                }),
                count: layer.indices.len() as u32,
                bind_group,
                opaque: layer.texture == 0,
            }
        };
        let layers = scene
            .layers
            .iter()
            .filter_map(|layer| {
                let view = views.get(layer.texture)?.as_ref()?;
                Some(gpu_layer(layer, view, &sampler))
            })
            .collect();
        let repeat = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("water"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            anisotropy_clamp: 16,
            ..wgpu::SamplerDescriptor::default()
        });
        let water = scene.water.as_ref().map(|(layer, image)| {
            let view = upload_layers(device, queue, std::slice::from_ref(image));
            gpu_layer(layer, &view, &repeat)
        });
        let models = ModelRenderer::new(
            device,
            queue,
            format,
            &globals_layout,
            &scene.groups,
            &scene.model_textures,
            &scene.animation,
        );
        Renderer {
            opaque,
            blended,
            globals,
            globals_bind_group,
            layers,
            models,
            water_pipeline,
            water,
            overlay: Overlay::new(device, format),
            anchors: Vec::new(),
            selected: None,
        }
    }

    /// Poses the models `elapsed` milliseconds into their animations.
    pub fn animate(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, elapsed: i32) {
        self.models.update(device, queue, elapsed);
    }

    /// Places the units of the simulation, `elapsed` milliseconds into the
    /// animations; models of new kinds are read
    /// from `chain`.
    pub fn set_units(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        chain: &mut ow3_formats::mpq::ArchiveChain,
        units: &[crate::sim_units::UnitDraw],
        elapsed: i32,
    ) {
        self.anchors = self.models.set_units(device, queue, chain, units, elapsed);
    }

    /// Lays out the health bars of the units for a view of `size` pixels.
    pub fn prepare_overlay(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        camera: &Camera,
        size: (u32, u32),
    ) {
        let bars: Vec<Bar> = self
            .anchors
            .iter()
            .filter_map(|a| {
                Some(Bar {
                    at: [a.head[0], a.head[1], a.head[2] + 24.0],
                    fill: a.life?,
                    selected: self.selected == Some(a.handle),
                })
            })
            .collect();
        let aspect = size.0 as f32 / size.1.max(1) as f32;
        self.overlay
            .set(device, queue, &bars, &camera.view_projection(aspect), size);
    }

    /// The unit under pixel (`x`, `y`) of a view of `size` pixels: the one
    /// whose upright line from foot to head passes closest, within 24
    /// pixels.
    pub fn pick(&self, x: f32, y: f32, camera: &Camera, size: (u32, u32)) -> Option<u32> {
        let (w, h) = (size.0.max(1) as f32, size.1.max(1) as f32);
        let m = camera.view_projection(w / h);
        let screen = |p| {
            crate::overlay::project(&m, p).map(|[x, y]| [(x + 1.0) * w / 2.0, (1.0 - y) * h / 2.0])
        };
        let mut best: Option<(f32, u32)> = None;
        for a in &self.anchors {
            let (Some(foot), Some(head)) = (screen(a.foot), screen(a.head)) else {
                continue;
            };
            // The distance from the cursor to the segment foot-head.
            let (dx, dy) = (head[0] - foot[0], head[1] - foot[1]);
            let length = dx * dx + dy * dy;
            let t = if length > 0.0 {
                (((x - foot[0]) * dx + (y - foot[1]) * dy) / length).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let (px, py) = (foot[0] + dx * t, foot[1] + dy * t);
            let distance = ((x - px).powi(2) + (y - py).powi(2)).sqrt();
            if distance < 24.0 && best.is_none_or(|(d, _)| distance < d) {
                best = Some((distance, a.handle));
            }
        }
        best.map(|(_, handle)| handle)
    }

    /// Draws the ground seen by `camera` into `target`.
    pub fn draw(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        camera: &Camera,
        aspect: f32,
    ) {
        let light = {
            let l = [-0.4f32, 0.4, 0.82];
            let n = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt();
            [l[0] / n, l[1] / n, l[2] / n, 0.0]
        };
        let globals = Globals {
            view_projection: camera.view_projection(aspect),
            light,
            water: [0.02, 0.09, 0.2, 1.0],
        };
        queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("frame"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ground"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(CLEAR),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            pass.set_bind_group(0, &self.globals_bind_group, &[]);
            for layer in &self.layers {
                pass.set_pipeline(if layer.opaque {
                    &self.opaque
                } else {
                    &self.blended
                });
                pass.set_bind_group(1, &layer.bind_group, &[]);
                pass.set_vertex_buffer(0, layer.vertices.slice(..));
                pass.set_index_buffer(layer.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..layer.count, 0, 0..1);
            }
            pass.set_bind_group(0, &self.globals_bind_group, &[]);
            self.models.draw(&mut pass);
            if let Some(water) = &self.water {
                pass.set_pipeline(&self.water_pipeline);
                pass.set_bind_group(0, &self.globals_bind_group, &[]);
                pass.set_bind_group(1, &water.bind_group, &[]);
                pass.set_vertex_buffer(0, water.vertices.slice(..));
                pass.set_index_buffer(water.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..water.count, 0, 0..1);
            }
            self.overlay.draw(&mut pass);
        }
        queue.submit([encoder.finish()]);
    }
}

/// A device without a window: a hardware adapter if there is one, else a
/// software one.
pub async fn headless_device() -> Result<(wgpu::Device, wgpu::Queue), String> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
    let mut adapter = None;
    for fallback in [false, true] {
        let options = wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: fallback,
            compatible_surface: None,
        };
        if let Ok(found) = instance.request_adapter(&options).await {
            adapter = Some(found);
            break;
        }
    }
    let adapter = adapter.ok_or("no graphics adapter (Vulkan, Metal, DX12 or GL) found")?;
    adapter
        .request_device(&wgpu::DeviceDescriptor::default())
        .await
        .map_err(|e| e.to_string())
}

/// Draws the ground seen by `camera` into an image of `width` x `height`
/// pixels, without a window.
pub fn render_to_image(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &Renderer,
    camera: &Camera,
    width: u32,
    height: u32,
) -> Image {
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("offscreen"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let depth = depth_view(device, width, height);
    renderer.draw(
        device,
        queue,
        &view,
        &depth,
        camera,
        width as f32 / height as f32,
    );

    // Rows of a copy are padded to 256 bytes.
    let row = 4 * width;
    let padded = row.div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: u64::from(padded) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("readback"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |result| {
        result.expect("map the readback buffer");
    });
    device.poll(wgpu::PollType::Wait).expect("wait for the GPU");
    let data = slice.get_mapped_range();
    let mut image = Image::new(width, height);
    for y in 0..height as usize {
        let start = y * padded as usize;
        image.rgba[y * row as usize..(y + 1) * row as usize]
            .copy_from_slice(&data[start..start + row as usize]);
    }
    image
}
