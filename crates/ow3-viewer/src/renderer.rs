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
/// The terrain fog (SPEC-0017, "Terrain fog") for the shaders whose
/// `globals` have `fog_params` (style, start, end, density) and
/// `fog_color` (the colour, and 1 when there is fog): `fogged` mixes a
/// colour with the fog's by the depth of the point along the camera's
/// view.
pub(crate) const FOG_WGSL: &str = r#"
fn fog_amount(depth: f32) -> f32 {
    if globals.fog_color.w == 0.0 {
        return 0.0;
    }
    let style = globals.fog_params.x;
    if style < 0.5 {
        let span = max(globals.fog_params.z - globals.fog_params.y, 0.001);
        return clamp((depth - globals.fog_params.y) / span, 0.0, 1.0);
    }
    let d = globals.fog_params.w * depth;
    if style < 1.5 {
        return 1.0 - exp(-d);
    }
    return 1.0 - exp(-d * d);
}

fn fogged(colour: vec3<f32>, depth: f32) -> vec3<f32> {
    return mix(colour, globals.fog_color.rgb, fog_amount(depth));
}
"#;

/// The fog of war (SPEC-0032, "The world under fog") for the shaders that
/// declare `globals.vision_area`, `globals.vision_shades`, `vision_map`
/// and `shadow_sampler`: `seen` turns a colour into that of the state of
/// the fog cells round a world point. Fogged: `0.34 v` plus a little,
/// more in blue; masked: `vision_shades.x` of it (0.098 with the map flag
/// `0x0010`, else 0). The map holds one texel a cell at its centre (0
/// masked, 0.5 fogged, 1 visible), filtered linearly between cells, and
/// the colour is blended between the states' colours by it.
///
/// PROVISIONAL: the blend across cell edges is open (SPEC-0032 open
/// question 4).
pub(crate) const VISION_WGSL: &str = r#"
fn seen(colour: vec3<f32>, world: vec2<f32>) -> vec3<f32> {
    let uv = (world - globals.vision_area.xy) / (128.0 * globals.vision_area.zw);
    let s = textureSampleLevel(vision_map, shadow_sampler, uv, 0.0).r;
    let fogged = vec3<f32>(0.340, 0.337, 0.338) * colour
        + vec3<f32>(9.3, 9.2, 18.9) / 255.0;
    let masked = globals.vision_shades.x * colour;
    if s < 0.5 {
        return mix(masked, fogged, s * 2.0);
    }
    return mix(fogged, colour, s * 2.0 - 1.0);
}
"#;

/// The values of the three fog states in the vision map: masked, fogged,
/// visible.
pub const VISION_LEVELS: [u8; 3] = [0, 128, 255];
/// What the fog leaves of cells never seen with the map flag `0x0010`.
pub const MASKED_PARTLY: f32 = 0.098;

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
    // The frame of the water texture in x.
    water: vec4<f32>,
    // Fog: style, start, end, density; colour and 1 when there is fog.
    fog_params: vec4<f32>,
    fog_color: vec4<f32>,
    // The fog of war: the lower left corner of the map and the cells
    // across and up (SPEC-0032).
    vision_area: vec4<f32>,
    // What the fog leaves of cells never seen, in x.
    vision_shades: vec4<f32>,
};
@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var shadow_map: texture_2d<f32>;
@group(0) @binding(2) var shadow_sampler: sampler;
@group(0) @binding(3) var vision_map: texture_2d<f32>;
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
    // The light of the ground is computed at the corner points and
    // interpolated over the triangles (SPEC-0017, "Sloped ground").
    @location(0) light: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) tint: vec4<f32>,
    @location(3) @interpolate(flat) cell: u32,
    @location(4) world: vec2<f32>,
    // The depth along the camera's view, for the fog.
    @location(5) depth: f32,
};

@vertex
fn vs(v: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip = globals.view_projection * vec4<f32>(v.position, 1.0);
    let facing = max(dot(normalize(v.normal), globals.terrain_sun.xyz), 0.0);
    out.light = min(
        globals.terrain_sun.w * globals.terrain_color.rgb * facing
            + globals.terrain_color.w * globals.terrain_ambient.rgb,
        vec3<f32>(1.0),
    );
    out.uv = v.uv;
    out.tint = v.tint;
    out.cell = v.cell;
    out.world = v.position.xy;
    out.depth = out.clip.w;
    return out;
}

