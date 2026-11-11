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
    dpi::{LogicalPosition, LogicalSize},
    event::{ElementState, Ime, MouseButton, MouseScrollDelta, Touch, TouchPhase, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    keyboard::{Key, NamedKey, PhysicalKey},
    window::{Window, WindowId},
};

/// Presentation path that successfully submitted a native frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeRenderer {
    Cpu,
    Gpu,
}

/// Application interface consumed by the native window host.
pub trait NativeApp {
    fn update(&mut self, input: &mut dyn FnMut(&mut crate::Runtime)) -> Result<(), String>;

    /// Inspect the current runtime without dispatching input or updating the app.
    /// Invoke the observer exactly once on the runtime retained after update.
    fn inspect(&self, observer: &mut dyn FnMut(&crate::Runtime)) -> Result<(), String>;

    /// Optional observer after a frame was actually presented.
    fn on_frame_presented(&self, _renderer: NativeRenderer) -> Result<(), String> {
        Ok(())
    }

    /// Optional observer after completion health service, without a new frame.
    /// `idle` means no queue work remains and completion wakeups will stop.
    fn on_gpu_health_checked(&self, _idle: bool) -> Result<(), String> {
        Ok(())
    }

    /// Optional observer before a failed GPU path falls back to CPU presentation.
    fn on_gpu_fallback(&self, _error: &str) -> Result<(), String> {
        Ok(())
    }

    /// Optional observer for OS window visibility/occlusion notifications.
    fn on_occlusion_changed(&self, _occluded: bool) -> Result<(), String> {
        Ok(())
    }

    /// Optional diagnostic observer for IME events before host dispatch.
    /// Applications should avoid collecting user text unless explicitly needed.
    fn on_ime_event(&self, _event: &Ime) -> Result<(), String> {
        Ok(())
    }

    fn on_dirty(&self, callback: Box<dyn Fn()>) -> Result<(), String> {
        let _ = callback;
        Ok(())
    }

    /// Receives the window's waker once, when the window is created. Keep a clone in
    /// worker threads and call [`NativeWaker::wake`] after publishing a result: the
    /// loop then runs `update` on its own thread and redraws.
    fn on_waker(&self, _waker: NativeWaker) -> Result<(), String> {
        Ok(())
    }

    /// Sees a key before the runtime does. Return `true` to mark it handled: the
    /// runtime then receives nothing for this event. Keys the demo host captures
    /// (see [`HostMode`]) and keys typed on its backend chooser are not offered.
    fn on_key_event(&mut self, _event: &NativeKeyEvent<'_>) -> Result<bool, String> {
        Ok(false)
    }

    /// The window's content area changed. `width` and `height` are logical pixels,
    /// `scale` is the display scale factor. Called once when the window is created and
    /// then on every resize or scale change; a minimized window may report zero.
    fn on_resized(&mut self, _width: f32, _height: f32, _scale: f32) -> Result<(), String> {
        Ok(())
    }

    /// The user asked to close the window. Return `false` to keep it open.
    fn on_close_requested(&mut self) -> Result<bool, String> {
        Ok(true)
    }

    /// Pointer input the runtime has no notion of: secondary and middle buttons, and a
    /// double click of the primary button (reported after the runtime received the
    /// second press). Not called while the demo's backend chooser is shown.
    fn on_pointer_event(&mut self, _event: &NativePointerEvent) -> Result<(), String> {
        Ok(())
    }
}

/// Modifier keys held during an input event. `command` is the platform's shortcut
/// modifier: Command on macOS, Control elsewhere.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NativeModifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub command: bool,
}

/// Key event offered to [`NativeApp::on_key_event`].
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct NativeKeyEvent<'a> {
    pub logical_key: &'a Key,
    pub physical_key: &'a PhysicalKey,
    /// Text the key produces, when it produces any.
    pub text: Option<&'a str>,
    pub pressed: bool,
    pub repeat: bool,
    pub modifiers: NativeModifiers,
    /// An input-method composition is in progress; the key usually belongs to it.
    pub composing: bool,
}

/// What [`NativeApp::on_pointer_event`] reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum NativePointerKind {
    Secondary { pressed: bool },
    Middle { pressed: bool },
    DoubleClick,
}

/// Pointer event in logical pixels of the window's content area.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct NativePointerEvent {
    pub kind: NativePointerKind,
    pub x: f32,
    pub y: f32,
    pub modifiers: NativeModifiers,
}

