use forma::native_gpu::{GpuWindow, Presentation};
#[cfg(not(any(target_os = "ios", target_os = "android")))]
use forma::native_window::apply_mobile_chrome;
#[cfg(target_os = "ios")]
use forma::native_window::{
    chooser_ui_for_viewport, two_buttons_ui_for_viewport, two_buttons_ui_for_viewport_with_state,
};
use forma::{
    frame_stats::FrameStats,
    native_window::{
        AuxiliaryWindows, DevProject, DevShortcut, DevTitle, HotReload, TargetPlatform, UiBackend,
        apply_egui_frame_bar, build_chooser_runtime, dev_shortcut, dev_title, inspect_report,
    },
};
use std::{
    num::NonZeroU32,
    sync::Arc,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, MouseButton, Touch, TouchPhase, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, NamedKey},
    window::{Window, WindowId},
};

const TITLE_INTERVAL: Duration = Duration::from_millis(250);

/// Outcome of one software frame, reported instead of applied in place: the pixel buffer
/// borrows the surface, and Linux keeps that borrow alive until the block ends.
enum CpuFrame {
    Presented,
    Painted(String),
    Failed(String),
}

struct App {
    ui_backend: Option<UiBackend>,
    platform: TargetPlatform,
    chooser: Option<forma::Runtime>,
    inspect_mode: bool,
    inspect_report: String,
    a11y_report: String,
    shift: bool,
    command: bool,
    control: bool,
    alt: bool,
    composing: bool,
    composition_owner: Option<(u64, i32)>,
    ime_cancelled: bool,
    ime_allowed: bool,
    accessibility: Option<accesskit_winit::Adapter>,
    accessibility_proxy: Option<winit::event_loop::EventLoopProxy<accesskit_winit::Event>>,
    accessibility_active: bool,
    accessibility_ids: forma::native_accessibility::PublishedTree,
    window: Option<Arc<Window>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    gpu: Option<GpuWindow>,
    surface_size: Option<(u32, u32)>,
    secondary_windows: AuxiliaryWindows,
    render_error: Option<String>,
    binding_error: Option<String>,
    surface_state: Presentation,
    button: forma::Runtime,
    pointer: (f32, f32),
    touch_last: Option<(f32, f32)>,
    last: Instant,
    frame_stats: FrameStats,
    continuous_redraw: bool,
    log_fps: bool,
    next_title_update: Instant,
    last_title: String,
    retry_at: Option<Instant>,
    hot: HotReload,
}

impl App {
    fn observe_ime_owner(&mut self) {
        standalone_observe_ime(
            &self.button,
            &mut self.composing,
            self.composition_owner,
            &mut self.ime_cancelled,
        );
    }
    fn ime_event(&mut self, event: winit::event::Ime) {
        standalone_ime_event(
            &mut self.button,
            &mut self.composing,
            &mut self.composition_owner,
            &mut self.ime_cancelled,
            event,
        );
    }
    fn update_accessibility(&mut self) {
        if !self.accessibility_active {
            return;
        }
        let scale = self
            .window
            .as_ref()
            .map_or(1., |window| window.scale_factor());
        let active_rt = self.chooser.as_ref().unwrap_or(&self.button);
        if let Ok(Some(update)) = self.accessibility_ids.update_if_changed(active_rt, scale) {
            if let Some(adapter) = &mut self.accessibility {
                adapter.update_if_active(|| update);
            }
        }
    }
    fn accessibility_action(&mut self, request: &accesskit::ActionRequest) {
        if request.target_tree != accesskit::TreeId::ROOT {
            return;
        }
        let pending_ime = self.composing || self.composition_owner.is_some();
        let previous = self.button.native_editor_session();
        let accepted = if let Some(chooser) = &mut self.chooser {
            self.accessibility_ids.apply(chooser, request)
        } else {
            self.accessibility_ids.apply(&mut self.button, request)
        };
        if self.chooser.is_none() {
            if (pending_ime
                && (previous.is_none() || self.button.native_editor_session() != previous))
                || (accepted
                    && pending_ime
                    && matches!(
                        request.action,
                        accesskit::Action::SetValue | accesskit::Action::SetTextSelection
                    ))
            {
                standalone_cancel_ime(
                    &mut self.button,
                    &mut self.composing,
                    &mut self.composition_owner,
                    &mut self.ime_cancelled,
                );
            }
            let binding_error = self.button.sync_ui_bindings_native().err();
            self.observe_ime_owner();
            self.set_binding_error(binding_error);
        }
    }

    fn set_render_error(&mut self, error: Option<String>) {
        if self.render_error != error {
            if let Some(error) = &error {
                eprintln!("Ошибка отрисовки: {error}");
            }
            self.render_error = error;
            self.next_title_update = Instant::now();
        }
    }

    fn set_binding_error(&mut self, error: Option<String>) {
        if self.binding_error != error {
            if let Some(error) = &error {
                eprintln!("Ошибка bindings: {error}");
            }
            self.binding_error = error;
            self.next_title_update = Instant::now();
        }
    }

    fn present_result(&mut self, result: Presentation, window: &Window) {
        let now = Instant::now();
        if self.surface_state != result {
            self.next_title_update = now;
        }
        self.surface_state = result;
        self.retry_at = None;
        match result {
            Presentation::Presented => {
                self.frame_stats.presented(now);
                if self.button.is_animating() || self.continuous_redraw {
                    window.request_redraw();
                }
            }
            Presentation::Retry => self.retry_at = Some(now + Duration::from_millis(16)),
            Presentation::Occluded if self.continuous_redraw => {
                self.retry_at = Some(now + Duration::from_millis(100));
            }
            _ => {}
        }
    }

    fn select_platform(&mut self, platform: TargetPlatform) {
        self.platform = platform;
        self.frame_stats = FrameStats::default();
        self.next_title_update = Instant::now();
        println!(
            "Активна платформа: {} ({}) · F6 — переключить на лету",
            platform.as_str(),
            platform.label()
        );
        if let Some(window) = &self.window {
            #[cfg(not(target_os = "ios"))]
            {
                let (w, h) = platform.viewport_size(
                    self.button.render_width().max(1.0),
                    self.button.render_height().max(1.0),
                );
                let _ = window.request_inner_size(LogicalSize::new(w, h));
                if platform != TargetPlatform::Desktop && self.surface.is_none() {
                    if let Ok(context) = softbuffer::Context::new(window.clone()) {
                        if let Ok(surf) = softbuffer::Surface::new(&context, window.clone()) {
                            self.surface = Some(surf);
                            self.surface_size = None;
                        }
                    }
                }
            }
            window.request_redraw();
        }
        self.update_title(Instant::now());
    }

