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
}

impl Action {
    /// The action of a key, by its name as winit gives it ("w",
    /// "ArrowUp", ...).
    pub fn of_key(name: &str) -> Option<Action> {
        Some(match name {
            "w" | "W" | "ArrowUp" => Action::Forward,
            "s" | "S" | "ArrowDown" => Action::Back,
            "a" | "A" | "ArrowLeft" => Action::Left,
            "d" | "D" | "ArrowRight" => Action::Right,
            "q" | "Q" => Action::TurnLeft,
            "e" | "E" => Action::TurnRight,
            "r" | "R" => Action::TiltUp,
            "f" | "F" => Action::TiltDown,
            "+" | "=" => Action::ZoomIn,
            "-" => Action::ZoomOut,
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

pub struct Controls {
    held: Vec<Action>,
    /// Where the camera is heading.
    pub goal: Camera,
}

impl Controls {
    pub fn new(camera: Camera) -> Controls {
        Controls {
            held: Vec::new(),
            goal: camera,
        }
    }

    /// A key went down or up; whether it is one of the controls.
    pub fn key(&mut self, action: Action, pressed: bool) {
        self.held.retain(|&a| a != action);
        if pressed {
            self.held.push(action);
        }
    }

    /// The mouse wheel turned by `lines`.
    pub fn wheel(&mut self, lines: f32) {
        self.goal.zoom(0.85f32.powf(lines));
    }

    /// Whether a key is held, so that frames should keep coming.
    pub fn moving(&self, camera: &Camera) -> bool {
        !self.held.is_empty() || *camera != self.goal
    }

    /// Moves the goal by the held keys over `dt` seconds and brings
    /// `camera` towards it; `ground` gives the ground height under a point.
    pub fn update(&mut self, camera: &mut Camera, dt: f32, ground: impl Fn(f32, f32) -> f32) {
        let dt = dt.clamp(0.0, 0.25);
        let held = |a| if self.held.contains(&a) { 1.0 } else { 0.0 };
        let forward = held(Action::Forward) - held(Action::Back);
        let right = held(Action::Right) - held(Action::Left);
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
