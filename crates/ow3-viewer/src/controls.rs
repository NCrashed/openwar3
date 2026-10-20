//! Smooth camera controls: held keys move a goal camera at steady speeds
//! per second, the mouse wheel zooms it, and the camera follows the goal
//! with exponential easing every frame.

use crate::camera::Camera;

/// What a held key does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Action {
    Forward,
    Back,
    Left,
    Right,
    TurnLeft,
    TurnRight,
    TiltUp,
    TiltDown,
    ZoomIn,
    ZoomOut,
    /// The game camera's rotation keys (SPEC-0023): Insert and Delete.
    RotateLeft,
    RotateRight,
}

impl Action {
    /// The action of a key, by its name as winit gives it ("q",
    /// "ArrowUp", ...).
    pub fn of_key(name: &str) -> Option<Action> {
        Some(match name {
            "ArrowUp" => Action::Forward,
            "ArrowDown" => Action::Back,
            "ArrowLeft" => Action::Left,
            "ArrowRight" => Action::Right,
            "q" | "Q" => Action::TurnLeft,
            "e" | "E" => Action::TurnRight,
            "r" | "R" => Action::TiltUp,
            "f" | "F" => Action::TiltDown,
            "+" | "=" => Action::ZoomIn,
            "-" => Action::ZoomOut,
            "Insert" => Action::RotateLeft,
            "Delete" => Action::RotateRight,
            _ => return None,
        })
    }
}

/// Screens of ground per second that panning covers, as a share of the
/// camera's distance.
const PAN_SPEED: f32 = 1.2;
/// Degrees per second of turning and tilting.
const TURN_SPEED: f32 = 90.0;
const TILT_SPEED: f32 = 45.0;
/// How many times closer per second the zoom keys bring the camera.
const ZOOM_SPEED: f32 = 2.5;
/// How fast the camera catches up with the goal: the share left after a
/// second is `exp(-EASE)`.
const EASE: f32 = 12.0;

/// Pixels from the edge of the screen in which the mouse pans the camera.
pub const EDGE: f32 = 4.0;

/// The zoom levels of the game camera (SPEC-0023, `[Camera]` of
/// `UI\\MiscData.txt`): the angle of attack in degrees below the horizon
/// (360 less `AOA`) and the distance.
///
/// PROVISIONAL: the last level is the one measured, angle 335 and
/// distance 1200, where the table gives 339 and 1100 (SPEC-0023 open
/// question 2).
pub const LEVELS: [(f32, f32); 6] = [
    (56.0, 1650.0),
    (49.0, 1600.0),
    (42.0, 1500.0),
    (35.0, 1400.0),
    (28.0, 1275.0),
    (25.0, 1200.0),
];
/// Seconds from one zoom level to the next, linear in time.
const LEVEL_SECONDS: f32 = 1.0;
/// The rotation without a key, with Insert held and with Delete held.
const ROTATION: [f32; 3] = [90.0, 20.0, 160.0];
/// Degrees per second the rotation turns: 70 in about 0.5 s.
const ROTATION_SPEED: f32 = 140.0;
/// World units per second the arrow keys move the target (`Forward` and
/// `Strafe` of `[CameraRates]`).
const SCROLL_SPEED: f32 = 3000.0;

/// The camera of the game (SPEC-0023, "The game camera and the player's
/// controls"): zoom levels, rotation while Insert or Delete is held, and
/// scrolling at a steady speed inside the camera bounds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Game {
    level: usize,
    /// The angle and distance the change to `level` started from, and
    /// the seconds it has gone on.
    from: (f32, f32),
    elapsed: f32,
    /// The least x and y and the greatest x and y of the target.
    pub bounds: Option<[f32; 4]>,
}

impl Game {
    pub fn new(bounds: Option<[f32; 4]>) -> Game {
        Game {
            level: 0,
            from: LEVELS[0],
            elapsed: LEVEL_SECONDS,
            bounds,
        }
    }

    /// A camera at zoom level 0 with the game's rotation and field of
    /// view, looking at `target`.
    pub fn camera(target: [f32; 3]) -> Camera {
        let (pitch, distance) = LEVELS[0];
        Camera {
            target,
            distance,
            yaw: ROTATION[0],
            pitch,
            fov: 70.0,
        }
    }

    pub fn level(&self) -> usize {
        self.level
    }
}

pub struct Controls {
    held: Vec<Action>,
    /// Panning by the mouse at the edge of the screen: -1, 0 or 1 to the
    /// right and forward.
    edge: [f32; 2],
    /// Where the camera is heading.
    pub goal: Camera,
    /// The game camera's state, while a game runs; else the free camera of
    /// the map viewer.
    pub game: Option<Game>,
}

impl Controls {
    pub fn new(camera: Camera) -> Controls {
        Controls {
            held: Vec::new(),
            edge: [0.0; 2],
            goal: camera,
            game: None,
        }
    }

