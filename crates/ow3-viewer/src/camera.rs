//! An RTS camera looking down at a point of the map, and the matrices it
//! needs. World coordinates: x east, y north, z up; matrices are
//! column-major, as WGSL expects.

pub type Mat4 = [[f32; 4]; 4];

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt().max(1e-9);
    [a[0] / l, a[1] / l, a[2] / l]
}

pub fn multiply(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut out = [[0.0; 4]; 4];
    for (c, column) in out.iter_mut().enumerate() {
        for (r, value) in column.iter_mut().enumerate() {
            *value = (0..4).map(|k| a[k][r] * b[c][k]).sum();
        }
    }
    out
}

/// A right-handed view matrix.
fn look_at(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> Mat4 {
    let f = normalize(sub(target, eye));
    let s = normalize(cross(f, up));
    let u = cross(s, f);
    [
        [s[0], u[0], -f[0], 0.0],
        [s[1], u[1], -f[1], 0.0],
        [s[2], u[2], -f[2], 0.0],
        [-dot(s, eye), -dot(u, eye), dot(f, eye), 1.0],
    ]
}

/// A perspective projection to the 0..1 depth range of wgpu.
fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    let f = 1.0 / (fov_y / 2.0).tan();
    [
        [f / aspect, 0.0, 0.0, 0.0],
        [0.0, f, 0.0, 0.0],
        [0.0, 0.0, far / (near - far), -1.0],
        [0.0, 0.0, near * far / (near - far), 0.0],
    ]
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// The point looked at, on the ground.
    pub target: [f32; 3],
    pub distance: f32,
    /// Degrees around the vertical axis; 90 looks north.
    pub yaw: f32,
    /// Degrees below the horizon.
    pub pitch: f32,
    /// Vertical field of view in degrees.
    pub fov: f32,
}

impl Camera {
    /// Looking at `target` from the south, at a usual game angle.
    pub fn new(target: [f32; 3]) -> Camera {
        Camera {
            target,
            distance: 2500.0,
            yaw: 90.0,
            pitch: 56.0,
            fov: 70.0,
        }
    }

    pub fn eye(&self) -> [f32; 3] {
        let (yaw, pitch) = (self.yaw.to_radians(), self.pitch.to_radians());
        let back = [
            -yaw.cos() * pitch.cos(),
            -yaw.sin() * pitch.cos(),
            pitch.sin(),
        ];
        [
            self.target[0] + back[0] * self.distance,
            self.target[1] + back[1] * self.distance,
            self.target[2] + back[2] * self.distance,
        ]
    }

    pub fn view_projection(&self, aspect: f32) -> Mat4 {
        let view = look_at(self.eye(), self.target, [0.0, 0.0, 1.0]);
        let far = self.distance * 4.0 + 20_000.0;
        let projection = perspective(self.fov.to_radians(), aspect, 16.0, far);
        multiply(&projection, &view)
    }

    /// Moves the target by `right` and `forward` world units in the
    /// directions the camera faces on the ground.
    pub fn pan(&mut self, right: f32, forward: f32) {
        let yaw = self.yaw.to_radians();
        let (fx, fy) = (yaw.cos(), yaw.sin());
        self.target[0] += fx * forward + fy * right;
        self.target[1] += fy * forward - fx * right;
    }

    pub fn zoom(&mut self, factor: f32) {
        self.distance = (self.distance * factor).clamp(200.0, 30_000.0);
    }

    pub fn turn(&mut self, yaw: f32, pitch: f32) {
        self.yaw = (self.yaw + yaw).rem_euclid(360.0);
        self.pitch = (self.pitch + pitch).clamp(10.0, 89.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(m: &Mat4, p: [f32; 3]) -> [f32; 3] {
        let v = [p[0], p[1], p[2], 1.0];
        let mut out = [0.0f32; 4];
        for (r, value) in out.iter_mut().enumerate() {
            *value = (0..4).map(|c| m[c][r] * v[c]).sum();
        }
        [out[0] / out[3], out[1] / out[3], out[2] / out[3]]
    }

    /// The target is in the middle of the screen; a point north of it is
    /// higher up the screen, a point east of it to the right.
    #[test]
    fn looks_north_at_the_target() {
        let camera = Camera::new([1000.0, 2000.0, 0.0]);
        let m = camera.view_projection(1.0);
        let centre = project(&m, camera.target);
        assert!(
            centre[0].abs() < 1e-4 && centre[1].abs() < 1e-4,
            "{centre:?}"
        );
        assert!((0.0..1.0).contains(&centre[2]));
        let north = project(&m, [1000.0, 2500.0, 0.0]);
        assert!(north[1] > 0.1, "{north:?}");
        let east = project(&m, [1500.0, 2000.0, 0.0]);
        assert!(east[0] > 0.1, "{east:?}");
    }

    #[test]
    fn pans_in_the_direction_it_faces() {
        let mut camera = Camera::new([0.0, 0.0, 0.0]);
        camera.pan(0.0, 100.0);
        assert!((camera.target[1] - 100.0).abs() < 1e-3 && camera.target[0].abs() < 1e-3);
        camera.pan(100.0, 0.0);
        assert!((camera.target[0] - 100.0).abs() < 1e-3);
    }
}
