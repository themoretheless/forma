//! Reusable software-rendered native window host for Forma applications.

use std::{
    num::NonZeroU32,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, Ime, MouseButton, MouseScrollDelta, Touch, TouchPhase, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    keyboard::{Key, NamedKey, PhysicalKey},
    window::{Window, WindowId},
};

mod accessibility;
mod app;
mod aux_window;
mod config;
mod demo;
mod dev_app;
mod dev_project;
mod extension;
mod frame;
mod ime;
mod ime_session;
mod input;
mod keys;
mod waker;

pub use app::*;
use aux_window::*;
pub use config::*;
pub use demo::*;
pub use dev_app::*;
pub use dev_project::*;
use extension::*;
use frame::*;
pub use ime_session::{ImeSession, ime_target, place_ime_caret};
pub use keys::*;
pub use waker::*;

/// A wheel event as a scroll in logical pixels: a line is 32 pixels, and a pixel delta is
/// divided by the window's scale factor. The sign turns "wheel down" into "content up".
pub fn wheel_scroll(delta: MouseScrollDelta, scale: f32) -> (f32, f32) {
    match delta {
        MouseScrollDelta::LineDelta(x, y) => (-x * 32., -y * 32.),
        MouseScrollDelta::PixelDelta(position) => {
            (-position.x as f32 / scale, -position.y as f32 / scale)
        }
    }
}

/// The scroll a touch that moved to `(x, y)` produces, if it has travelled more than half a
/// pixel since the position in `last`.
///
/// `last` advances only when a scroll is produced. Advancing it on every event loses a slow
/// drag for good: on a high-frequency touch device each step is below the threshold.
pub fn touch_scroll_step(last: &mut Option<(f32, f32)>, x: f32, y: f32) -> Option<(f32, f32)> {
    let (dx, dy) = last.map_or((0., 0.), |(last_x, last_y)| (last_x - x, last_y - y));
    (dx.abs() > 0.5 || dy.abs() > 0.5).then(|| {
        *last = Some((x, y));
        (dx, dy)
    })
}

/// The cursor shown at a point of the scene: a text beam over an editable control.
fn pointer_cursor(runtime: &crate::Runtime, x: f32, y: f32) -> winit::window::CursorIcon {
    let index = runtime.hit_index(x, y);
    if index >= 0 && runtime.control_editable(index as usize) {
        winit::window::CursorIcon::Text
    } else {
        winit::window::CursorIcon::Default
    }
}

/// Whether a resize is answered with a frame inside the event itself. The Metal layer presents
/// with the window's transaction, so a frame drawn later would show the old size stretched.
fn resize_presents_now(gpu: bool) -> bool {
    cfg!(target_os = "macos") && gpu
}

/// Physical window size to the logical size reported to the application.
fn logical_size(width: u32, height: u32, scale: f32) -> (f32, f32) {
    let scale = if scale > 0. { scale } else { 1. };
    (width as f32 / scale, height as f32 / scale)
}

const DOUBLE_CLICK_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);

const DOUBLE_CLICK_DISTANCE: f32 = 4.;

/// Recognizes a double click from primary presses; winit reports none.
#[derive(Default)]
struct ClickTracker {
    last: Option<(Instant, f32, f32)>,
}

impl ClickTracker {
    /// Records a primary press and tells whether it completes a double click. A
    /// completed pair is consumed, so a third press starts a new one.
    fn press(&mut self, at: Instant, x: f32, y: f32) -> bool {
        let double = self.last.is_some_and(|(then, lx, ly)| {
            at.saturating_duration_since(then) <= DOUBLE_CLICK_INTERVAL
                && (x - lx).abs() <= DOUBLE_CLICK_DISTANCE
                && (y - ly).abs() <= DOUBLE_CLICK_DISTANCE
        });
        self.last = if double { None } else { Some((at, x, y)) };
        double
    }
}

#[derive(Clone)]
struct RedrawHandle {
    window: Weak<Window>,
}

impl RedrawHandle {
    fn new(window: &Arc<Window>) -> Self {
        Self {
            window: Arc::downgrade(window),
        }
    }

    fn request_redraw(&self) {
        if let Some(window) = self.window.upgrade() {
            window.request_redraw();
        }
    }
}