/// Maps a mouse button to an application pointer event. The primary button belongs to
/// the runtime; back, forward and other buttons stay unreported.
fn button_pointer_kind(button: MouseButton, pressed: bool) -> Option<NativePointerKind> {
    match button {
        MouseButton::Right => Some(NativePointerKind::Secondary { pressed }),
        MouseButton::Middle => Some(NativePointerKind::Middle { pressed }),
        _ => None,
    }
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

/// Events the host posts to its own loop.
enum HostEvent {
    Accessibility(accesskit_winit::Event),
    Wake,
}

impl From<accesskit_winit::Event> for HostEvent {
    fn from(event: accesskit_winit::Event) -> Self {
        Self::Accessibility(event)
    }
}

struct WakerShared {
    // Whether the proxy is `Sync` differs between winit platforms; the lock makes the
    // handle `Sync` on all of them.
    proxy: Mutex<EventLoopProxy<HostEvent>>,
    pending: AtomicBool,
}

/// Thread-safe handle that makes the window loop update the application and redraw.
/// Calls made before the loop has served the previous one are merged into it.
#[derive(Clone)]
pub struct NativeWaker {
    shared: Arc<WakerShared>,
}

impl NativeWaker {
    fn new(proxy: EventLoopProxy<HostEvent>) -> Self {
        Self {
            shared: Arc::new(WakerShared {
                proxy: Mutex::new(proxy),
                pending: AtomicBool::new(false),
            }),
        }
    }

    fn proxy(&self) -> EventLoopProxy<HostEvent> {
        self.shared
            .proxy
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Asks the loop to call `NativeApp::update` and redraw. Returns `false` once the
    /// loop has ended, so a worker can stop.
    pub fn wake(&self) -> bool {
        if !begin_wake(&self.shared.pending) {
            return true;
        }
        let sent = self.proxy().send_event(HostEvent::Wake).is_ok();
        if !sent {
            self.shared.pending.store(false, Ordering::Release);
        }
        sent
    }
}

/// Claims the pending wake; `false` means one is already queued and covers this call.
fn begin_wake(pending: &AtomicBool) -> bool {
    !pending.swap(true, Ordering::AcqRel)
}

/// Releases the pending wake before the update it stands for, so a result published
/// during that update queues the next one.
fn finish_wake(pending: &AtomicBool) {
    pending.store(false, Ordering::Release);
}

impl NativeApp for crate::Runtime {
    fn inspect(&self, observer: &mut dyn FnMut(&crate::Runtime)) -> Result<(), String> {
        observer(self);
        Ok(())
    }

    fn update(&mut self, input: &mut dyn FnMut(&mut crate::Runtime)) -> Result<(), String> {
        input(self);
        self.sync_ui_bindings_native().map(|_| ())
    }
}

/// Native presentation profile. `Egui` currently provides CPU compatibility styling;
/// `Svet` uses desktop GPU vector presentation with CPU fallback. Application
/// scheduling is owned separately by NativeApp/Runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiBackend {
    Egui,
    Svet,
}

impl UiBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Egui => "egui",
            Self::Svet => "svet",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Egui => "egui-compatible CPU presentation",
            Self::Svet => "Svet GPU vector presentation (CPU fallback)",
        }
    }

    pub fn toggle(self) -> Self {
        match self {
            Self::Egui => Self::Svet,
            Self::Svet => Self::Egui,
        }
    }

    pub fn from_arg(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "egui" | "1" => Some(Self::Egui),
            "svet" | "2" => Some(Self::Svet),
            _ => None,
        }
    }

    /// Resolves backend from CLI flags (`--backend egui|svet`, `--egui`, `--svet`) or `FORMA_BACKEND`.
    /// Returns `None` when interactive selection should be shown (`--ask` or `FORMA_BACKEND=ask`).
    pub fn resolve_from_env_and_args() -> Option<Self> {
        Self::resolve(
            std::env::args().skip(1),
            std::env::var("FORMA_BACKEND").ok().as_deref(),
        )
    }

    fn resolve(args: impl IntoIterator<Item = String>, env: Option<&str>) -> Option<Self> {
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--egui" | "--backend=egui" => return Some(Self::Egui),
                "--svet" | "--backend=svet" => return Some(Self::Svet),
                "--ask" | "--choose-backend" | "--backend=ask" => return None,
                "--backend" | "-b" => {
                    if let Some(next) = args.next() {
                        if matches!(next.trim().to_ascii_lowercase().as_str(), "ask" | "choose") {
                            return None;
                        }
                        if let Some(parsed) = Self::from_arg(&next) {
                            return Some(parsed);
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(env_val) = env {
            if matches!(
                env_val.trim().to_ascii_lowercase().as_str(),
                "ask" | "choose"
            ) {
                return None;
            }
            if let Some(parsed) = Self::from_arg(env_val) {
                return Some(parsed);
            }
        }
        None
    }
}

pub const BACKEND_CHOOSER_UI: &str = r#"component BackendLauncher {
    Frame {
        width: 480;
        height: 260;
        padding: 36;
        gap: 20;
        background: #141923;
        radius: 16;
        clip: true;
        Button {
            key: 'choose_egui';
            width: 408;
            height: 76;
            text: '[1] Запустить на egui (Immediate-Mode)';
            background: #5b8af5;
            hoverBackground: #7ba3ff;
            pressedBackground: #426cd4;
            clicked -> actions.choose_egui();
        }
        Button {
            key: 'choose_svet';
            width: 408;
            height: 76;
            text: '[2] Запустить на svet (Reactive GPU/UMA)';
            background: #58c496;
            hoverBackground: #76dfb1;
            pressedBackground: #3fa177;
            clicked -> actions.choose_svet();
        }
    }
}"#;

pub fn chooser_ui_for_viewport(vw: f32, vh: f32) -> String {
    let w = vw.max(320.0).round() as i32;
    let h = vh.max(360.0).round() as i32;
    let pad_x = 24;
    let btn_w = (w - pad_x * 2).max(200);
    format!(
        r#"component BackendLauncher {{
    Frame {{
        width: {w};
        height: {h};
        padding: 88 {pad_x} 24 {pad_x};
        gap: 20;
        background: #141923;
        radius: 0;
        clip: true;
        Button {{
            key: 'choose_egui';
            width: {btn_w};
            height: 76;
            text: '[1] Запустить на egui (Immediate-Mode)';
            background: #5b8af5;
            hoverBackground: #7ba3ff;
            pressedBackground: #426cd4;
            clicked -> actions.choose_egui();
        }}
        Button {{
            key: 'choose_svet';
            width: {btn_w};
            height: 76;
            text: '[2] Запустить на svet (Reactive GPU/UMA)';
            background: #58c496;
            hoverBackground: #76dfb1;
            pressedBackground: #3fa177;
            clicked -> actions.choose_svet();
        }}
    }}
}}"#
    )
}

pub fn two_buttons_ui_for_viewport(vw: f32, vh: f32) -> String {
    two_buttons_ui_for_viewport_with_state(vw, vh, None, None)
}

