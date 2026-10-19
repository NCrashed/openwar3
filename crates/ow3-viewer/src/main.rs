//! `ow3-viewer <map> [--screenshot <out.png>] [--size <w>x<h>] [--camera <x>,<y>,<distance>,<yaw>,<pitch>] [--time <seconds>] [--no-sim] [--select <unit> [--order <x>,<y>]] [--alt] [--fog <style>,<start>,<end>,<density>,<r>,<g>,<b>]`
//!
//! Shows a map in a window: arrows or WASD move, the mouse wheel or +/-
//! zooms, Q/E turn, R/F tilt, C prints the camera as a `--camera`
//! argument, Escape quits. A left click selects a unit;
//! a right click then sends it to the ground point or attacks the enemy
//! clicked (with Ctrl held, any unit clicked), and H stops it. The unit
//! under the mouse shows its circle and health bar; with Alt held every
//! unit shows its bar (`--alt` in screenshots). The map's
//! script runs in the simulation, which places the units (`--no-sim`: the
//! units placed in the editor instead). With `--screenshot` the view
//! `--time` seconds in is drawn into a PNG file without a window, on a GPU
//! or the software Vulkan driver. Game archives come from `game-data/1.27b`
//! or the directory in `OW3_GAME_DATA`.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use ow3_viewer::camera::Camera;
use ow3_viewer::controls::{Action, Controls};
use ow3_viewer::renderer::{self, Renderer};
use ow3_viewer::scene::Scene;
use ow3_viewer::sim_units::Sim;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
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
    /// Draw the units placed in the editor instead of running the script.
    no_sim: bool,
    /// The unit selected in a screenshot, and a point it is sent to at
    /// `time`, 0.25 s before the picture.
    select: Option<u32>,
    order: Option<[f32; 2]>,
    /// Every health bar shown, as with Alt held.
    alt: bool,
    /// A terrain fog in place of the script's, for checking it.
    fog: Option<ow3_sim::game::TerrainFog>,
}

fn options() -> Result<Options, String> {
    let mut args = std::env::args().skip(1);
    let mut options = Options {
        map: PathBuf::new(),
        screenshot: None,
        size: (1280, 720),
        camera: None,
        time: 0.0,
        no_sim: false,
        select: None,
        order: None,
        alt: false,
        fog: None,
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
            "--no-sim" => options.no_sim = true,
            "--alt" => options.alt = true,
            "--fog" => {
                let v: Result<Vec<f32>, _> = value("--fog")?
                    .split(',')
                    .map(|n| n.trim().parse())
                    .collect();
                let v = v.map_err(|_| "bad --fog")?;
                let [style, start, end, density, r, g, b] = v[..] else {
                    return Err("--fog takes seven numbers".into());
                };
                options.fog = Some(ow3_sim::game::TerrainFog {
                    style: style as i32,
                    start,
                    end,
                    density,
                    color: [r, g, b],
                });
            }
            "--select" => {
                options.select = Some(
                    value("--select")?
                        .trim()
                        .parse()
                        .map_err(|_| "bad --select")?,
                );
            }
            "--order" => {
                let v = value("--order")?;
                let (x, y) = v.split_once(',').ok_or("--order is <x>,<y>")?;
                let x = x.trim().parse().map_err(|_| "bad --order")?;
                let y = y.trim().parse().map_err(|_| "bad --order")?;
                options.order = Some([x, y]);
            }
            "--time" => {
                options.time = value("--time")?.trim().parse().map_err(|_| "bad --time")?;
            }
            _ if map.is_none() => map = Some(PathBuf::from(arg)),
            _ => return Err(format!("unexpected argument {arg}")),
        }
    }
    options.map = map.ok_or("usage: ow3-viewer <map> [--screenshot <out.png>] [--size <w>x<h>] [--camera <x>,<y>,<distance>,<yaw>,<pitch>] [--time <seconds>] [--no-sim] [--select <unit> [--order <x>,<y>]] [--alt] [--fog <style>,<start>,<end>,<density>,<r>,<g>,<b>]")?;
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
    // The target stands on the ground.
    camera.target[2] =
        ow3_viewer::terrain_mesh::ground_z(&scene.terrain, camera.target[0], camera.target[1]);
    camera
}

