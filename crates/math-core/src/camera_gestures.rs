//! Numerical orbit/pan/pinch/wheel updates, independent of browser event APIs.
use std::f64::consts::PI;
#[derive(Clone, Copy, Debug)]
pub struct State {
    pub yaw: f64,
    pub pitch: f64,
    pub dist: f64,
    pub tx: f64,
    pub ty: f64,
    pub tz: f64,
}
impl State {
    pub fn finite(self) -> Option<Self> {
        [self.yaw, self.pitch, self.dist, self.tx, self.ty, self.tz]
            .iter()
            .all(|v| v.is_finite())
            .then_some(self)
    }
}
pub fn clamp_distance(distance: f64) -> f64 {
    if distance.is_finite() {
        distance.clamp(0.01, 1e12)
    } else {
        50.
    }
}
pub fn wheel(distance: f64, delta: f64) -> f64 {
    clamp_distance(distance * (delta.clamp(-1000., 1000.) * 0.001).exp())
}
pub fn orbit(mut state: State, dx: f64, dy: f64) -> Option<State> {
    state.finite()?;
    if !dx.is_finite() || !dy.is_finite() {
        return None;
    }
    state.yaw -= dx * 0.005;
    if state.yaw > PI || state.yaw < -PI {
        state.yaw = ((state.yaw + PI) % (2. * PI) + 2. * PI) % (2. * PI) - PI
    }
    state.pitch = (state.pitch + dy * 0.005).clamp(-PI / 2. + 0.001, PI / 2. - 0.001);
    state.finite()
}
pub fn pan(mut state: State, dx: f64, dy: f64, height: f64, fov: f64) -> Option<State> {
    state.finite()?;
    if ![dx, dy, height, fov].iter().all(|v| v.is_finite()) {
        return None;
    }
    let scale = 2. * state.dist * (fov / 2.).tan() / height.max(1.);
    let cy = state.yaw.cos();
    let sy = state.yaw.sin();
    let cp = state.pitch.cos();
    let sp = state.pitch.sin();
    state.tx += (-dx * cy + dy * (-sp * sy)) * scale;
    state.ty += (-dx * sy + dy * (sp * cy)) * scale;
    state.tz += dy * cp * scale;
    state.finite()
}
pub fn pinch(points: [[f64; 2]; 4], mut state: State, height: f64, fov: f64) -> Option<State> {
    if !points.iter().flatten().all(|v| v.is_finite()) {
        return None;
    }
    let [a, b, c, d] = points;
    let previous = (b[0] - a[0]).hypot(b[1] - a[1]);
    let current = (d[0] - c[0]).hypot(d[1] - c[1]);
    if previous > 1e-6 && current > 1e-6 {
        state.dist =
            clamp_distance(state.dist * (previous / current).clamp((-1_f64).exp(), 1_f64.exp()))
    }
    pan(
        state,
        (c[0] + d[0] - a[0] - b[0]) / 2.,
        (c[1] + d[1] - a[1] - b[1]) / 2.,
        height,
        fov,
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> State {
        State {
            yaw: 0.,
            pitch: 0.,
            dist: 10.,
            tx: 0.,
            ty: 0.,
            tz: 0.,
        }
    }
    #[test]
    fn orbit_wraps_and_pan_uses_camera_basis() {
        let s = orbit(state(), -1000., 1000.).unwrap();
        assert!((-PI..=PI).contains(&s.yaw));
        assert!(s.pitch < PI / 2.);
        let p = pan(state(), 10., 5., 100., PI / 4.).unwrap();
        assert!(p.tx < 0. && p.tz > 0.);
        assert_eq!(p.ty, 0.);
    }
    #[test]
    fn distance_limits_and_nonfinite_fallback() {
        assert_eq!(clamp_distance(f64::NAN), 50.);
        assert_eq!(wheel(f64::INFINITY, 1.), 50.);
        assert_eq!(wheel(1e12, 1000.), 1e12);
        assert_eq!(wheel(10., f64::NAN), 50.);
    }
    #[test]
    fn collapsed_pinch_pans_without_spurious_zoom() {
        let s = pinch(
            [[0., 0.], [0., 0.], [1., 1.], [1., 1.]],
            state(),
            100.,
            PI / 4.,
        )
        .unwrap();
        assert_eq!(s.dist, 10.);
        assert!(s.tx < 0. && s.tz > 0.);
        assert!(pan(state(), f64::NAN, 0., 100., 1.).is_none());
    }
}