@fragment
fn fs(v: VertexOut) -> @location(0) vec4<f32> {
    let texel = shaded(textureSample(atlas, atlas_sampler, v.uv, v.cell), v.world);
    return vec4<f32>(fogged(seen(texel.rgb * v.light * v.tint.rgb, v.world), v.depth), texel.a);
}

@fragment
fn fs_water(v: VertexOut) -> @location(0) vec4<f32> {
    // SPEC-0014, "Colour and blending": the frame of the time, times the
    // colour of the depth and the light of the unit model for a normal
    // straight up, not clamped; its alpha blends it over the drawn ground.
    let texel = textureSample(atlas, atlas_sampler, v.uv, u32(globals.water.x));
    let light = globals.unit_sun.w * globals.unit_color.rgb * max(globals.unit_sun.z, 0.0)
        + globals.unit_color.w * globals.unit_ambient.rgb;
    return vec4<f32>(fogged(seen(texel.rgb * v.tint.rgb * light, v.world), v.depth), v.tint.a);
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
    water: [f32; 4],
    fog_params: [f32; 4],
    fog_color: [f32; 4],
    vision_area: [f32; 4],
    vision_shades: [f32; 4],
    /// The direction from the scene toward the camera, for billboards.
    camera_toward: [f32; 4],
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
    particles: crate::particles::Particles,
    water_pipeline: wgpu::RenderPipeline,
    water: Option<GpuLayer>,
    /// The type of the water, for its animation, and the frame shown.
    water_kind: Option<ow3_render::water::WaterType>,
    water_frame: u32,
    /// The fog for the shaders: style, start, end, density; colour and 1
    /// when there is fog.
    fog: ([f32; 4], [f32; 4]),
    overlay: Overlay,
    ui: crate::ui::UiRenderer,
    portrait: ModelView,
    /// The time of day indicator (SPEC-0028).
    time_indicator: ModelView,
    /// Selection circles, made when first needed.
    circles: Option<crate::circles::Circles>,
    format: wgpu::TextureFormat,
    /// The units placed last, for health bars and picking.
    anchors: Vec<UnitAnchor>,
    /// The unit picked with the mouse.
    pub selected: Option<u32>,
    /// All the units selected, which show their circles.
    pub selection: Vec<u32>,
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
    /// The fog of war one texel a cell ([`VISION_LEVELS`]), and the lower
    /// left corner of the map and the cells across and up.
    vision: wgpu::Texture,
    vision_area: [f32; 4],
    /// What the fog leaves of cells never seen (SPEC-0032).
    vision_masked: f32,
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
    let view = r8_texture(device, queue, width, height, &texels);
    let t = &scene.terrain;
    let area = [t.origin_x, t.origin_y, width as f32, height as f32];
    (view, area)
}

/// A texture of one byte per texel, rows from the bottom.
fn r8_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    width: u32,
    height: u32,
    texels: &[u8],
) -> wgpu::TextureView {
    r8_texture_of(device, queue, width, height, texels).1
}

/// Writes all texels of a one-byte texture, rows from the bottom.
fn write_r8(queue: &wgpu::Queue, texture: &wgpu::Texture, texels: &[u8]) {
    let size = texture.size();
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        texels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size.width),
            rows_per_image: Some(size.height),
        },
        size,
    );
}

