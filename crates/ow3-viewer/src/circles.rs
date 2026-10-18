//! Selection circles on the ground under selected units.
//!
//! The settings come from `[SelectionCircle]` of `UI\MiscData.txt`: a
//! circle is `ScaleFactor` times the unit's selection scale across, drawn
//! with the texture of the first size class (`Size##`) it fits in, in the
//! colour of the unit's owner as the local player sees it (`ColorFriend`,
//! `ColorNeutral`, `ColorEnemy`). The texture lies on a grid that follows
//! the ground, a little above it.

use bytemuck::{Pod, Zeroable};
use ow3_formats::mpq::ArchiveChain;
use ow3_render::image::Image;

use crate::camera::Mat4;
use crate::renderer::{DEPTH_FORMAT, upload_texture};

const SHADER: &str = r#"
struct Globals {
    view_projection: mat4x4<f32>,
};
@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var image: texture_2d<f32>;
@group(1) @binding(1) var image_sampler: sampler;

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
    return textureSample(image, image_sampler, v.uv) * v.colour;
}
"#;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    uv: [f32; 2],
    colour: [f32; 4],
}

/// Cells of the grid along each side of a circle.
const GRID: usize = 8;
/// World units between the ground and a circle.
const LIFT: f32 = 2.0;

/// A circle to draw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Circle {
    pub centre: [f32; 2],
    /// The unit's selection scale.
    pub scale: f32,
    pub colour: [f32; 4],
}

/// The settings of `[SelectionCircle]`.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// The largest size of each class but the last, and the texture of
    /// every class.
    pub sizes: Vec<f32>,
    pub textures: Vec<String>,
    pub scale_factor: f32,
    pub friend: [f32; 4],
    pub neutral: [f32; 4],
    pub enemy: [f32; 4],
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            sizes: Vec::new(),
            textures: Vec::new(),
            scale_factor: 72.0,
            friend: [0.0, 1.0, 0.0, 1.0],
            neutral: [1.0, 1.0, 0.0, 1.0],
            enemy: [1.0, 0.0, 0.0, 1.0],
        }
    }
}

/// An ARGB colour of 0 to 255 values as RGBA of 0 to 1.
fn argb(value: &str) -> Option<[f32; 4]> {
    let v: Vec<f32> = value
        .split(',')
        .map(|p| p.trim().parse::<f32>().map(|x| x / 255.0))
        .collect::<Result<_, _>>()
        .ok()?;
    match v[..] {
        [a, r, g, b] => Some([r, g, b, a]),
        _ => None,
    }
}

impl Settings {
    /// The `[SelectionCircle]` section of a `UI\MiscData.txt`.
    pub fn parse(text: &str) -> Settings {
        let mut settings = Settings::default();
        let mut inside = false;
        let mut count = 0;
        let mut sizes = std::collections::BTreeMap::new();
        let mut textures = std::collections::BTreeMap::new();
        for line in text.lines() {
            let line = line.split("//").next().unwrap_or("").trim();
            if line.starts_with('[') {
                inside = line.eq_ignore_ascii_case("[SelectionCircle]");
                continue;
            }
            let Some((key, value)) = line.split_once('=').filter(|_| inside) else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            match key {
                "NumSizes" => count = value.parse().unwrap_or(0),
                "ScaleFactor" => settings.scale_factor = value.parse().unwrap_or(72.0),
                "ColorFriend" => settings.friend = argb(value).unwrap_or(settings.friend),
                "ColorNeutral" => settings.neutral = argb(value).unwrap_or(settings.neutral),
                "ColorEnemy" => settings.enemy = argb(value).unwrap_or(settings.enemy),
                _ => {
                    let index = |prefix: &str| key.strip_prefix(prefix)?.parse::<usize>().ok();
                    if let Some(i) = index("Size") {
                        sizes.insert(i, value.parse::<f32>().unwrap_or(0.0));
                    } else if let Some(i) = index("Texture") {
                        textures.insert(i, format!("{value}.blp"));
                    }
                }
            }
        }
        for i in 0..count {
            let Some(texture) = textures.get(&i) else {
                break;
            };
            settings.textures.push(texture.clone());
            if i + 1 < count {
                settings
                    .sizes
                    .push(sizes.get(&i).copied().unwrap_or(f32::MAX));
            }
        }
        settings
    }

    /// The settings of the game or the map.
    pub fn load(chain: &mut ArchiveChain) -> Settings {
        chain
            .read("UI\\MiscData.txt")
            .map(|d| Settings::parse(&String::from_utf8_lossy(&d)))
            .unwrap_or_default()
    }

    /// The size class of a circle `size` across.
    fn class(&self, size: f32) -> usize {
        self.sizes
            .iter()
            .position(|&s| size <= s)
            .unwrap_or(self.sizes.len())
    }
}

/// The triangles of a circle `size` across at `centre`, on the ground of
/// height `ground(x, y)`.
fn triangles(
    centre: [f32; 2],
    size: f32,
    colour: [f32; 4],
    ground: &impl Fn(f32, f32) -> f32,
) -> Vec<Vertex> {
    let corner = |i: usize, j: usize| {
        let (u, v) = (i as f32 / GRID as f32, j as f32 / GRID as f32);
        let (x, y) = (centre[0] + (u - 0.5) * size, centre[1] + (0.5 - v) * size);
        Vertex {
            position: [x, y, ground(x, y) + LIFT],
            uv: [u, v],
            colour,
        }
    };
    let mut out = Vec::with_capacity(GRID * GRID * 6);
    for j in 0..GRID {
        for i in 0..GRID {
            let [a, b, c, d] = [
                corner(i, j),
                corner(i + 1, j),
                corner(i + 1, j + 1),
                corner(i, j + 1),
            ];
            out.extend([a, b, c, a, c, d]);
        }
    }
    out
}

