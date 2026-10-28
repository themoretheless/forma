//! Orbit display camera: fit depth, dolly/optical zoom and WebGPU matrices.
use crate::viewport::{Matrix, Point};
pub struct Options {
    pub yaw: f64,
    pub pitch: f64,
    pub distance: f64,
    pub target: Point,
    pub aspect: f64,
    pub fov_y: f64,
    pub perspective: bool,
    pub bounds: Option<(Point, Point)>,
    pub background_radius: f64,
}
pub struct Frame {
    pub eye: Point,
    pub view_projection: Matrix,
    pub eye_distance: f64,
    pub near: f64,
    pub far: f64,
}
fn length(v: Point) -> f64 {
    v[0].hypot(v[1]).hypot(v[2])
}
fn dot(a: Point, b: Point) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: Point, b: Point) -> Point {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
// Preserve the renderer's f32 matrix storage at each stage, while computing in f64.
fn stored(m: Matrix) -> Matrix {
    m.map(|v| v as f32 as f64)
}
fn view(eye: Point, target: Point) -> Matrix {
    let mut z = std::array::from_fn(|i| eye[i] - target[i]);
    let mut norm = length(z);
    if norm < 1e-10 {
        z = [0., 0., 1.];
        norm = 1.
    }
    z = z.map(|v| v / norm);
    let mut x = cross([0., 0., 1.], z);
    let mut norm = length(x);
    if norm < 1e-10 {
        x = cross(
            if z[2].abs() > 0.9 {
                [0., 1., 0.]
            } else {
                [0., 0., 1.]
            },
            z,
        );
        norm = length(x)
    }
    x = x.map(|v| v / norm);
    let y = cross(z, x);
    stored([
        x[0],
        x[1],
        x[2],
        -dot(x, eye),
        y[0],
        y[1],
        y[2],
        -dot(y, eye),
        z[0],
        z[1],
        z[2],
        -dot(z, eye),
        0.,
        0.,
        0.,
        1.,
    ])
}
pub fn frame(o: &Options) -> Option<Frame> {
    if ![
        o.yaw,
        o.pitch,
        o.distance,
        o.aspect,
        o.fov_y,
        o.background_radius,
    ]
    .iter()
    .chain(&o.target)
    .all(|v| v.is_finite())
        || o.distance <= 0.
        || o.aspect <= 0.
        || o.fov_y <= 0.
        || o.fov_y >= std::f64::consts::PI
        || o.background_radius < 0.
    {
        return None;
    }
    let cp = o.pitch.cos();
    let backward = [cp * o.yaw.sin(), -cp * o.yaw.cos(), o.pitch.sin()];
    let mut front_depth = 0.;
    let mut eye_distance = o.distance;
    let mut extent = o.background_radius;
    if let Some((min, max)) = o.bounds {
        if !(0..3).all(|i| min[i].is_finite() && max[i].is_finite() && min[i] <= max[i]) {
            return None;
        }
        let center: Point = std::array::from_fn(|i| min[i] * 0.5 + max[i] * 0.5);
        let half: Point = std::array::from_fn(|i| max[i] * 0.5 - min[i] * 0.5);
        let offset: Point = std::array::from_fn(|i| center[i] - o.target[i]);
        front_depth = dot(backward, offset) + dot(backward.map(f64::abs), half);
        let radius = length(half);
        eye_distance = eye_distance.max(front_depth + 0.01_f64.max(radius * 0.05));
        extent = extent.max(length(offset) + radius);
    }
    let eye = std::array::from_fn(|i| o.target[i] + eye_distance * backward[i]);
    let near = 0.001_f64.max((eye_distance - front_depth) * 0.05);
    let far = (near + 1.).max(eye_distance + extent * 1.1);
    let half_height = o.distance * (o.fov_y / 2.).tan();
    let mut projection = [0.; 16];
    let nf = 1. / (near - far);
    if o.perspective {
        let f = 1. / (half_height / eye_distance).atan().tan();
        projection[0] = f / o.aspect;
        projection[5] = f;
        projection[10] = far * nf;
        projection[11] = far * near * nf;
        projection[14] = -1.;
    } else {
        projection[0] = 2. / (2. * half_height * o.aspect);
        projection[5] = 2. / (2. * half_height);
        projection[10] = nf;
        projection[11] = near * nf;
        projection[15] = 1.;
    }
    let projection = stored(projection);
    let view = view(eye, o.target);
    let matrix = stored(std::array::from_fn(|index| {
        let r = index / 4;
        let c = index % 4;
        let mut sum = 0.;
        for k in 0..4 {
            sum += projection[r * 4 + k] * view[k * 4 + c]
        }
        sum
    }));
    if !matrix
        .iter()
        .chain(&eye)
        .chain(&[eye_distance, near, far])
        .all(|v| v.is_finite())
    {
        return None;
    }
    Some(Frame {
        eye,
        view_projection: matrix,
        eye_distance,
        near,
        far,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn options() -> Options {
        Options {
            yaw: 0.,
            pitch: 0.,
            distance: 0.01,
            target: [0.; 3],
            aspect: 1.,
            fov_y: 1.,
            perspective: true,
            bounds: Some(([-1.; 3], [1.; 3])),
            background_radius: 10.,
        }
    }
    #[test]
    fn keeps_eye_outside_bounds_and_extends_far_plane() {
        let f = frame(&options()).unwrap();
        assert!(f.eye_distance > 1.);
        assert!(f.near > 0.);
        assert!(f.far > f.eye_distance + 10.);
    }
    #[test]
    fn refuses_invalid_parameters() {
        let mut o = options();
        o.distance = 0.;
        assert!(frame(&o).is_none());
        o.distance = 1.;
        o.bounds = Some(([2.; 3], [1.; 3]));
        assert!(frame(&o).is_none());
    }
}
