//! Colour transitions of a control.

pub(crate) struct Animation {
    pub(crate) from: [f32; 4],
    pub(crate) current: [f32; 4],
    pub(crate) target: [f32; 4],
    pub(crate) elapsed: f32,
    pub(crate) duration: f32,
}
impl Animation {
    pub(crate) fn new(color: [u8; 4]) -> Self {
        let c = color.map(|x| x as f32);
        Self {
            from: c,
            current: c,
            target: c,
            elapsed: 0.,
            duration: 0.,
        }
    }
    pub(crate) fn target(&mut self, color: [u8; 4], duration: f32) {
        let c = color.map(|x| x as f32);
        if c != self.target {
            self.from = self.current;
            self.target = c;
            self.duration = duration;
            self.elapsed = 0.;
            if duration == 0. {
                self.current = c;
            }
        }
    }
    pub(crate) fn active(&self) -> bool {
        self.current != self.target && self.elapsed < self.duration
    }
    pub(crate) fn tick(&mut self, dt: f32) -> bool {
        if !self.active() {
            return false;
        }
        self.elapsed = (self.elapsed + dt).min(self.duration);
        let t = self.elapsed / self.duration;
        let eased = t * t * (3. - 2. * t);
        for k in 0..4 {
            self.current[k] = self.from[k] + (self.target[k] - self.from[k]) * eased;
        }
        if t == 1. {
            self.current = self.target;
        }
        self.active()
    }
    pub(crate) fn color(&self) -> [u8; 4] {
        self.current.map(|x| x.clamp(0., 255.) as u8)
    }
}