pub fn two_buttons_ui_for_viewport_with_state(
    vw: f32,
    vh: f32,
    last_clicked_key: Option<&str>,
    clicks: Option<(u32, u32)>,
) -> String {
    let w = vw.max(320.0).round() as i32;
    let h = vh.max(360.0).round() as i32;
    let pad_x = 24;
    let btn_w = (w - pad_x * 2).max(200);
    let (c1, c2) = clicks.unwrap_or((0, 0));
    let first_text = if c1 > 0 {
        format!("Первая кнопка · нажата ({c1})")
    } else {
        "Первая кнопка".to_string()
    };
    let second_text = if c2 > 0 {
        format!("Вторая кнопка · нажата ({c2})")
    } else {
        "Вторая кнопка".to_string()
    };
    let (first_bg, first_hover, first_pressed) = if last_clicked_key == Some("first") {
        ("#4c7df0", "#6c97ff", "#3662cc")
    } else {
        ("#8ca5ff", "#a8baff", "#6a83da")
    };
    let (second_bg, second_hover, second_pressed) = if last_clicked_key == Some("second") {
        ("#58c496", "#76dfb1", "#3fa177")
    } else {
        ("#aad9bd", "#c7eed7", "#78b493")
    };
    format!(
        r#"component TwoButtons {{
    Frame {{
        width: {w};
        height: {h};
        padding: 88 {pad_x} 24 {pad_x};
        gap: 20;
        background: #181e2a;
        radius: 0;
        clip: true;
        Button {{
            key: 'first';
            width: {btn_w};
            height: 76;
            text: '{first_text}';
            background: {first_bg};
            hoverBackground: {first_hover};
            pressedBackground: {first_pressed};
            clicked -> actions.first();
        }}
        Button {{
            key: 'second';
            width: {btn_w};
            height: 76;
            text: '{second_text}';
            background: {second_bg};
            hoverBackground: {second_hover};
            pressedBackground: {second_pressed};
            clicked -> actions.second();
        }}
    }}
}}"#
    )
}

pub fn build_chooser_runtime() -> Result<crate::Runtime, String> {
    crate::Runtime::from_sources(BACKEND_CHOOSER_UI, crate::BUTTON_COMPONENT)
}

/// Target platform for Forma native runtime (`desktop`, `ios`, `android`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TargetPlatform {
    #[default]
    Desktop,
    Ios,
    Android,
}

