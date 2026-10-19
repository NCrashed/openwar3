//! The game's interface drawn over the view: textured rectangles and text
//! in the interface's coordinates, 0.8 wide and 0.6 high from the lower
//! left corner of the window, stretched over the whole window.

use std::collections::HashMap;

use ab_glyph::{Font, FontVec, PxScale, ScaleFont};
use bytemuck::{Pod, Zeroable};
use ow3_formats::mpq::ArchiveChain;
use ow3_render::image::Image;

use crate::renderer::{DEPTH_FORMAT, upload_texture};

/// The size of the interface's coordinates.
pub const WIDTH: f32 = 0.8;
pub const HEIGHT: f32 = 0.6;

const SHADER: &str = r#"
struct VertexIn {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) colour: vec4<f32>,
};
struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) colour: vec4<f32>,
};

@group(0) @binding(0) var image: texture_2d<f32>;
@group(0) @binding(1) var image_sampler: sampler;

@vertex
fn vs(v: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip = vec4<f32>(v.position, 0.0, 1.0);
    out.uv = v.uv;
    out.colour = v.colour;
    return out;
}

@fragment
fn fs(v: VertexOut) -> @location(0) vec4<f32> {
    return textureSample(image, image_sampler, v.uv) * v.colour;
}

@fragment
fn fs_key(v: VertexOut) -> @location(0) vec4<f32> {
    let c = textureSample(image, image_sampler, v.uv) * v.colour;
    if (c.a < 0.75) {
        discard;
    }
    return vec4<f32>(c.rgb, 1.0);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
struct Vertex {
    position: [f32; 2],
    uv: [f32; 2],
    colour: [f32; 4],
}

/// How a rectangle mixes with what is under it (`AlphaMode` of the
/// interface files).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Blend {
    /// By its alpha.
    #[default]
    Alpha,
    /// Only where its alpha is high, opaque there (`ALPHAKEY`).
    Key,
    /// Added (`ADD`).
    Add,
}

/// A textured rectangle.
#[derive(Clone, Debug, PartialEq)]
pub struct Quad {
    /// Left, bottom, right, top in interface coordinates.
    pub rect: [f32; 4],
    /// The texture coordinates as `TexCoord` gives them: left, right, top,
    /// bottom.
    pub uv: [f32; 4],
    /// The texture's file in the archives.
    pub texture: String,
    pub colour: [f32; 4],
    pub blend: Blend,
}

/// Horizontal placement of a text in its box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Justify {
    #[default]
    Left,
    Centre,
    Right,
}

/// A line of text in a box, centred vertically.
#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    /// Left, bottom, right, top of its box in interface coordinates.
    pub rect: [f32; 4],
    /// The font's size in interface coordinates and its file.
    pub size: f32,
    pub font: String,
    pub text: String,
    pub colour: [f32; 4],
    pub justify: Justify,
}

/// What the interface shows in a frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frame {
    pub quads: Vec<Quad>,
    pub texts: Vec<Text>,
}

/// What a draw call binds.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Bind {
    Texture(String),
    Glyphs,
}

const ATLAS: u32 = 1024;

/// A glyph in the atlas: its rectangle there and its offset from the pen.
type Placed = ([u32; 4], [f32; 2]);

/// Glyphs drawn into one texture, row by row.
struct Atlas {
    pixels: Vec<u8>,
    /// Where each glyph (font, glyph id, pixel size) is: its rectangle in
    /// the atlas and its offset from the pen.
    glyphs: HashMap<(String, u16, u32), Option<Placed>>,
    x: u32,
    y: u32,
    row: u32,
    dirty: bool,
}

impl Atlas {
    fn new() -> Atlas {
        Atlas {
            pixels: vec![0; (ATLAS * ATLAS * 4) as usize],
            glyphs: HashMap::new(),
            x: 0,
            y: 0,
            row: 0,
            dirty: true,
        }
    }

    /// The place of a glyph of `font` at `px` pixels, drawn first if new.
    fn glyph(
        &mut self,
        font_name: &str,
        font: &FontVec,
        id: ab_glyph::GlyphId,
        px: u32,
    ) -> Option<Placed> {
        let key = (font_name.to_owned(), id.0, px);
        if let Some(found) = self.glyphs.get(&key) {
            return *found;
        }
        let glyph = id.with_scale_and_position(PxScale::from(px as f32), ab_glyph::point(0.0, 0.0));
        let placed = font.outline_glyph(glyph).and_then(|outline| {
            let bounds = outline.px_bounds();
            let (w, h) = (bounds.width() as u32 + 1, bounds.height() as u32 + 1);
            if self.x + w > ATLAS {
                self.x = 0;
                self.y += self.row + 1;
                self.row = 0;
            }
            if self.y + h > ATLAS {
                return None;
            }
            let (ox, oy) = (self.x, self.y);
            outline.draw(|x, y, c| {
                let i = (((oy + y) * ATLAS + ox + x) * 4) as usize;
                self.pixels[i..i + 4].copy_from_slice(&[255, 255, 255, (c * 255.0) as u8]);
            });
            self.x += w + 1;
            self.row = self.row.max(h);
            self.dirty = true;
            Some(([ox, oy, w, h], [bounds.min.x, bounds.min.y]))
        });
        self.glyphs.insert(key, placed);
        placed
    }
}

