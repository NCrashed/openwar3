//! `ow3-viewer <map> [--screenshot <out.png>] [--size <w>x<h>] [--camera <x>,<y>,<distance>,<yaw>,<pitch>] [--time <seconds>]`
//!
//! Shows a map's ground in a window: arrows or WASD move, the mouse wheel
//! or +/- zooms, Q/E turn, R/F tilt, Escape quits. With `--screenshot`
//! the view is drawn into a PNG file without a window, on a GPU or the
//! software Vulkan driver. Game archives come from `game-data/1.27b` or
//! the directory in `OW3_GAME_DATA`.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use ow3_viewer::camera::Camera;
use ow3_viewer::renderer::{self, Renderer};
use ow3_viewer::scene::Scene;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

struct Options {
    map: PathBuf,
    screenshot: Option<PathBuf>,
    size: (u32, u32),
    camera: Option<Vec<f32>>,
    /// Seconds into the animations of a screenshot.
    time: f32,
}

fn options() -> Result<Options, String> {
    let mut args = std::env::args().skip(1);
    let mut options = Options {
        map: PathBuf::new(),
        screenshot: None,
        size: (1280, 720),
        camera: None,
        time: 0.0,
    };
    let mut map = None;
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "--screenshot" => options.screenshot = Some(value("--screenshot")?.into()),
            "--size" => {
                let v = value("--size")?;
                let (w, h) = v.split_once('x').ok_or("--size is <width>x<height>")?;
                options.size = (
                    w.parse().map_err(|_| "bad width")?,
                    h.parse().map_err(|_| "bad height")?,
                );
            }
            "--camera" => {
                let v = value("--camera")?;
                let numbers: Result<Vec<f32>, _> = v.split(',').map(|n| n.trim().parse()).collect();
                options.camera = Some(numbers.map_err(|_| "bad --camera")?);
            }
            "--time" => {
                options.time = value("--time")?.trim().parse().map_err(|_| "bad --time")?;
            }
            _ if map.is_none() => map = Some(PathBuf::from(arg)),
            _ => return Err(format!("unexpected argument {arg}")),
        }
    }
    options.map = map.ok_or("usage: ow3-viewer <map> [--screenshot <out.png>] [--size <w>x<h>] [--camera <x>,<y>,<distance>,<yaw>,<pitch>]")?;
    Ok(options)
}

fn camera(scene: &Scene, values: Option<&[f32]>) -> Camera {
    let mut camera = Camera::new(ow3_viewer::terrain_mesh::centre(&scene.terrain));
    if let Some(v) = values {
        let get = |i: usize, default: f32| v.get(i).copied().unwrap_or(default);
        camera.target[0] = get(0, camera.target[0]);
        camera.target[1] = get(1, camera.target[1]);
        camera.distance = get(2, camera.distance);
        camera.yaw = get(3, camera.yaw);
        camera.pitch = get(4, camera.pitch);
    }
    camera
}

fn screenshot(scene: &Scene, options: &Options, out: &PathBuf) -> Result<(), String> {
    let (device, queue) = pollster::block_on(renderer::headless_device())?;
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let renderer = Renderer::new(&device, &queue, format, scene);
    let camera = camera(scene, options.camera.as_deref());
    let (w, h) = options.size;
    renderer.animate(&queue, (options.time * 1000.0) as i32);
    let image = renderer::render_to_image(&device, &queue, &renderer, &camera, w, h);
    let file = std::fs::File::create(out).map_err(|e| format!("{}: {e}", out.display()))?;
    image
        .write_png(std::io::BufWriter::new(file))
        .map_err(|e| e.to_string())?;
    println!("{}: {w}x{h}", out.display());
    Ok(())
}

/// The window and what draws into it, made once the event loop runs.
struct Gpu {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    depth: wgpu::TextureView,
    renderer: Renderer,
}

struct App {
    scene: Scene,
    camera: Camera,
    size: (u32, u32),
    gpu: Option<Gpu>,
    /// When the animations started.
    started: std::time::Instant,
    error: Option<String>,
}