/// [`r8_texture`] with the texture, to write it again later.
fn r8_texture_of(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    width: u32,
    height: u32,
    texels: &[u8],
) -> (wgpu::Texture, wgpu::TextureView) {
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
    write_r8(queue, &texture, texels);
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

/// One model of the interface drawn over the view through a projection of
/// its own, with globals of its own (plain light, no fog, no shadow): the
/// portrait of the selected unit, the time of day indicator (SPEC-0028).
struct ModelView {
    models: ModelRenderer,
    globals: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    /// The pixels it is drawn into: left, top, width, height; `None` when
    /// nothing is shown.
    rect: Option<[f32; 4]>,
}

impl ModelView {
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        globals_layout: &wgpu::BindGroupLayout,
    ) -> ModelView {
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("portrait globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let no_shadow = r8_texture(device, queue, 1, 1, &[0]);
        let all_seen = r8_texture(device, queue, 1, 1, &[255]);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("portrait globals"),
            layout: globals_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: globals.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&no_shadow),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&all_seen),
                },
            ],
        });
        let models = ModelRenderer::new(
            device,
            queue,
            format,
            globals_layout,
            &[],
            &std::collections::BTreeMap::new(),
            &crate::models::Animation::default(),
        );
        ModelView {
            models,
            globals,
            bind_group,
            rect: None,
        }
    }

    /// Poses `model`, a path read from `chain` and a team colour, in
    /// `pose`; its index among the models read, `None` if it cannot be
    /// read. Nothing is shown until [`ModelView::aim`].
    fn pose(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        chain: &mut ow3_formats::mpq::ArchiveChain,
        model: (&str, u8),
        pose: crate::models::UnitPose,
        elapsed: i32,
    ) -> Option<usize> {
        let (path, team) = model;
        self.rect = None;
        let draw = crate::sim_units::UnitDraw {
            replaceable: None,
            model: path.to_owned(),
            team,
            matrix: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            pose,
            life: None,
            handle: 0,
            selection: 0.0,
            relation: None,
            tint: [1.0; 3],
        };
        self.models
            .set_units(device, queue, chain, &[draw], elapsed);
        self.models.update(device, queue, elapsed);
        self.models.animation().index_of(path)
    }

    /// Shows the model through `view_projection` in the pixels `rect`
    /// (left, top, width, height).
    fn aim(
        &mut self,
        queue: &wgpu::Queue,
        view_projection: Mat4,
        toward: [f32; 3],
        rect: [f32; 4],
    ) {
        let light = PLAIN_LIGHT;
        let globals = Globals {
            view_projection,
            terrain_sun: [0.0; 4],
            terrain_color: [0.0; 4],
            terrain_ambient: [0.0; 4],
            unit_sun: [
                light.direction[0],
                light.direction[1],
                light.direction[2],
                light.intensity,
            ],
            unit_color: [
                light.color[0],
                light.color[1],
                light.color[2],
                light.ambient_intensity,
            ],
            unit_ambient: [light.ambient[0], light.ambient[1], light.ambient[2], 0.0],
            shadow_area: [0.0, 0.0, 1.0, 1.0],
            water: [0.0; 4],
            fog_params: [0.0; 4],
            fog_color: [0.0; 4],
            vision_area: [0.0, 0.0, 1.0, 1.0],
            vision_shades: [0.0; 4],
            camera_toward: [toward[0], toward[1], toward[2], 0.0],
        };
        queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));
        self.rect = Some(rect);
    }

    /// Draws the model if it is shown, its depth cleared first.
    fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        depth: &wgpu::TextureView,
    ) {
        let Some([x, y, w, h]) = self.rect else {
            return;
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("interface model"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
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
        pass.set_viewport(x, y, w, h, 0.0, 1.0);
        pass.set_scissor_rect(x as u32, y as u32, w as u32, h as u32);
        pass.set_bind_group(0, &self.bind_group, &[]);
        self.models.draw(&mut pass);
    }
}

/// The pixels (left, top, width, height) of interface rectangle `rect`
/// (left, bottom, right, top in 0.8 x 0.6 units) in a window `size`
/// pixels large.
fn pixels(rect: [f32; 4], size: (u32, u32)) -> [f32; 4] {
    let (w, h) = (size.0 as f32, size.1 as f32);
    [
        rect[0] / crate::ui::WIDTH * w,
        (1.0 - rect[3] / crate::ui::HEIGHT) * h,
        (rect[2] - rect[0]) / crate::ui::WIDTH * w,
        (rect[3] - rect[1]) / crate::ui::HEIGHT * h,
    ]
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
        format: wgpu::TextureFormat::Rgba8Unorm,
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
        format: wgpu::TextureFormat::Rgba8Unorm,
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
            source: wgpu::ShaderSource::Wgsl(
                format!("{SHADER}{SHADOW_WGSL}{FOG_WGSL}{VISION_WGSL}").into(),
            ),
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
                // The fog of war.
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
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
        // The fog of war, one texel a cell, all seen until a game sets it.
        let t = &scene.terrain;
        let cells = (
            t.width.saturating_sub(1).max(1),
            t.height.saturating_sub(1).max(1),
        );
        let (vision, vision_view) = r8_texture_of(
            device,
            queue,
            cells.0,
            cells.1,
            &vec![255; (cells.0 * cells.1) as usize],
        );
        let vision_area = [t.origin_x, t.origin_y, cells.0 as f32, cells.1 as f32];
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
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&vision_view),
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
        let water = scene.water.as_ref().map(|(layer, frames, _)| {
            let view = upload_layers(device, queue, frames);
            gpu_layer(layer, &view, &repeat)
        });
        let particles = crate::particles::Particles::new(
            device,
            queue,
            format,
            &scene.animation,
            &scene.emitters,
            &scene.particle_textures,
        );
        let models = ModelRenderer::new(
            device,
            queue,
            format,
            &globals_layout,
            &scene.groups,
            &scene.model_textures,
            &scene.animation,
        );
        let portrait = ModelView::new(device, queue, format, &globals_layout);
        let time_indicator = ModelView::new(device, queue, format, &globals_layout);
        Renderer {
            opaque,
            blended,
            globals,
            globals_bind_group,
            layers,
            models,
            particles,
            water_pipeline,
            water,
            water_kind: scene.water.as_ref().map(|(_, _, kind)| kind.clone()),
            water_frame: 0,
            fog: ([0.0; 4], [0.0; 4]),
            overlay: Overlay::new(device, format),
            ui: crate::ui::UiRenderer::new(device, format),
            portrait,
            time_indicator,
            circles: None,
            format,
            anchors: Vec::new(),
            selected: None,
            selection: Vec::new(),
            hovered: None,
            all_bars: false,
            terrain_light: PLAIN_LIGHT,
            unit_light: PLAIN_LIGHT,
            shadow_area,
            vision,
            vision_area,
            vision_masked: 0.0,
        }
    }

    /// Moves the particles to `elapsed` milliseconds, facing `camera`.
    pub fn prepare_particles(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        camera: &Camera,
        elapsed: i32,
    ) {
        self.particles
            .update(device, queue, self.models.animation(), camera, elapsed);
    }

    /// The terrain fog the script set (SPEC-0017), or none.
    pub fn set_fog(&mut self, fog: Option<ow3_sim::game::TerrainFog>) {
        self.fog = fog.map_or(([0.0; 4], [0.0; 4]), |f| {
            (
                [f.style as f32, f.start, f.end, f.density],
                [f.color[0], f.color[1], f.color[2], 1.0],
            )
        });
        self.particles.set_fog(self.fog);
    }

    /// The water shows the frame of game time `seconds` (SPEC-0014).
    pub fn set_game_time(&mut self, seconds: f32) {
        if let Some(kind) = &self.water_kind {
            self.water_frame = ow3_render::water::frame(kind, seconds);
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
        let emitters = self.models.unit_emitters(chain, units, elapsed);
        self.particles
            .set_units(device, queue, chain, self.models.animation(), &emitters);
    }

    /// Hides the placed models `hidden` (group, instance), drawn with the
    /// units in a pose of their own instead.
    pub fn hide_statics(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        hidden: std::collections::BTreeSet<(usize, usize)>,
    ) {
        self.models.hide_statics(device, queue, hidden);
    }

    /// Lays out the health bars of the units for a view of `size` pixels.
    /// Lays out the interface `frame` over the view for a window `size`
    /// pixels large.
    pub fn prepare_ui(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        chain: &mut ow3_formats::mpq::ArchiveChain,
        frame: &crate::ui::Frame,
        size: (u32, u32),
    ) {
        self.ui.set(device, queue, chain, frame, size);
    }

    /// Shows `model` (read from `chain`, in the colours of `team`) in the
    /// portrait frame of the interface (SPEC-0028) of a window `size` pixels
    /// large, `elapsed` milliseconds into its Portrait sequence; `None`
    /// shows nothing.
    pub fn set_portrait(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        chain: &mut ow3_formats::mpq::ArchiveChain,
        model: Option<(&str, u8)>,
        size: (u32, u32),
        elapsed: i32,
    ) {
        self.portrait.rect = None;
        let Some((path, team)) = model else {
            return;
        };
        let pose = crate::models::UnitPose::Portrait(elapsed);
        let Some(index) = self
            .portrait
            .pose(device, queue, chain, (path, team), pose, elapsed)
        else {
            return;
        };
        let Some(camera) = self
            .portrait
            .models
            .animation()
            .mdx(index)
            .cameras
            .first()
            .cloned()
        else {
            return;
        };
        let rect = pixels(crate::hud::PORTRAIT, size);
        if rect[2] < 1.0 || rect[3] < 1.0 {
            return;
        }
        let view_projection = crate::camera::model_view_projection(
            camera.position,
            camera.target,
            camera.field_of_view,
            camera.near,
            camera.far,
            rect[2] / rect[3],
        );
        let toward = {
            let (p, t) = (camera.position, camera.target);
            let d = [p[0] - t[0], p[1] - t[1], p[2] - t[2]];
            let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-6);
            [d[0] / n, d[1] / n, d[2] / n]
        };
        self.portrait.aim(queue, view_projection, toward, rect);
    }

    /// Shows the time of day indicator `model` (SPEC-0028) over the
    /// interface of a window `size` pixels large: the model's coordinates
    /// are those of the interface (0.8 x 0.6, larger z nearer), and it
    /// stands `time` milliseconds into its Stand sequence.
    pub fn set_time_indicator(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        chain: &mut ow3_formats::mpq::ArchiveChain,
        model: Option<&str>,
        size: (u32, u32),
        time: i32,
    ) {
        self.time_indicator.rect = None;
        let Some(path) = model else {
            return;
        };
        let pose = crate::models::UnitPose::Once(time);
        if self
            .time_indicator
            .pose(device, queue, chain, (path, 0), pose, time)
            .is_none()
        {
            return;
        }
        let screen = [0.0, 0.0, crate::ui::WIDTH, crate::ui::HEIGHT];
        let rect = pixels(screen, size);
        // x and y to clip space over the interface, z to depth: 1 - z.
        let view_projection = [
            [2.0 / crate::ui::WIDTH, 0.0, 0.0, 0.0],
            [0.0, 2.0 / crate::ui::HEIGHT, 0.0, 0.0],
            [0.0, 0.0, -1.0, 0.0],
            [-1.0, -1.0, 1.0, 1.0],
        ];
        // Its coordinates are those of the interface, larger z nearer.
        self.time_indicator
            .aim(queue, view_projection, [0.0, 0.0, 1.0], rect);
    }

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
        // Across the screen: the camera's right on the ground.
        let yaw = camera.yaw.to_radians();
        let right = [yaw.sin(), -yaw.cos(), 0.0];
        self.overlay
            .set(device, queue, &bars, &camera.view_projection(), size, right);
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
                    && (self.selected == Some(a.handle)
                        || self.selection.contains(&a.handle)
                        || self.hovered == Some(a.handle))
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
            &camera.view_projection(),
            ground,
        );
    }

    /// The units whose feet are within the pixels from `a` to `b` (any two
    /// corners) of a view of `size` pixels, in the order they were placed.
    pub fn units_in(
        &self,
        a: (f32, f32),
        b: (f32, f32),
        camera: &Camera,
        size: (u32, u32),
    ) -> Vec<u32> {
        let (w, h) = (size.0.max(1) as f32, size.1.max(1) as f32);
        let m = camera.view_projection();
        let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
        let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
        self.anchors
            .iter()
            .filter(|a| a.handle != 0 && a.life.is_some())
            .filter(|a| {
                crate::overlay::project(&m, a.foot).is_some_and(|[x, y]| {
                    let (px, py) = ((x + 1.0) * w / 2.0, (1.0 - y) * h / 2.0);
                    px >= x0 && px <= x1 && py >= y0 && py <= y1
                })
            })
            .map(|a| a.handle)
            .collect()
    }

    /// The units on the screen: those whose feet are in the view of the
    /// world (SPEC-0028), for a double click.
    pub fn units_on_screen(&self, camera: &Camera, size: (u32, u32)) -> Vec<u32> {
        let h = size.1 as f32;
        let world = crate::hud::WORLD;
        let top = (1.0 - world[3] / crate::ui::HEIGHT) * h;
        let bottom = (1.0 - world[1] / crate::ui::HEIGHT) * h;
        self.units_in((0.0, top), (size.0 as f32, bottom), camera, size)
    }

    /// The unit under pixel (`x`, `y`) of a view of `size` pixels: the one
    /// whose upright line from foot to head passes closest, within 24
    /// pixels or the radius of its selection circle on the screen, the
    /// distance taken relative to that reach (so a peasant next to a town
    /// hall is still picked near its own line).
    pub fn pick(&self, x: f32, y: f32, camera: &Camera, size: (u32, u32)) -> Option<u32> {
        let (w, h) = (size.0.max(1) as f32, size.1.max(1) as f32);
        let m = camera.view_projection();
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
            let radius = a.selection * crate::overlay::WIDTH_PER_SCALE / 2.0;
            let side = [a.foot[0] + radius, a.foot[1], a.foot[2]];
            let reach = screen(side).map_or(0.0, |s| {
                ((s[0] - foot[0]).powi(2) + (s[1] - foot[1]).powi(2)).sqrt()
            });
            let reach = reach.max(24.0);
            let score = distance / reach;
            if score < 1.0 && best.is_none_or(|(d, _)| score < d) {
                best = Some((score, a.handle));
            }
        }
        best.map(|(_, handle)| handle)
    }

    /// Shows the fog of war: the state of each cell (masked 0, fogged 1,
    /// visible 2, rows from the bottom), or with `None` all seen; cells
    /// never seen keep `masked` of their colour.
    pub fn set_vision(&mut self, queue: &wgpu::Queue, cells: Option<&[u8]>, masked: f32) {
        self.vision_masked = masked;
        let size = self.vision.size();
        let n = (size.width * size.height) as usize;
        let texels: Vec<u8> = match cells {
            Some(c) if c.len() == n => c
                .iter()
                .map(|&s| VISION_LEVELS[usize::from(s.min(2))])
                .collect(),
            _ => vec![255; n],
        };
        write_r8(queue, &self.vision, &texels);
    }

    /// Draws the ground seen by `camera` into `target`.
    pub fn draw(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        camera: &Camera,
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
            view_projection: camera.view_projection(),
            terrain_sun,
            terrain_color,
            terrain_ambient,
            unit_sun,
            unit_color,
            unit_ambient,
            shadow_area: self.shadow_area,
            water: [self.water_frame as f32, 0.0, 0.0, 0.0],
            fog_params: self.fog.0,
            fog_color: self.fog.1,
            vision_area: self.vision_area,
            vision_shades: [self.vision_masked, 0.0, 0.0, 0.0],
            camera_toward: {
                let (eye, at) = (camera.eye(), camera.target);
                let d = [eye[0] - at[0], eye[1] - at[1], eye[2] - at[2]];
                let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-6);
                [d[0] / n, d[1] / n, d[2] / n, 0.0]
            },
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
            // Particles over the water: the foam of the shores lies on it.
            self.particles.draw(&mut pass);
            self.overlay.draw(&mut pass);
            self.ui.draw(&mut pass, false);
        }
        self.portrait.draw(&mut encoder, target, depth);
        // The console over the portrait.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("interface"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.ui.draw(&mut pass, true);
        }
        // The time of day indicator over the console.
        self.time_indicator.draw(&mut encoder, target, depth);
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
    let format = wgpu::TextureFormat::Rgba8Unorm;
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
    renderer.draw(device, queue, &view, &depth, camera);

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
