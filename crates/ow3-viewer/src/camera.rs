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

/// A perspective projection to the 0..1 depth range of wgpu with the
/// tangents of half the horizontal and vertical fields of view.
fn perspective(tan_x: f32, tan_y: f32, near: f32, far: f32) -> Mat4 {
    [
        [1.0 / tan_x, 0.0, 0.0, 0.0],
        [0.0, 1.0 / tan_y, 0.0, 0.0],
        [0.0, 0.0, far / (near - far), -1.0],
        [0.0, 0.0, near * far / (near - far), 0.0],
    ]
}

/// The tangents of the half-angles across and up a frame `aspect` times as
/// wide as high, for field of view `fov` in radians spread over the
/// frame's diagonal in proportion to its sides (SPEC-0023, "Field of
/// view"): up and down `v = (fov / 2) / sqrt(1 + aspect^2)`, across
/// `aspect * tan(v)`.
pub fn frame_tangents(fov: f32, aspect: f32) -> (f32, f32) {
    let v = fov * (1.0 / (1.0 + aspect * aspect).sqrt()) * 0.5;
    let t = v.tan();
    (aspect * t, t)
}

/// The world frame of the interface (SPEC-0023, SPEC-0028): left, bottom,
/// right, top in its 0.8 x 0.6 units. The 3D view is computed for it and
/// goes on beyond it under the bars.
const WORLD_FRAME: [f32; 4] = [0.0, 0.13, 0.8, 0.58];

/// For the window: the world frame's half height and the height of its
/// centre, both in normalised device coordinates.
fn world_frame_ndc() -> (f32, f32) {
    let [_, bottom, _, top] = WORLD_FRAME;
    let half = (top - bottom) / 0.6;
    let centre = (top + bottom) / 0.6 - 1.0;
    (half, centre)
}