    /// The game camera: zoom levels, Insert and Delete, steady scrolling
    /// inside `bounds`.
    pub fn game(camera: Camera, bounds: Option<[f32; 4]>) -> Controls {
        Controls {
            game: Some(Game::new(bounds)),
            ..Controls::new(camera)
        }
    }

    /// One step of the game camera's zoom: `closer` one level up, else one
    /// down; past the first and last levels nothing changes.
    pub fn step_zoom(&mut self, closer: bool) {
        let Some(game) = &mut self.game else {
            return;
        };
        let level = if closer {
            (game.level + 1).min(LEVELS.len() - 1)
        } else {
            game.level.saturating_sub(1)
        };
        if level != game.level {
            game.level = level;
            game.from = (self.goal.pitch, self.goal.distance);
            game.elapsed = 0.0;
        }
    }

    /// A key went down or up; whether it is one of the controls.
    pub fn key(&mut self, action: Action, pressed: bool) {
        self.held.retain(|&a| a != action);
        if pressed {
            self.held.push(action);
        }
    }

    /// The mouse is at (`x`, `y`) pixels of a view `size` large, or out of
    /// it (`None`): at an edge it pans the camera that way.
    pub fn cursor(&mut self, at: Option<(f32, f32)>, size: (f32, f32)) {
        let side = |p: f32, length: f32| {
            if p < EDGE {
                -1.0
            } else if p >= length - EDGE {
                1.0
            } else {
                0.0
            }
        };
        self.edge = at.map_or([0.0; 2], |(x, y)| [side(x, size.0), -side(y, size.1)]);
    }

    /// The mouse wheel turned by `lines`.
    pub fn wheel(&mut self, lines: f32) {
        if self.game.is_some() {
            // One notch, one level.
            if lines != 0.0 {
                self.step_zoom(lines > 0.0);
            }
            return;
        }
        self.goal.zoom(0.85f32.powf(lines));
    }

    /// Whether a key is held, so that frames should keep coming.
    pub fn moving(&self, camera: &Camera) -> bool {
        let zooming = self
            .game
            .is_some_and(|g| g.elapsed < LEVEL_SECONDS || self.goal.yaw != self.rotation());
        zooming || !self.held.is_empty() || self.edge != [0.0; 2] || *camera != self.goal
    }

    /// The rotation the game camera turns to by the held keys.
    fn rotation(&self) -> f32 {
        if self.held.contains(&Action::RotateLeft) {
            ROTATION[1]
        } else if self.held.contains(&Action::RotateRight) {
            ROTATION[2]
        } else {
            ROTATION[0]
        }
    }

    /// Moves the goal by the held keys over `dt` seconds and brings
    /// `camera` towards it; `ground` gives the ground height under a point.
    pub fn update(&mut self, camera: &mut Camera, dt: f32, ground: impl Fn(f32, f32) -> f32) {
        let dt = dt.clamp(0.0, 0.25);
        if self.game.is_some() {
            self.update_game(camera, dt, ground);
            return;
        }
        let held = |a| if self.held.contains(&a) { 1.0 } else { 0.0 };
        let forward = (held(Action::Forward) - held(Action::Back) + self.edge[1]).clamp(-1.0, 1.0);
        let right = (held(Action::Right) - held(Action::Left) + self.edge[0]).clamp(-1.0, 1.0);
        let step = self.goal.distance * PAN_SPEED * dt;
        self.goal.pan(right * step, forward * step);
        self.goal.turn(
            (held(Action::TurnLeft) - held(Action::TurnRight)) * TURN_SPEED * dt,
            (held(Action::TiltUp) - held(Action::TiltDown)) * TILT_SPEED * dt,
        );
        let zoom = held(Action::ZoomOut) - held(Action::ZoomIn);
        if zoom != 0.0 {
            self.goal.zoom(ZOOM_SPEED.powf(zoom * dt));
        }
        self.goal.target[2] = ground(self.goal.target[0], self.goal.target[1]);

        let k = 1.0 - (-EASE * dt).exp();
        let ease = |from: f32, to: f32| {
            let next = from + (to - from) * k;
            // Close enough: stop, so that frames can rest.
            if (to - next).abs() < 1e-3 { to } else { next }
        };
        for i in 0..3 {
            camera.target[i] = ease(camera.target[i], self.goal.target[i]);
        }
        camera.distance = ease(camera.distance, self.goal.distance);
        camera.pitch = ease(camera.pitch, self.goal.pitch);
        // The shorter way round.
        let turn = (self.goal.yaw - camera.yaw + 540.0).rem_euclid(360.0) - 180.0;
        camera.yaw = (ease(0.0, turn) + camera.yaw).rem_euclid(360.0);
        if (self.goal.yaw - camera.yaw).abs() < 1e-3 {
            camera.yaw = self.goal.yaw;
        }
    }

