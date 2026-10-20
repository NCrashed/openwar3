//! `ow3-viewer <map> [--screenshot <out.png>] [--size <w>x<h>] [--windowed] [--camera <x>,<y>,<distance>,<yaw>,<pitch>] [--time <seconds>] [--no-sim] [--select <unit> [--order <x>,<y>]] [--alt] [--fog <style>,<start>,<end>,<density>,<r>,<g>,<b>]`
//!
//! Shows a map on the full screen (in a window with `--windowed` or
//! `--size`; F11 switches): the arrows or the mouse at an edge of the
//! screen move, the mouse wheel or +/- zooms, Q/E turn, R/F tilt (all
//! after the hotkeys of the command card), C prints the camera as a
//! `--camera` argument. A left click selects a unit, a drag the player's
//! units in its rectangle (at most 12); Shift adds (or takes out), Ctrl or
//! a double click takes all of the clicked one's type on the screen, Tab
//! moves to the next subgroup, Ctrl and a digit makes a control group, a
//! digit selects it (twice: the camera goes there). A right click sends
//! the selected units to the ground point (a group order) or attacks the
//! enemy clicked (with Ctrl held, any unit clicked). The console below
//! shows the resources, the selected unit or group and the lead's command
//! card: a click on a button or its hotkey (by the key's
//! place, in any layout, before the camera's keys) gives the order, and
//! one that takes a point waits for a left click on the ground (a right
//! click drops it). Escape drops a waiting order or closes a list, and
//! else quits. A left click on the minimap moves the camera there. The unit
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
use winit::keyboard::{Key, NamedKey, PhysicalKey};
use winit::window::{Fullscreen, Window, WindowId};

struct Options {
    map: PathBuf,
    screenshot: Option<PathBuf>,
    size: (u32, u32),
    /// A window of `size` in place of the full screen.
    windowed: bool,
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
        windowed: false,
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
                options.windowed = true;
            }
            "--windowed" => options.windowed = true,
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
    options.map = map.ok_or("usage: ow3-viewer <map> [--screenshot <out.png>] [--size <w>x<h>] [--windowed] [--camera <x>,<y>,<distance>,<yaw>,<pitch>] [--time <seconds>] [--no-sim] [--select <unit> [--order <x>,<y>]] [--alt] [--fog <style>,<start>,<end>,<density>,<r>,<g>,<b>]")?;
    Ok(options)
}

