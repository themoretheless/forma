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
            target_x: None, target_y: None, target_width: None, target_height: None, target_radius: None,
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
        [x + self.target_x.unwrap_or((width-w)*0.5).clamp(0., width-w),
         y + self.target_y.unwrap_or((height-h)*0.5).clamp(0., height-h), w, h]
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
    let distance = dx.max(0.).hypot(dy.max(0.)) + dx.max(dy).min(0.) - r;
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
    pub fn color_at(self, p: [f32; 2]) -> [u8; 4] {
        let mut color = self.color;
        let amount = if self.full {
            1.
        } else {
            (1. - (p[0] - self.pointer[0]).hypot(p[1] - self.pointer[1]) / self.reach.max(0.0001))
                .clamp(0., 1.)
        };
        color[3] = (color[3] as f32 * amount).round() as u8;
        color
    }
    /// Two vec4 paint records; geometry references the first record's index.
    pub fn gpu(self) -> [f32; 8] {
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
    pub fn rebase(&mut self, bounds: [f32; 4], spec: &Reveal) {
        self.pointer(self.pointer, bounds, spec);
    }
    pub fn pointer(&mut self, pointer: Option<[f32; 2]>, bounds: [f32; 4], spec: &Reveal) {
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
    pub fn reduced_motion(&mut self, reduced: bool) {
        self.reduced_motion = reduced;
        if reduced {
            self.progress = self.target;
            self.duration = 0.;
        }
    }
    pub fn active(&self) -> bool {
        self.progress != self.target && self.elapsed < self.duration
    }
    pub fn tick(&mut self, dt: f32) -> bool {
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
    pub fn paint(&self, spec: &Reveal, bounds: [f32; 4], disabled: bool, focused: bool) -> Paint {
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
        if (pointer[0] - nearest[0]).hypot(pointer[1] - nearest[1]) >= reach {
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
mod tests {
    use super::*;
    #[test]
    fn proximity_expands_reverses_and_obeys_reduced_motion() {
        let spec = Reveal::default();
        let bounds = [20., 20., 200., 40.];
        let mut state = State::default();
        state.pointer(Some([10., 40.]), bounds, &spec);
        let near = state.paint(&spec, bounds, false, false);
        assert_eq!(near.reach, 72.);
        assert!(near.color_at([20., 40.])[3] > 0);
        assert_eq!(near.color_at([220., 40.])[3], 0);
        assert!(!state.active());
        state.pointer(Some([30., 40.]), bounds, &spec);
        assert!(state.active());
        state.tick(140.);
        let mid = state.paint(&spec, bounds, false, false).reach;
        assert!(mid > 72. && mid < 1188.);
        state.tick(140.);
        assert!(!state.active());
        assert!((state.paint(&spec, bounds, false, false).reach - 1188.).abs() < 0.001);
        state.pointer(Some([10., 40.]), bounds, &spec);
        assert!(state.active());
        state.reduced_motion(true);
        assert!(!state.active());
        assert_eq!(state.paint(&spec, bounds, false, false).reach, 72.);
        state.pointer(Some([30., 40.]), bounds, &spec);
        assert!(!state.active());
        assert!((state.paint(&spec, bounds, false, false).reach - 1188.).abs() < 0.001);
        state.pointer(None, bounds, &spec);
        assert_eq!(state.paint(&spec, bounds, false, false), Paint::default());
        assert!(state.paint(&spec, bounds, false, true).full);
        assert_eq!(state.paint(&spec, bounds, true, true), Paint::default());
    }
    #[test]
    fn pointer_motion_repaints_the_border_without_rebuilding_cached_text() {
        let source = "component Demo { Frame { width:260; height:100; padding:20; background:#00000000; Button { width:200; height:40; text:'Cached'; } } }";
        let template = "component Button { Rectangle { radius:8; background:#102030; Reveal { color:#ff8800; } Text { text:props.text; color:#ffffff; fontSize:14; } PointerArea { clicked -> events.clicked(); } } }";
        let mut b = crate::Button::from_sources(source, template).unwrap();
        let geometry = b.vector_snapshot(1., false);
        let before = b.content_pixels(260, 100, 1.);
        assert_eq!(b.raster_stats(), vec![1, 1]);
        b.reveal_pointer(-10., 40., true);
        assert!(!b.hover);
        assert!(!b.is_animating());
        let near = b.content_pixels(260, 100, 1.);
        let pixel = |x: usize, y: usize| (y * 260 + x) * 4;
        assert_ne!(
            &near[pixel(20, 40)..pixel(20, 40) + 4],
            &before[pixel(20, 40)..pixel(20, 40) + 4]
        );
        assert_eq!(
            &near[pixel(219, 40)..pixel(219, 40) + 4],
            &before[pixel(219, 40)..pixel(219, 40) + 4]
        );
        assert_eq!(b.raster_stats(), vec![1, 2]);
        assert!(std::sync::Arc::ptr_eq(
            &geometry,
            &b.vector_snapshot(1., false)
        ));
        let revision = b.visual_revision();
        b.reveal_pointer(-1000., 40., true);
        let far_revision = b.visual_revision();
        assert_ne!(far_revision, revision);
        b.reveal_pointer(-1001., 41., true);
        assert_eq!(
            b.visual_revision(),
            far_revision,
            "invisible proximity does not dirty the control"
        );
        b.pointer(30., 40., 0);
        assert!(b.is_animating());
        b.set_reduced_motion(true);
        assert!(!b.is_animating());
        b.reveal_pointer(0., 0., false);
        b.focus(true);
        let focused = b.content_pixels(260, 100, 1.);
        assert_eq!(
            &focused[pixel(20, 40)..pixel(20, 40) + 4],
            &focused[pixel(219, 40)..pixel(219, 40) + 4]
        );
        let mut disabled = crate::Button::from_sources(
            &source.replace("width:200;", "width:200; disabled:true;"),
            template,
        )
        .unwrap();
        let before = disabled.content_pixels(260, 100, 1.);
        disabled.reveal_pointer(30., 40., true);
        disabled.focus(true);
        assert!(!disabled.is_animating());
        assert_eq!(disabled.content_pixels(260, 100, 1.), before);
    }

    #[cfg(feature = "gpu")]
    #[test]
    #[ignore = "requires a real GPU; run explicitly with --features gpu -- --ignored"]
    fn reveal_cpu_gpu_agree_across_states_and_dpi_without_geometry_uploads() {
        pollster::block_on(async {
            let adapter = wgpu::Instance::default()
                .request_adapter(&Default::default())
                .await
                .expect("real GPU required");
            let mut renderer = crate::gpu::Renderer::new(&adapter, wgpu::TextureFormat::Rgba8Unorm)
                .await
                .unwrap();
            for target in ["", "targetX:10; targetWidth:18; targetHeight:18; targetRadius:5;", "targetX:10; targetWidth:18; targetHeight:18; targetRadius:9;"] {
            let template = format!("component Button {{ Rectangle {{ radius:8; Reveal {{ width:2; color:#e6782080; {target} }} PointerArea {{ clicked -> events.clicked(); }} }} }}");
            for multi in [false, true] {
                let second = if multi {
                    "Button { key:'second'; width:140; height:35; }"
                } else {
                    ""
                };
                let source = format!("component Demo {{ Frame {{ width:180; height:130; padding:20; gap:10; background:#00000000; Button {{ key:'first'; width:140; height:35; }} {second} }} }}");
                for scale in [1., 1.25, 2.] {
                    let (w, h) = ((180. * scale) as u32, (130. * scale) as u32);
                    let texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("Reveal CPU/GPU agreement"),
                        size: wgpu::Extent3d {
                            width: w,
                            height: h,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                            | wgpu::TextureUsages::COPY_SRC,
                        view_formats: &[],
                    });
                    let view = texture.create_view(&Default::default());
                    let stride = (w * 4).div_ceil(256) * 256;
                    let readback = renderer.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("Reveal readback"),
                        size: stride as u64 * h as u64,
                        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                        mapped_at_creation: false,
                    });
                    for state in [
                        "hidden",
                        "outside",
                        "inside-half",
                        "inside-full",
                        "focus",
                        "disabled",
                        "reduced",
                    ] {
                        let source = if state == "disabled" {
                            source.replace("width:140;", "width:140; disabled:true;")
                        } else {
                            source.clone()
                        };
                        let mut model = crate::Runtime::from_sources(&source, &template).unwrap();
                        renderer
                            .draw(&model, &view, w, h, scale, false, false)
                            .unwrap();
                        let uploads = renderer.uploads;
                        match state {
                            "outside" => model.reveal_pointer(-10., 40., true),
                            "inside-half" => {
                                model.reveal_pointer(30., 40., true);
                                model.tick(140.);
                            }
                            "inside-full" => {
                                model.reveal_pointer(30., 40., true);
                                model.tick(280.);
                            }
                            "focus" => {
                                model.focus_control(0);
                            }
                            "disabled" => {
                                model.reveal_pointer(30., 40., true);
                                model.tick(280.);
                            }
                            "reduced" => {
                                model.set_reduced_motion(true);
                                model.reveal_pointer(30., 40., true);
                            }
                            _ => {}
                        }
                        renderer
                            .draw(&model, &view, w, h, scale, false, false)
                            .unwrap();
                        assert_eq!(
                            renderer.uploads, uploads,
                            "Reveal must change paint, never upload glyph geometry"
                        );
                        let mut encoder =
                            renderer.device.create_command_encoder(&Default::default());
                        encoder.copy_texture_to_buffer(
                            wgpu::TexelCopyTextureInfo {
                                texture: &texture,
                                mip_level: 0,
                                origin: wgpu::Origin3d::ZERO,
                                aspect: wgpu::TextureAspect::All,
                            },
                            wgpu::TexelCopyBufferInfo {
                                buffer: &readback,
                                layout: wgpu::TexelCopyBufferLayout {
                                    offset: 0,
                                    bytes_per_row: Some(stride),
                                    rows_per_image: Some(h),
                                },
                            },
                            wgpu::Extent3d {
                                width: w,
                                height: h,
                                depth_or_array_layers: 1,
                            },
                        );
                        renderer.queue.submit(Some(encoder.finish()));
                        let (send, recv) = std::sync::mpsc::channel();
                        readback
                            .slice(..)
                            .map_async(wgpu::MapMode::Read, move |result| {
                                send.send(result).unwrap()
                            });
                        renderer
                            .device
                            .poll(wgpu::PollType::wait_indefinitely())
                            .unwrap();
                        recv.recv().unwrap().unwrap();
                        let mapped = readback.slice(..).get_mapped_range();
                        let cpu = model.content_pixels(w, h, scale);
                        let mut max_error = 0;
                        for y in 0..h as usize {
                            for x in 0..w as usize {
                                let p = &cpu[(y * w as usize + x) * 4..][..4];
                                for k in 0..4 {
                                    let expected = if k == 3 {
                                        p[k]
                                    } else {
                                        ((p[k] as u32 * p[3] as u32 + 127) / 255) as u8
                                    };
                                    max_error = max_error.max(
                                        expected.abs_diff(mapped[y * stride as usize + x * 4 + k]),
                                    );
                                }
                            }
                        }
                        assert!(max_error <= 2, "Reveal {state}, multi={multi}, scale={scale}: channel error {max_error}/255");
                        drop(mapped);
                        readback.unmap();
                    }
                }
            }
            }
        });
    }
}