fn screenshot(
    scene: &Scene,
    sim: Option<&mut Sim>,
    options: &Options,
    out: &PathBuf,
) -> Result<(), String> {
    let (device, queue) = pollster::block_on(renderer::headless_device())?;
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut renderer = Renderer::new(&device, &queue, format, scene);
    let camera = camera(scene, options.camera.as_deref());
    let (w, h) = options.size;
    let mut sim_hour = 12.0;
    let mut sim_seconds = options.time;
    let mut sim_fog = None;
    renderer.selected = options.select;
    renderer.all_bars = options.alt;
    let mut sim = sim;
    if let Some(sim) = sim.as_deref_mut() {
        // The simulation takes at most a few seconds of real time at once.
        let mut left = (f64::from(options.time) * 1e6) as u64;
        while left > 0 {
            let step = left.min(1_000_000);
            sim.advance(step);
            left -= step;
        }
        // An order at `--time`, drawn 0.25 s later.
        if let (Some(unit), Some([x, y])) = (options.select, options.order) {
            let z = ow3_viewer::terrain_mesh::ground_z(&scene.terrain, x, y);
            if let Some(line) = sim.command(unit, None, Some([x, y, z]), false) {
                println!("{line}");
            }
            sim.advance(250_000);
        }
        sim_hour = sim.time_of_day();
        sim_seconds = sim.game_seconds();
        sim_fog = sim.terrain_fog();
        let units = sim.draws();
        renderer.set_units(
            &device,
            &queue,
            &mut sim.chain,
            &units,
            (options.time * 1000.0) as i32,
        );
        let terrain = &scene.terrain;
        renderer.prepare_circles(&device, &queue, &mut sim.chain, &camera, |x, y| {
            ow3_viewer::terrain_mesh::ground_z(terrain, x, y)
        });
    }
    renderer.animate(&device, &queue, (options.time * 1000.0) as i32);
    renderer.prepare_particles(&device, &queue, &camera, (options.time * 1000.0) as i32);
    light(&mut renderer, scene, sim_hour);
    renderer.set_game_time(sim_seconds);
    renderer.set_fog(options.fog.or(sim_fog));
    renderer.prepare_overlay(&device, &queue, &camera, (w, h));
    if let Some(sim) = sim {
        let frame = sim.interface();
        renderer.prepare_ui(&device, &queue, &mut sim.chain, &frame, (w, h));
    }
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
    /// The running map, if its script runs.
    sim: Option<Sim>,
    last_frame: std::time::Instant,
    controls: Controls,
    /// The mouse cursor in pixels.
    cursor: (f32, f32),
    /// Whether Ctrl is held: a right click then attacks any unit.
    ctrl: bool,
    /// Whether Alt is held: every unit shows its health bar.
    alt: bool,
    /// The fog of `--fog`, in place of the script's.
    fog: Option<ow3_sim::game::TerrainFog>,
    /// Frames since `counted`, for the frame rate in the title.
    frames: u32,
    counted: std::time::Instant,
    fps: f32,
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
        // Colours are computed on the values of the textures as they are
        // shown, as the formulas of the specs are (SPEC-0017): a surface
        // without the sRGB conversion.
        if let Some(&plain) = capabilities.formats.iter().find(|f| !f.is_srgb()) {
            config.format = plain;
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
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.config.width = size.width.max(1);
                    gpu.config.height = size.height.max(1);
                    gpu.surface.configure(&gpu.device, &gpu.config);
                    gpu.depth =
                        renderer::depth_view(&gpu.device, gpu.config.width, gpu.config.height);
                }
            }
            WindowEvent::KeyboardInput {
                event: KeyEvent {
                    logical_key, state, ..
                },
                ..
            } => {
                if logical_key == Key::Named(NamedKey::Escape) {
                    event_loop.exit();
                }
                let name = match &logical_key {
                    Key::Named(named) => format!("{named:?}"),
                    Key::Character(c) => c.to_string(),
                    _ => String::new(),
                };
                // H: the selected unit stops.
                if matches!(name.as_str(), "h" | "H")
                    && state == ElementState::Pressed
                    && let (Some(gpu), Some(sim)) = (&self.gpu, &mut self.sim)
                    && let Some(unit) = gpu.renderer.selected
                {
                    sim.stop(unit);
                }
                // C: prints the camera as a `--camera` argument.
                if matches!(name.as_str(), "c" | "C") && state == ElementState::Pressed {
                    let c = &self.camera;
                    println!(
                        "--camera {:.0},{:.0},{:.0},{:.1},{:.1}",
                        c.target[0], c.target[1], c.distance, c.yaw, c.pitch
                    );
                }
                if let Some(action) = Action::of_key(&name) {
                    self.controls.key(action, state == ElementState::Pressed);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
                self.controls.wheel(lines);
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.ctrl = modifiers.state().control_key();
                self.alt = modifiers.state().alt_key();
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x as f32, position.y as f32);
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                if let Some(gpu) = &mut self.gpu {
                    let size = (gpu.config.width, gpu.config.height);
                    let picked =
                        gpu.renderer
                            .pick(self.cursor.0, self.cursor.1, &self.camera, size);
                    gpu.renderer.selected = picked;
                    if let (Some(handle), Some(sim)) = (picked, &self.sim)
                        && let Some(line) = sim.describe(handle)
                    {
                        println!("{line}");
                    }
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Right,
                ..
            } => {
                if let (Some(gpu), Some(sim)) = (&mut self.gpu, &mut self.sim)
                    && let Some(unit) = gpu.renderer.selected
                {
                    let size = (gpu.config.width, gpu.config.height);
                    let (x, y) = self.cursor;
                    let on = gpu.renderer.pick(x, y, &self.camera, size);
                    let terrain = &self.scene.terrain;
                    let at = self.camera.ground_point(x, y, size, |x, y| {
                        ow3_viewer::terrain_mesh::ground_z(terrain, x, y)
                    });
                    if let Some(line) = sim.command(unit, on, at, self.ctrl) {
                        println!("{line}");
                    }
                }
            }
            WindowEvent::RedrawRequested => self.frame(),
            _ => {}
        }
    }
}

