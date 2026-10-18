//! Drawing the ground with wgpu, to a window's surface or to an image.

use bytemuck::{Pod, Zeroable};
use ow3_render::image::Image;
use wgpu::util::DeviceExt;

use crate::camera::{Camera, Mat4};
use crate::model_renderer::{ModelRenderer, UnitAnchor};
use crate::overlay::{Bar, Overlay};
use crate::scene::Scene;
use crate::terrain_mesh::{Layer, Vertex, atlas_cells};
use ow3_render::daylight::Lighting;

pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Colour of the sky behind the map.
const CLEAR: wgpu::Color = wgpu::Color {
    r: 0.02,
    g: 0.02,
    b: 0.03,
    a: 1.0,
};

/// The baked shadows of `war3map.shd` (SPEC-0017, "Shadows") for the
/// shaders that declare `globals.shadow_area`, `shadow_map` and
/// `shadow_sampler`: `shaded` moves a texture colour towards the colour
/// of shadow by the shadow amount at a world point, before the light.
/// The map holds the samples of `ow3_render::shadows`, block by block,
/// and linear filtering between the samples of a block interpolates them.
pub(crate) const SHADOW_WGSL: &str = r#"
const SHADOW_COLOUR: vec3<f32> = vec3<f32>(0.048, 0.048, 0.095);
const SHADOW_AMOUNT: f32 = 0.502;

fn shaded(texel: vec4<f32>, world: vec2<f32>) -> vec4<f32> {
    let offset = world - globals.shadow_area.xy;
    let block = floor(offset / 512.0);
    let samples = block * 16.0 + (offset - block * 512.0) * (15.0 / 512.0);
    let uv = (samples + 0.5) / globals.shadow_area.zw;
    let s = textureSampleLevel(shadow_map, shadow_sampler, uv, 0.0).r;
    return vec4<f32>(texel.rgb + s * SHADOW_AMOUNT * (SHADOW_COLOUR - texel.rgb), texel.a);
}
"#;

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
    // The shadow map: the lower left corner of the map, and the size of
    // the sample grid.
    shadow_area: vec4<f32>,
};
@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var shadow_map: texture_2d<f32>;
@group(0) @binding(2) var shadow_sampler: sampler;
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
    @location(4) world: vec2<f32>,
};

@vertex
fn vs(v: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip = globals.view_projection * vec4<f32>(v.position, 1.0);
    out.normal = v.normal;
    out.uv = v.uv;
    out.tint = v.tint;
    out.cell = v.cell;
    out.world = v.position.xy;
    return out;
}

@fragment
fn fs(v: VertexOut) -> @location(0) vec4<f32> {
    let texel = shaded(textureSample(atlas, atlas_sampler, v.uv, v.cell), v.world);
    let n = normalize(v.normal);
    let facing = max(dot(n, globals.terrain_sun.xyz), 0.0);
    let light = min(
        globals.terrain_sun.w * globals.terrain_color.rgb * facing
            + globals.terrain_color.w * globals.terrain_ambient.rgb,
        vec3<f32>(1.0),
    );
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
    terrain_sun: [f32; 4],
    terrain_color: [f32; 4],
    terrain_ambient: [f32; 4],
    unit_sun: [f32; 4],
    unit_color: [f32; 4],
    unit_ambient: [f32; 4],
    shadow_area: [f32; 4],
}

/// The light when no day/night model can be read: a white sun from the
/// south west at 55 degrees, half of it ambient.
pub const PLAIN_LIGHT: Lighting = Lighting {
    direction: [-0.4, -0.4, 0.82],
    color: [1.0, 1.0, 1.0],
    intensity: 0.5,
    ambient: [1.0, 1.0, 1.0],
    ambient_intensity: 0.55,
};

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
    /// Selection circles, made when first needed.
    circles: Option<crate::circles::Circles>,
    format: wgpu::TextureFormat,
    /// The units placed last, for health bars and picking.
    anchors: Vec<UnitAnchor>,
    /// The unit picked with the mouse.
    pub selected: Option<u32>,
    /// The unit under the mouse, which shows its circle and bar.
    pub hovered: Option<u32>,
    /// Alt is held: every unit shows its bar.
    pub all_bars: bool,
    /// The light of the ground and of models (SPEC-0017).
    pub terrain_light: Lighting,
    pub unit_light: Lighting,
    /// The shadow map: the lower left corner of the map and the size of
    /// the sample grid.
    shadow_area: [f32; 4],
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