impl TargetPlatform {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Ios => "ios",
            Self::Android => "android",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::Desktop => "Desktop",
            Self::Ios => "iOS",
            Self::Android => "Android",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Desktop => "Desktop (macOS / Windows / Linux)",
            Self::Ios => "iOS (iPhone 16 Pro · 393×852 · Metal)",
            Self::Android => "Android (Pixel 9 · 412×915 · Vulkan)",
        }
    }

    pub fn viewport_size(self, fallback_w: f32, fallback_h: f32) -> (f32, f32) {
        match self {
            Self::Desktop => (fallback_w.max(1.0), fallback_h.max(1.0)),
            Self::Ios => (393.0, 852.0),
            Self::Android => (412.0, 915.0),
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Desktop => Self::Ios,
            Self::Ios => Self::Android,
            Self::Android => Self::Desktop,
        }
    }

    pub fn from_arg(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "desktop" | "macos" | "windows" | "linux" => Some(Self::Desktop),
            "ios" | "iphone" | "ipad" => Some(Self::Ios),
            "android" | "pixel" => Some(Self::Android),
            _ => None,
        }
    }

    /// Resolves target platform from CLI flags (`--platform ios|android|desktop`, `--ios`, `--android`)
    /// or `FORMA_PLATFORM` environment variable.
    pub fn resolve_from_env_and_args() -> Self {
        Self::resolve(
            std::env::args().skip(1),
            std::env::var("FORMA_PLATFORM").ok().as_deref(),
        )
    }

    fn resolve(args: impl IntoIterator<Item = String>, env: Option<&str>) -> Self {
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--ios" | "--platform=ios" => return Self::Ios,
                "--android" | "--platform=android" => return Self::Android,
                "--desktop" | "--platform=desktop" => return Self::Desktop,
                "--platform" | "-p" => {
                    if let Some(next) = args.next() {
                        if let Some(parsed) = Self::from_arg(&next) {
                            return parsed;
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(env_val) = env {
            if let Some(parsed) = Self::from_arg(env_val) {
                return parsed;
            }
        }
        Self::compiled()
    }

    /// Platform of the compilation target, without consulting the process environment.
    pub const fn compiled() -> Self {
        if cfg!(target_os = "ios") {
            Self::Ios
        } else if cfg!(target_os = "android") {
            Self::Android
        } else {
            Self::Desktop
        }
    }
}

/// Applies an immediate-mode `egui` frame accent bar on top of a software pixel buffer
/// so `egui` and `svet` modes are visually distinguishable in addition to window title and logs.
pub fn apply_egui_frame_bar(buffer: &mut [u32], width: u32, height: u32) {
    if width == 0 || height < 4 {
        return;
    }
    let bar_h = 3u32.min(height);
    for y in 0..bar_h {
        let row = (y * width) as usize;
        for x in 0..width as usize {
            if let Some(px) = buffer.get_mut(row + x) {
                *px = 0x5B8AF5;
            }
        }
    }
}

/// Renders mobile device indicators (iOS Dynamic Island + Home Indicator; Android status bar +
/// navigation gesture pill) into a pixel buffer when running in `iOS` or `Android` platform mode.
pub fn apply_mobile_chrome(
    buffer: &mut [u32],
    width: u32,
    height: u32,
    scale: f32,
    platform: TargetPlatform,
) {
    if platform == TargetPlatform::Desktop || width < 32 || height < 32 {
        return;
    }
    let s = scale.max(1.0);
    let fill_rect = |buf: &mut [u32], x0: u32, y0: u32, w: u32, h: u32, color: u32, round: bool| {
        let x1 = (x0 + w).min(width);
        let y1 = (y0 + h).min(height);
        let r = if round { (h / 2) as i32 } else { 0 };
        for y in y0..y1 {
            let row = (y * width) as usize;
            let dy = if round {
                let cy = (y0 as i32) + (h as i32) / 2;
                (y as i32 - cy).abs()
            } else {
                0
            };
            for x in x0..x1 {
                if round && r > 0 {
                    let left_cx = (x0 as i32) + r;
                    let right_cx = (x1 as i32) - r;
                    let dx = if (x as i32) < left_cx {
                        left_cx - (x as i32)
                    } else if (x as i32) > right_cx {
                        (x as i32) - right_cx
                    } else {
                        0
                    };
                    if dx * dx + dy * dy > r * r {
                        continue;
                    }
                }
                if let Some(px) = buf.get_mut(row + x as usize) {
                    *px = color;
                }
            }
        }
    };
    match platform {
        TargetPlatform::Ios => {
            // iOS Dynamic Island pill at top center + Home Indicator bar at bottom center
            let island_w = ((122.0 * s) as u32).min(width.saturating_sub(24));
            let island_h = ((26.0 * s) as u32).clamp(8, height / 6);
            let island_x = (width.saturating_sub(island_w)) / 2;
            let island_y = ((10.0 * s) as u32).min(height.saturating_sub(island_h));
            fill_rect(
                buffer, island_x, island_y, island_w, island_h, 0x090B10, true,
            );
            // Subtle camera lens dot inside Dynamic Island
            let lens_size = ((8.0 * s) as u32).clamp(3, island_h.saturating_sub(4));
            let lens_x = island_x + island_w.saturating_sub(lens_size + (8.0 * s) as u32);
            let lens_y = island_y + (island_h.saturating_sub(lens_size)) / 2;
            fill_rect(buffer, lens_x, lens_y, lens_size, lens_size, 0x1E2942, true);

            // Home Indicator pill at bottom
            let home_w = ((134.0 * s) as u32).min(width.saturating_sub(32));
            let home_h = ((5.0 * s) as u32).clamp(3, 12);
            let home_x = (width.saturating_sub(home_w)) / 2;
            let home_y = height.saturating_sub(home_h + ((8.0 * s) as u32).max(4));
            fill_rect(buffer, home_x, home_y, home_w, home_h, 0xE2E8F5, true);
        }
        TargetPlatform::Android => {
            // Android punch-hole camera at top center + status accent + bottom gesture pill
            let hole_size = ((14.0 * s) as u32).clamp(6, height / 10);
            let hole_x = (width.saturating_sub(hole_size)) / 2;
            let hole_y = ((10.0 * s) as u32).min(height.saturating_sub(hole_size));
            fill_rect(buffer, hole_x, hole_y, hole_size, hole_size, 0x090B10, true);

            // Android gesture navigation bar at bottom
            let nav_w = ((108.0 * s) as u32).min(width.saturating_sub(32));
            let nav_h = ((4.0 * s) as u32).clamp(3, 10);
            let nav_x = (width.saturating_sub(nav_w)) / 2;
            let nav_y = height.saturating_sub(nav_h + ((8.0 * s) as u32).max(4));
            fill_rect(buffer, nav_x, nav_y, nav_w, nav_h, 0x76DFB1, true);
        }
        TargetPlatform::Desktop => {}
    }
}

/// Who the native window host serves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HostMode {
    /// Demo and probe behaviour: backend chooser when no backend is set, F6/F7 (and the
    /// test-hook F8/F9) captured by the host, diagnostic title suffix, egui accent bar,
    /// startup lines on stdout.
    #[default]
    Demo,
    /// An embedding application owns the window: no chooser, no captured function keys,
    /// the title exactly as configured, no accent bar, nothing printed on stdout.
    Application,
}

/// Initial settings for a Forma native window.
pub struct NativeWindowConfig {
    pub title: String,
    pub width: f32,
    pub height: f32,
    pub resizable: bool,
    pub backend: Option<UiBackend>,
    pub platform: TargetPlatform,
    pub mode: HostMode,
}

impl NativeWindowConfig {
    /// Settings for an embedding application. Unlike `Default`, this reads neither
    /// `std::env::args` nor the `FORMA_*` variables: Svet presentation on the platform
    /// of the compilation target, 800x600, resizable, [`HostMode::Application`].
    pub fn application(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            width: 800.,
            height: 600.,
            resizable: true,
            backend: Some(UiBackend::Svet),
            platform: TargetPlatform::compiled(),
            mode: HostMode::Application,
        }
    }

    /// Backend the host starts with. `None` shows the backend chooser, which only the
    /// demo mode has: an application without an explicit backend gets Svet.
    pub fn resolved_backend(&self) -> Option<UiBackend> {
        match self.mode {
            HostMode::Demo => self.backend,
            HostMode::Application => Some(self.backend.unwrap_or(UiBackend::Svet)),
        }
    }
}

impl Default for NativeWindowConfig {
    fn default() -> Self {
        Self {
            title: "Forma".into(),
            width: 800.,
            height: 600.,
            resizable: true,
            backend: UiBackend::resolve_from_env_and_args(),
            platform: TargetPlatform::resolve_from_env_and_args(),
            mode: HostMode::Demo,
        }
    }
}

/// Window title. An application gets its own title untouched; the demo appends the
/// active backend, the emulated platform and the function-key hints.
fn window_title(
    mode: HostMode,
    title: &str,
    backend: Option<UiBackend>,
    platform: TargetPlatform,
    gpu_active: bool,
) -> String {
    if mode == HostMode::Application {
        return title.to_owned();
    }
    let plat_suffix = if platform != TargetPlatform::Desktop {
        format!(" · {}", platform.short_label())
    } else {
        String::new()
    };
    match backend {
        Some(b) => format!(
            "{} [{}{}] · F6: платформа · F7: бэкенд",
            title,
            if gpu_active && b == UiBackend::Svet && platform == TargetPlatform::Desktop {
                "svet/GPU"
            } else {
                b.as_str()
            },
            plat_suffix
        ),
        None => format!("{title}{plat_suffix} · Выберите бэкенд: [1] egui / [2] svet"),
    }
}

/// Function keys the demo host keeps for itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HostShortcut {
    NextPlatform,
    ToggleBackend,
    InjectDeviceLoss,
}