/// The view and projection of a camera stored in a model (a portrait's),
/// from `position` to `target` with field of view `fov` (radians) and the
/// clipping distances `near` and `far`, into a frame `aspect` times as
/// wide as high (SPEC-0028, "Portrait").
pub fn model_view_projection(
    position: [f32; 3],
    target: [f32; 3],
    fov: f32,
    near: f32,
    far: f32,
    aspect: f32,
) -> Mat4 {
    let view = look_at(position, target, [0.0, 0.0, 1.0]);
    let (tan_x, tan_y) = frame_tangents(fov, aspect);
    let projection = perspective(tan_x, tan_y, near, far);
    multiply(&projection, &view)
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
    /// Field of view in degrees across the diagonal of a 4:3 frame, which
    /// is stretched over the window whatever its shape (SPEC-0023,
    /// `CAMERA_FIELD_FIELD_OF_VIEW`).
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

    /// The tangents of the half-angles across and up the world frame
    /// (16:9, SPEC-0023).
    fn tangents(&self) -> (f32, f32) {
        let [left, bottom, right, top] = WORLD_FRAME;
        frame_tangents(self.fov.to_radians(), (right - left) / (top - bottom))
    }

    /// The projection fills the world frame of the window, the target at
    /// its centre, and goes on beyond it (SPEC-0023).
    pub fn view_projection(&self) -> Mat4 {
        let view = look_at(self.eye(), self.target, [0.0, 0.0, 1.0]);
        let far = self.distance * 4.0 + 20_000.0;
        let (tan_x, tan_y) = self.tangents();
        let (half, centre) = world_frame_ndc();
        let mut projection = perspective(tan_x, tan_y / half, 16.0, far);
        // y in the window = half * y in the frame + centre: the centre
        // times the clip w (the view's -z).
        projection[2][1] = -centre;
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

    /// The eye and the direction of the ray through pixel (`x`, `y`) of a
    /// view `size` pixels large.
    pub fn ray(&self, x: f32, y: f32, size: (u32, u32)) -> ([f32; 3], [f32; 3]) {
        let (w, h) = (size.0.max(1) as f32, size.1.max(1) as f32);
        let eye = self.eye();
        let f = normalize(sub(self.target, eye));
        let s = normalize(cross(f, [0.0, 0.0, 1.0]));
        let u = cross(s, f);
        let (tan_x, tan_y) = self.tangents();
        let (half, centre) = world_frame_ndc();
        let (nx, ny) = (
            (2.0 * x / w - 1.0) * tan_x,
            (1.0 - 2.0 * y / h - centre) / half * tan_y,
        );
        let direction = normalize([
            f[0] + s[0] * nx + u[0] * ny,
            f[1] + s[1] * nx + u[1] * ny,
            f[2] + s[2] * nx + u[2] * ny,
        ]);
        (eye, direction)
    }

    /// Where the rays through the corners of the world frame of a window
    /// `size` pixels large meet the plane of the target's height, round
    /// it from the lower left (SPEC-0028, "Camera rectangle"); `None` when
    /// a corner looks above it.
    pub fn frame_on_plane(&self, size: (u32, u32)) -> Option<[[f32; 2]; 4]> {
        let (w, h) = (size.0 as f32, size.1 as f32);
        let [_, bottom, _, top] = WORLD_FRAME;
        let y = |v: f32| (1.0 - v / 0.6) * h;
        let corners = [(0.0, y(bottom)), (w, y(bottom)), (w, y(top)), (0.0, y(top))];
        let mut out = [[0.0; 2]; 4];
        for (o, (x, y)) in out.iter_mut().zip(corners) {
            let (eye, d) = self.ray(x, y, size);
            if d[2] >= 0.0 {
                return None;
            }
            let t = (self.target[2] - eye[2]) / d[2];
            *o = [eye[0] + d[0] * t, eye[1] + d[1] * t];
        }
        Some(out)
    }

    /// Where the ray through pixel (`x`, `y`) first meets the ground of
    /// height `ground(x, y)`: marched in steps of 16, then halved down.
    pub fn ground_point(
        &self,
        x: f32,
        y: f32,
        size: (u32, u32),
        ground: impl Fn(f32, f32) -> f32,
    ) -> Option<[f32; 3]> {
        let (eye, d) = self.ray(x, y, size);
        let at = |t: f32| [eye[0] + d[0] * t, eye[1] + d[1] * t, eye[2] + d[2] * t];
        let below = |t: f32| {
            let p = at(t);
            p[2] <= ground(p[0], p[1])
        };
        let far = self.distance * 4.0 + 20_000.0;
        let mut t = 0.0;
        while t < far {
            let next = t + 16.0;
            if below(next) {
                let (mut lo, mut hi) = (t, next);
                for _ in 0..16 {
                    let mid = (lo + hi) / 2.0;
                    if below(mid) {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                return Some(at(hi));
            }
            t = next;
        }
        None
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

    /// The target is at the centre of the world frame, 0.355 of the window
    /// up from its bottom (SPEC-0023); a point north of it is higher up the
    /// screen, a point east of it to the right.
    #[test]
    fn looks_north_at_the_target() {
        let camera = Camera::new([1000.0, 2000.0, 0.0]);
        let m = camera.view_projection();
        let centre = project(&m, camera.target);
        let (_, frame_centre) = world_frame_ndc();
        assert!((frame_centre - (2.0 * 0.355 / 0.6 - 1.0)).abs() < 1e-3);
        assert!(
            centre[0].abs() < 1e-4 && (centre[1] - frame_centre).abs() < 1e-4,
            "{centre:?}"
        );
        assert!((0.0..1.0).contains(&centre[2]));
        let north = project(&m, [1000.0, 2500.0, 0.0]);
        assert!(north[1] > frame_centre + 0.1, "{north:?}");
        let east = project(&m, [1500.0, 2000.0, 0.0]);
        assert!(east[0] > 0.1, "{east:?}");
    }

    /// The tangents SPEC-0023 and SPEC-0028 give: the world frame at 70 and
    /// 120 degrees, the footman's portrait at 45.
    #[test]
    fn tangents_over_the_diagonal() {
        let world = 0.8 / 0.45;
        let close = |(x, y): (f32, f32), (ex, ey): (f32, f32)| {
            (x - ex).abs() < 5e-4 && (y - ey).abs() < 5e-4
        };
        let t70 = frame_tangents(70f32.to_radians(), world);
        assert!(close(t70, (0.5490, 0.3088)), "{t70:?}");
        let t120 = frame_tangents(120f32.to_radians(), world);
        assert!(close(t120, (1.0027, 0.5640)), "{t120:?}");
        let portrait = frame_tangents(std::f32::consts::FRAC_PI_4, 0.09 / 0.0856);
        assert!(close(portrait, (0.2917, 0.2774)), "{portrait:?}");
    }

    /// The ray through a pixel meets flat ground where that ground point
    /// is drawn.
    #[test]
    fn rays_meet_the_ground_where_it_is_drawn() {
        let mut camera = Camera::new([1000.0, 2000.0, 50.0]);
        camera.yaw = 70.0;
        let size = (1600, 900);
        let m = camera.view_projection();
        for (x, y) in [(800.0, 450.0), (100.0, 800.0), (1500.0, 100.0)] {
            let p = camera.ground_point(x, y, size, |_, _| 50.0).unwrap();
            assert!((p[2] - 50.0).abs() < 0.5, "{p:?}");
            let s = project(&m, p);
            let pixel = [(s[0] + 1.0) * 800.0, (1.0 - s[1]) * 450.0];
            assert!(
                (pixel[0] - x).abs() < 0.5 && (pixel[1] - y).abs() < 0.5,
                "{pixel:?} for {x} {y}"
            );
        }
    }

    /// Verification 1 of SPEC-0023: straight down from 1000 in a window of
    /// 1016 x 734, a tile is 118.4 pixels across and 114.1 up the screen by
    /// the rule (116.75 and 112.5 measured, the models read standing above
    /// the ground), the interface's 4:3 stretched; between fields of view
    /// of 70 and 120 degrees the scale differs by 1.826.
    #[test]
    fn a_tile_seen_from_above() {
        let mut camera = Camera::new([0.0, 0.0, 0.0]);
        camera.distance = 1000.0;
        camera.pitch = 89.999;
        let across = |camera: &Camera, w: f32| {
            project(&camera.view_projection(), [128.0, 0.0, 0.0])[0] * w / 2.0
        };
        let (w, h) = (1016.0, 734.0);
        let a = across(&camera, w);
        let north = project(&camera.view_projection(), [0.0, 128.0, 0.0]);
        let (_, centre) = world_frame_ndc();
        let up = (north[1] - centre) * h / 2.0;
        assert!((a - 118.4).abs() < 0.2, "{a}");
        assert!((up - 114.1).abs() < 0.2, "{up}");
        let mut wide = camera;
        wide.fov = 120.0;
        let ratio = a / across(&wide, w);
        assert!((ratio - 1.826).abs() < 0.005, "{ratio}");
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

#[cfg(test)]
mod frame_tests {
    use super::*;

    /// The minimap's camera rectangle against SPEC-0028 ("Camera
    /// rectangle"), in the pixels of IceCrown's minimap in a 1024 x 768
    /// window: x = world x / 130.16 + 104.41, y = 669.2 - world y / 136.35.
    #[test]
    fn camera_rectangles_of_the_spec() {
        for (pitch, distance, expected) in [
            (56.0, 1650.0, [95.6, 113.2, 663.5, 672.9]),
            (56.0, 3000.0, [88.4, 120.4, 658.8, 676.0]),
            (89.9, 1650.0, [97.5, 111.4, 665.5, 672.9]),
        ] {
            let camera = Camera {
                target: [0.0; 3],
                distance,
                yaw: 90.0,
                pitch,
                fov: 70.0,
            };
            let points = camera.frame_on_plane((1024, 768)).unwrap();
            let px = |x: f32| x / 130.16 + 104.41;
            let py = |y: f32| 669.2 - y / 136.35;
            let xs = points.map(|p| px(p[0]));
            let ys = points.map(|p| py(p[1]));
            let got = [
                xs.iter().copied().fold(f32::INFINITY, f32::min),
                xs.iter().copied().fold(f32::NEG_INFINITY, f32::max),
                ys.iter().copied().fold(f32::INFINITY, f32::min),
                ys.iter().copied().fold(f32::NEG_INFINITY, f32::max),
            ];
            for (g, e) in got.iter().zip(expected) {
                assert!((g - e).abs() < 0.3, "{pitch} {distance}: {got:?}");
            }
        }
    }
}