impl App {
    /// Moves the camera and the simulation on and draws the next frame.
    fn frame(&mut self) {
        let Some(gpu) = &mut self.gpu else {
            return;
        };
        let now = std::time::Instant::now();
        let dt = now.duration_since(self.last_frame);
        self.last_frame = now;
        let terrain = &self.scene.terrain;
        self.controls
            .update(&mut self.camera, dt.as_secs_f32(), |x, y| {
                ow3_viewer::terrain_mesh::ground_z(terrain, x, y)
            });
        let elapsed = self.started.elapsed().as_millis() as i32;
        if let Some(sim) = &mut self.sim {
            sim.advance(dt.as_micros() as u64);
            let units = sim.draws();
            gpu.renderer
                .set_units(&gpu.device, &gpu.queue, &mut sim.chain, &units, elapsed);
            // The unit under the mouse, as the units stand in this frame.
            let size = (gpu.config.width, gpu.config.height);
            gpu.renderer.hovered =
                gpu.renderer
                    .pick(self.cursor.0, self.cursor.1, &self.camera, size);
            gpu.renderer.prepare_circles(
                &gpu.device,
                &gpu.queue,
                &mut sim.chain,
                &self.camera,
                |x, y| ow3_viewer::terrain_mesh::ground_z(terrain, x, y),
            );
        }
        gpu.renderer.animate(&gpu.device, &gpu.queue, elapsed);
        gpu.renderer
            .prepare_particles(&gpu.device, &gpu.queue, &self.camera, elapsed);
        let hour = self.sim.as_ref().map_or(12.0, Sim::time_of_day);
        let seconds = self
            .sim
            .as_ref()
            .map_or(elapsed as f32 / 1000.0, Sim::game_seconds);
        gpu.renderer.set_game_time(seconds);
        gpu.renderer
            .set_fog(self.fog.or(self.sim.as_ref().and_then(Sim::terrain_fog)));
        light(&mut gpu.renderer, &self.scene, hour);
        let size = (gpu.config.width, gpu.config.height);
        gpu.renderer.all_bars = self.alt;
        gpu.renderer
            .prepare_overlay(&gpu.device, &gpu.queue, &self.camera, size);
        if let Some(sim) = &mut self.sim {
            let frame = sim.interface();
            gpu.renderer
                .prepare_ui(&gpu.device, &gpu.queue, &mut sim.chain, &frame, size);
        }
        match gpu.surface.get_current_texture() {
            Ok(frame) => {
                let view = frame
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                gpu.renderer
                    .draw(&gpu.device, &gpu.queue, &view, &gpu.depth, &self.camera);
                frame.present();
            }
            Err(_) => gpu.surface.configure(&gpu.device, &gpu.config),
        }
        // The frame rate and the selected unit in the title, twice a second.
        self.frames += 1;
        let counted = now.duration_since(self.counted).as_secs_f32();
        if counted >= 0.5 {
            self.fps = self.frames as f32 / counted;
            self.frames = 0;
            self.counted = now;
            let selected = gpu
                .renderer
                .selected
                .and_then(|h| self.sim.as_ref()?.describe(h));
            let title = match selected {
                Some(line) => format!("openwar3 - {:.0} fps - {line}", self.fps),
                None => format!("openwar3 - {:.0} fps", self.fps),
            };
            gpu.window.set_title(&title);
        }
        // Models move: draw the next frame too.
        gpu.window.request_redraw();
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
    // Without a simulation the units placed in the editor stand in.
    let mut sim = if options.no_sim {
        None
    } else {
        match Sim::start(&game_data, &options.map) {
            Ok(sim) => Some(sim),
            Err(e) => {
                eprintln!("the map's script does not run ({e}); showing the placed units");
                None
            }
        }
    };
    let day_night = sim.as_ref().and_then(Sim::day_night_models);
    let scene = match Scene::load(&game_data, &options.map, sim.is_some(), day_night) {
        Ok(scene) => scene,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    if let Some(out) = &options.screenshot {
        return match screenshot(&scene, sim.as_mut(), &options, out) {
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
        sim,
        last_frame: std::time::Instant::now(),
        controls: Controls::new(camera),
        cursor: (0.0, 0.0),
        ctrl: false,
        alt: false,
        fog: options.fog,
        frames: 0,
        counted: std::time::Instant::now(),
        fps: 0.0,
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

/// The light of `hour` o'clock from the scene's day/night models.
fn light(renderer: &mut Renderer, scene: &Scene, hour: f32) {
    let at = |model: &Option<ow3_render::daylight::DayLight>| {
        model.as_ref().map_or(renderer::PLAIN_LIGHT, |m| m.at(hour))
    };
    renderer.terrain_light = at(&scene.terrain_light);
    renderer.unit_light = at(&scene.unit_light);
}