fn camera(scene: &Scene, values: Option<&[f32]>, start: Option<[f32; 2]>, game: bool) -> Camera {
    // Where the script put the camera (the melee start: the workers), else
    // the centre of the map.
    let centre = ow3_viewer::terrain_mesh::centre(&scene.terrain);
    let target = start.map_or(centre, |[x, y]| [x, y, 0.0]);
    let mut camera = if game {
        ow3_viewer::controls::Game::camera(target)
    } else {
        Camera::new(target)
    };
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
    let start = sim
        .as_deref()
        .and_then(Sim::camera_target)
        .map(|(at, _)| at);
    let camera = camera(scene, options.camera.as_deref(), start, sim.is_some());
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
        renderer.set_vision(&queue, sim.vision().as_deref(), sim.vision_masked());
        let mut units = sim.draws();
        let (posed, hidden) = scene.posed(&sim.destructable_poses());
        units.extend(posed);
        renderer.hide_statics(&device, &queue, hidden);
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
        let mut selection = ow3_viewer::selection::Selection::default();
        if let Some(h) = options.select {
            selection.click(h);
        }
        let mut frame = sim.interface(&selection, None);
        frame
            .quads
            .extend(sim.minimap_quads(camera.frame_on_plane((w, h))));
        renderer.prepare_ui(&device, &queue, &mut sim.chain, &frame, (w, h));
        let portrait = options.select.and_then(|h| sim.portrait(h));
        renderer.set_portrait(
            &device,
            &queue,
            &mut sim.chain,
            portrait.as_ref().map(|(m, t)| (m.as_str(), *t)),
            (w, h),
            (options.time * 1000.0) as i32,
        );
        let indicator = sim.time_indicator();
        renderer.set_time_indicator(
            &device,
            &queue,
            &mut sim.chain,
            indicator.as_ref().map(|(m, _)| m.as_str()),
            (w, h),
            indicator.as_ref().map_or(0, |(_, t)| *t),
        );
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
    /// Whether the window covers the screen.
    fullscreen: bool,
    /// The moves of the camera by the script seen so far.
    camera_moves: u32,
    /// The fog of war shown, to upload it when it changes.
    vision: Option<Vec<u8>>,
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
    /// Whether Shift is held: clicks and rectangles add to the selection.
    shift: bool,
    /// The local player's selection and control groups (SPEC-0028).
    selection: ow3_viewer::selection::Selection,
    /// Where the left button went down over the world, for a rectangle.
    drag: Option<(f32, f32)>,
    /// The left button went down on the minimap and is held: the camera
    /// follows the mouse over it.
    minimap_drag: bool,
    /// The last click on a unit and the last recall of a group, for double
    /// clicks and double presses.
    last_click: Option<(std::time::Instant, u32)>,
    last_recall: Option<(std::time::Instant, usize)>,
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
            .with_inner_size(winit::dpi::PhysicalSize::new(self.size.0, self.size.1))
            .with_fullscreen(self.fullscreen.then_some(Fullscreen::Borderless(None)));
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
                event:
                    KeyEvent {
                        logical_key,
                        physical_key,
                        state,
                        ..
                    },
                ..
            } => {
                // Escape drops a waiting order or closes a list, else quits.
                if logical_key == Key::Named(NamedKey::Escape)
                    && state == ElementState::Pressed
                    && !self.escape()
                {
                    event_loop.exit();
                }
                if logical_key == Key::Named(NamedKey::F11)
                    && state == ElementState::Pressed
                    && let Some(gpu) = &self.gpu
                {
                    self.fullscreen = !self.fullscreen;
                    gpu.window
                        .set_fullscreen(self.fullscreen.then_some(Fullscreen::Borderless(None)));
                    return;
                }
                let name = match &logical_key {
                    Key::Named(named) => format!("{named:?}"),
                    Key::Character(c) => c.to_string(),
                    _ => String::new(),
                };
                // The hotkeys of the command card come before the camera's.
                // By the key's place, so that they work in any layout.
                let code = match physical_key {
                    PhysicalKey::Code(code) => format!("{code:?}"),
                    PhysicalKey::Unidentified(_) => String::new(),
                };
                // Tab and the control groups, by the key's place.
                if state == ElementState::Pressed && self.selection_key(&code) {
                    return;
                }
                let letter = code.strip_prefix("Key").map(str::to_owned);
                if state == ElementState::Pressed && self.hotkey(letter.as_deref().unwrap_or(&name))
                {
                    return;
                }
                // C: prints the camera as a `--camera` argument.
                if matches!(name.as_str(), "c" | "C") && state == ElementState::Pressed {
                    let c = &self.camera;
                    println!(
                        "--camera {:.0},{:.0},{:.0},{:.1},{:.1}",
                        c.target[0], c.target[1], c.distance, c.yaw, c.pitch
                    );
                }
                // Page Up and Page Down: the game camera's zoom levels,
                // one a press, repeated while held (SPEC-0023).
                if state == ElementState::Pressed && matches!(name.as_str(), "PageUp" | "PageDown")
                {
                    self.controls.step_zoom(name == "PageUp");
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
                self.shift = modifiers.state().shift_key();
                self.alt = modifiers.state().alt_key();
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x as f32, position.y as f32);
                // Dragging over the minimap, the camera goes with the
                // mouse, to the minimap's edge when it leaves it.
                if self.minimap_drag
                    && let (Some(p), Some(sim)) = (self.ui_point(), &self.sim)
                {
                    let [x, y] = sim.minimap.to_world_clamped(p);
                    self.controls.goal.target[0] = x;
                    self.controls.goal.target[1] = y;
                }
                if let Some(gpu) = &self.gpu {
                    let size = (gpu.config.width as f32, gpu.config.height as f32);
                    self.controls.cursor(Some(self.cursor), size);
                    gpu.window.request_redraw();
                }
            }
            WindowEvent::CursorLeft { .. } => self.controls.cursor(None, (0.0, 0.0)),
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                self.left_click();
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                self.left_release();
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Right,
                ..
            } => {
                // A right click drops an order waiting for its point.
                if let Some(sim) = &mut self.sim
                    && sim.hud.pending.take().is_some()
                {
                    return;
                }
                let all = self.selection.units().to_vec();
                let point = self.ui_point();
                if let (Some(gpu), Some(sim)) = (&mut self.gpu, &mut self.sim)
                    && !all.is_empty()
                {
                    // The own units selected take the order, all of them.
                    let own = sim.selectable(&all);
                    let size = (gpu.config.width, gpu.config.height);
                    let (x, y) = self.cursor;
                    let terrain = &self.scene.terrain;
                    let ground = |x, y| ow3_viewer::terrain_mesh::ground_z(terrain, x, y);
                    // On the minimap: `smart` to its point, no target
                    // (SPEC-0028, "Minimap").
                    let minimap = point.and_then(|p| sim.minimap.to_world(p));
                    let (on, at) = match minimap {
                        Some([wx, wy]) => (None, Some([wx, wy, ground(wx, wy)])),
                        None => (
                            gpu.renderer.pick(x, y, &self.camera, size),
                            self.camera.ground_point(x, y, size, ground),
                        ),
                    };
                    if let Some(line) = sim.command_group(&own, on, at, self.ctrl, self.shift) {
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
    /// The mouse in the interface's coordinates.
    fn ui_point(&self) -> Option<[f32; 2]> {
        let gpu = self.gpu.as_ref()?;
        let (w, h) = (
            gpu.config.width.max(1) as f32,
            gpu.config.height.max(1) as f32,
        );
        Some([
            self.cursor.0 / w * ow3_viewer::ui::WIDTH,
            (1.0 - self.cursor.1 / h) * ow3_viewer::ui::HEIGHT,
        ])
    }

    /// The point on the ground under the mouse.
    fn ground_point(&self) -> Option<[f32; 2]> {
        let gpu = self.gpu.as_ref()?;
        let size = (gpu.config.width, gpu.config.height);
        let terrain = &self.scene.terrain;
        let [x, y, _] = self
            .camera
            .ground_point(self.cursor.0, self.cursor.1, size, |x, y| {
                ow3_viewer::terrain_mesh::ground_z(terrain, x, y)
            })?;
        Some([x, y])
    }

    /// A command button pressed for the selected unit: an order that takes
    /// a point waits for it, the others are carried out.
    fn press(&mut self, button: ow3_viewer::hud::Button) {
        // A dark button (its requirements not met) does nothing.
        if !button.enabled {
            return;
        }
        let (all, active) = self.selected_units();
        let Some(sim) = &mut self.sim else {
            return;
        };
        if active.is_empty() {
            return;
        }
        if button.command.targets() {
            sim.hud.pending = Some(button.command);
        } else if let Some(line) = sim.run_group(&all, &active, button.command, None, self.shift) {
            println!("{line}");
        }
        self.sync_selection();
    }

    /// The units selected and those of the active subgroup.
    fn selected_units(&self) -> (Vec<u32>, Vec<u32>) {
        let Some(sim) = &self.sim else {
            return (Vec::new(), Vec::new());
        };
        (
            self.selection.units().to_vec(),
            self.selection.active_units(|h| sim.kind(h)),
        )
    }

    /// Forgets the selected units that are gone and shows the selection:
    /// the circles of all, the lead's card and portrait.
    fn sync_selection(&mut self) {
        let (Some(gpu), Some(sim)) = (&mut self.gpu, &self.sim) else {
            return;
        };
        self.selection.retain(|h| sim.kind(h).is_some());
        gpu.renderer.selected = self.selection.lead(|h| sim.kind(h));
        gpu.renderer.selection = self.selection.units().to_vec();
    }

    /// The left button went up: a click selects the unit under the mouse
    /// (with Shift added or taken out, with Ctrl or a double click all of
    /// its type on the screen), a drag the units in its rectangle (with
    /// Shift added) (SPEC-0028, "Selection").
    fn left_release(&mut self) {
        self.minimap_drag = false;
        let Some(start) = self.drag.take() else {
            return;
        };
        let Some(gpu) = &self.gpu else {
            return;
        };
        let size = (gpu.config.width, gpu.config.height);
        let end = self.cursor;
        let moved = (end.0 - start.0).abs().max((end.1 - start.1).abs());
        if moved >= DRAG {
            let inside = gpu.renderer.units_in(start, end, &self.camera, size);
            if let Some(sim) = &self.sim {
                let chosen = sim.selectable(&inside);
                if self.shift {
                    self.selection.add(&chosen);
                } else if !chosen.is_empty() {
                    self.selection.set(&chosen);
                }
            }
        } else {
            let picked = gpu.renderer.pick(end.0, end.1, &self.camera, size);
            let Some(picked) = picked else {
                return;
            };
            let now = std::time::Instant::now();
            let double = self.last_click.is_some_and(|(at, h)| {
                h == picked && now.duration_since(at) < std::time::Duration::from_millis(350)
            });
            self.last_click = Some((now, picked));
            let own = self
                .sim
                .as_ref()
                .is_some_and(|s| !s.selectable(&[picked]).is_empty());
            if (self.ctrl || double) && own {
                let on_screen = gpu.renderer.units_on_screen(&self.camera, size);
                if let Some(sim) = &self.sim {
                    self.selection.set(&sim.same_type(picked, &on_screen));
                }
            } else if self.shift && own {
                self.selection.shift_click(picked);
            } else {
                self.selection.click(picked);
            }
            if let Some(line) = self.sim.as_ref().and_then(|s| s.describe(picked)) {
                println!("{line}");
            }
        }
        if let Some(sim) = &mut self.sim {
            sim.hud.card = ow3_viewer::hud::Card::Main;
        }
        self.sync_selection();
    }

    /// Tab and the digits: the next subgroup, and the control groups
    /// (SPEC-0028): Ctrl and a digit assigns the selection to its group,
    /// Shift and a digit adds it, a digit selects the group, and a second
    /// press moves the camera to it. Whether the key was one of these.
    fn selection_key(&mut self, code: &str) -> bool {
        if code == "Tab" {
            if let Some(sim) = &self.sim {
                self.selection.next_subgroup(|h| sim.kind(h));
                sim_card_main(&mut self.sim);
            }
            self.sync_selection();
            return true;
        }
        let Some(digit) = code
            .strip_prefix("Digit")
            .and_then(|d| d.parse::<u32>().ok())
        else {
            return false;
        };
        let Some(group) = ow3_viewer::selection::group_of_digit(digit) else {
            return false;
        };
        if self.ctrl {
            self.selection.assign(group);
        } else if self.shift {
            self.selection.add_to_group(group);
        } else {
            let now = std::time::Instant::now();
            let again = self.last_recall.is_some_and(|(at, g)| {
                g == group && now.duration_since(at) < std::time::Duration::from_millis(500)
            });
            self.last_recall = Some((now, group));
            if self.selection.recall(group) {
                sim_card_main(&mut self.sim);
                if again
                    && let Some(sim) = &self.sim
                    && let Some(view) = self
                        .selection
                        .lead(|h| sim.kind(h))
                        .and_then(|h| sim.units().into_iter().find(|u| u.handle == h))
                {
                    self.controls.goal.target[0] = view.x;
                    self.controls.goal.target[1] = view.y;
                }
            }
        }
        self.sync_selection();
        true
    }

    /// A left click: a button of the console, the point of a waiting
    /// order, or a unit to select.
    fn left_click(&mut self) {
        let point = self.ui_point();
        let selected = self.gpu.as_ref().and_then(|g| g.renderer.selected);
        // The buttons beside the minimap (SPEC-0028).
        if let (Some(p), Some(sim)) = (point, &mut self.sim)
            && let Some(button) = ow3_viewer::minimap::button_at(p)
        {
            sim.minimap.toggles.press(button);
            return;
        }
        // A signal waiting for its point: on the minimap or the ground.
        if self.sim.as_ref().is_some_and(|s| s.minimap.toggles.signal) {
            let on_minimap = point.and_then(|p| self.sim.as_ref()?.minimap.to_world(p));
            let at = on_minimap.or_else(|| {
                point
                    .filter(|&p| !ow3_viewer::hud::Hud::covers(p))
                    .and_then(|_| self.ground_point())
            });
            if let (Some(at), Some(sim)) = (at, &mut self.sim) {
                let now = sim.game_seconds();
                sim.minimap.toggles.signal_at(at, now);
            }
            return;
        }
        // A click on the minimap moves the camera there, and while the
        // button is held the camera follows the mouse.
        if let (Some(p), Some(sim)) = (point, &self.sim)
            && let Some([x, y]) = sim.minimap.to_world(p)
        {
            self.controls.goal.target[0] = x;
            self.controls.goal.target[1] = y;
            self.minimap_drag = true;
            return;
        }
        if let (Some(p), Some(sim)) = (point, &self.sim)
            && ow3_viewer::hud::Hud::covers(p)
        {
            // A group cell (SPEC-0028): a click selects that unit or makes
            // its subgroup active, Shift takes it out, Ctrl keeps its type.
            let cells = self.selection.cells(|h| sim.kind(h));
            if cells.len() > 1
                && let Some(&(h, _)) =
                    ow3_viewer::hud::Hud::group_cell_at(p).and_then(|i| cells.get(i))
            {
                if self.shift {
                    self.selection.shift_click(h);
                } else if self.ctrl {
                    self.selection.keep_type_of(h, |h| sim.kind(h));
                } else {
                    self.selection.click_cell(h, |h| sim.kind(h));
                }
                sim_card_main(&mut self.sim);
                self.sync_selection();
                return;
            }
            let s = selected.and_then(|h| sim.selected(h));
            if let Some(button) = sim.hud.button_at(s.as_ref(), p) {
                self.press(button);
            }
            return;
        }
        let ground = self.ground_point();
        let on = self.gpu.as_ref().and_then(|gpu| {
            let size = (gpu.config.width, gpu.config.height);
            gpu.renderer
                .pick(self.cursor.0, self.cursor.1, &self.camera, size)
        });
        let (all, active) = self.selected_units();
        if let (Some(sim), Some(_)) = (&mut self.sim, selected)
            && let Some(command) = sim.hud.pending.take()
        {
            // Moves, attacks and patrols go to all units selected, the
            // others to the active subgroup.
            use ow3_viewer::hud::Command;
            let units = match command {
                Command::Move | Command::Attack | Command::Patrol => &all,
                _ => &active,
            };
            // On a unit: the order with it as the target, if it takes one.
            let line = on
                .and_then(|t| sim.run_on_group(units, command, t))
                .or_else(|| sim.run_group(&all, &active, command, ground, self.shift));
            if let Some(line) = line {
                println!("{line}");
            }
            return;
        }
        // Else a click or a rectangle, decided when the button goes up.
        self.drag = Some(self.cursor);
    }

    /// A hotkey of the command card for the selected unit; whether it was
    /// one.
    fn hotkey(&mut self, key: &str) -> bool {
        let mut chars = key.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else {
            return false;
        };
        let (Some(gpu), Some(sim)) = (&self.gpu, &self.sim) else {
            return false;
        };
        let s = gpu.renderer.selected.and_then(|h| sim.selected(h));
        let Some(button) = sim.hud.button_of_key(s.as_ref(), c) else {
            return false;
        };
        self.press(button);
        true
    }

    /// Escape: drops a waiting order or closes a list; whether it did.
    fn escape(&mut self) -> bool {
        let Some(sim) = &mut self.sim else {
            return false;
        };
        if sim.hud.pending.take().is_some() {
            return true;
        }
        // A signal waiting for its point (SPEC-0028).
        if sim.minimap.toggles.signal {
            sim.minimap.toggles.signal = false;
            return true;
        }
        if sim.hud.card != ow3_viewer::hud::Card::Main {
            sim.hud.card = ow3_viewer::hud::Card::Main;
            return true;
        }
        false
    }

    /// Moves the camera and the simulation on and draws the next frame.
    fn frame(&mut self) {
        self.sync_selection();
        // The ground under the mouse, outside the console.
        let ground = self
            .ui_point()
            .filter(|&p| !ow3_viewer::hud::Hud::covers(p))
            .and_then(|_| self.ground_point());
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
            // A script that put the camera elsewhere moves it at once.
            if let Some(([x, y], n)) = sim.camera_target()
                && n != self.camera_moves
            {
                self.camera_moves = n;
                let z = ow3_viewer::terrain_mesh::ground_z(&self.scene.terrain, x, y);
                self.camera.target = [x, y, z];
                self.controls.goal.target = [x, y, z];
            }
            sim.ghost = ground;
            // The fog of war, when its update changed it.
            let vision = sim.vision();
            if vision != self.vision {
                gpu.renderer
                    .set_vision(&gpu.queue, vision.as_deref(), sim.vision_masked());
                self.vision = vision;
            }
            let mut units = sim.draws();
            // Trees hit or felled, in a pose of their own.
            let (posed, hidden) = self.scene.posed(&sim.destructable_poses());
            units.extend(posed);
            gpu.renderer.hide_statics(&gpu.device, &gpu.queue, hidden);
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
            let (w, h) = (size.0.max(1) as f32, size.1.max(1) as f32);
            let mouse = [
                self.cursor.0 / w * ow3_viewer::ui::WIDTH,
                (1.0 - self.cursor.1 / h) * ow3_viewer::ui::HEIGHT,
            ];
            let mut frame = sim.interface(&self.selection, Some(mouse));
            frame
                .quads
                .extend(sim.minimap_quads(self.camera.frame_on_plane(size)));
            if let Some(start) = self.drag {
                frame.quads.extend(drag_box(start, self.cursor, size));
            }
            gpu.renderer
                .prepare_ui(&gpu.device, &gpu.queue, &mut sim.chain, &frame, size);
            let portrait = gpu.renderer.selected.and_then(|h| sim.portrait(h));
            gpu.renderer.set_portrait(
                &gpu.device,
                &gpu.queue,
                &mut sim.chain,
                portrait.as_ref().map(|(m, t)| (m.as_str(), *t)),
                size,
                elapsed,
            );
            let indicator = sim.time_indicator();
            gpu.renderer.set_time_indicator(
                &gpu.device,
                &gpu.queue,
                &mut sim.chain,
                indicator.as_ref().map(|(m, _)| m.as_str()),
                size,
                indicator.as_ref().map_or(0, |(_, t)| *t),
            );
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
    let start = sim.as_ref().and_then(Sim::camera_target);
    let camera = camera(
        &scene,
        options.camera.as_deref(),
        start.map(|(at, _)| at),
        sim.is_some(),
    );
    // A game has the game's camera, unless `--camera` set another.
    let controls = match &sim {
        Some(sim) if options.camera.is_none() => Controls::game(camera, Some(sim.camera_bounds())),
        _ => Controls::new(camera),
    };
    let mut app = App {
        scene,
        camera,
        size: options.size,
        fullscreen: !options.windowed,
        camera_moves: start.map_or(0, |(_, n)| n),
        vision: None,
        gpu: None,
        started: std::time::Instant::now(),
        sim,
        last_frame: std::time::Instant::now(),
        controls,
        cursor: (0.0, 0.0),
        ctrl: false,
        shift: false,
        selection: ow3_viewer::selection::Selection::default(),
        drag: None,
        minimap_drag: false,
        last_click: None,
        last_recall: None,
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

/// Pixels the mouse moves with the left button down before it draws a
/// rectangle instead of clicking.
const DRAG: f32 = 4.0;

/// Back to the main command card, as a new selection shows it.
fn sim_card_main(sim: &mut Option<Sim>) {
    if let Some(sim) = sim {
        sim.hud.card = ow3_viewer::hud::Card::Main;
    }
}

/// The rectangle being dragged from pixel `a` to the mouse at `b` in a
/// window `size` pixels large, as four thin green bars of the interface;
/// nothing for a drag shorter than `DRAG`.
///
/// PROVISIONAL: the game's look of it is not specified.
fn drag_box(a: (f32, f32), b: (f32, f32), size: (u32, u32)) -> Vec<ow3_viewer::ui::Quad> {
    if (b.0 - a.0).abs().max((b.1 - a.1).abs()) < DRAG {
        return Vec::new();
    }
    let (w, h) = (size.0.max(1) as f32, size.1.max(1) as f32);
    let ui = |p: (f32, f32)| {
        [
            p.0 / w * ow3_viewer::ui::WIDTH,
            (1.0 - p.1 / h) * ow3_viewer::ui::HEIGHT,
        ]
    };
    let (p, q) = (ui(a), ui(b));
    let (x0, x1) = (p[0].min(q[0]), p[0].max(q[0]));
    let (y0, y1) = (p[1].min(q[1]), p[1].max(q[1]));
    let t = 0.0015;
    [
        [x0, y0, x1, y0 + t],
        [x0, y1 - t, x1, y1],
        [x0, y0, x0 + t, y1],
        [x1 - t, y0, x1, y1],
    ]
    .into_iter()
    .map(|rect| ow3_viewer::ui::Quad {
        rect,
        uv: [0.0, 1.0, 0.0, 1.0],
        texture: String::new(),
        colour: [0.0, 1.0, 0.0, 1.0],
        blend: ow3_viewer::ui::Blend::Alpha,
    })
    .collect()
}
