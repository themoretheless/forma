//! Reusable proximity-border primitive and its paint-only interaction state.
//! Coordinates and radii are logical pixels in the same space as the control.

#[derive(Clone, Debug, PartialEq)]
pub struct Reveal {
    pub target_x: Option<f32>,
    pub target_y: Option<f32>,
    pub target_width: Option<f32>,
    pub target_height: Option<f32>,
    pub target_radius: Option<f32>,
    pub width: f32,
    pub color: [u8; 4],
    pub radius: f32,
    pub stop: f32,
    pub active_radius: f32,
    pub active_stop: f32,
    pub duration_ms: f32,
}

impl Default for Reveal {
    fn default() -> Self {
        Self {
            target_x: None,
            target_y: None,
            target_width: None,
            target_height: None,
            target_radius: None,
            width: 1.,
            color: [222, 106, 25, 255],
            radius: 120.,
            stop: 0.6,
            active_radius: 1200.,
            active_stop: 0.99,
            duration_ms: 280.,
        }
    }
}

impl Reveal {
    pub(crate) fn bounds(&self, bounds: [f32; 4]) -> [f32; 4] {
        let [x, y, width, height] = bounds;
        let w = self.target_width.unwrap_or(width).min(width);
        let h = self.target_height.unwrap_or(height).min(height);
        [
            x + self
                .target_x
                .unwrap_or((width - w) * 0.5)
                .clamp(0., width - w),
            y + self
                .target_y
                .unwrap_or((height - h) * 0.5)
                .clamp(0., height - h),
            w,
            h,
        ]
    }
}

pub(crate) fn band_coverage(
    p: [f32; 2],
    bounds: [f32; 4],
    radius: f32,
    width: f32,
    scale: f32,
) -> f32 {
    let r = radius.min(bounds[2] * 0.5).min(bounds[3] * 0.5);
    let dx = (p[0] - bounds[0] - bounds[2] * 0.5).abs() - (bounds[2] * 0.5 - r);
    let dy = (p[1] - bounds[1] - bounds[3] * 0.5).abs() - (bounds[3] * 0.5 - r);
    let inv_scale_half = 0.5 / scale;
    let distance = if dx <= 0. && dy <= 0. {
        let d = dx.max(dy) - r;
        if d <= -width - inv_scale_half || d >= inv_scale_half {
            return 0.;
        }
        d
    } else {
        dx.max(0.).hypot(dy.max(0.)) + dx.max(dy).min(0.) - r
    };
    let outer = (0.5 - distance * scale).clamp(0., 1.);
    let inner = (0.5 - (distance + width) * scale).clamp(0., 1.);
    (outer - inner).max(0.)
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Paint {
    pub pointer: [f32; 2],
    pub reach: f32,
    pub color: [u8; 4],
    pub full: bool,
}

impl Paint {
    pub(crate) fn color_at(self, p: [f32; 2]) -> [u8; 4] {
        let mut color = self.color;
        let amount = if self.full {
            1.
        } else {
            let reach = self.reach.max(0.0001);
            let dx = p[0] - self.pointer[0];
            let dy = p[1] - self.pointer[1];
            let d2 = dx * dx + dy * dy;
            if d2 >= reach * reach {
                color[3] = 0;
                return color;
            }
            (1. - d2.sqrt() / reach).clamp(0., 1.)
        };
        color[3] = (color[3] as f32 * amount).round() as u8;
        color
    }
    /// Two vec4 paint records; geometry references the first record's index.
    pub(crate) fn gpu(self) -> [f32; 8] {
        [
            self.pointer[0],
            self.pointer[1],
            self.reach,
            if self.full { 1. } else { 0. },
            self.color[0] as f32 / 255.,
            self.color[1] as f32 / 255.,
            self.color[2] as f32 / 255.,
            self.color[3] as f32 / 255.,
        ]
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct State {
    pointer: Option<[f32; 2]>,
    from: f32,
    progress: f32,
    target: f32,
    elapsed: f32,
    duration: f32,
    reduced_motion: bool,
}

// CSS `ease`: cubic-bezier(.25,.1,.25,1), matching the reference transition.
fn ease(t: f32) -> f32 {
    if t >= 1. {
        return 1.;
    }
    let cubic = |v: f32, a: f32, b: f32| {
        3. * (1. - v).powi(2) * v * a + 3. * (1. - v) * v * v * b + v.powi(3)
    };
    let (mut lo, mut hi) = (0., 1.);
    for _ in 0..16 {
        let mid = (lo + hi) * 0.5;
        if cubic(mid, 0.25, 0.25) < t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    cubic((lo + hi) * 0.5, 0.1, 1.)
}

impl State {
    pub(crate) fn rebase(&mut self, bounds: [f32; 4], spec: &Reveal) {
        self.pointer(self.pointer, bounds, spec);
    }
    pub(crate) fn pointer(&mut self, pointer: Option<[f32; 2]>, bounds: [f32; 4], spec: &Reveal) {
        self.pointer = pointer.filter(|p| p.iter().all(|n| n.is_finite()));
        let Some([x, y]) = self.pointer else {
            self.progress = 0.;
            self.target = 0.;
            self.from = 0.;
            self.duration = 0.;
            return;
        };
        let inside = x >= bounds[0]
            && y >= bounds[1]
            && x <= bounds[0] + bounds[2]
            && y <= bounds[1] + bounds[3];
        let target = if inside { 1. } else { 0. };
        if target != self.target {
            self.from = self.progress;
            self.target = target;
            self.elapsed = 0.;
            self.duration = if self.reduced_motion {
                0.
            } else {
                spec.duration_ms
            };
            if self.duration == 0. {
                self.progress = target;
            }
        }
    }
    pub(crate) fn reduced_motion(&mut self, reduced: bool) {
        self.reduced_motion = reduced;
        if reduced {
            self.progress = self.target;
            self.duration = 0.;
        }
    }
    pub(crate) fn active(&self) -> bool {
        self.progress != self.target && self.elapsed < self.duration
    }
    pub(crate) fn tick(&mut self, dt: f32) -> bool {
        if self.active() {
            self.elapsed = (self.elapsed + dt).min(self.duration);
            self.progress =
                self.from + (self.target - self.from) * ease(self.elapsed / self.duration);
            if self.elapsed == self.duration {
                self.progress = self.target;
            }
        }
        self.active()
    }
    pub(crate) fn paint(
        &self,
        spec: &Reveal,
        bounds: [f32; 4],
        disabled: bool,
        focused: bool,
    ) -> Paint {
        if disabled || spec.width == 0. || spec.color[3] == 0 {
            return Paint::default();
        }
        if focused {
            return Paint {
                color: spec.color,
                full: true,
                ..Paint::default()
            };
        }
        let Some(pointer) = self.pointer else {
            return Paint::default();
        };
        let radius = spec.radius + (spec.active_radius - spec.radius) * self.progress;
        let stop = spec.stop + (spec.active_stop - spec.stop) * self.progress;
        let reach = radius * stop;
        let nearest = [
            pointer[0].clamp(bounds[0], bounds[0] + bounds[2]),
            pointer[1].clamp(bounds[1], bounds[1] + bounds[3]),
        ];
        // Far-away movement must not dirty every control or repaint cached text.
        let dx = pointer[0] - nearest[0];
        let dy = pointer[1] - nearest[1];
        if dx * dx + dy * dy >= reach * reach {
            return Paint::default();
        }
        Paint {
            pointer,
            reach,
            color: spec.color,
            full: false,
        }
    }
}

#[cfg(test)]
mod tests;