    fn select_backend(&mut self, backend: UiBackend) {
        self.ui_backend = Some(backend);
        self.button
            .set_svet_document_scheduling(backend == UiBackend::Svet);
        self.chooser = None;
        self.frame_stats = FrameStats::default();
        self.next_title_update = Instant::now();
        println!(
            "Активен бэкенд: {} ({}) · F7 — переключить на лету",
            backend.as_str(),
            backend.label()
        );
        if let Some(window) = &self.window {
            #[cfg(not(target_os = "ios"))]
            {
                let (w, h) = self.platform.viewport_size(
                    self.button.render_width().max(1.0),
                    self.button.render_height().max(1.0),
                );
                let _ = window.request_inner_size(LogicalSize::new(w, h));
                if (backend == UiBackend::Egui || self.platform != TargetPlatform::Desktop)
                    && self.surface.is_none()
                {
                    if let Ok(context) = softbuffer::Context::new(window.clone()) {
                        if let Ok(surf) = softbuffer::Surface::new(&context, window.clone()) {
                            self.surface = Some(surf);
                            self.surface_size = None;
                        }
                    }
                }
            }
            window.request_redraw();
        }
        self.update_title(Instant::now());
    }

    /// Shared by normal redraw and the synchronous AppKit live-resize path.
    fn draw(&mut self, window: &Arc<Window>) {
        let size = window.inner_size();
        let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            self.present_result(Presentation::ZeroSize, window);
            return;
        };
        #[cfg(target_os = "ios")]
        {
            let scale = (window.scale_factor() as f32).max(1.0);
            let logical_w = (size.width as f32 / scale).round();
            let logical_h = (size.height as f32 / scale).round();
            if let Some(chooser) = &mut self.chooser {
                if (chooser.width() - logical_w).abs() > 1.0
                    || (chooser.height() - logical_h).abs() > 1.0
                {
                    let src = chooser_ui_for_viewport(logical_w, logical_h);
                    if let Ok(mut next) =
                        forma::Runtime::from_sources(&src, forma::BUTTON_COMPONENT)
                    {
                        next.preserve_interaction(chooser);
                        *chooser = next;
                    }
                }
            } else if self.hot.is_demo()
                && ((self.button.width() - logical_w).abs() > 1.0
                    || (self.button.height() - logical_h).abs() > 1.0)
            {
                let src = two_buttons_ui_for_viewport(logical_w, logical_h);
                if let Ok(mut next) = forma::Runtime::from_sources(&src, forma::BUTTON_COMPONENT) {
                    next.preserve_interaction(&self.button);
                    next.set_svet_document_scheduling(self.ui_backend == Some(UiBackend::Svet));
                    self.button = next;
                }
            }
        }
        #[cfg(target_os = "ios")]
        let use_gpu = true;
        #[cfg(not(target_os = "ios"))]
        let use_gpu = self.chooser.is_none()
            && self.ui_backend != Some(UiBackend::Egui)
            && self.platform == TargetPlatform::Desktop;
        #[cfg(target_os = "macos")]
        if self.gpu.is_some() {
            if let Err(error) = forma::native_gpu::set_visible(window, use_gpu) {
                self.set_render_error(Some(error));
            }
        }
        let active_runtime = self.chooser.as_ref().unwrap_or(&self.button);
        if use_gpu {
            if let Some(gpu) = &mut self.gpu {
                match gpu.draw(active_runtime, window) {
                    Ok(result) => {
                        self.set_render_error(None);
                        self.present_result(result, window);
                        return;
                    }
                    Err(error) => {
                        eprintln!("GPU остановлен, CPU fallback: {error}");
                        #[cfg(target_os = "macos")]
                        if let Err(error) = forma::native_gpu::set_visible(window, false) {
                            eprintln!("Metal layer fallback: {error}");
                        }
                        self.gpu = None;
                        self.frame_stats = FrameStats::default(); // Do not mix backend cadences.
                        self.next_title_update = Instant::now();
                        let context = softbuffer::Context::new(window.clone()).unwrap();
                        self.surface =
                            Some(softbuffer::Surface::new(&context, window.clone()).unwrap());
                        self.surface_size = None;
                    }
                }
            }
        } else if self.surface.is_none() {
            let context = softbuffer::Context::new(window.clone()).unwrap();
            self.surface = Some(softbuffer::Surface::new(&context, window.clone()).unwrap());
            self.surface_size = None;
        }
        // Linux keeps the `Result` temporary of `buffer_mut` alive through its error arm, so
        // the frame runs in a block and the app is only touched after that borrow ends.
        let frame = 'cpu: {
            let surface = self.surface.as_mut().unwrap();
            if self.surface_size != Some((size.width, size.height)) {
                if let Err(error) = surface.resize(width, height) {
                    break 'cpu CpuFrame::Failed(error.to_string());
                }
                self.surface_size = Some((size.width, size.height));
            }
            let mut buffer = match surface.buffer_mut() {
                Ok(buffer) => buffer,
                Err(error) => break 'cpu CpuFrame::Failed(error.to_string()),
            };
            let scale = window.scale_factor() as f32;
            let error = active_runtime
                .paint_native(&mut buffer, size.width, size.height, scale)
                .err()
                .map(str::to_owned);
            if error.is_some() {
                buffer.fill(0x111319);
            } else {
                if self.chooser.is_none() && self.ui_backend == Some(UiBackend::Egui) {
                    apply_egui_frame_bar(&mut buffer, size.width, size.height);
                }
                #[cfg(not(any(target_os = "ios", target_os = "android")))]
                if self.chooser.is_none() {
                    apply_mobile_chrome(&mut buffer, size.width, size.height, scale, self.platform);
                }
            }
            // As on GPU, count only successful presentation, not a render attempt.
            match buffer.present() {
                Ok(()) => error.map_or(CpuFrame::Presented, CpuFrame::Painted),
                Err(error) => break 'cpu CpuFrame::Failed(error.to_string()),
            }
        };
        match frame {
            CpuFrame::Presented => {
                self.set_render_error(None);
                self.present_result(Presentation::Presented, window);
            }
            CpuFrame::Painted(error) => {
                self.set_render_error(Some(error));
                self.present_result(Presentation::Presented, window);
            }
            CpuFrame::Failed(error) => {
                self.set_render_error(Some(error));
                self.present_result(Presentation::Retry, window);
            }
        }
    }

    fn update_title(&mut self, now: Instant) {
        let Some(window) = &self.window else {
            return;
        };
        let choosing = self.chooser.is_some();
        let title = dev_title(&DevTitle {
            platform: self.platform,
            choosing,
            backend: self.ui_backend,
            gpu: self.gpu.is_some(),
            error: self
                .binding_error
                .as_deref()
                .or(self.render_error.as_deref()),
            surface: self.surface_state,
            rate: self.frame_stats.snapshot(now),
            continuous: self.continuous_redraw,
            clicks: self.button.clicks(),
        });
        if title != self.last_title {
            window.set_title(&title);
            if self.log_fps && !choosing {
                println!("Frame stats: {title}");
            }
            self.last_title = title;
        }
        self.next_title_update = now + TITLE_INTERVAL;
    }
}

