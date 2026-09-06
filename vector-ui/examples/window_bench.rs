//! Real native-window benchmark, separate from the offscreen throughput harness.
//! Usage: --features native --example window_bench -- scene.ui Button.ui W H SECONDS
//! W/H are requested physical pixels; the OS may choose another size. One second
//! warms up, then continuous animated redraws run for SECONDS before auto-exit.
//! FPS counts successful SurfaceTexture::present calls, NOT confirmed display
//! scanouts. FIFO/compositor pacing, visibility/occlusion and other workloads can
//! affect it. Timing includes surface acquisition and present; no per-frame GPU
//! wait/readback is inserted. CPU and Rust allocator metrics cover all threads.
//! Accounted GPU buffers + ONE target payload are not total VRAM/swapchain memory.

#[cfg(feature = "native")]
mod bench_support;
#[cfg(all(feature = "native", target_os = "macos"))]
#[path = "../src/metal_resize.rs"]
mod metal_resize;

#[cfg(feature = "native")]
#[global_allocator]
static ALLOCATOR: bench_support::CountingAllocator = bench_support::CountingAllocator::new();

#[cfg(not(feature = "native"))]
fn main() {
    panic!("Build window_bench with --features native");
}

#[cfg(feature = "native")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    native::run()
}

#[cfg(feature = "native")]
mod native {
    use super::{bench_support::*, ALLOCATOR};
    use forma_vector::{gpu::Renderer, Button};
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };
    use winit::{
        application::ApplicationHandler,
        dpi::PhysicalSize,
        event::WindowEvent,
        event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
        window::{Window, WindowId},
    };

    const WARMUP: Duration = Duration::from_secs(1);

    struct Phase {
        start: Instant,
        process: ProcessSnapshot,
        allocations: AllocationSnapshot,
        uploads: u64,
    }

    struct Finished {
        wall_seconds: f64,
        process: ProcessSnapshot,
        allocations: AllocationSnapshot,
    }

    #[derive(Clone, Copy, Debug)]
    struct WindowState {
        visible: Option<bool>,
        focused: bool,
        physical_size: PhysicalSize<u32>,
        scale: f64,
    }

    struct App {
        requested: PhysicalSize<u32>,
        duration: Duration,
        initial_process: ProcessSnapshot,
        model: Button,
        pointer_inside: [f32; 2],
        window: Option<Arc<Window>>,
        gpu: Option<WindowGpu>,
        warmup_start: Option<Instant>,
        phase: Option<Phase>,
        finished: Option<Finished>,
        last_tick: Option<Instant>,
        last_present: Option<Instant>,
        retry_at: Option<Instant>,
        intervals_ms: Vec<f64>,
        draw_ms: Vec<f64>,
        frames: u64,
        nonpresent_attempts: u64,
        measured_resizes: u64,
        was_occluded: bool,
        actual: PhysicalSize<u32>,
        actual_scale: f64,
        window_at_exit: Option<WindowState>,
        error: Option<String>,
    }

    impl App {
        fn diagnostics(&self) -> String {
            let surface = self.gpu.as_ref().map(|gpu| gpu.acquired);
            let window = self.window_at_exit.map(|state| {
                (
                    state.visible,
                    state.focused,
                    state.physical_size,
                    state.scale,
                )
            });
            format!("surface acquisition totals including warmup: {surface:?}; window BEFORE event-loop exit (visible, focused, physical size, scale): {window:?}; occlusion event observed: {}", self.was_occluded)
        }

        fn stop(&mut self, event_loop: &ActiveEventLoop) {
            if self.finished.is_none() {
                if let Some(phase) = &self.phase {
                    let wall_seconds = phase.start.elapsed().as_secs_f64();
                    let allocations = ALLOCATOR.snapshot();
                    match ProcessSnapshot::capture() {
                        Ok(process) => {
                            self.finished = Some(Finished {
                                wall_seconds,
                                process,
                                allocations,
                            })
                        }
                        Err(error) => self.error = Some(error.to_string()),
                    }
                }
            }
            // winit closes every NSWindow on event-loop exit. Read visibility
            // here, not after run_app returns (which would always report false).
            self.window_at_exit = self.window.as_ref().map(|window| WindowState {
                visible: window.is_visible(),
                focused: window.has_focus(),
                physical_size: window.inner_size(),
                scale: window.scale_factor(),
            });
            event_loop.exit();
        }

        fn fail(&mut self, event_loop: &ActiveEventLoop, error: String) {
            self.error = Some(error);
            self.stop(event_loop);
        }

        fn deadline(&self) -> Option<Instant> {
            self.phase
                .as_ref()
                .map(|phase| phase.start + self.duration)
                .or_else(|| {
                    self.warmup_start
                        .map(|start| start + WARMUP + self.duration)
                })
        }

        fn draw(&mut self, event_loop: &ActiveEventLoop) {
            let now = Instant::now();
            if self
                .phase
                .as_ref()
                .is_some_and(|phase| now - phase.start >= self.duration)
            {
                self.stop(event_loop);
                return;
            }
            let Some(warmup_start) = self.warmup_start else {
                return;
            };
            if self.phase.is_none() && now - warmup_start >= WARMUP {
                let process = match ProcessSnapshot::capture() {
                    Ok(process) => process,
                    Err(error) => {
                        self.fail(event_loop, error.to_string());
                        return;
                    }
                };
                ALLOCATOR.reset_peak();
                self.phase = Some(Phase {
                    start: Instant::now(),
                    process,
                    allocations: ALLOCATOR.snapshot(),
                    uploads: self.gpu.as_ref().unwrap().renderer.uploads,
                });
                self.last_present = None;
            }

            // Reverse the hover target every 80 ms to maintain an active visual
            // workload; tick uses elapsed wall time, not a synthetic fixed delta.
            let begin = Instant::now();
            let hover = ((now - warmup_start).as_millis() / 80) % 2 == 0;
            self.model.pointer(
                if hover { self.pointer_inside[0] } else { -1. },
                self.pointer_inside[1],
                0,
            );
            let dt = self
                .last_tick
                .map_or(0., |last| (now - last).as_secs_f32() * 1000.);
            self.model.tick(dt);
            self.last_tick = Some(now);
            let result = self
                .gpu
                .as_mut()
                .unwrap()
                .draw(&self.model, self.window.as_ref().unwrap());
            let present_end = Instant::now();
            match result {
                Ok(Some((size, scale))) => {
                    self.retry_at = None;
                    self.actual = size;
                    self.actual_scale = scale;
                    if self.phase.is_some() {
                        self.frames += 1;
                        self.draw_ms
                            .push((present_end - begin).as_secs_f64() * 1000.);
                        if let Some(last) = self.last_present {
                            self.intervals_ms
                                .push((present_end - last).as_secs_f64() * 1000.);
                        }
                        self.last_present = Some(present_end);
                    }
                }
                Ok(None) => {
                    // Do not spin on Occluded/Timeout: AppKit needs event-loop
                    // time to update visibility and finish showing the window.
                    self.retry_at = Some(present_end + Duration::from_millis(16));
                    if self.phase.is_some() {
                        self.nonpresent_attempts += 1;
                    }
                }
                Err(error) => {
                    self.fail(event_loop, error);
                    return;
                }
            }
            if self
                .phase
                .as_ref()
                .is_some_and(|phase| phase.start.elapsed() >= self.duration)
            {
                self.stop(event_loop);
            } else if self.retry_at.is_none() {
                self.window.as_ref().unwrap().request_redraw();
            }
        }
    }

    impl ApplicationHandler for App {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.window.is_some() {
                return;
            }
            let window = match event_loop.create_window(
                Window::default_attributes()
                    .with_title("Forma BENCHMARK — temporary, closes automatically")
                    .with_inner_size(self.requested)
                    .with_visible(true)
                    .with_resizable(false),
            ) {
                Ok(window) => Arc::new(window),
                Err(error) => {
                    self.fail(event_loop, error.to_string());
                    return;
                }
            };
            let gpu = match pollster::block_on(WindowGpu::new(window.clone())) {
                Ok(gpu) => gpu,
                Err(error) => {
                    self.fail(event_loop, error);
                    return;
                }
            };
            self.actual = window.inner_size();
            self.actual_scale = window.scale_factor();
            // Scoped to this temporary window. CLI-launched macOS processes can
            // otherwise remain behind the terminal and get no Metal drawables.
            window.set_visible(true);
            window.focus_window();
            self.gpu = Some(gpu);
            self.window = Some(window);
            self.warmup_start = Some(Instant::now());
            self.window.as_ref().unwrap().request_redraw();
        }

        fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
            if self.window.is_none() || self.finished.is_some() {
                return;
            }
            match event {
                WindowEvent::CloseRequested => self.fail(
                    event_loop,
                    "Benchmark window closed before completion".into(),
                ),
                WindowEvent::RedrawRequested => self.draw(event_loop),
                WindowEvent::Resized(_) => {
                    if self.phase.is_some() {
                        self.measured_resizes += 1;
                    }
                    // Same transaction-sensitive AppKit path as the application.
                    #[cfg(target_os = "macos")]
                    self.draw(event_loop);
                    #[cfg(not(target_os = "macos"))]
                    self.window.as_ref().unwrap().request_redraw();
                }
                WindowEvent::ScaleFactorChanged { .. } => {
                    self.window.as_ref().unwrap().request_redraw()
                }
                WindowEvent::Occluded(occluded) => {
                    self.was_occluded |= occluded;
                    if !occluded {
                        self.window.as_ref().unwrap().request_redraw();
                    }
                }
                _ => {}
            }
        }

        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            if let Some(deadline) = self.deadline() {
                if Instant::now() >= deadline {
                    if self.phase.is_none() {
                        self.error = Some("No redraws after warmup (possibly occluded)".into());
                    }
                    self.stop(event_loop);
                } else {
                    // Also exits on time when OS suppresses redraws/occludes us.
                    if self.retry_at.is_some_and(|retry| Instant::now() >= retry) {
                        self.retry_at = None;
                        self.window.as_ref().unwrap().request_redraw();
                    }
                    event_loop.set_control_flow(ControlFlow::WaitUntil(
                        self.retry_at.map_or(deadline, |retry| retry.min(deadline)),
                    ));
                }
            }
        }
    }

    #[derive(Clone, Copy, Debug, Default)]
    struct AcquireCounters {
        success: u64,
        suboptimal: u64,
        occluded: u64,
        timeout: u64,
        outdated: u64,
        zero_size: u64,
    }

    struct WindowGpu {
        surface: wgpu::Surface<'static>,
        renderer: Renderer,
        config: wgpu::SurfaceConfiguration,
        adapter_name: String,
        backend: String,
        acquired: AcquireCounters,
    }

    impl WindowGpu {
        async fn new(window: Arc<Window>) -> Result<Self, String> {
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(
                Box::new(window.clone()),
            ));
            let surface = instance
                .create_surface(window.clone())
                .map_err(|e| e.to_string())?;
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    compatible_surface: Some(&surface),
                    ..Default::default()
                })
                .await
                .map_err(|e| e.to_string())?;
            let size = window.inner_size();
            let mut config = surface
                .get_default_config(&adapter, size.width.max(1), size.height.max(1))
                .ok_or("No GPU surface configuration")?;
            if let Some(format) = surface
                .get_capabilities(&adapter)
                .formats
                .into_iter()
                .find(|f| !f.is_srgb())
            {
                config.format = format;
            }
            config.present_mode = wgpu::PresentMode::Fifo;
            let renderer = Renderer::new(&adapter, config.format).await?;
            surface.configure(&renderer.device, &config);
            #[cfg(target_os = "macos")]
            super::metal_resize::configure(&window)?;
            let info = adapter.get_info();
            Ok(Self {
                surface,
                renderer,
                config,
                adapter_name: info.name,
                backend: format!("{:?}", info.backend),
                acquired: AcquireCounters::default(),
            })
        }

        fn draw(
            &mut self,
            model: &Button,
            window: &Window,
        ) -> Result<Option<(PhysicalSize<u32>, f64)>, String> {
            let size = window.inner_size();
            if size.width == 0 || size.height == 0 {
                self.acquired.zero_size += 1;
                return Ok(None);
            }
            if self.config.width != size.width || self.config.height != size.height {
                self.config.width = size.width;
                self.config.height = size.height;
                self.surface.configure(&self.renderer.device, &self.config);
            }
            let (frame, suboptimal) = match self.surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(frame) => {
                    self.acquired.success += 1;
                    (frame, false)
                }
                wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                    self.acquired.suboptimal += 1;
                    (frame, true)
                }
                wgpu::CurrentSurfaceTexture::Timeout => {
                    self.acquired.timeout += 1;
                    return Ok(None);
                }
                wgpu::CurrentSurfaceTexture::Occluded => {
                    self.acquired.occluded += 1;
                    return Ok(None);
                }
                wgpu::CurrentSurfaceTexture::Outdated => {
                    self.acquired.outdated += 1;
                    self.surface.configure(&self.renderer.device, &self.config);
                    return Ok(None);
                }
                error => return Err(format!("GPU surface: {error:?}")),
            };
            let size = frame.texture.size();
            let scale = window.scale_factor();
            self.renderer.draw(
                model,
                &frame.texture.create_view(&Default::default()),
                size.width,
                size.height,
                scale as f32,
                true,
                true,
            )?;
            window.pre_present_notify();
            frame.present();
            if suboptimal {
                self.config.width = 0;
            }
            Ok(Some((PhysicalSize::new(size.width, size.height), scale)))
        }
    }

    fn percentile(values: &mut [f64], q: f64) -> f64 {
        if values.is_empty() {
            return 0.;
        }
        values.sort_unstable_by(f64::total_cmp);
        values[((values.len() as f64 * q).ceil() as usize)
            .saturating_sub(1)
            .min(values.len() - 1)]
    }

    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
        let args: Vec<_> = std::env::args().collect();
        if args.len() != 6 {
            return Err(
                "window_bench scene.ui Button.ui physical_width physical_height seconds".into(),
            );
        }
        let width: u32 = args[3].parse()?;
        let height: u32 = args[4].parse()?;
        let seconds: f64 = args[5].parse()?;
        if !(100..=8192).contains(&width)
            || !(100..=8192).contains(&height)
            || !seconds.is_finite()
            || !(0.5..=30.).contains(&seconds)
        {
            return Err("Dimensions must be 100..8192; seconds must be 0.5..30".into());
        }
        let initial_process = ProcessSnapshot::capture()?;
        let model = Button::from_sources(
            &std::fs::read_to_string(&args[1])?,
            &std::fs::read_to_string(&args[2])?,
        )
        .map_err(std::io::Error::other)?;
        let bounds = model.bounds();
        let capacity = (seconds * 2000.).ceil() as usize;
        let mut app = App {
            requested: PhysicalSize::new(width, height),
            duration: Duration::from_secs_f64(seconds),
            initial_process,
            pointer_inside: [bounds[0] + bounds[2] * 0.5, bounds[1] + bounds[3] * 0.5],
            model,
            window: None,
            gpu: None,
            warmup_start: None,
            phase: None,
            finished: None,
            last_tick: None,
            last_present: None,
            retry_at: None,
            intervals_ms: Vec::with_capacity(capacity),
            draw_ms: Vec::with_capacity(capacity),
            frames: 0,
            nonpresent_attempts: 0,
            measured_resizes: 0,
            was_occluded: false,
            actual: PhysicalSize::new(0, 0),
            actual_scale: 0.,
            window_at_exit: None,
            error: None,
        };
        EventLoop::new()?.run_app(&mut app)?;
        if let Some(error) = &app.error {
            return Err(format!("{error}; {}", app.diagnostics()).into());
        }
        let phase = app.phase.as_ref().ok_or("No measurement phase")?;
        let finish = app.finished.as_ref().ok_or("No measurement result")?;
        if app.frames == 0 {
            return Err(format!(
                "No frames presented; GUI may be unavailable/occluded. {}",
                app.diagnostics()
            )
            .into());
        }
        let a = finish.allocations.delta_since(phase.allocations);
        let cpu_seconds = finish.process.cpu_seconds - phase.process.cpu_seconds;
        let gpu = app.gpu.as_ref().unwrap();
        let resources = gpu.renderer.resource_stats();
        let p50 = percentile(&mut app.intervals_ms, 0.50);
        let p95 = percentile(&mut app.intervals_ms, 0.95);
        let p99 = percentile(&mut app.intervals_ms, 0.99);
        let draw_p50 = percentile(&mut app.draw_ms, 0.50);
        let draw_p95 = percentile(&mut app.draw_ms, 0.95);
        let draw_p99 = percentile(&mut app.draw_ms, 0.99);
        let target_bytes = app.actual.width as u64 * app.actual.height as u64 * 4;
        let window_visible = match app.window_at_exit.and_then(|state| state.visible) {
            Some(true) => "true",
            Some(false) => "false",
            None => "null",
        };
        let window_focused = match app.window_at_exit.map(|state| state.focused) {
            Some(true) => "true",
            Some(false) => "false",
            None => "null",
        };
        println!(concat!(
            "{{\"benchmark\":\"native-window\",\"fps_metric\":\"successful present calls, not confirmed scanouts\",",
            "\"adapter\":{:?},\"backend\":{:?},\"present_mode\":\"Fifo\",\"requested_width\":{},\"requested_height\":{},\"actual_width\":{},\"actual_height\":{},\"scale\":{},",
            "\"warmup_seconds\":1,\"requested_seconds\":{},\"wall_seconds\":{},\"frames\":{},\"present_calls_per_second\":{},\"interval_samples\":{},",
            "\"present_interval_ms\":{{\"p50\":{},\"p95\":{},\"p99\":{}}},\"acquire_draw_present_ms\":{{\"p50\":{},\"p95\":{},\"p99\":{}}},",
            "\"nonpresent_attempts\":{},\"measured_resizes\":{},\"occlusion_event_observed\":{},\"cpu_seconds\":{},\"cpu_percent_one_core\":{},",
            "\"surface_acquisition_totals_including_warmup\":{{\"success\":{},\"suboptimal\":{},\"occluded\":{},\"timeout\":{},\"outdated\":{},\"zero_size\":{}}},",
            "\"window_visible_before_exit\":{},\"window_focused_before_exit\":{},",
            "\"rss_initial_bytes\":{},\"rss_before_bytes\":{},\"rss_after_bytes\":{},\"process_lifetime_peak_rss_bytes\":{},",
            "\"rust_allocations\":{},\"rust_reallocations\":{},\"rust_deallocations\":{},\"rust_requested_bytes\":{},\"rust_live_before_bytes\":{},\"rust_live_after_bytes\":{},\"rust_peak_live_bytes\":{},",
            "\"owned_gpu_buffer_bytes\":{},\"owned_gpu_buffer_count\":{},\"one_surface_target_payload_bytes_estimate\":{},\"unknown_swapchain_and_driver_bytes\":null,\"geometry_uploads\":{},\"cpu_geometry_cache_bytes\":{},\"cpu_shared_geometry_bytes\":{}}}"
        ), gpu.adapter_name, gpu.backend, width, height, app.actual.width, app.actual.height, app.actual_scale,
        seconds, finish.wall_seconds, app.frames, app.frames as f64 / finish.wall_seconds, app.intervals_ms.len(), p50, p95, p99, draw_p50, draw_p95, draw_p99,
        app.nonpresent_attempts, app.measured_resizes, app.was_occluded, cpu_seconds, finish.process.cpu_pct_one_core_since(phase.process, finish.wall_seconds),
        gpu.acquired.success, gpu.acquired.suboptimal, gpu.acquired.occluded, gpu.acquired.timeout, gpu.acquired.outdated, gpu.acquired.zero_size,
        window_visible, window_focused,
        app.initial_process.resident_bytes, phase.process.resident_bytes, finish.process.resident_bytes, finish.process.peak_resident_bytes,
        a.allocations, a.reallocations, a.deallocations, a.requested_bytes, phase.allocations.live_bytes, finish.allocations.live_bytes, finish.allocations.peak_live_bytes,
        resources.buffer_bytes, resources.buffer_count, target_bytes, gpu.renderer.uploads - phase.uploads, resources.cpu_geometry_cache_bytes, resources.cpu_shared_geometry_bytes);
        Ok(())
    }
}