/// Paint the runtime retained after synchronization, without another app update.
fn paint_app<A: NativeApp>(
    app: &A,
    out: &mut [u32],
    width: u32,
    height: u32,
    scale: f32,
) -> Result<bool, String> {
    let mut painted = Err("NativeApp.inspect did not observe a runtime".to_owned());
    app.inspect(&mut |runtime| {
        painted = runtime
            .paint_native(out, width, height, scale)
            .map(|_| runtime.is_animating())
            .map_err(str::to_owned);
    })?;
    painted
}

struct NativeHost<A: NativeApp + 'static> {
    app: A,
    config: NativeWindowConfig,
    backend: Option<UiBackend>,
    platform: TargetPlatform,
    chooser: Option<crate::Runtime>,
    accessibility: Option<accesskit_winit::Adapter>,
    waker: Option<NativeWaker>,
    accessibility_active: bool,
    accessibility_ids: crate::native_accessibility::PublishedTree,
    window: Option<Arc<Window>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    gpu: Option<crate::native_gpu::GpuWindow>,
    gpu_attempted: bool,
    retry_at: Option<Instant>,
    gpu_health_at: Option<Instant>,
    #[cfg(feature = "native-test-hooks")]
    idle_loss_for_test: bool,
    occluded: bool,
    animation_at: Option<Instant>,
    pointer: (f32, f32),
    touch_last: Option<(f32, f32)>,
    touch_owner: Option<(winit::event::DeviceId, u64)>,
    shift: bool,
    command: bool,
    control: bool,
    alt: bool,
    ime: ImeSession,
    blurred_editor: Option<String>,
    clicks: ClickTracker,
    last_frame: Instant,
    extension: Extension<A>,
}