pub struct Circles {
    settings: Settings,
    pipeline: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    globals_bind_group: wgpu::BindGroup,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// The bind group of each size class, read when first needed (`None`
    /// when its texture cannot be read).
    textures: Vec<Option<Option<wgpu::BindGroup>>>,
    vertices: wgpu::Buffer,
    capacity: usize,
    /// The vertex ranges to draw with each class.
    draws: Vec<(usize, std::ops::Range<u32>)>,
}

impl Circles {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, settings: Settings) -> Circles {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("circles"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("circle globals"),
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
            label: Some("circle texture"),
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
            label: Some("circle globals"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("circle globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("circles"),
            bind_group_layouts: &[&globals_layout, &texture_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("circles"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x4],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            // On the ground: hidden by what stands in front, not written.
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
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("circle"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let capacity = GRID * GRID * 6 * 4;
        Circles {
            textures: vec![None; settings.textures.len()],
            settings,
            pipeline,
            globals,
            globals_bind_group,
            texture_layout,
            sampler,
            vertices: vertex_buffer(device, capacity),
            capacity,
            draws: Vec::new(),
        }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// The bind group of size class `class`, its texture read from `chain`
    /// the first time.
    fn texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        chain: &mut ArchiveChain,
        class: usize,
    ) -> bool {
        let slot = &mut self.textures[class];
        if slot.is_none() {
            let image = chain
                .read(&self.settings.textures[class])
                .and_then(|d| ow3_formats::blp::decode(&d).ok());
            *slot = Some(image.map(|image| {
                let view = upload_texture(device, queue, &Image::from(image));
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("circle texture"),
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
                })
            }));
        }
        matches!(slot, Some(Some(_)))
    }

    /// Lays out `circles` for the next frame.
    #[allow(clippy::too_many_arguments)]
    pub fn set(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        chain: &mut ArchiveChain,
        circles: &[Circle],
        view_projection: &Mat4,
        ground: impl Fn(f32, f32) -> f32,
    ) {
        queue.write_buffer(&self.globals, 0, bytemuck::cast_slice(view_projection));
        let mut vertices = Vec::new();
        self.draws.clear();
        if self.settings.textures.is_empty() {
            return;
        }
        for circle in circles {
            let size = circle.scale * self.settings.scale_factor;
            let class = self.settings.class(size);
            if !self.texture(device, queue, chain, class) {
                continue;
            }
            let start = vertices.len() as u32;
            vertices.extend(triangles(circle.centre, size, circle.colour, &ground));
            self.draws.push((class, start..vertices.len() as u32));
        }
        if vertices.len() > self.capacity {
            self.capacity = vertices.len() * 2;
            self.vertices = vertex_buffer(device, self.capacity);
        }
        queue.write_buffer(&self.vertices, 0, bytemuck::cast_slice(&vertices));
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass) {
        if self.draws.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.globals_bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        for (class, range) in &self.draws {
            if let Some(Some(bind_group)) = &self.textures[*class] {
                pass.set_bind_group(1, bind_group, &[]);
                pass.draw(range.clone(), 0..1);
            }
        }
    }
}

fn vertex_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("circles"),
        size: (capacity * std::mem::size_of::<Vertex>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECTION: &str = "[Light]\nDirection=0.3,0.3,-0.25\n\n[SelectionCircle]\n// comment\nNumSizes=3\nSize00=100\nTexture00=A\\Small\nSize01=300\nTexture01=A\\Med\nTexture02=A\\Large\nScaleFactor=72\nColorFriend=255,0,255,0\nColorEnemy=255,255,0,0\n\n[Blight]\nTexture=X\n";

    #[test]
    fn settings_and_classes() {
        let s = Settings::parse(SECTION);
        assert_eq!(s.textures, ["A\\Small.blp", "A\\Med.blp", "A\\Large.blp"]);
        assert_eq!(s.sizes, [100.0, 300.0]);
        assert_eq!(s.scale_factor, 72.0);
        assert_eq!(s.friend, [0.0, 1.0, 0.0, 1.0]);
        assert_eq!(s.enemy, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(s.class(72.0), 0);
        assert_eq!(s.class(100.0), 0);
        assert_eq!(s.class(288.0), 1);
        assert_eq!(s.class(400.0), 2);
    }

    /// The grid spans the circle's size and lies on the ground.
    #[test]
    fn grid_on_the_ground() {
        let v = triangles([100.0, 200.0], 72.0, [1.0; 4], &|x, _| x * 0.1);
        assert_eq!(v.len(), GRID * GRID * 6);
        let xs = v.iter().map(|p| p.position[0]);
        let (lo, hi) = (
            xs.clone().fold(f32::MAX, f32::min),
            xs.fold(f32::MIN, f32::max),
        );
        assert_eq!((lo, hi), (64.0, 136.0));
        assert!(
            v.iter()
                .all(|p| (p.position[2] - (p.position[0] * 0.1 + LIFT)).abs() < 1e-4)
        );
    }
}
