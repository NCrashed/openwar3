//! Flat shapes over the view: health bars over units, drawn in screen
//! space after everything else, without depth.

use bytemuck::{Pod, Zeroable};

use crate::camera::Mat4;
use crate::renderer::DEPTH_FORMAT;

const SHADER: &str = r#"
struct VertexIn {
    @location(0) position: vec2<f32>,
    @location(1) colour: vec4<f32>,
};
struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) colour: vec4<f32>,
};

@vertex
fn vs(v: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip = vec4<f32>(v.position, 0.0, 1.0);
    out.colour = v.colour;
    return out;
}

@fragment
fn fs(v: VertexOut) -> @location(0) vec4<f32> {
    return v.colour;
}
"#;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
struct Vertex {
    /// Normalised device coordinates.
    position: [f32; 2],
    colour: [f32; 4],
}

/// A health bar over a unit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bar {
    /// The point in the world the bar stands over.
    pub at: [f32; 3],
    /// The share of life, 0 to 1.
    pub fill: f32,
    pub selected: bool,
}

/// Bar size in pixels.
const BAR_WIDTH: f32 = 36.0;
const BAR_HEIGHT: f32 = 5.0;
/// Pixels of the outline around a selected unit's bar.
const OUTLINE: f32 = 2.0;

/// `point` on the screen in normalised device coordinates, or `None`
/// behind the camera.
pub fn project(view_projection: &Mat4, point: [f32; 3]) -> Option<[f32; 2]> {
    let v = [point[0], point[1], point[2], 1.0];
    let mut out = [0.0f32; 4];
    for (r, value) in out.iter_mut().enumerate() {
        *value = (0..4).map(|c| view_projection[c][r] * v[c]).sum();
    }
    (out[3] > 1e-3).then(|| [out[0] / out[3], out[1] / out[3]])
}

/// The triangles of `bars` for a view `size` pixels large.
fn triangles(bars: &[Bar], view_projection: &Mat4, size: (u32, u32)) -> Vec<Vertex> {
    let (w, h) = (size.0.max(1) as f32, size.1.max(1) as f32);
    let mut out = Vec::new();
    let mut rect = |x0: f32, y0: f32, x1: f32, y1: f32, colour: [f32; 4]| {
        // Pixels from the bar's centre, turned into device coordinates.
        let corners = [[x0, y0], [x1, y0], [x1, y1], [x0, y0], [x1, y1], [x0, y1]];
        out.extend(corners.map(|[x, y]| Vertex {
            position: [x * 2.0 / w, y * 2.0 / h],
            colour,
        }));
    };
    for bar in bars {
        let Some([cx, cy]) = project(view_projection, bar.at) else {
            continue;
        };
        if cx.abs() > 1.2 || cy.abs() > 1.2 {
            continue;
        }
        let (px, py) = (cx * w / 2.0, cy * h / 2.0);
        let (hw, hh) = (BAR_WIDTH / 2.0, BAR_HEIGHT / 2.0);
        if bar.selected {
            let o = OUTLINE;
            rect(
                px - hw - o,
                py - hh - o,
                px + hw + o,
                py + hh + o,
                [1.0, 1.0, 1.0, 1.0],
            );
        }
        rect(px - hw, py - hh, px + hw, py + hh, [0.0, 0.0, 0.0, 0.8]);
        let fill = bar.fill.clamp(0.0, 1.0);
        // Green when full, through yellow, to red when nearly dead.
        let colour = [
            (2.0 * (1.0 - fill)).min(1.0),
            (2.0 * fill).min(1.0),
            0.0,
            1.0,
        ];
        rect(
            px - hw + 1.0,
            py - hh + 1.0,
            px - hw + 1.0 + (BAR_WIDTH - 2.0) * fill,
            py + hh - 1.0,
            colour,
        );
    }
    out
}

pub struct Overlay {
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    capacity: usize,
    count: u32,
}

impl Overlay {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Overlay {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("overlay"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("overlay"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("overlay"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4],
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
        });
        let capacity = 1024;
        Overlay {
            pipeline,
            vertices: vertex_buffer(device, capacity),
            capacity,
            count: 0,
        }
    }

    /// Lays out `bars` for the next frame.
    pub fn set(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bars: &[Bar],
        view_projection: &Mat4,
        size: (u32, u32),
    ) {
        let vertices = triangles(bars, view_projection, size);
        if vertices.len() > self.capacity {
            self.capacity = vertices.len() * 2;
            self.vertices = vertex_buffer(device, self.capacity);
        }
        queue.write_buffer(&self.vertices, 0, bytemuck::cast_slice(&vertices));
        self.count = vertices.len() as u32;
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass) {
        if self.count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.draw(0..self.count, 0..1);
    }
}

fn vertex_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("overlay"),
        size: (capacity * std::mem::size_of::<Vertex>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bar in the middle of the view: its background and fill, the fill
    /// as wide as the share of life.
    #[test]
    fn bars_at_their_units() {
        let camera = crate::camera::Camera::new([0.0, 0.0, 0.0]);
        let m = camera.view_projection(1.0);
        let bar = Bar {
            at: [0.0, 0.0, 0.0],
            fill: 0.5,
            selected: false,
        };
        let v = triangles(&[bar], &m, (400, 400));
        assert_eq!(v.len(), 12);
        let width = |q: &[Vertex]| q[1].position[0] - q[0].position[0];
        let full = width(&v[0..6]) * 200.0;
        let filled = width(&v[6..12]) * 200.0;
        assert!((full - BAR_WIDTH).abs() < 1e-3, "{full}");
        assert!((filled - (BAR_WIDTH - 2.0) / 2.0).abs() < 1e-3, "{filled}");
        let selected = triangles(
            &[Bar {
                selected: true,
                ..bar
            }],
            &m,
            (400, 400),
        );
        assert_eq!(selected.len(), 18);
    }
}
