//! Presenting frames: the title, the backend choice, drawing and redraw scheduling.

use super::*;

/// Whether a frame goes to the GPU surface. A desktop window uses it for the Svet backend and
/// falls back to the software surface for the egui profile, an emulated phone and the backend
/// chooser. On iOS the window is the device screen and everything is drawn by the GPU.
pub(super) fn uses_gpu(
    ios: bool,
    backend: Option<UiBackend>,
    platform: TargetPlatform,
    choosing: bool,
) -> bool {
    ios || (backend == Some(UiBackend::Svet) && platform == TargetPlatform::Desktop && !choosing)
}

/// The Dynamic Island and navigation pill are painted where a phone is emulated, not on one.
pub(super) fn emulates_device_chrome() -> bool {
    cfg!(not(any(target_os = "ios", target_os = "android")))
}

pub(super) fn software_surface(
    window: &Arc<Window>,
) -> Result<softbuffer::Surface<Arc<Window>, Arc<Window>>, String> {
    let context = softbuffer::Context::new(window.clone()).map_err(|e| e.to_string())?;
    softbuffer::Surface::new(&context, window.clone()).map_err(|e| e.to_string())
}

impl<A: NativeApp + 'static> NativeHost<A> {
    pub(super) fn sync_window_title(&mut self) {
        let status = self.status();
        let title = match self.extension() {
            Some(extension) => extension.title(&status, Instant::now()),
            None => window_title(
                self.config.mode,
                &self.config.title,
                self.backend,
                self.platform,
                self.gpu.is_some(),
            ),
        };
        if let Some(window) = &self.window {
            window.set_title(&title);
        }
    }

    /// Startup lines of the demo; an application owns its stdout.
    pub(super) fn announce_startup(&self) {
        if self.config.mode == HostMode::Application {
            return;
        }
        if self.platform != TargetPlatform::Desktop {
            println!(
                "Платформа Forma: {} ({}) · F6 — переключить на лету",
                self.platform.as_str(),
                self.platform.label()
            );
        }
        if let Some(b) = self.backend {
            println!(
                "Запуск Forma на бэкенде: {} ({}) · F6: платформа · F7: бэкенд",
                b.as_str(),
                b.label()
            );
        } else {
            println!(
                "Выбор бэкенда запуска Forma: нажмите [1] egui или [2] svet в окне (или клавиши 1 / 2; F6: платформа, F7: бэкенд)"
            );
        }
    }

    pub(super) fn select_platform(&mut self, platform: TargetPlatform) {
        self.platform = platform;
        println!(
            "Активна платформа: {} ({}) · F6 — переключить на лету",
            platform.as_str(),
            platform.label()
        );
        if let Some(extension) = self.extension() {
            extension.platform_selected(platform);
        }
        self.sync_window_title();
        if let Some(window) = &self.window {
            let (w, h) = platform.viewport_size(self.config.width, self.config.height);
            let _ = window.request_inner_size(LogicalSize::new(w, h));
            window.request_redraw();
        }
    }

    pub(super) fn select_backend(&mut self, backend: UiBackend) {
        self.backend = Some(backend);
        self.gpu_attempted = self.gpu.is_some();
        self.retry_at = None;
        self.animation_at = None;
        self.chooser = None;
        println!("Активен бэкенд: {} ({})", backend.as_str(), backend.label());
        if let Some(extension) = self.extension() {
            extension.backend_selected(backend);
        }
        self.sync_window_title();
        if let Some(window) = &self.window {
            let (w, h) = self
                .platform
                .viewport_size(self.config.width, self.config.height);
            let _ = window.request_inner_size(LogicalSize::new(w, h));
            window.request_redraw();
        }
    }

    pub(super) fn draw(&mut self) -> Result<(), String> {
        // Input events may request redraws during a surface timeout. Honor the
        // deadline here as well as in about_to_wait, before updating/acquiring.
        if self.occluded {
            return Ok(());
        }
        if let Some(deadline) = self.retry_at {
            if Instant::now() < deadline {
                return Ok(());
            }
            self.retry_at = None;
        }
        self.animation_at = None;
        let window = self
            .window
            .as_ref()
            .cloned()
            .ok_or_else(|| "Native window is not initialized".to_owned())?;
        let size = window.inner_size();
        let now = Instant::now();
        let delta_ms = (now - self.last_frame).as_secs_f32() * 1000.;
        self.last_frame = now;
        let mut animating = false;
        if let Some(chooser) = &mut self.chooser {
            chooser.tick(delta_ms.min(100.));
            animating = chooser.is_animating();
        } else {
            self.app.update(&mut |runtime| {
                runtime.tick(delta_ms.min(100.));
            })?;
        }

        let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return Ok(());
        };
        let (gpu_allowed, continuous) = self.extension().map_or((true, false), |extension| {
            (extension.allows_gpu(), extension.redraws_continuously())
        });
        let use_gpu = uses_gpu(
            cfg!(target_os = "ios"),
            self.backend,
            self.platform,
            self.chooser.is_some(),
        );
        if use_gpu && gpu_allowed && self.gpu.is_none() && !self.gpu_attempted {
            self.gpu_attempted = true;
            match pollster::block_on(crate::native_gpu::GpuWindow::new(window.clone())) {
                Ok(gpu) => {
                    self.gpu = Some(gpu);
                    self.sync_window_title();
                }
                Err(error) => {
                    self.app.on_gpu_fallback(&error)?;
                    eprintln!("Native GPU initialization failed; CPU fallback: {error}");
                    // A failed setup can leave a native layer attached to the view.
                    // Hide it before returning to the software surface when present.
                    let _ = crate::native_gpu::set_visible(&window, false);
                }
            }
        }
        if self.gpu.is_some() {
            crate::native_gpu::set_visible(&window, use_gpu)?;
        }
        if use_gpu {
            if let Some(gpu) = &mut self.gpu {
                let mut presentation =
                    Err("NativeApp.inspect did not observe a runtime".to_owned());
                if let Some(chooser) = &self.chooser {
                    presentation = gpu.draw(chooser, &window);
                } else {
                    self.app.inspect(&mut |runtime| {
                        animating = runtime.is_animating();
                        presentation = gpu.draw(runtime, &window);
                    })?;
                }
                match presentation {
                    Ok(crate::native_gpu::Presentation::Presented) => {
                        if self.chooser.is_none() {
                            self.app.on_frame_presented(NativeRenderer::Gpu)?;
                        }
                        self.retry_at = None;
                        self.gpu_health_at =
                            Some(Instant::now() + std::time::Duration::from_millis(16));
                        if animating || continuous {
                            window.request_redraw();
                        }
                        return Ok(());
                    }
                    Ok(crate::native_gpu::Presentation::Retry) => {
                        self.retry_at = Some(Instant::now() + std::time::Duration::from_millis(16));
                        return Ok(());
                    }
                    Ok(
                        crate::native_gpu::Presentation::Occluded
                        | crate::native_gpu::Presentation::ZeroSize,
                    ) => {
                        self.retry_at = None;
                        return Ok(());
                    }
                    Err(error) => {
                        self.app.on_gpu_fallback(&error)?;
                        eprintln!("Native GPU stopped; CPU fallback: {error}");
                        crate::native_gpu::set_visible(&window, false)?;
                        self.gpu = None;
                        self.retry_at = None;
                        self.gpu_health_at = None;
                        self.sync_window_title();
                    }
                }
            }
        }
        if self.surface.is_none() {
            self.surface = Some(software_surface(&window)?);
        }
        let surface = self
            .surface
            .as_mut()
            .ok_or_else(|| "Native window surface is not initialized".to_owned())?;
        surface.resize(width, height).map_err(|e| e.to_string())?;
        let mut buffer = surface.buffer_mut().map_err(|e| e.to_string())?;
        let scale = window.scale_factor() as f32;
        if let Some(chooser) = &mut self.chooser {
            chooser
                .paint_native(&mut buffer, size.width, size.height, scale)
                .map_err(str::to_owned)?;
        } else {
            animating = paint_app(&self.app, &mut buffer, size.width, size.height, scale)?;
            if shows_frame_bar(self.config.mode, self.backend) {
                apply_egui_frame_bar(&mut buffer, size.width, size.height);
            }
            if emulates_device_chrome() {
                apply_mobile_chrome(&mut buffer, size.width, size.height, scale, self.platform);
            }
        }
        buffer.present().map_err(|e| e.to_string())?;
        if self.chooser.is_none() {
            self.app.on_frame_presented(NativeRenderer::Cpu)?;
        }

        if continuous {
            // A frame rate measurement presents as fast as the surface accepts frames.
            window.request_redraw();
        } else if animating {
            // Start-to-start cadence: a costly CPU frame is not delayed again.
            // Input-triggered redraws may still paint before this deadline.
            self.animation_at = Some(now + std::time::Duration::from_nanos(16_666_667));
        }
        Ok(())
    }

    /// A worker thread asked for an update: run it now, even while the window is
    /// occluded and nothing is drawn. The demo's backend chooser precedes the
    /// application, which is updated by the first frame after the choice.
    pub(super) fn woken(&mut self) -> Result<(), String> {
        if let Some(waker) = &self.waker {
            finish_wake(&waker.shared.pending);
        }
        if self.chooser.is_some() {
            return Ok(());
        }
        self.app.update(&mut |_| {})?;
        self.update_accessibility()?;
        if self.window.is_some() {
            self.update_ime()?;
        }
        Ok(())
    }

    /// Reports the window's content size to the application.
    pub(super) fn report_size(&mut self) -> Result<(), String> {
        let Some(window) = &self.window else {
            return Ok(());
        };
        let size = window.inner_size();
        let scale = window.scale_factor() as f32;
        let (width, height) = logical_size(size.width, size.height, scale);
        if cfg!(target_os = "ios") {
            self.fit_chooser(width.round(), height.round());
        }
        self.app.on_resized(width, height, scale)
    }

    /// Rebuilds the backend chooser for a screen it does not fill. A phone gives the window
    /// its size, so the chooser follows it instead of asking for its own.
    pub(super) fn fit_chooser(&mut self, width: f32, height: f32) {
        let Some(chooser) = &mut self.chooser else {
            return;
        };
        if (chooser.width() - width).abs() <= 1. && (chooser.height() - height).abs() <= 1. {
            return;
        }
        let source = chooser_ui_for_viewport(width, height);
        if let Ok(mut next) = crate::Runtime::from_sources(&source, crate::BUTTON_COMPONENT) {
            next.preserve_interaction(chooser);
            *chooser = next;
        }
    }

    pub(super) fn scheduled_redraw(&mut self, now: Instant) -> (bool, Option<Instant>) {
        if self.occluded {
            self.retry_at = None;
            self.animation_at = None;
            return (false, self.gpu_health_at);
        }
        let mut ready = false;
        for deadline in [&mut self.retry_at, &mut self.animation_at] {
            if deadline.is_some_and(|at| at <= now) {
                *deadline = None;
                ready = true;
            }
        }
        (
            ready,
            [self.retry_at, self.animation_at, self.gpu_health_at]
                .into_iter()
                .flatten()
                .min(),
        )
    }
}