impl App {
    fn start(&mut self, event_loop: &ActiveEventLoop) -> Result<Gpu, String> {
        let attributes = Window::default_attributes()
            .with_title("openwar3")
            .with_inner_size(winit::dpi::PhysicalSize::new(self.size.0, self.size.1));
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|e| e.to_string())?,
        );
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| e.to_string())?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
        }))
        .map_err(|e| e.to_string())?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .map_err(|e| e.to_string())?;
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or("the surface does not suit the adapter")?;
        let capabilities = surface.get_capabilities(&adapter);
        if let Some(&srgb) = capabilities.formats.iter().find(|f| f.is_srgb()) {
            config.format = srgb;
        }
        surface.configure(&device, &config);
        let depth = renderer::depth_view(&device, config.width, config.height);
        let renderer = Renderer::new(&device, &queue, config.format, &self.scene);
        Ok(Gpu {
            window,
            surface,
            device,
            queue,
            config,
            depth,
            renderer,
        })
    }

    fn key(&mut self, key: &Key) -> bool {
        let step = self.camera.distance * 0.08;
        match key {
            Key::Named(NamedKey::ArrowUp) => self.camera.pan(0.0, step),
            Key::Named(NamedKey::ArrowDown) => self.camera.pan(0.0, -step),
            Key::Named(NamedKey::ArrowLeft) => self.camera.pan(-step, 0.0),
            Key::Named(NamedKey::ArrowRight) => self.camera.pan(step, 0.0),
            Key::Character(c) => match c.as_str() {
                "w" | "W" => self.camera.pan(0.0, step),
                "s" | "S" => self.camera.pan(0.0, -step),
                "a" | "A" => self.camera.pan(-step, 0.0),
                "d" | "D" => self.camera.pan(step, 0.0),
                "q" | "Q" => self.camera.turn(5.0, 0.0),
                "e" | "E" => self.camera.turn(-5.0, 0.0),
                "r" | "R" => self.camera.turn(0.0, 3.0),
                "f" | "F" => self.camera.turn(0.0, -3.0),
                "+" | "=" => self.camera.zoom(0.9),
                "-" => self.camera.zoom(1.1),
                _ => return false,
            },
            _ => return false,
        }
        true
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_none() {
            match self.start(event_loop) {
                Ok(gpu) => self.gpu = Some(gpu),
                Err(e) => {
                    self.error = Some(e);
                    event_loop.exit();
                }
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let mut redraw = false;
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.config.width = size.width.max(1);
                    gpu.config.height = size.height.max(1);
                    gpu.surface.configure(&gpu.device, &gpu.config);
                    gpu.depth =
                        renderer::depth_view(&gpu.device, gpu.config.width, gpu.config.height);
                    redraw = true;
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key,
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                if logical_key == Key::Named(NamedKey::Escape) {
                    event_loop.exit();
                }
                redraw = self.key(&logical_key);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
                self.camera.zoom(0.9f32.powf(lines));
                redraw = true;
            }
            WindowEvent::RedrawRequested => {
                if let Some(gpu) = &self.gpu {
                    match gpu.surface.get_current_texture() {
                        Ok(frame) => {
                            let view = frame
                                .texture
                                .create_view(&wgpu::TextureViewDescriptor::default());
                            let aspect = gpu.config.width as f32 / gpu.config.height as f32;
                            let elapsed = self.started.elapsed().as_millis() as i32;
                            gpu.renderer.animate(&gpu.queue, elapsed);
                            gpu.renderer.draw(
                                &gpu.device,
                                &gpu.queue,
                                &view,
                                &gpu.depth,
                                &self.camera,
                                aspect,
                            );
                            frame.present();
                            // Models move: draw the next frame too.
                            gpu.window.request_redraw();
                        }
                        Err(_) => gpu.surface.configure(&gpu.device, &gpu.config),
                    }
                }
            }
            _ => {}
        }
        if redraw && let Some(gpu) = &self.gpu {
            gpu.window.request_redraw();
        }
    }
}

fn main() -> ExitCode {
    let options = match options() {
        Ok(options) => options,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let game_data = std::env::var_os("OW3_GAME_DATA")
        .map_or_else(|| PathBuf::from("game-data/1.27b"), PathBuf::from);
    let scene = match Scene::load(&game_data, &options.map) {
        Ok(scene) => scene,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    if let Some(out) = &options.screenshot {
        return match screenshot(&scene, &options, out) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }
    let camera = camera(&scene, options.camera.as_deref());
    let mut app = App {
        scene,
        camera,
        size: options.size,
        gpu: None,
        started: std::time::Instant::now(),
        error: None,
    };
    let event_loop = match EventLoop::new() {
        Ok(event_loop) => event_loop,
        Err(e) => {
            eprintln!("cannot open a window: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("{e}");
        return ExitCode::FAILURE;
    }
    match app.error {
        Some(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
        None => ExitCode::SUCCESS,
    }
}