    /// The game camera over `dt` seconds: the target scrolls at a steady
    /// speed and stays inside the bounds, the angle and distance go
    /// linearly to the zoom level, the rotation to that of the held key.
    /// The camera is the goal at once.
    fn update_game(&mut self, camera: &mut Camera, dt: f32, ground: impl Fn(f32, f32) -> f32) {
        let held = |a| if self.held.contains(&a) { 1.0 } else { 0.0 };
        let forward = (held(Action::Forward) - held(Action::Back) + self.edge[1]).clamp(-1.0, 1.0);
        let right = (held(Action::Right) - held(Action::Left) + self.edge[0]).clamp(-1.0, 1.0);
        // PROVISIONAL: while the camera is rotated, along the screen's
        // directions on the ground.
        let step = SCROLL_SPEED * dt;
        self.goal.pan(right * step, forward * step);
        let rotation = self.rotation();
        let Some(game) = &mut self.game else {
            return;
        };
        if let Some([x0, y0, x1, y1]) = game.bounds
            && x0 <= x1
            && y0 <= y1
        {
            self.goal.target[0] = self.goal.target[0].clamp(x0, x1);
            self.goal.target[1] = self.goal.target[1].clamp(y0, y1);
        }
        self.goal.target[2] = ground(self.goal.target[0], self.goal.target[1]);

        game.elapsed = (game.elapsed + dt).min(LEVEL_SECONDS);
        let t = game.elapsed / LEVEL_SECONDS;
        let (pitch, distance) = LEVELS[game.level];
        self.goal.pitch = game.from.0 + (pitch - game.from.0) * t;
        self.goal.distance = game.from.1 + (distance - game.from.1) * t;

        let turn = ROTATION_SPEED * dt;
        let left = rotation - self.goal.yaw;
        self.goal.yaw = if left.abs() <= turn {
            rotation
        } else {
            self.goal.yaw + turn.copysign(left)
        };
        *camera = self.goal;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Holding a key for a second moves the goal by the pan speed, and the
    /// camera gets close to it.
    #[test]
    fn held_keys_move_steadily() {
        let mut camera = Camera::new([0.0, 0.0, 0.0]);
        let mut controls = Controls::new(camera);
        controls.key(Action::Forward, true);
        for _ in 0..60 {
            controls.update(&mut camera, 1.0 / 60.0, |_, _| 0.0);
        }
        let expected = camera.distance * PAN_SPEED;
        assert!((controls.goal.target[1] - expected).abs() < 1.0);
        assert!(controls.goal.target[1] - camera.target[1] < expected * 0.1);
        controls.key(Action::Forward, false);
        for _ in 0..200 {
            controls.update(&mut camera, 1.0 / 60.0, |_, _| 0.0);
        }
        assert_eq!(camera, controls.goal);
        assert!(!controls.moving(&camera));
    }

    /// The game camera (SPEC-0023): a wheel notch moves one level in a
    /// second, linearly; Insert turns to 20 in half a second and back;
    /// Up scrolls 3000 a second and stops at the bounds.
    #[test]
    fn the_game_camera() {
        let mut camera = Game::camera([0.0, 0.0, 0.0]);
        let mut controls = Controls::game(camera, Some([-1000.0, -1000.0, 1000.0, 2000.0]));
        controls.wheel(1.0);
        for _ in 0..10 {
            controls.update(&mut camera, 0.05, |_, _| 0.0);
        }
        assert!(
            (camera.distance - 1625.0).abs() < 0.1,
            "{}",
            camera.distance
        );
        assert!((camera.pitch - 52.5).abs() < 0.01, "{}", camera.pitch);
        for _ in 0..10 {
            controls.update(&mut camera, 0.05, |_, _| 0.0);
        }
        assert_eq!((camera.pitch, camera.distance), LEVELS[1]);
        // Past the levels nothing changes.
        for _ in 0..8 {
            controls.wheel(-1.0);
        }
        assert_eq!(controls.game.unwrap().level(), 0);

        controls.key(Action::RotateLeft, true);
        for _ in 0..10 {
            controls.update(&mut camera, 0.05, |_, _| 0.0);
        }
        assert_eq!(camera.yaw, 20.0);
        controls.key(Action::RotateLeft, false);
        for _ in 0..10 {
            controls.update(&mut camera, 0.05, |_, _| 0.0);
        }
        assert_eq!(camera.yaw, 90.0);

        controls.key(Action::Forward, true);
        controls.update(&mut camera, 0.25, |_, _| 0.0);
        assert!(
            (camera.target[1] - 750.0).abs() < 0.1,
            "{:?}",
            camera.target
        );
        for _ in 0..8 {
            controls.update(&mut camera, 0.25, |_, _| 0.0);
        }
        assert_eq!(camera.target[1], 2000.0);
    }

    /// Turning goes the shorter way across 0 degrees.
    #[test]
    fn turns_the_shorter_way() {
        let mut camera = Camera::new([0.0, 0.0, 0.0]);
        camera.yaw = 350.0;
        let mut controls = Controls::new(camera);
        controls.goal.yaw = 10.0;
        controls.update(&mut camera, 0.05, |_, _| 0.0);
        assert!(camera.yaw > 350.0 || camera.yaw < 10.0, "{}", camera.yaw);
    }
}