impl<A: NativeApp + 'static> NativeHost<A> {
    fn event(&mut self, event: WindowEvent, event_loop: &ActiveEventLoop) -> Result<(), String> {
        match event {
            WindowEvent::CloseRequested => {
                if self.app.on_close_requested()? {
                    event_loop.exit();
                    return Ok(());
                }
            }
            WindowEvent::RedrawRequested => return self.draw(),
            WindowEvent::Resized(_) if resize_presents_now(self.gpu.is_some()) => {
                self.report_size()?;
                return self.draw();
            }
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                self.report_size()?;
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let scale = self
                    .window
                    .as_ref()
                    .ok_or_else(|| "Native window is not initialized".to_owned())?
                    .scale_factor() as f32;
                self.pointer = (position.x as f32 / scale, position.y as f32 / scale);
                let (x, y) = self.pointer;
                if let Some(chooser) = &mut self.chooser {
                    chooser.pointer(x, y, 0);
                } else {
                    self.focus_sensitive_input(&mut |runtime| runtime.pointer(x, y, 0))?;
                    let mut cursor = winit::window::CursorIcon::Default;
                    self.app
                        .inspect(&mut |runtime| cursor = pointer_cursor(runtime, x, y))?;
                    if let Some(window) = &self.window {
                        window.set_cursor(cursor);
                    }
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if let Some(chooser) = &mut self.chooser {
                    chooser.pointer(-1., -1., 3);
                } else {
                    self.focus_sensitive_input(&mut |runtime| runtime.pointer(-1., -1., 3))?;
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                let (x, y) = self.pointer;
                let pressed = state == ElementState::Pressed;
                let kind = if pressed { 1 } else { 2 };
                if let Some(chooser) = &mut self.chooser {
                    let pick = chooser_pick(chooser, |chooser| {
                        chooser.pointer(x, y, kind);
                    });
                    if let Some(chosen) = pick {
                        self.select_backend(chosen);
                    }
                } else {
                    self.primary_button(pressed, Instant::now())?;
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let kind = button_pointer_kind(button, state == ElementState::Pressed);
                if let (Some(kind), None) = (kind, &self.chooser) {
                    self.pointer_event(kind)?;
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let scale = self
                    .window
                    .as_ref()
                    .ok_or_else(|| "Native window is not initialized".to_owned())?
                    .scale_factor() as f32;
                let (dx, dy) = wheel_scroll(delta, scale);
                if let Some(chooser) = &mut self.chooser {
                    chooser.scroll(dx, dy);
                } else {
                    self.focus_sensitive_input(&mut |runtime| runtime.scroll(dx, dy))?;
                }
            }
            WindowEvent::Touch(Touch {
                device_id,
                id,
                phase,
                location,
                ..
            }) => {
                let scale = self
                    .window
                    .as_ref()
                    .ok_or_else(|| "Native window is not initialized".to_owned())?
                    .scale_factor() as f32;
                let (x, y) = (location.x as f32 / scale, location.y as f32 / scale);
                self.touch_input(device_id, id, phase, x, y)?;
            }
            WindowEvent::Focused(focused) => self.focus_changed(focused)?,

            WindowEvent::ModifiersChanged(modifiers) => {
                let held = NativeModifiers::from_state(modifiers.state());
                (self.shift, self.alt) = (held.shift, held.alt);
                (self.control, self.command) = (held.control, held.command);
            }
            WindowEvent::Ime(event) => self.ime_event(event)?,
            WindowEvent::KeyboardInput { event, .. } if self.extension_key(&event, event_loop) => {}
            #[cfg(feature = "native-test-hooks")]
            WindowEvent::KeyboardInput { event, .. }
                if self.config.mode == HostMode::Demo
                    && event.logical_key == Key::Named(NamedKey::F9) =>
            {
                if event.state == ElementState::Pressed && !event.repeat {
                    if let Some(gpu) = &self.gpu {
                        let _ = gpu;
                        self.idle_loss_for_test = true;
                    }
                }
                // Exercise completion service, bypassing the generic input
                // redraw for both press and release. Ordinary builds omit this.
                return Ok(());
            }
            WindowEvent::KeyboardInput { event, .. } => self.keyboard_input(event)?,
            WindowEvent::Occluded(occluded) => {
                self.app.on_occlusion_changed(occluded)?;
                self.occluded = occluded;
                self.retry_at = None;
                self.animation_at = None;
                if occluded {
                    return Ok(());
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            _ => {}
        }
        self.update_ime()?;
        if let Some(window) = &self.window {
            window.request_redraw();
        }
        Ok(())
    }
}

impl<A: NativeApp + 'static> ApplicationHandler<HostEvent> for NativeHost<A> {
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: HostEvent) {
        let event = match event {
            HostEvent::Accessibility(event) => event,
            HostEvent::Wake => {
                if let Err(error) = self.woken() {
                    eprintln!("{error}");
                    event_loop.exit();
                    return;
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
                return;
            }
        };
        if !self
            .window
            .as_ref()
            .is_some_and(|window| window.id() == event.window_id)
        {
            return;
        }
        let result = match event.window_event {
            accesskit_winit::WindowEvent::InitialTreeRequested => {
                self.accessibility_ids.invalidate();
                self.accessibility_active = true;
                self.update_accessibility()
            }
            accesskit_winit::WindowEvent::AccessibilityDeactivated => {
                self.accessibility_active = false;
                Ok(())
            }
            accesskit_winit::WindowEvent::ActionRequested(request) => self
                .accessibility_action(&request)
                .and_then(|_| self.update_accessibility())
                .and_then(|_| self.update_ime()),
        };
        if let Err(error) = result {
            eprintln!("Accessibility update failed: {error}");
            event_loop.exit();
            return;
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        #[cfg(feature = "native-test-hooks")]
        if self.idle_loss_for_test {
            self.idle_loss_for_test = false;
            if let Some(gpu) = &self.gpu {
                gpu.inject_device_loss_for_testing();
                self.gpu_health_at = Some(now);
            }
        }
        if self.gpu_health_at.is_some_and(|at| at <= now) {
            self.gpu_health_at = None;
            if let Some(gpu) = &self.gpu {
                let health = gpu.poll_idle();
                if let Ok(idle) = &health {
                    if let Err(error) = self.app.on_gpu_health_checked(*idle) {
                        eprintln!("GPU health observer failed: {error}");
                        event_loop.exit();
                        return;
                    }
                }
                match health {
                    Ok(false) => {
                        self.gpu_health_at = Some(now + std::time::Duration::from_millis(16))
                    }
                    Ok(true) => {}
                    Err(error) => {
                        let error = format!("Idle GPU completion: {error}");
                        let fallback = self.app.on_gpu_fallback(&error).and_then(|_| {
                            if let Some(window) = &self.window {
                                crate::native_gpu::set_visible(window, false)?;
                            }
                            Ok(())
                        });
                        if let Err(error) = fallback {
                            eprintln!("{error}");
                            event_loop.exit();
                            return;
                        }
                        eprintln!("Native GPU completion failed; CPU fallback: {error}");
                        self.gpu = None;
                        self.retry_at = None;
                        self.sync_window_title();
                        if !self.occluded {
                            if let Some(window) = &self.window {
                                window.request_redraw();
                            }
                        }
                    }
                }
            }
        }
        let (redraw, deadline) = self.scheduled_redraw(now);
        let (extension_redraw, extension_deadline) = self.extension_idle(now);
        let deadline = [deadline, extension_deadline].into_iter().flatten().min();
        if redraw || extension_redraw {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
        event_loop.set_control_flow(deadline.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let (plat_w, plat_h) = self
            .platform
            .viewport_size(self.config.width, self.config.height);
        let init_w = self
            .chooser
            .as_ref()
            .map_or(plat_w, |c| c.render_width().max(plat_w));
        let init_h = self
            .chooser
            .as_ref()
            .map_or(plat_h, |c| c.render_height().max(plat_h));
        type CpuWindowSurface = (
            Arc<Window>,
            Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
        );
        let setup = || -> Result<CpuWindowSurface, String> {
            let window = Arc::new(
                event_loop
                    .create_window(
                        Window::default_attributes()
                            .with_visible(false)
                            .with_title(self.config.title.clone())
                            .with_inner_size(LogicalSize::new(init_w, init_h))
                            .with_resizable(self.config.resizable),
                    )
                    .map_err(|e| e.to_string())?,
            );
            // On iOS the GPU draws every frame; the software surface is created by the first
            // frame that needs it, after a GPU failure.
            let surface = if cfg!(target_os = "ios") {
                None
            } else {
                Some(software_surface(&window)?)
            };
            let redraw = RedrawHandle::new(&window);
            self.app
                .on_dirty(Box::new(move || redraw.request_redraw()))?;
            if let Some(waker) = &self.waker {
                self.app.on_waker(waker.clone())?;
            }
            Ok((window, surface))
        };
        match setup() {
            Ok((window, surface)) => {
                if let Some(waker) = &self.waker {
                    self.accessibility = Some(accesskit_winit::Adapter::with_event_loop_proxy(
                        event_loop,
                        &window,
                        waker.proxy(),
                    ));
                }
                window.set_visible(true);
                window.request_redraw();
                if let Some(extension) = self.extension() {
                    extension.window_created(&window);
                }
                self.window = Some(window);
                self.surface = surface;
                self.sync_window_title();
                if let Err(error) = self.report_size() {
                    eprintln!("{error}");
                    event_loop.exit();
                    return;
                }
                self.announce_startup();
            }
            Err(error) => {
                eprintln!("{error}");
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self
            .window
            .as_ref()
            .is_some_and(|window| window.id() != window_id)
        {
            if let Some(extension) = self.extension() {
                extension.window_event(window_id, event);
            }
            return;
        }
        if let (Some(adapter), Some(window)) = (&mut self.accessibility, &self.window) {
            adapter.process_event(window, &event);
        }
        if let Err(error) = self
            .event(event, event_loop)
            .and_then(|_| self.update_accessibility())
        {
            eprintln!("{error}");
            event_loop.exit();
        }
    }
}

/// Runs a Forma application with desktop Svet GPU presentation and CPU fallback.
pub fn run<A: NativeApp + 'static>(app: A, config: NativeWindowConfig) -> Result<(), String> {
    run_host(app, config, no_extension)
}

fn run_host<A: NativeApp + 'static>(
    app: A,
    config: NativeWindowConfig,
    extension: Extension<A>,
) -> Result<(), String> {
    let backend = config.resolved_backend();
    let platform = config.platform;
    let chooser = if backend.is_none() {
        Some(build_chooser_runtime()?)
    } else {
        None
    };
    let event_loop = EventLoop::<HostEvent>::with_user_event()
        .build()
        .map_err(|e| e.to_string())?;
    let waker = NativeWaker::new(event_loop.create_proxy());
    event_loop.set_control_flow(ControlFlow::Wait);
    event_loop
        .run_app(&mut NativeHost {
            app,
            config,
            backend,
            platform,
            chooser,
            accessibility: None,
            waker: Some(waker),
            accessibility_active: false,
            accessibility_ids: Default::default(),
            window: None,
            surface: None,
            gpu: None,
            gpu_attempted: false,
            retry_at: None,
            gpu_health_at: None,
            #[cfg(feature = "native-test-hooks")]
            idle_loss_for_test: false,
            occluded: false,
            animation_at: None,
            pointer: (0., 0.),
            touch_last: None,
            touch_owner: None,
            shift: false,
            command: false,
            control: false,
            alt: false,
            ime: ImeSession::default(),
            blurred_editor: None,
            clicks: Default::default(),
            last_frame: Instant::now(),
            extension,
        })
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod dev_tests;
#[cfg(test)]
mod tests;