/// Decides whether a key belongs to the host. In application mode none does, so every
/// key reaches the application and the runtime.
fn host_shortcut(
    mode: HostMode,
    key: &Key,
    pressed: bool,
    repeat: bool,
    test_hooks: bool,
) -> Option<HostShortcut> {
    if mode == HostMode::Application || !pressed || repeat {
        return None;
    }
    match key {
        Key::Named(NamedKey::F6) => Some(HostShortcut::NextPlatform),
        Key::Named(NamedKey::F7) => Some(HostShortcut::ToggleBackend),
        Key::Named(NamedKey::F8) if test_hooks => Some(HostShortcut::InjectDeviceLoss),
        _ => None,
    }
}

/// The egui accent bar marks the demo's compatibility profile; an application frame
/// is presented as painted.
fn shows_frame_bar(mode: HostMode, backend: Option<UiBackend>) -> bool {
    mode == HostMode::Demo && backend == Some(UiBackend::Egui)
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
    composing: bool,
    ime_cancelled: bool,
    composition_owner: Option<(u64, i32)>,
    blurred_editor: Option<String>,
    clicks: ClickTracker,
    last_frame: Instant,
}

impl<A: NativeApp + 'static> NativeHost<A> {
    fn accessibility_action(&mut self, request: &accesskit::ActionRequest) -> Result<bool, String> {
        if request.target_tree != accesskit::TreeId::ROOT {
            return Ok(false);
        }
        let pending_ime = self.composing || self.composition_owner.is_some();
        let mut accepted = false;
        let published = std::mem::take(&mut self.accessibility_ids);
        let result = if let Some(chooser) = &mut self.chooser {
            accepted = published.apply(chooser, request);
            Ok(())
        } else {
            self.focus_sensitive_input(&mut |runtime| {
                accepted = published.apply(runtime, request);
            })
        };
        self.accessibility_ids = published;
        result?;
        if accepted
            && pending_ime
            && matches!(
                request.action,
                accesskit::Action::SetValue | accesskit::Action::SetTextSelection
            )
        {
            self.composing = false;
            self.composition_owner = None;
            self.ime_cancelled = true;
        }
        Ok(accepted)
    }

    fn update_accessibility(&mut self) -> Result<(), String> {
        if !self.accessibility_active {
            return Ok(());
        }
        let scale = self
            .window
            .as_ref()
            .map_or(1., |window| window.scale_factor());
        let mut update = None;
        let published = &mut self.accessibility_ids;
        if let Some(chooser) = &self.chooser {
            update = Some(published.update_if_changed(chooser, scale));
        } else {
            self.app.inspect(&mut |runtime| {
                update = Some(published.update_if_changed(runtime, scale))
            })?;
        }
        let update = update.ok_or_else(|| {
            "NativeApp.inspect did not observe accessibility runtime".to_owned()
        })??;
        if let (Some(adapter), Some(update)) = (&mut self.accessibility, update) {
            adapter.update_if_active(|| update);
        }
        Ok(())
    }

    fn sync_window_title(&self) {
        if let Some(window) = &self.window {
            window.set_title(&window_title(
                self.config.mode,
                &self.config.title,
                self.backend,
                self.platform,
                self.gpu.is_some(),
            ));
        }
    }

    /// Startup lines of the demo; an application owns its stdout.
    fn announce_startup(&self) {
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

    fn select_platform(&mut self, platform: TargetPlatform) {
        self.platform = platform;
        println!(
            "Активна платформа: {} ({}) · F6 — переключить на лету",
            platform.as_str(),
            platform.label()
        );
        self.sync_window_title();
        if let Some(window) = &self.window {
            let (w, h) = platform.viewport_size(self.config.width, self.config.height);
            let _ = window.request_inner_size(LogicalSize::new(w, h));
            window.request_redraw();
        }
    }

    fn select_backend(&mut self, backend: UiBackend) {
        self.backend = Some(backend);
        self.gpu_attempted = self.gpu.is_some();
        self.retry_at = None;
        self.animation_at = None;
        self.chooser = None;
        println!("Активен бэкенд: {} ({})", backend.as_str(), backend.label());
        self.sync_window_title();
        if let Some(window) = &self.window {
            let (w, h) = self
                .platform
                .viewport_size(self.config.width, self.config.height);
            let _ = window.request_inner_size(LogicalSize::new(w, h));
            window.request_redraw();
        }
    }

    fn draw(&mut self) -> Result<(), String> {
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
        let use_gpu = self.backend == Some(UiBackend::Svet)
            && self.platform == TargetPlatform::Desktop
            && self.chooser.is_none();
        if use_gpu && self.gpu.is_none() && !self.gpu_attempted {
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
                self.app.inspect(&mut |runtime| {
                    animating = runtime.is_animating();
                    presentation = gpu.draw(runtime, &window);
                })?;
                match presentation {
                    Ok(crate::native_gpu::Presentation::Presented) => {
                        self.app.on_frame_presented(NativeRenderer::Gpu)?;
                        self.retry_at = None;
                        self.gpu_health_at =
                            Some(Instant::now() + std::time::Duration::from_millis(16));
                        if animating {
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
            apply_mobile_chrome(&mut buffer, size.width, size.height, scale, self.platform);
        }
        buffer.present().map_err(|e| e.to_string())?;
        if self.chooser.is_none() {
            self.app.on_frame_presented(NativeRenderer::Cpu)?;
        }

        if animating {
            // Start-to-start cadence: a costly CPU frame is not delayed again.
            // Input-triggered redraws may still paint before this deadline.
            self.animation_at = Some(now + std::time::Duration::from_nanos(16_666_667));
        }
        Ok(())
    }

    /// A worker thread asked for an update: run it now, even while the window is
    /// occluded and nothing is drawn. The demo's backend chooser precedes the
    /// application, which is updated by the first frame after the choice.
    fn woken(&mut self) -> Result<(), String> {
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

    fn modifiers(&self) -> NativeModifiers {
        NativeModifiers {
            shift: self.shift,
            control: self.control,
            alt: self.alt,
            command: self.command,
        }
    }

    /// Offers a key to the application ahead of the runtime; `true` means handled.
    fn offer_key(
        &mut self,
        logical_key: &Key,
        physical_key: &PhysicalKey,
        text: Option<&str>,
        pressed: bool,
        repeat: bool,
    ) -> Result<bool, String> {
        let event = NativeKeyEvent {
            logical_key,
            physical_key,
            text,
            pressed,
            repeat,
            modifiers: self.modifiers(),
            composing: self.composing,
        };
        self.app.on_key_event(&event)
    }

    fn pointer_event(&mut self, kind: NativePointerKind) -> Result<(), String> {
        let (x, y) = self.pointer;
        let event = NativePointerEvent {
            kind,
            x,
            y,
            modifiers: self.modifiers(),
        };
        self.app.on_pointer_event(&event)
    }

    /// Primary button on the application (not the chooser): the runtime gets the
    /// press or release first, then a completed double click is reported.
    fn primary_button(&mut self, pressed: bool, at: Instant) -> Result<(), String> {
        let (x, y) = self.pointer;
        let kind = if pressed { 1 } else { 2 };
        self.focus_sensitive_input(&mut |runtime| runtime.pointer(x, y, kind))?;
        if pressed && self.clicks.press(at, x, y) {
            self.pointer_event(NativePointerKind::DoubleClick)?;
        }
        Ok(())
    }

    /// Reports the window's content size to the application.
    fn report_size(&mut self) -> Result<(), String> {
        let Some(window) = &self.window else {
            return Ok(());
        };
        let size = window.inner_size();
        let scale = window.scale_factor() as f32;
        let (width, height) = logical_size(size.width, size.height, scale);
        self.app.on_resized(width, height, scale)
    }

    fn scheduled_redraw(&mut self, now: Instant) -> (bool, Option<Instant>) {
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

    /// Observe the retained runtime after application updates have completed.
    fn ime_presentation(&mut self) -> Result<(bool, Vec<f32>), String> {
        let owner = self.composition_owner;
        let mut retired = false;
        let mut presentation = (false, Vec::new());
        let result = self.app.inspect(&mut |runtime| {
            retired = owner.is_some() && runtime.native_editor_session() != owner;
            presentation = (
                runtime.text_editing()
                    && !runtime.control_readonly(runtime.focused_index().max(0) as usize),
                runtime.text_caret_bounds(),
            );
        });
        // The old preedit belongs to the retired runtime; never clear text in
        // its replacement while observing presentation.
        if retired {
            self.composing = false;
            self.ime_cancelled = true;
        }
        result?;
        Ok(presentation)
    }

    fn update_ime(&mut self) -> Result<(), String> {
        if self.chooser.is_some() {
            if let Some(window) = &self.window {
                window.set_ime_allowed(false);
            }
            return Ok(());
        }
        let (editing, caret) = self.ime_presentation()?;
        let window = self
            .window
            .as_ref()
            .ok_or_else(|| "Native window is not initialized".to_owned())?;
        window.set_ime_allowed(editing);
        if caret.len() == 4 {
            window.set_ime_cursor_area(
                LogicalPosition::new(caret[0] as f64, caret[1] as f64),
                LogicalSize::new(caret[2] as f64, caret[3] as f64),
            );
        }
        Ok(())
    }

    fn ime_event(&mut self, event: Ime) -> Result<(), String> {
        self.app.on_ime_event(&event)?;
        match event {
            Ime::Preedit(text, _) => self.preedit_ime(&text),
            Ime::Commit(text) => self.commit_ime(&text),
            Ime::Enabled => {
                self.composing = false;
                self.ime_cancelled = false;
                self.composition_owner = None;
                Ok(())
            }
            Ime::Disabled => {
                self.composing = false;
                self.ime_cancelled = true;
                self.composition_owner = None;
                self.app.update(&mut |runtime| runtime.text_preedit(""))
            }
        }
    }

    fn preedit_ime(&mut self, text: &str) -> Result<(), String> {
        let previous = self.composition_owner;
        let mut owner = previous;
        if !text.is_empty() {
            self.ime_cancelled = false;
        }
        self.composing = !text.is_empty();
        let result = self.app.update(&mut |runtime| {
            if !text.is_empty() {
                owner = runtime.native_editor_session();
                runtime.text_preedit(text);
            } else if previous.is_none() || runtime.native_editor_session() == previous {
                // An empty preedit removes marked pixels, but does not end the
                // ownership of the commit which commonly follows it.
                runtime.text_preedit("");
            }
        });
        self.composition_owner = owner;
        result
    }

    fn commit_ime(&mut self, text: &str) -> Result<(), String> {
        let owner = self.composition_owner.take();
        self.composing = false;
        if self.ime_cancelled {
            return Ok(());
        }
        let mut retired = false;
        let result = self.app.update(&mut |runtime| {
            // A commit queued for an old editor must not enter a replacement
            // document or a newly focused field. Direct commits without a
            // preceding preedit remain supported.
            if owner.is_none() || runtime.native_editor_session() == owner {
                if text.is_empty() {
                    runtime.text_preedit("");
                } else {
                    runtime.text_insert(text);
                }
            } else {
                retired = true;
            }
        });
        if retired {
            self.ime_cancelled = true;
        }
        result
    }

    fn focus_sensitive_input(
        &mut self,
        input: &mut dyn FnMut(&mut crate::Runtime),
    ) -> Result<(), String> {
        if !self.composing && self.composition_owner.is_none() {
            return self.app.update(input);
        }
        let mut retired = false;
        let mut owner = self.composition_owner;
        let result = self.app.update(&mut |runtime| {
            let previous = runtime.native_editor_session();
            owner = owner.or(previous);
            input(runtime);
            let next = runtime.native_editor_session();
            if previous.is_none() || previous != next {
                runtime.text_preedit("");
                retired = true;
            }
        });
        self.composition_owner = owner;
        if retired {
            self.composing = false;
            self.ime_cancelled = true;
            self.composition_owner = None;
        }
        result
    }

    fn touch_input(
        &mut self,
        device: winit::event::DeviceId,
        id: u64,
        phase: TouchPhase,
        x: f32,
        y: f32,
    ) -> Result<(), String> {
        let owner = (device, id);
        if phase == TouchPhase::Started {
            if self.touch_owner.is_some() {
                return Ok(());
            }
            self.touch_owner = Some(owner);
        } else if self.touch_owner != Some(owner) {
            return Ok(());
        }
        if matches!(phase, TouchPhase::Ended | TouchPhase::Cancelled) {
            self.touch_owner = None;
        }
        self.pointer = (x, y);
        match phase {
            TouchPhase::Started => {
                self.touch_last = Some((x, y));
                if let Some(chooser) = &mut self.chooser {
                    chooser.pointer(x, y, 0);
                    chooser.pointer(x, y, 1);
                } else {
                    self.focus_sensitive_input(&mut |runtime| {
                        runtime.pointer(x, y, 0);
                        runtime.pointer(x, y, 1);
                    })?;
                }
            }
            TouchPhase::Moved => {
                let (dx, dy) = self
                    .touch_last
                    .map_or((0., 0.), |(lx, ly)| (lx - x, ly - y));
                // Keep the last consumed position until enough motion has
                // accumulated; updating on every subthreshold event loses a
                // slow drag forever on high-frequency touch devices.
                if dx.abs() > 0.5 || dy.abs() > 0.5 {
                    self.touch_last = Some((x, y));
                }
                if let Some(chooser) = &mut self.chooser {
                    chooser.pointer(x, y, 0);
                    if dx.abs() > 0.5 || dy.abs() > 0.5 {
                        chooser.scroll(dx, dy);
                    }
                } else {
                    self.focus_sensitive_input(&mut |runtime| {
                        runtime.pointer(x, y, 0);
                        if dx.abs() > 0.5 || dy.abs() > 0.5 {
                            runtime.scroll(dx, dy);
                        }
                    })?;
                }
            }
            TouchPhase::Ended => {
                self.touch_last = None;
                if let Some(chooser) = &mut self.chooser {
                    let before_clicks = chooser.clicks();
                    chooser.pointer(x, y, 2);
                    if chooser.clicks() != before_clicks {
                        let chosen = if chooser.key() == "choose_egui" {
                            UiBackend::Egui
                        } else {
                            UiBackend::Svet
                        };
                        self.select_backend(chosen);
                    }
                } else {
                    self.focus_sensitive_input(&mut |runtime| runtime.pointer(x, y, 2))?;
                }
            }
            TouchPhase::Cancelled => {
                self.touch_last = None;
                if let Some(chooser) = &mut self.chooser {
                    chooser.pointer(-1., -1., 3);
                } else {
                    self.focus_sensitive_input(&mut |runtime| runtime.pointer(-1., -1., 3))?;
                }
            }
        }
        Ok(())
    }

    fn focus_changed(&mut self, focused: bool) -> Result<(), String> {
        if !focused {
            // OS focus loss may omit the matching IME/modifier/touch end events.
            self.ime_cancelled |= self.composing || self.composition_owner.is_some();
            self.composing = false;
            self.shift = false;
            self.command = false;
            self.control = false;
            self.alt = false;
            self.touch_last = None;
            self.touch_owner = None;
        }
        let mut remembered = self.blurred_editor.take();
        if let Some(chooser) = &mut self.chooser {
            chooser.focus(focused);
            if !focused {
                chooser.pointer(-1., -1., 3);
            }
        } else {
            let result = self.app.update(&mut |runtime| {
                if !focused {
                    if runtime.text_editing() {
                        remembered = Some(runtime.control_key(runtime.focused_index() as usize));
                    }
                    runtime.focus(false);
                    runtime.pointer(-1., -1., 3);
                } else {
                    // An activation click may select an editor before the OS
                    // Focused(true) event arrives. Its newer choice supersedes
                    // the editor remembered at blur.
                    if runtime.focused_index() >= 0 {
                        return;
                    }
                    // Resolve the stable key again: controls may reorder while the
                    // window is inactive. Ambiguous/noneditable/disabled targets
                    // fall back to the runtime's ordinary focus admission.
                    let restored = remembered.as_ref().is_some_and(|key| {
                        let target = {
                            let mut matches = (0..runtime.control_count())
                                .filter(|index| runtime.control_key(*index) == *key);
                            let first = matches.next();
                            if matches.next().is_some() {
                                None
                            } else {
                                first
                            }
                        };
                        target.is_some_and(|index| {
                            runtime.control_editable(index) && runtime.focus_control(index)
                        })
                    });
                    if !restored {
                        runtime.focus(true);
                    }
                }
            });
            if !focused || result.is_err() {
                self.blurred_editor = remembered;
            }
            result?;
        }
        Ok(())
    }

    fn keyboard_input(&mut self, event: winit::event::KeyEvent) -> Result<(), String> {
        let pressed = event.state == ElementState::Pressed;
        match host_shortcut(
            self.config.mode,
            &event.logical_key,
            pressed,
            event.repeat,
            cfg!(feature = "native-test-hooks"),
        ) {
            Some(HostShortcut::NextPlatform) => {
                let next = self.platform.next();
                self.select_platform(next);
                return Ok(());
            }
            Some(HostShortcut::ToggleBackend) => {
                let next = self.backend.map_or(UiBackend::Svet, UiBackend::toggle);
                self.select_backend(next);
                return Ok(());
            }
            Some(HostShortcut::InjectDeviceLoss) => {
                #[cfg(feature = "native-test-hooks")]
                if let Some(gpu) = &self.gpu {
                    gpu.inject_device_loss_for_testing();
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
                return Ok(());
            }
            None => {}
        }
        if let Some(chooser) = &mut self.chooser {
            if pressed && !event.repeat {
                match &event.logical_key {
                    Key::Character(c) if c.as_str() == "1" || c.eq_ignore_ascii_case("e") => {
                        self.select_backend(UiBackend::Egui);
                        return Ok(());
                    }
                    Key::Character(c) if c.as_str() == "2" || c.eq_ignore_ascii_case("s") => {
                        self.select_backend(UiBackend::Svet);
                        return Ok(());
                    }
                    Key::Named(NamedKey::Tab) => {
                        if !chooser.focus_next(self.shift) {
                            chooser.focus(false);
                            chooser.focus_next(self.shift);
                        }
                        return Ok(());
                    }
                    _ => {}
                }
            }
            let key = match &event.logical_key {
                Key::Named(NamedKey::Space) => 1,
                Key::Named(NamedKey::Enter) => 2,
                Key::Named(NamedKey::Escape) => 3,
                _ => 0,
            };
            let before_clicks = chooser.clicks();
            chooser.key_event(key, pressed, event.repeat);
            if chooser.clicks() != before_clicks {
                let chosen = if chooser.key() == "choose_egui" {
                    UiBackend::Egui
                } else {
                    UiBackend::Svet
                };
                self.select_backend(chosen);
            }
            return Ok(());
        }
        if self.offer_key(
            &event.logical_key,
            &event.physical_key,
            event.text.as_deref(),
            pressed,
            event.repeat,
        )? {
            return Ok(());
        }
        let named = match &event.logical_key {
            Key::Named(key) => format!("{key:?}"),
            _ => String::new(),
        };
        if pressed {
            let mut handled = false;
            self.app
                .update(&mut |runtime| handled = runtime.range_key(&named))?;
            if handled {
                return Ok(());
            }
        }

        let mut editing = false;
        self.app
            .update(&mut |runtime| editing = runtime.text_editing())?;
        if editing
            && !matches!(
                &event.logical_key,
                Key::Named(NamedKey::Tab) | Key::Named(NamedKey::Escape)
            )
        {
            if pressed && !self.composing {
                // Ctrl+Option belongs to macOS accessibility, not text editing.
                if cfg!(target_os = "macos") && self.control && self.alt {
                    return Ok(());
                }
                let key = match &event.logical_key {
                    Key::Character(text) => text.to_string(),
                    Key::Named(_) => named,
                    _ => String::new(),
                };
                let shift = self.shift;
                // Ctrl+Alt is AltGr for text-producing keys on Windows and Linux.
                let alt_gr = cfg!(not(target_os = "macos"))
                    && self.command
                    && self.alt
                    && event.text.is_some();
                let command = self.command && !alt_gr;
                let word_modifier = if cfg!(target_os = "macos") {
                    self.alt && !self.command
                } else {
                    self.command && !self.alt
                };
                let key = if command {
                    crate::native_clipboard::command_key(&event.physical_key, &key)
                } else {
                    &key
                };
                self.app.update(&mut |runtime| {
                    let word_handled = word_modifier && runtime.text_word_key(key, shift);
                    let clipboard_handled = !word_handled
                        && command
                        && crate::native_clipboard::handle_key(runtime, key);
                    if !word_handled
                        && !clipboard_handled
                        && !runtime.text_key(key, shift, command)
                        && !command
                    {
                        if let Some(text) = &event.text {
                            if !text.chars().any(char::is_control) {
                                runtime.text_insert(text);
                            }
                        }
                    }
                })?;
            }
            return Ok(());
        }
        self.navigation_key(&event.logical_key, pressed, event.repeat)
    }

    fn navigation_key(
        &mut self,
        logical_key: &Key,
        pressed: bool,
        repeat: bool,
    ) -> Result<(), String> {
        let cancel_composition = (self.composing || self.composition_owner.is_some())
            && pressed
            && !repeat
            && matches!(logical_key, Key::Named(NamedKey::Tab | NamedKey::Escape));
        if cancel_composition {
            self.composing = false;
            self.ime_cancelled = true;
        }
        let key = match logical_key {
            Key::Named(NamedKey::Space) => 1,
            Key::Named(NamedKey::Enter) => 2,
            Key::Named(NamedKey::Escape) => 3,
            Key::Named(NamedKey::ArrowUp) => 4,
            Key::Named(NamedKey::ArrowDown) => 5,
            _ => 0,
        };
        let shift = self.shift;
        self.app.update(&mut |runtime| {
            if cancel_composition {
                runtime.text_preedit("");
            }
            if *logical_key == Key::Named(NamedKey::Tab)
                && pressed
                && !repeat
                && !runtime.focus_next(shift)
            {
                runtime.focus(false);
                runtime.focus_next(shift);
            }
            runtime.key_event(key, pressed, repeat);
        })?;
        Ok(())
    }

    fn event(&mut self, event: WindowEvent, event_loop: &ActiveEventLoop) -> Result<(), String> {
        match event {
            WindowEvent::CloseRequested => {
                if self.app.on_close_requested()? {
                    event_loop.exit();
                    return Ok(());
                }
            }
            WindowEvent::RedrawRequested => return self.draw(),
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
                    let before_clicks = chooser.clicks();
                    chooser.pointer(x, y, kind);
                    if chooser.clicks() != before_clicks {
                        let chosen = if chooser.key() == "choose_egui" {
                            UiBackend::Egui
                        } else {
                            UiBackend::Svet
                        };
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
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (-x * 32., -y * 32.),
                    MouseScrollDelta::PixelDelta(position) => {
                        (-position.x as f32 / scale, -position.y as f32 / scale)
                    }
                };
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
                self.shift = modifiers.state().shift_key();
                self.alt = modifiers.state().alt_key();
                self.control = modifiers.state().control_key();
                self.command = if cfg!(target_os = "macos") {
                    modifiers.state().super_key()
                } else {
                    modifiers.state().control_key()
                };
            }
            WindowEvent::Ime(event) => self.ime_event(event)?,
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
        if redraw {
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
        type CpuWindowSurface = (Arc<Window>, softbuffer::Surface<Arc<Window>, Arc<Window>>);
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
            let context = softbuffer::Context::new(window.clone()).map_err(|e| e.to_string())?;
            let surface =
                softbuffer::Surface::new(&context, window.clone()).map_err(|e| e.to_string())?;
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
                self.window = Some(window);
                self.surface = Some(surface);
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
            composing: false,
            ime_cancelled: false,
            composition_owner: None,
            blurred_editor: None,
            clicks: Default::default(),
            last_frame: Instant::now(),
        })
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests;