impl ApplicationHandler<accesskit_winit::Event> for App {
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: accesskit_winit::Event) {
        if !self
            .window
            .as_ref()
            .is_some_and(|window| window.id() == event.window_id)
        {
            return;
        }
        match event.window_event {
            accesskit_winit::WindowEvent::InitialTreeRequested => {
                self.accessibility_ids.invalidate();
                self.accessibility_active = true;
                self.update_accessibility();
            }
            accesskit_winit::WindowEvent::AccessibilityDeactivated => {
                self.accessibility_active = false;
            }
            accesskit_winit::WindowEvent::ActionRequested(request) => {
                self.accessibility_action(&request);
                self.update_accessibility();
                if let Some(window) = &self.window {
                    let editing = self.button.text_editing()
                        && !self
                            .button
                            .control_readonly(self.button.focused_index().max(0) as usize);
                    if editing != self.ime_allowed {
                        window.set_ime_allowed(editing);
                        self.ime_allowed = editing;
                    }
                    let caret = self.button.text_caret_bounds();
                    if caret.len() == 4 {
                        window.set_ime_cursor_area(
                            winit::dpi::LogicalPosition::new(caret[0] as f64, caret[1] as f64),
                            LogicalSize::new(caret[2] as f64, caret[3] as f64),
                        );
                    }
                    window.request_redraw();
                }
            }
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        event_loop.set_control_flow(ControlFlow::Wait);
        let init_rt = self.chooser.as_ref().unwrap_or(&self.button);
        let (init_w, init_h) = self
            .platform
            .viewport_size(init_rt.render_width(), init_rt.render_height());
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_visible(false)
                        .with_title(
                            "Forma · ожидание кадра · F6: платформа · F7: бэкенд · F8: FPS-тест",
                        )
                        .with_inner_size(LogicalSize::new(init_w, init_h)),
                )
                .unwrap(),
        );
        if let Some(proxy) = &self.accessibility_proxy {
            self.accessibility = Some(accesskit_winit::Adapter::with_event_loop_proxy(
                event_loop,
                &window,
                proxy.clone(),
            ));
        }
        window.set_visible(true);
        if std::env::var("FORMA_RENDERER").as_deref() != Ok("cpu") {
            match pollster::block_on(GpuWindow::new(window.clone())) {
                Ok(gpu) => self.gpu = Some(gpu),
                Err(error) => eprintln!("GPU недоступен, CPU fallback: {error}"),
            }
        }
        #[cfg(target_os = "ios")]
        if self.gpu.is_none() {
            let context = softbuffer::Context::new(window.clone()).unwrap();
            self.surface = Some(softbuffer::Surface::new(&context, window.clone()).unwrap());
            println!("Renderer: CPU fallback");
        }
        #[cfg(not(target_os = "ios"))]
        if self.gpu.is_none()
            || self.chooser.is_some()
            || self.ui_backend == Some(UiBackend::Egui)
            || self.platform != TargetPlatform::Desktop
        {
            let context = softbuffer::Context::new(window.clone()).unwrap();
            self.surface = Some(softbuffer::Surface::new(&context, window.clone()).unwrap());
            if self.gpu.is_none() {
                println!("Renderer: CPU fallback");
            }
        }
        #[cfg(target_os = "ios")]
        {
            let size = window.inner_size();
            let scale = (window.scale_factor() as f32).max(1.0);
            let logical_w = (size.width as f32 / scale).round();
            let logical_h = (size.height as f32 / scale).round();
            if let Some(chooser) = &mut self.chooser {
                if (chooser.width() - logical_w).abs() > 1.0
                    || (chooser.height() - logical_h).abs() > 1.0
                {
                    let src = chooser_ui_for_viewport(logical_w, logical_h);
                    if let Ok(mut next) =
                        forma::Runtime::from_sources(&src, forma::BUTTON_COMPONENT)
                    {
                        next.preserve_interaction(chooser);
                        *chooser = next;
                    }
                }
            } else if self.hot.is_demo()
                && ((self.button.width() - logical_w).abs() > 1.0
                    || (self.button.height() - logical_h).abs() > 1.0)
            {
                let src = two_buttons_ui_for_viewport(logical_w, logical_h);
                if let Ok(mut next) = forma::Runtime::from_sources(&src, forma::BUTTON_COMPONENT) {
                    next.preserve_interaction(&self.button);
                    next.set_svet_document_scheduling(self.ui_backend == Some(UiBackend::Svet));
                    self.button = next;
                }
            }
        }
        window.focus_window();
        window.request_redraw();
        self.window = Some(window);
        if self.platform != TargetPlatform::Desktop {
            println!(
                "Платформа Forma: {} ({}) · F6 — переключить на лету",
                self.platform.as_str(),
                self.platform.label()
            );
        }
        if let Some(b) = self.ui_backend {
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
        println!(
            "F8: непрерывный FPS-тест; обычный режим перерисовывает окно только при изменениях"
        );
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self.secondary_windows.contains(window_id) {
            self.secondary_windows.window_event(window_id, event);
            return;
        }
        let Some(window) = self.window.as_ref().cloned() else {
            return;
        };
        if let Some(adapter) = &mut self.accessibility {
            adapter.process_event(&window, &event);
        }
        if self.chooser.is_some() {
            match event {
                WindowEvent::CloseRequested => event_loop.exit(),
                WindowEvent::CursorMoved { position, .. } => {
                    let p = position.to_logical::<f32>(window.scale_factor());
                    self.pointer = (p.x, p.y);
                    if let Some(chooser) = &mut self.chooser {
                        chooser.pointer(p.x, p.y, 0);
                    }
                    window.request_redraw();
                }
                WindowEvent::CursorLeft { .. } => {
                    if let Some(chooser) = &mut self.chooser {
                        chooser.pointer(-1., -1., 3);
                    }
                    window.request_redraw();
                }
                WindowEvent::MouseInput {
                    state,
                    button: MouseButton::Left,
                    ..
                } => {
                    let mut picked = None;
                    if let Some(chooser) = &mut self.chooser {
                        let before = chooser.clicks();
                        chooser.pointer(
                            self.pointer.0,
                            self.pointer.1,
                            if state == ElementState::Pressed { 1 } else { 2 },
                        );
                        if chooser.clicks() != before {
                            picked = Some(if chooser.key() == "choose_egui" {
                                UiBackend::Egui
                            } else {
                                UiBackend::Svet
                            });
                        }
                    }
                    if let Some(backend) = picked {
                        self.select_backend(backend);
                    } else {
                        window.request_redraw();
                    }
                }
                WindowEvent::Touch(Touch {
                    phase, location, ..
                }) => {
                    let p = location.to_logical::<f32>(window.scale_factor());
                    self.pointer = (p.x, p.y);
                    let mut picked = None;
                    if let Some(chooser) = &mut self.chooser {
                        match phase {
                            TouchPhase::Started => {
                                self.touch_last = Some((p.x, p.y));
                                chooser.pointer(p.x, p.y, 0);
                                chooser.pointer(p.x, p.y, 1);
                            }
                            TouchPhase::Moved => {
                                self.touch_last = Some((p.x, p.y));
                                chooser.pointer(p.x, p.y, 0);
                            }
                            TouchPhase::Ended => {
                                self.touch_last = None;
                                let before = chooser.clicks();
                                chooser.pointer(p.x, p.y, 2);
                                if chooser.clicks() != before {
                                    picked = Some(if chooser.key() == "choose_egui" {
                                        UiBackend::Egui
                                    } else {
                                        UiBackend::Svet
                                    });
                                }
                            }
                            TouchPhase::Cancelled => {
                                self.touch_last = None;
                                chooser.pointer(-1., -1., 3);
                            }
                        }
                    }
                    if let Some(backend) = picked {
                        self.select_backend(backend);
                    } else {
                        window.request_redraw();
                    }
                }
                WindowEvent::ModifiersChanged(modifiers) => {
                    self.shift = modifiers.state().shift_key();
                    self.control = modifiers.state().control_key();
                    self.alt = modifiers.state().alt_key();
                    self.command = if cfg!(target_os = "macos") {
                        modifiers.state().super_key()
                    } else {
                        modifiers.state().control_key()
                    };
                }
                WindowEvent::KeyboardInput { event, .. } => {
                    let pressed = event.state == ElementState::Pressed;
                    if pressed && !event.repeat {
                        match &event.logical_key {
                            Key::Character(c)
                                if c.as_str() == "1" || c.eq_ignore_ascii_case("e") =>
                            {
                                self.select_backend(UiBackend::Egui);
                                return;
                            }
                            Key::Character(c)
                                if c.as_str() == "2" || c.eq_ignore_ascii_case("s") =>
                            {
                                self.select_backend(UiBackend::Svet);
                                return;
                            }
                            Key::Named(NamedKey::F6) => {
                                let next = self.platform.next();
                                self.select_platform(next);
                                return;
                            }
                            Key::Named(NamedKey::F7) => {
                                self.select_backend(UiBackend::Svet);
                                return;
                            }
                            _ => {}
                        }
                    }
                    let mut picked = None;
                    if let Some(chooser) = &mut self.chooser {
                        if event.logical_key == Key::Named(NamedKey::Tab)
                            && pressed
                            && !event.repeat
                            && !chooser.focus_next(self.shift)
                        {
                            chooser.focus(false);
                            chooser.focus_next(self.shift);
                        }
                        let key = match event.logical_key {
                            Key::Named(NamedKey::Space) => 1,
                            Key::Named(NamedKey::Enter) => 2,
                            Key::Named(NamedKey::Escape) => 3,
                            _ => 0,
                        };
                        let before = chooser.clicks();
                        chooser.key_event(key, pressed, event.repeat);
                        if chooser.clicks() != before {
                            picked = Some(if chooser.key() == "choose_egui" {
                                UiBackend::Egui
                            } else {
                                UiBackend::Svet
                            });
                        }
                    }
                    if let Some(backend) = picked {
                        self.select_backend(backend);
                    } else {
                        window.request_redraw();
                    }
                }
                WindowEvent::Focused(focused) => {
                    if let Some(chooser) = &mut self.chooser {
                        chooser.focus(focused);
                    }
                    window.request_redraw();
                }
                WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                    window.request_redraw();
                }
                WindowEvent::RedrawRequested => {
                    let now = Instant::now();
                    if let Some(chooser) = &mut self.chooser {
                        chooser.tick((now - self.last).as_secs_f32() * 1000.);
                    }
                    self.last = now;
                    self.draw(&window);
                }
                _ => {}
            }
            self.update_accessibility();
            return;
        }
        let clicks = self.button.clicks();
        let dismissals = self.button.dismiss_count();
        let revision = self.button.visual_revision();
        let was_animating = self.button.is_animating();
        let redraw = matches!(event, WindowEvent::RedrawRequested);
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::CursorMoved { position, .. } => {
                let p = position.to_logical::<f32>(window.scale_factor());
                self.pointer = (p.x, p.y);
                self.button.pointer(p.x, p.y, 0);
                let index = self.button.hit_index(p.x, p.y);
                window.set_cursor(
                    if index >= 0 && self.button.control_editable(index as usize) {
                        winit::window::CursorIcon::Text
                    } else {
                        winit::window::CursorIcon::Default
                    },
                );
            }
            WindowEvent::CursorLeft { .. } => self.button.pointer(-1., -1., 3),
            WindowEvent::MouseInput {
                button: MouseButton::Left,
                ..
            } if self.inspect_mode => {
                let index = self.button.hit_index(self.pointer.0, self.pointer.1);
                if index >= 0 {
                    self.button.focus_control(index as usize);
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                self.button.pointer(
                    self.pointer.0,
                    self.pointer.1,
                    if state == ElementState::Pressed { 1 } else { 2 },
                );
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (dx, dy) =
                    forma::native_window::wheel_scroll(delta, window.scale_factor() as f32);
                self.button.scroll(dx, dy);
            }
            WindowEvent::Touch(Touch {
                phase, location, ..
            }) => {
                let p = location.to_logical::<f32>(window.scale_factor());
                self.pointer = (p.x, p.y);
                match phase {
                    TouchPhase::Started => {
                        self.touch_last = Some((p.x, p.y));
                        self.button.pointer(p.x, p.y, 0);
                        if self.inspect_mode {
                            let index = self.button.hit_index(p.x, p.y);
                            if index >= 0 {
                                self.button.focus_control(index as usize);
                            }
                        } else {
                            self.button.pointer(p.x, p.y, 1);
                        }
                    }
                    TouchPhase::Moved => {
                        let scroll =
                            forma::native_window::touch_scroll_step(&mut self.touch_last, p.x, p.y);
                        self.button.pointer(p.x, p.y, 0);
                        if let Some((dx, dy)) = scroll {
                            self.button.scroll(dx, dy);
                        }
                    }
                    TouchPhase::Ended => {
                        self.touch_last = None;
                        if !self.inspect_mode {
                            self.button.pointer(p.x, p.y, 2);
                        }
                    }
                    TouchPhase::Cancelled => {
                        self.touch_last = None;
                        self.button.pointer(-1., -1., 3);
                    }
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.shift = modifiers.state().shift_key();
                self.control = modifiers.state().control_key();
                self.alt = modifiers.state().alt_key();
                self.command = if cfg!(target_os = "macos") {
                    modifiers.state().super_key()
                } else {
                    modifiers.state().control_key()
                };
            }
            WindowEvent::Ime(event) => self.ime_event(event),
            WindowEvent::KeyboardInput { event, .. } => {
                if (self.composing || self.composition_owner.is_some())
                    && event.state == ElementState::Pressed
                    && !event.repeat
                    && matches!(
                        event.logical_key,
                        Key::Named(NamedKey::Tab | NamedKey::Escape)
                    )
                {
                    standalone_cancel_ime(
                        &mut self.button,
                        &mut self.composing,
                        &mut self.composition_owner,
                        &mut self.ime_cancelled,
                    );
                }
                if event.logical_key == Key::Named(NamedKey::F6)
                    && event.state == ElementState::Pressed
                    && !event.repeat
                {
                    let next = self.platform.next();
                    self.select_platform(next);
                } else if event.logical_key == Key::Named(NamedKey::F7)
                    && event.state == ElementState::Pressed
                    && !event.repeat
                {
                    let next = self.ui_backend.map_or(UiBackend::Egui, UiBackend::toggle);
                    self.select_backend(next);
                } else if let Some(shortcut) = dev_shortcut(
                    &event.logical_key,
                    event.state == ElementState::Pressed,
                    event.repeat,
                ) {
                    match shortcut {
                        DevShortcut::Popup => self.secondary_windows.toggle_popup(event_loop),
                        DevShortcut::Inspect => {
                            self.inspect_mode = !self.inspect_mode;
                            self.button.focus(false);
                            self.inspect_report.clear();
                            self.a11y_report.clear();
                            println!(
                                "F9: выбор элемента {}",
                                if self.inspect_mode {
                                    "включён"
                                } else {
                                    "выключен"
                                }
                            );
                        }
                        DevShortcut::FpsTest => {
                            self.continuous_redraw = !self.continuous_redraw;
                            self.frame_stats = FrameStats::default();
                            self.next_title_update = Instant::now();
                            self.retry_at = None;
                            window.request_redraw();
                        }
                    }
                } else if event.state == ElementState::Pressed
                    && self.button.range_key(&match &event.logical_key {
                        Key::Named(key) => format!("{key:?}"),
                        _ => String::new(),
                    })
                {
                    // Range navigation belongs to the same runtime as pointer input.
                } else if self.button.text_editing()
                    && !matches!(
                        event.logical_key,
                        Key::Named(NamedKey::Tab) | Key::Named(NamedKey::Escape)
                    )
                {
                    if event.state == ElementState::Pressed
                        && !self.composing
                        && !(cfg!(target_os = "macos") && self.control && self.alt)
                    {
                        let key = match &event.logical_key {
                            Key::Character(text) => text.to_string(),
                            Key::Named(name) => format!("{name:?}"),
                            _ => String::new(),
                        };
                        let alt_gr = cfg!(not(target_os = "macos"))
                            && self.control
                            && self.alt
                            && event.text.is_some();
                        let command = self.command && !alt_gr;
                        let word_modifier = if cfg!(target_os = "macos") {
                            self.alt && !command
                        } else {
                            self.control && !self.alt
                        };
                        let key = if command {
                            forma::native_clipboard::command_key(&event.physical_key, &key)
                        } else {
                            &key
                        };
                        let word_handled =
                            word_modifier && self.button.text_word_key(key, self.shift);
                        let clipboard_handled = !word_handled
                            && command
                            && forma::native_clipboard::handle_key(&mut self.button, key);
                        if !word_handled
                            && !clipboard_handled
                            && !self.button.text_key(key, self.shift, command)
                            && !command
                        {
                            if let Some(text) = event.text {
                                if !text.chars().any(char::is_control) {
                                    self.button.text_insert(&text);
                                }
                            }
                        }
                    }
                } else {
                    let key = match event.logical_key {
                        Key::Named(NamedKey::Space) => 1,
                        Key::Named(NamedKey::Enter) => 2,
                        Key::Named(NamedKey::Escape) => 3,
                        _ => 0,
                    };
                    if event.logical_key == Key::Named(NamedKey::Tab)
                        && event.state == ElementState::Pressed
                        && !event.repeat
                        && !self.button.focus_next(self.shift)
                    {
                        self.button.focus(false);
                        self.button.focus_next(self.shift);
                    }
                    self.button
                        .key_event(key, event.state == ElementState::Pressed, event.repeat);
                }
            }
            WindowEvent::Focused(focused) => {
                self.button.focus(focused);
                if !focused {
                    self.shift = false;
                    self.command = false;
                    self.control = false;
                    self.alt = false;
                    standalone_cancel_ime(
                        &mut self.button,
                        &mut self.composing,
                        &mut self.composition_owner,
                        &mut self.ime_cancelled,
                    );
                    self.button.pointer(-1., -1., 3);
                }
            }
            WindowEvent::Occluded(false) => window.request_redraw(),
            WindowEvent::Occluded(true) => {
                self.surface_state = Presentation::Occluded;
                self.next_title_update = Instant::now();
            }
            WindowEvent::Resized(_) => {
                // Keep synchronous transaction-aware Metal presentation on live resize.
                #[cfg(target_os = "macos")]
                if self.gpu.is_some() {
                    self.draw(&window);
                    return;
                }
                window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => window.request_redraw(),
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                self.button.tick((now - self.last).as_secs_f32() * 1000.);
                self.last = now;
                self.draw(&window);
            }
            _ => {}
        }
        if self.inspect_mode {
            let report = inspect_report(&self.button);
            if report != self.inspect_report {
                println!("FORMA_INSPECT {}", report);
                self.inspect_report = report;
            }
            let a11y = self.button.accessibility_tree_json();
            if a11y != self.a11y_report {
                println!("FORMA_A11Y {}", a11y);
                self.a11y_report = a11y;
            }
        }
        self.observe_ime_owner();
        let editing = self.button.text_editing()
            && !self
                .button
                .control_readonly(self.button.focused_index().max(0) as usize);
        if editing != self.ime_allowed {
            window.set_ime_allowed(editing);
            self.ime_allowed = editing;
        }
        let caret = self.button.text_caret_bounds();
        if caret.len() == 4 {
            window.set_ime_cursor_area(
                winit::dpi::LogicalPosition::new(caret[0] as f64, caret[1] as f64),
                LogicalSize::new(caret[2] as f64, caret[3] as f64),
            );
        }
        if !redraw {
            let binding_error = self.button.sync_ui_bindings_native().err();
            self.observe_ime_owner();
            self.set_binding_error(binding_error);
            let started = !was_animating && self.button.is_animating();
            if started {
                self.last = Instant::now();
            }
            if started || self.button.visual_revision() != revision {
                window.request_redraw();
            }
        }
        if self.button.clicks() != clicks {
            let active_key = self.button.key();
            println!(
                "{}.clicked → {} (событие; обработчик приложения пока не подключён)",
                active_key,
                self.button.action()
            );
            #[cfg(target_os = "ios")]
            if self.hot.is_demo() {
                let c1 = self.button.control_clicks(0);
                let c2 = self.button.control_clicks(1);
                let src = two_buttons_ui_for_viewport_with_state(
                    self.button.width(),
                    self.button.height(),
                    Some(active_key.as_str()),
                    Some((c1, c2)),
                );
                if let Ok(mut next) = forma::Runtime::from_sources(&src, forma::BUTTON_COMPONENT) {
                    next.preserve_interaction(&self.button);
                    next.set_svet_document_scheduling(self.ui_backend == Some(UiBackend::Svet));
                    self.button = next;
                }
            }
            window.request_redraw();
            self.next_title_update = Instant::now();
        }
        if self.button.dismiss_count() != dismissals {
            let layer = self.button.dismissed_index().max(0) as usize;
            println!(
                "{}.dismissed → {} (модальный слой закрыт: Escape или щелчок по backdrop)",
                self.button.modal_key(layer),
                self.button.modal_action(layer)
            );
        }
        self.update_accessibility();
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if let Some((mut next, message)) = self.hot.poll(now, &self.button) {
            next.preserve_interaction(&self.button);
            next.set_svet_document_scheduling(self.ui_backend == Some(UiBackend::Svet));
            self.button = next;
            self.binding_error = None;
            self.next_title_update = Instant::now();
            self.inspect_report.clear();
            self.a11y_report.clear();
            if let Some(window) = &self.window {
                window.request_redraw();
            }
            println!("{message}");
        }
        if self.retry_at.is_some_and(|time| time <= now) {
            self.retry_at = None;
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
        let idle_deadline = self.frame_stats.idle_deadline();
        if now >= self.next_title_update || idle_deadline.is_some_and(|deadline| deadline <= now) {
            self.update_title(now);
        }
        // Updating the title never causes a redraw. Stop all meter wakeups at idle.
        let meter_deadline = idle_deadline
            .filter(|deadline| *deadline > now)
            .map(|deadline| deadline.min(self.next_title_update));
        let anim_active = self
            .chooser
            .as_ref()
            .map_or_else(|| self.button.is_animating(), |c| c.is_animating());
        if anim_active && self.retry_at.is_none() {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
        let anim_deadline = anim_active.then_some(now + Duration::from_millis(16));
        let hot_deadline = self.hot.deadline();
        let deadline = self
            .retry_at
            .into_iter()
            .chain(anim_deadline)
            .chain(meter_deadline)
            .chain(hot_deadline)
            .min();
        event_loop.set_control_flow(deadline.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }
}

/// What the command line asks for.
struct CliArgs {
    source_path: Option<String>,
    template_path: Option<String>,
    backend: Option<UiBackend>,
    platform: TargetPlatform,
    mcp: bool,
}

/// Reads the arguments after the program name. `backend`, `platform` and `mcp` are the values
/// already resolved from the environment; a flag replaces them.
fn parse_cli_args(
    args: impl IntoIterator<Item = String>,
    mut backend: Option<UiBackend>,
    mut platform: TargetPlatform,
    mut mcp: bool,
) -> CliArgs {
    let mut positional = Vec::new();
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--mcp" | "mcp" => mcp = true,
            "--egui" | "--backend=egui" => backend = Some(UiBackend::Egui),
            "--svet" | "--backend=svet" => backend = Some(UiBackend::Svet),
            "--ask" | "--choose-backend" | "--backend=ask" => backend = None,
            "--backend" | "-b" => {
                if let Some(next) = iter.next() {
                    if matches!(next.trim().to_ascii_lowercase().as_str(), "ask" | "choose") {
                        backend = None;
                    } else if let Some(b) = UiBackend::from_arg(&next) {
                        backend = Some(b);
                    }
                }
            }
            "--ios" | "--platform=ios" => platform = TargetPlatform::Ios,
            "--android" | "--platform=android" => platform = TargetPlatform::Android,
            "--desktop" | "--platform=desktop" => platform = TargetPlatform::Desktop,
            "--platform" | "-p" => {
                if let Some(next) = iter.next() {
                    if let Some(p) = TargetPlatform::from_arg(&next) {
                        platform = p;
                    }
                }
            }
            _ => positional.push(arg),
        }
    }
    let mut positional = positional.into_iter();
    CliArgs {
        source_path: positional.next(),
        template_path: positional.next(),
        backend,
        platform,
        mcp,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let CliArgs {
        source_path,
        template_path,
        backend: ui_backend,
        platform,
        mcp: mcp_mode,
    } = parse_cli_args(
        std::env::args().skip(1),
        UiBackend::resolve_from_env_and_args(),
        TargetPlatform::resolve_from_env_and_args(),
        std::env::var("FORMA_MCP").as_deref() == Ok("1"),
    );
    let DevProject {
        runtime: mut button,
        source,
        template,
        hot,
    } = DevProject::load(source_path, template_path)?;
    button.set_svet_document_scheduling(ui_backend == Some(UiBackend::Svet));
    let chooser = if ui_backend.is_none() {
        Some(build_chooser_runtime().map_err(std::io::Error::other)?)
    } else {
        None
    };
    if mcp_mode {
        let mut mcp_app = forma::mcp::McpApp::from_runtime(button, source, template);
        mcp_app.backend = ui_backend.map_or_else(|| "egui".into(), |b| b.as_str().into());
        mcp_app.platform = platform.as_str().into();
        return forma::mcp::serve_stdio(mcp_app).map_err(|e| std::io::Error::other(e).into());
    }
    let event_loop = EventLoop::<accesskit_winit::Event>::with_user_event().build()?;
    let accessibility_proxy = event_loop.create_proxy();
    event_loop.run_app(&mut App {
        ui_backend,
        platform,
        chooser,
        inspect_mode: false,
        inspect_report: String::new(),
        a11y_report: String::new(),
        shift: false,
        command: false,
        control: false,
        alt: false,
        composing: false,
        composition_owner: None,
        ime_cancelled: false,
        ime_allowed: false,
        accessibility: None,
        accessibility_proxy: Some(accessibility_proxy),
        accessibility_active: false,
        accessibility_ids: Default::default(),
        window: None,
        surface: None,
        gpu: None,
        surface_size: None,
        secondary_windows: AuxiliaryWindows::default(),
        render_error: None,
        binding_error: None,
        surface_state: Presentation::Presented,
        button,
        pointer: (-1., -1.),
        touch_last: None,
        last: Instant::now(),
        frame_stats: FrameStats::default(),
        continuous_redraw: std::env::var("FORMA_FPS_TEST").as_deref() == Ok("1"),
        log_fps: std::env::var("FORMA_FPS_LOG").as_deref() == Ok("1"),
        next_title_update: Instant::now(),
        last_title: String::new(),
        retry_at: None,
        hot,
    })?;
    Ok(())
}

fn standalone_observe_ime(
    button: &forma::Runtime,
    composing: &mut bool,
    owner: Option<(u64, i32)>,
    cancelled: &mut bool,
) {
    if owner.is_some() && owner != button.native_editor_session() {
        *composing = false;
        *cancelled = true;
    }
}

fn standalone_cancel_ime(
    button: &mut forma::Runtime,
    composing: &mut bool,
    owner: &mut Option<(u64, i32)>,
    cancelled: &mut bool,
) {
    *composing = false;
    *owner = None;
    *cancelled = true;
    button.text_preedit("");
}

fn standalone_ime_event(
    button: &mut forma::Runtime,
    composing: &mut bool,
    composition_owner: &mut Option<(u64, i32)>,
    ime_cancelled: &mut bool,
    event: winit::event::Ime,
) {
    use winit::event::Ime;
    standalone_observe_ime(button, composing, *composition_owner, ime_cancelled);
    match event {
        Ime::Enabled => {
            *composing = false;
            *ime_cancelled = false;
            *composition_owner = None;
        }
        Ime::Disabled => {
            *composing = false;
            *ime_cancelled = true;
            *composition_owner = None;
            button.text_preedit("");
        }
        Ime::Preedit(text, _) => {
            if !text.is_empty() {
                *composition_owner = button.native_editor_session();
                *ime_cancelled = false;
            }
            *composing = !text.is_empty();
            if composition_owner.is_none() || *composition_owner == button.native_editor_session() {
                button.text_preedit(&text);
            }
        }
        Ime::Commit(text) => {
            *composing = false;
            let owner = composition_owner.take();
            if !*ime_cancelled && (owner.is_none() || owner == button.native_editor_session()) {
                if text.is_empty() {
                    button.text_preedit("");
                } else {
                    button.text_insert(&text);
                }
            }
        }
    }
}

#[cfg(test)]
mod standalone_ime_tests {
    use super::standalone_ime_event;
    use winit::event::Ime;
    fn runtime() -> forma::Runtime {
        forma::Runtime::from_sources("component App { Frame { width:240; height:100; Button { key:'first'; } Button { key:'second'; } } }","component Button { Rectangle { ContentInput { value:'seed'; width:200; height:32; } PointerArea { clicked -> events.clicked(); } } }").unwrap()
    }
    #[test]
    fn standalone_ime_rejects_retired_owner_and_allows_fresh_preedit() {
        let mut rt = runtime();
        rt.focus_control(0);
        let (mut composing, mut owner, mut cancelled) = (false, None, false);
        standalone_ime_event(
            &mut rt,
            &mut composing,
            &mut owner,
            &mut cancelled,
            Ime::Preedit("old".into(), None),
        );
        rt.focus_control(1);
        standalone_ime_event(
            &mut rt,
            &mut composing,
            &mut owner,
            &mut cancelled,
            Ime::Commit("wrong".into()),
        );
        assert_eq!(rt.text_value(0), "seed");
        assert_eq!(rt.text_value(1), "seed");
        standalone_ime_event(
            &mut rt,
            &mut composing,
            &mut owner,
            &mut cancelled,
            Ime::Preedit("fresh".into(), None),
        );
        standalone_ime_event(
            &mut rt,
            &mut composing,
            &mut owner,
            &mut cancelled,
            Ime::Preedit("".into(), None),
        );
        standalone_ime_event(
            &mut rt,
            &mut composing,
            &mut owner,
            &mut cancelled,
            Ime::Commit("fresh".into()),
        );
        assert!(rt.text_value(1).contains("fresh"));
        assert_eq!(rt.text_value(0), "seed");
    }
    #[test]
    fn standalone_ime_binding_sync_retires_rebuilt_session_but_not_idle_session() {
        let files=std::collections::BTreeMap::from([("App.ui".into(),"component App { state a: String = 'first'; state b: String = 'second'; Column { width:240; height:140; TextInput { key:'a'; width:200; height:32; value <-> state.a; } TextInput { key:'b'; width:200; height:32; value <-> state.b; } } }".into())]);
        for svet in [false, true] {
            let mut rt =
                forma::Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
            rt.set_svet_document_scheduling(svet);
            rt.sync_ui_bindings_native().unwrap();
            rt.focus_control(0);
            rt.text_key("End", false, false);
            let (mut composing, mut owner, mut cancelled) = (false, None, false);
            standalone_ime_event(
                &mut rt,
                &mut composing,
                &mut owner,
                &mut cancelled,
                Ime::Preedit("pending".into(), None),
            );
            let original = owner;
            assert!(!rt.sync_ui_bindings_native().unwrap());
            assert_eq!(rt.native_editor_session(), original);
            super::standalone_observe_ime(&rt, &mut composing, owner, &mut cancelled);
            assert!(composing);
            assert!(!cancelled);
            // A sibling binding causes scene reconstruction while preserving focus/key.
            rt.set_binding_value(1, "updated", false).unwrap();
            assert!(rt.sync_ui_bindings_native().unwrap());
            assert_ne!(rt.native_editor_session(), original);
            assert_eq!(rt.focused_index(), 0);
            super::standalone_observe_ime(&rt, &mut composing, owner, &mut cancelled);
            assert!(!composing);
            assert!(cancelled);
            for text in ["late", "again"] {
                standalone_ime_event(
                    &mut rt,
                    &mut composing,
                    &mut owner,
                    &mut cancelled,
                    Ime::Commit(text.into()),
                );
            }
            assert_eq!(rt.text_value(0), "first");
            assert_eq!(rt.text_value(1), "updated");
            standalone_ime_event(
                &mut rt,
                &mut composing,
                &mut owner,
                &mut cancelled,
                Ime::Preedit("new".into(), None),
            );
            standalone_ime_event(
                &mut rt,
                &mut composing,
                &mut owner,
                &mut cancelled,
                Ime::Commit("新".into()),
            );
            rt.sync_ui_bindings_native().unwrap();
            assert_eq!(rt.text_value(0), "first新");
            assert_eq!(rt.text_value(1), "updated");
            assert!(rt.ui_state_json().contains("first新"));
            assert!(!rt.ui_state_json().contains("late"));
        }
    }
    #[test]
    fn standalone_navigation_cancellation_survives_empty_preedit_and_late_commits() {
        let mut rt = runtime();
        rt.focus_control(0);
        let (mut composing, mut owner, mut cancelled) = (false, None, false);
        standalone_ime_event(
            &mut rt,
            &mut composing,
            &mut owner,
            &mut cancelled,
            Ime::Preedit("pending".into(), None),
        );
        standalone_ime_event(
            &mut rt,
            &mut composing,
            &mut owner,
            &mut cancelled,
            Ime::Preedit("".into(), None),
        );
        assert!(!composing);
        assert!(owner.is_some());
        super::standalone_cancel_ime(&mut rt, &mut composing, &mut owner, &mut cancelled);
        for text in ["late", "again"] {
            standalone_ime_event(
                &mut rt,
                &mut composing,
                &mut owner,
                &mut cancelled,
                Ime::Commit(text.into()),
            );
        }
        assert_eq!(rt.text_value(0), "seed");
        assert_eq!(rt.text_value(1), "seed");
        standalone_ime_event(
            &mut rt,
            &mut composing,
            &mut owner,
            &mut cancelled,
            Ime::Preedit("new".into(), None),
        );
        standalone_ime_event(
            &mut rt,
            &mut composing,
            &mut owner,
            &mut cancelled,
            Ime::Commit("new".into()),
        );
        assert!(rt.text_value(0).contains("new"));
    }
    #[test]
    fn standalone_ime_rejects_replacement_and_disabled_commit() {
        let mut rt = runtime();
        rt.focus_control(0);
        let (mut composing, mut owner, mut cancelled) = (false, None, false);
        standalone_ime_event(
            &mut rt,
            &mut composing,
            &mut owner,
            &mut cancelled,
            Ime::Preedit("old".into(), None),
        );
        rt = runtime();
        rt.focus_control(0);
        standalone_ime_event(
            &mut rt,
            &mut composing,
            &mut owner,
            &mut cancelled,
            Ime::Commit("wrong".into()),
        );
        assert_eq!(rt.text_value(0), "seed");
        standalone_ime_event(
            &mut rt,
            &mut composing,
            &mut owner,
            &mut cancelled,
            Ime::Disabled,
        );
        standalone_ime_event(
            &mut rt,
            &mut composing,
            &mut owner,
            &mut cancelled,
            Ime::Commit("late".into()),
        );
        assert_eq!(rt.text_value(0), "seed");
        standalone_ime_event(
            &mut rt,
            &mut composing,
            &mut owner,
            &mut cancelled,
            Ime::Enabled,
        );
        rt.text_key("a", false, true);
        standalone_ime_event(
            &mut rt,
            &mut composing,
            &mut owner,
            &mut cancelled,
            Ime::Commit("".into()),
        );
        assert_eq!(rt.text_value(0), "seed");
        assert_eq!(rt.text_selected(), "seed");
        standalone_ime_event(
            &mut rt,
            &mut composing,
            &mut owner,
            &mut cancelled,
            Ime::Commit("direct".into()),
        );
        assert_eq!(rt.text_value(0), "direct");
    }
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    fn cli(args: &[&str]) -> CliArgs {
        parse_cli_args(
            args.iter().map(|arg| (*arg).to_owned()),
            Some(UiBackend::Svet),
            TargetPlatform::Desktop,
            false,
        )
    }

    #[test]
    fn command_line_separates_flags_from_the_scene_and_template_paths() {
        let args = cli(&[]);
        assert!(args.source_path.is_none() && args.template_path.is_none());
        assert_eq!(args.backend, Some(UiBackend::Svet));
        assert_eq!(args.platform, TargetPlatform::Desktop);
        assert!(!args.mcp);

        let args = cli(&["--egui", "a.ui", "--ios", "b.ui", "ignored.ui", "--mcp"]);
        assert_eq!(args.source_path.as_deref(), Some("a.ui"));
        assert_eq!(args.template_path.as_deref(), Some("b.ui"));
        assert_eq!(args.backend, Some(UiBackend::Egui));
        assert_eq!(args.platform, TargetPlatform::Ios);
        assert!(args.mcp);

        assert_eq!(cli(&["--ask"]).backend, None);
        assert_eq!(cli(&["-b", "choose"]).backend, None);
        assert_eq!(cli(&["--backend", "egui"]).backend, Some(UiBackend::Egui));
        // An unknown value is consumed and changes nothing.
        let args = cli(&["--backend", "nope", "-p", "nope", "scene.ui"]);
        assert_eq!(args.backend, Some(UiBackend::Svet));
        assert_eq!(args.platform, TargetPlatform::Desktop);
        assert_eq!(args.source_path.as_deref(), Some("scene.ui"));
        assert_eq!(cli(&["-p", "pixel"]).platform, TargetPlatform::Android);
        assert_eq!(
            cli(&["--platform=android", "--desktop"]).platform,
            TargetPlatform::Desktop
        );
        assert!(cli(&["mcp"]).mcp);
    }
}