pub struct UiRenderer {
    alpha: wgpu::RenderPipeline,
    key: wgpu::RenderPipeline,
    add: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    textures: HashMap<String, Option<wgpu::BindGroup>>,
    fonts: HashMap<String, Option<FontVec>>,
    atlas: Atlas,
    glyphs: Option<(wgpu::Texture, wgpu::BindGroup)>,
    vertices: wgpu::Buffer,
    capacity: usize,
    draws: Vec<(Bind, Blend, std::ops::Range<u32>)>,
}

fn vertex_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("interface vertices"),
        size: (capacity * std::mem::size_of::<Vertex>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

impl UiRenderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> UiRenderer {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("interface"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("interface texture"),
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
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("interface"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = |entry: &str, blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("interface"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![
                            0 => Float32x2, 1 => Float32x2, 2 => Float32x4
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
                // Over everything: the depth is kept but not tested.
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: false,
                    depth_compare: wgpu::CompareFunction::Always,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let additive = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::OVER,
        };
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("interface"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let capacity = 4096;
        UiRenderer {
            alpha: pipeline("fs", Some(wgpu::BlendState::ALPHA_BLENDING)),
            key: pipeline("fs_key", None),
            add: pipeline("fs", Some(additive)),
            layout,
            sampler,
            textures: HashMap::new(),
            fonts: HashMap::new(),
            atlas: Atlas::new(),
            glyphs: None,
            vertices: vertex_buffer(device, capacity),
            capacity,
            draws: Vec::new(),
        }
    }

    fn bind(&self, device: &wgpu::Device, view: &wgpu::TextureView) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("interface texture"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }

    /// Whether texture `path` can be drawn, read from `chain` the first time.
    fn texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        chain: &mut ArchiveChain,
        path: &str,
    ) -> bool {
        if !self.textures.contains_key(path) {
            let image = read_image(chain, path);
            let bind = image.map(|image| {
                let view = upload_texture(device, queue, &image);
                self.bind(device, &view)
            });
            self.textures.insert(path.to_owned(), bind);
        }
        matches!(self.textures.get(path), Some(Some(_)))
    }

    /// Lays out `frame` for the next frame of a window `size` pixels large.
    pub fn set(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        chain: &mut ArchiveChain,
        frame: &Frame,
        size: (u32, u32),
    ) {
        let mut vertices: Vec<Vertex> = Vec::new();
        self.draws.clear();
        let clip = |x: f32, y: f32| [x / WIDTH * 2.0 - 1.0, y / HEIGHT * 2.0 - 1.0];
        for quad in &frame.quads {
            if !self.texture(device, queue, chain, &quad.texture) {
                continue;
            }
            let start = vertices.len() as u32;
            let [x0, y0, x1, y1] = quad.rect;
            let [u0, u1, v0, v1] = quad.uv;
            let corners = [
                (x0, y0, u0, v1),
                (x1, y0, u1, v1),
                (x1, y1, u1, v0),
                (x0, y0, u0, v1),
                (x1, y1, u1, v0),
                (x0, y1, u0, v0),
            ];
            vertices.extend(corners.map(|(x, y, u, v)| Vertex {
                position: clip(x, y),
                uv: [u, v],
                colour: quad.colour,
            }));
            self.draws.push((
                Bind::Texture(quad.texture.clone()),
                quad.blend,
                start..vertices.len() as u32,
            ));
        }
        // Text: glyphs at whole pixel sizes, placed in pixels.
        let (w, h) = (size.0.max(1) as f32, size.1.max(1) as f32);
        let start = vertices.len() as u32;
        for text in &frame.texts {
            if !self.fonts.contains_key(&text.font) {
                let font = chain
                    .read(&text.font)
                    .and_then(|d| FontVec::try_from_vec(d).ok());
                self.fonts.insert(text.font.clone(), font);
            }
            let Some(Some(font)) = self.fonts.get(&text.font) else {
                continue;
            };
            let px = (text.size / HEIGHT * h).round().max(1.0) as u32;
            let scaled = font.as_scaled(PxScale::from(px as f32));
            let ids: Vec<ab_glyph::GlyphId> = text.text.chars().map(|c| font.glyph_id(c)).collect();
            let mut width = 0.0;
            for (i, &id) in ids.iter().enumerate() {
                if i > 0 {
                    width += scaled.kern(ids[i - 1], id);
                }
                width += scaled.h_advance(id);
            }
            let [x0, y0, x1, y1] = text.rect;
            let (left, right) = (x0 / WIDTH * w, x1 / WIDTH * w);
            let mut pen = match text.justify {
                Justify::Left => left,
                Justify::Right => right - width,
                Justify::Centre => (left + right - width) / 2.0,
            }
            .round();
            // The baseline, the line's middle on the box's middle.
            let middle = (1.0 - (y0 + y1) / 2.0 / HEIGHT) * h;
            let baseline = (middle + (scaled.ascent() + scaled.descent()) / 2.0).round();
            for (i, &id) in ids.iter().enumerate() {
                if i > 0 {
                    pen += scaled.kern(ids[i - 1], id);
                }
                if let Some(([ax, ay, aw, ah], [ox, oy])) =
                    self.atlas.glyph(&text.font, font, id, px)
                {
                    let (gx, gy) = (pen + ox, baseline + oy);
                    let to_clip = |x: f32, y: f32| [x / w * 2.0 - 1.0, 1.0 - y / h * 2.0];
                    let uv = |x: u32, y: u32| [x as f32 / ATLAS as f32, y as f32 / ATLAS as f32];
                    let corners = [
                        (gx, gy + ah as f32, uv(ax, ay + ah)),
                        (gx + aw as f32, gy + ah as f32, uv(ax + aw, ay + ah)),
                        (gx + aw as f32, gy, uv(ax + aw, ay)),
                        (gx, gy + ah as f32, uv(ax, ay + ah)),
                        (gx + aw as f32, gy, uv(ax + aw, ay)),
                        (gx, gy, uv(ax, ay)),
                    ];
                    vertices.extend(corners.map(|(x, y, uv)| Vertex {
                        position: to_clip(x, y),
                        uv,
                        colour: text.colour,
                    }));
                }
                pen += scaled.h_advance(id);
            }
        }
        if vertices.len() as u32 > start {
            self.upload_glyphs(device, queue);
            self.draws
                .push((Bind::Glyphs, Blend::Alpha, start..vertices.len() as u32));
        }
        if vertices.len() > self.capacity {
            self.capacity = vertices.len() * 2;
            self.vertices = vertex_buffer(device, self.capacity);
        }
        queue.write_buffer(&self.vertices, 0, bytemuck::cast_slice(&vertices));
    }

    /// Puts the glyph atlas on the device when it has changed.
    fn upload_glyphs(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        if self.glyphs.is_none() {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("glyphs"),
                size: wgpu::Extent3d {
                    width: ATLAS,
                    height: ATLAS,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let bind = self.bind(device, &view);
            self.glyphs = Some((texture, bind));
        }
        if !std::mem::take(&mut self.atlas.dirty) {
            return;
        }
        let Some((texture, _)) = &self.glyphs else {
            return;
        };
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &self.atlas.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * ATLAS),
                rows_per_image: Some(ATLAS),
            },
            wgpu::Extent3d {
                width: ATLAS,
                height: ATLAS,
                depth_or_array_layers: 1,
            },
        );
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass) {
        if self.draws.is_empty() {
            return;
        }
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        for (bind, blend, range) in &self.draws {
            let group = match bind {
                Bind::Texture(path) => self.textures.get(path).and_then(Option::as_ref),
                Bind::Glyphs => self.glyphs.as_ref().map(|(_, b)| b),
            };
            let Some(group) = group else {
                continue;
            };
            pass.set_pipeline(match blend {
                Blend::Alpha => &self.alpha,
                Blend::Key => &self.key,
                Blend::Add => &self.add,
            });
            pass.set_bind_group(0, group, &[]);
            pass.draw(range.clone(), 0..1);
        }
    }
}

/// The image of a BLP texture file, tried with and without the `.blp`
/// extension (the interface files name textures without it).
fn read_image(chain: &mut ArchiveChain, path: &str) -> Option<Image> {
    let names = if path.to_ascii_lowercase().ends_with(".blp") {
        vec![path.to_owned()]
    } else {
        vec![format!("{path}.blp"), path.to_owned()]
    };
    names.into_iter().find_map(|name| {
        let data = chain.read(&name)?;
        ow3_formats::blp::decode(&data).ok().map(Image::from)
    })
}