/// The shadow samples of `scene` (`ow3_render::shadows`) as a one-channel
/// texture, and the lower left corner of the map and the grid size.
fn shadow_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &Scene,
) -> (wgpu::TextureView, [f32; 4]) {
    let (width, height, texels) = match &scene.shadows {
        Some(s) => {
            let (width, height, values) = ow3_render::shadows::samples(s);
            let texels = values.iter().map(|v| (v * 255.0).round() as u8).collect();
            (width, height, texels)
        }
        None => (1, 1, vec![0u8]),
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("shadows"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &texels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    let t = &scene.terrain;
    let area = [t.origin_x, t.origin_y, width as f32, height as f32];
    (
        texture.create_view(&wgpu::TextureViewDescriptor::default()),
        area,
    )
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
            source: wgpu::ShaderSource::Wgsl(format!("{SHADER}{SHADOW_WGSL}").into()),
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // The baked shadows of the ground.
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
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
        // The shadow samples, block by block, rows from the bottom as in
        // the file; without a shadow map, a single texel without shadow.
        let (shadow_view, shadow_area) = shadow_texture(device, queue, scene);
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadows"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..wgpu::SamplerDescriptor::default()
        });
        let globals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &globals_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: globals.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&shadow_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&shadow_sampler),
                },
            ],
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
            circles: None,
            format,
            anchors: Vec::new(),
            selected: None,
            hovered: None,
            all_bars: false,
            terrain_light: PLAIN_LIGHT,
            unit_light: PLAIN_LIGHT,
            shadow_area,
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
        // As in the game: every bar while Alt is held, otherwise only the
        // one of the unit under the mouse.
        let bars: Vec<Bar> = self
            .anchors
            .iter()
            .filter(|a| self.all_bars || self.hovered == Some(a.handle))
            .filter_map(|a| {
                Some(Bar {
                    at: a.head,
                    fill: a.life?,
                    width: a.selection * crate::overlay::WIDTH_PER_SCALE,
                })
            })
            .collect();
        let aspect = size.0 as f32 / size.1.max(1) as f32;
        // Across the screen: the camera's right on the ground.
        let yaw = camera.yaw.to_radians();
        let right = [yaw.sin(), -yaw.cos(), 0.0];
        self.overlay.set(
            device,
            queue,
            &bars,
            &camera.view_projection(aspect),
            size,
            right,
        );
    }

    /// Lays out the selection circles of the selected unit and the one under
    /// the mouse on the ground of height `ground(x, y)`, their textures read
    /// from `chain`.
    pub fn prepare_circles(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        chain: &mut ow3_formats::mpq::ArchiveChain,
        camera: &Camera,
        aspect: f32,
        ground: impl Fn(f32, f32) -> f32,
    ) {
        use crate::circles::{Circle, Circles, Settings};
        use ow3_sim::game::Relation;
        let format = self.format;
        let circles = self
            .circles
            .get_or_insert_with(|| Circles::new(device, format, Settings::load(chain)));
        let settings = circles.settings();
        let list: Vec<Circle> = self
            .anchors
            .iter()
            .filter(|a| {
                a.life.is_some()
                    && (self.selected == Some(a.handle) || self.hovered == Some(a.handle))
            })
            .map(|a| Circle {
                centre: [a.foot[0], a.foot[1]],
                scale: a.selection,
                colour: match a.relation {
                    Some(Relation::Friend) => settings.friend,
                    Some(Relation::Enemy) => settings.enemy,
                    _ => settings.neutral,
                },
            })
            .collect();
        circles.set(
            device,
            queue,
            chain,
            &list,
            &camera.view_projection(aspect),
            ground,
        );
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
        // Handle 0: effects, which cannot be picked.
        for a in self.anchors.iter().filter(|a| a.handle != 0) {
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
        let parts = |l: &Lighting| {
            [
                [l.direction[0], l.direction[1], l.direction[2], l.intensity],
                [l.color[0], l.color[1], l.color[2], l.ambient_intensity],
                [l.ambient[0], l.ambient[1], l.ambient[2], 0.0],
            ]
        };
        let [terrain_sun, terrain_color, terrain_ambient] = parts(&self.terrain_light);
        let [unit_sun, unit_color, unit_ambient] = parts(&self.unit_light);
        let globals = Globals {
            view_projection: camera.view_projection(aspect),
            terrain_sun,
            terrain_color,
            terrain_ambient,
            unit_sun,
            unit_color,
            unit_ambient,
            shadow_area: self.shadow_area,
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
            if let Some(circles) = &self.circles {
                circles.draw(&mut pass);
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
