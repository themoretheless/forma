//! Reusable software-rendered native window host for Forma applications.

use std::{
    num::NonZeroU32,
    sync::{Arc, Weak},
    time::Instant,
};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalPosition, LogicalSize},
    event::{ElementState, Ime, MouseButton, MouseScrollDelta, Touch, TouchPhase, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, NamedKey},
    window::{Window, WindowId},
};

/// Application interface consumed by the native window host.
pub trait NativeApp {
    fn update(
        &mut self,
        input: &mut dyn FnMut(&mut crate::Runtime),
    ) -> Result<(), String>;

    fn on_dirty(&self, callback: Box<dyn Fn()>) -> Result<(), String> {
        let _ = callback;
        Ok(())
    }
}

impl NativeApp for crate::Runtime {
    fn update(
        &mut self,
        input: &mut dyn FnMut(&mut crate::Runtime),
    ) -> Result<(), String> {
        input(self);
        self.sync_ui_bindings_native().map(|_| ())
    }
}

/// Execution backend for Forma native windows (`egui` immediate-mode vs `svet` reactive GPU/vector).
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
            Self::Egui => "egui (Immediate-Mode UI)",
            Self::Svet => "svet (Reactive Push-Pull DAG + UMA/DUMA GPU)",
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
        let mut args = std::env::args().skip(1);
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
        if let Ok(env_val) = std::env::var("FORMA_BACKEND") {
            if matches!(env_val.trim().to_ascii_lowercase().as_str(), "ask" | "choose") {
                return None;
            }
            if let Some(parsed) = Self::from_arg(&env_val) {
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
        let mut args = std::env::args().skip(1);
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
        if let Ok(env_val) = std::env::var("FORMA_PLATFORM") {
            if let Some(parsed) = Self::from_arg(&env_val) {
                return parsed;
            }
        }
        #[cfg(target_os = "ios")]
        {
            return Self::Ios;
        }
        #[cfg(target_os = "android")]
        {
            return Self::Android;
        }
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        {
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
            fill_rect(buffer, island_x, island_y, island_w, island_h, 0x090B10, true);
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

/// Initial settings for a Forma native window.
pub struct NativeWindowConfig {
    pub title: String,
    pub width: f32,
    pub height: f32,
    pub resizable: bool,
    pub backend: Option<UiBackend>,
    pub platform: TargetPlatform,
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
        }
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

struct NativeHost<A: NativeApp + 'static> {
    app: A,
    config: NativeWindowConfig,
    backend: Option<UiBackend>,
    platform: TargetPlatform,
    chooser: Option<crate::Runtime>,
    window: Option<Arc<Window>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    pointer: (f32, f32),
    touch_last: Option<(f32, f32)>,
    shift: bool,
    command: bool,
    alt: bool,
    composing: bool,
    last_frame: Instant,
}

impl<A: NativeApp + 'static> NativeHost<A> {
    fn sync_window_title(&self) {
        if let Some(window) = &self.window {
            let plat_suffix = if self.platform != TargetPlatform::Desktop {
                format!(" · {}", self.platform.short_label())
            } else {
                String::new()
            };
            let title = match self.backend {
                Some(b) => format!(
                    "{} [{}{}] · F6: платформа · F7: бэкенд",
                    self.config.title,
                    b.as_str(),
                    plat_suffix
                ),
                None => format!(
                    "{}{} · Выберите бэкенд: [1] egui / [2] svet",
                    self.config.title, plat_suffix
                ),
            };
            window.set_title(&title);
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
        let window = self
            .window
            .as_ref()
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
                animating = runtime.is_animating();
            })?;
        }

        let (Some(width), Some(height)) = (
            NonZeroU32::new(size.width),
            NonZeroU32::new(size.height),
        ) else {
            return Ok(());
        };
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
            let mut painted = Ok(());
            self.app.update(&mut |runtime| {
                painted = runtime
                    .paint_native(&mut buffer, size.width, size.height, scale)
                    .map_err(str::to_owned);
            })?;
            painted?;
            if self.backend == Some(UiBackend::Egui) {
                apply_egui_frame_bar(&mut buffer, size.width, size.height);
            }
            apply_mobile_chrome(&mut buffer, size.width, size.height, scale, self.platform);
        }
        buffer.present().map_err(|e| e.to_string())?;

        if animating {
            window.request_redraw();
        }
        Ok(())
    }

    fn update_ime(&mut self) -> Result<(), String> {
        if self.chooser.is_some() {
            if let Some(window) = &self.window {
                window.set_ime_allowed(false);
            }
            return Ok(());
        }
        let mut editing = false;
        let mut caret = Vec::new();
        self.app.update(&mut |runtime| {
            editing = runtime.text_editing();
            caret = runtime.text_caret_bounds();
        })?;
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

    fn keyboard_input(&mut self, event: winit::event::KeyEvent) -> Result<(), String> {
        let pressed = event.state == ElementState::Pressed;
        if pressed && !event.repeat && event.logical_key == Key::Named(NamedKey::F6) {
            let next = self.platform.next();
            self.select_platform(next);
            return Ok(());
        }
        if pressed && !event.repeat && event.logical_key == Key::Named(NamedKey::F7) {
            let next = self.backend.map_or(UiBackend::Svet, UiBackend::toggle);
            self.select_backend(next);
            return Ok(());
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
                let key = match &event.logical_key {
                    Key::Character(text) => text.to_string(),
                    Key::Named(_) => named,
                    _ => String::new(),
                };
                let shift = self.shift;
                // Ctrl+Alt is AltGr for text-producing keys on Windows and Linux,
                // not an application shortcut such as Undo or Select All.
                let alt_gr = cfg!(not(target_os = "macos"))
                    && self.command
                    && self.alt
                    && event.text.is_some();
                let command = self.command && !alt_gr;
                self.app.update(&mut |runtime| {
                    if !runtime.text_key(&key, shift, command) && !command {
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

        let key = match &event.logical_key {
            Key::Named(NamedKey::Space) => 1,
            Key::Named(NamedKey::Enter) => 2,
            Key::Named(NamedKey::Escape) => 3,
            _ => 0,
        };
        let shift = self.shift;
        let repeat = event.repeat;
        self.app.update(&mut |runtime| {
            if event.logical_key == Key::Named(NamedKey::Tab) && pressed && !repeat {
                if !runtime.focus_next(shift) {
                    runtime.focus(false);
                    runtime.focus_next(shift);
                }
            }
            runtime.key_event(key, pressed, repeat);
        })?;
        Ok(())
    }

    fn event(
        &mut self,
        event: WindowEvent,
        event_loop: &ActiveEventLoop,
    ) -> Result<(), String> {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
                return Ok(());
            }
            WindowEvent::RedrawRequested => return self.draw(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
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
                    self.app.update(&mut |runtime| runtime.pointer(x, y, 0))?;
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if let Some(chooser) = &mut self.chooser {
                    chooser.pointer(-1., -1., 3);
                } else {
                    self.app
                        .update(&mut |runtime| runtime.pointer(-1., -1., 3))?;
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                let (x, y) = self.pointer;
                let kind = if state == ElementState::Pressed { 1 } else { 2 };
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
                    self.app
                        .update(&mut |runtime| runtime.pointer(x, y, kind))?;
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
                    self.app.update(&mut |runtime| runtime.scroll(dx, dy))?;
                }
            }
            WindowEvent::Touch(Touch {
                phase, location, ..
            }) => {
                let scale = self
                    .window
                    .as_ref()
                    .ok_or_else(|| "Native window is not initialized".to_owned())?
                    .scale_factor() as f32;
                let (x, y) = (location.x as f32 / scale, location.y as f32 / scale);
                self.pointer = (x, y);
                match phase {
                    TouchPhase::Started => {
                        self.touch_last = Some((x, y));
                        if let Some(chooser) = &mut self.chooser {
                            chooser.pointer(x, y, 0);
                            chooser.pointer(x, y, 1);
                        } else {
                            self.app.update(&mut |runtime| {
                                runtime.pointer(x, y, 0);
                                runtime.pointer(x, y, 1);
                            })?;
                        }
                    }
                    TouchPhase::Moved => {
                        let (dx, dy) = self
                            .touch_last
                            .map_or((0., 0.), |(lx, ly)| (lx - x, ly - y));
                        self.touch_last = Some((x, y));
                        if let Some(chooser) = &mut self.chooser {
                            chooser.pointer(x, y, 0);
                            if dx.abs() > 0.5 || dy.abs() > 0.5 {
                                chooser.scroll(dx, dy);
                            }
                        } else {
                            self.app.update(&mut |runtime| {
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
                            self.app.update(&mut |runtime| runtime.pointer(x, y, 2))?;
                        }
                    }
                    TouchPhase::Cancelled => {
                        self.touch_last = None;
                        if let Some(chooser) = &mut self.chooser {
                            chooser.pointer(-1., -1., 3);
                        } else {
                            self.app
                                .update(&mut |runtime| runtime.pointer(-1., -1., 3))?;
                        }
                    }
                }
            }
            WindowEvent::Focused(focused) => {
                if let Some(chooser) = &mut self.chooser {
                    chooser.focus(focused);
                    if !focused {
                        chooser.pointer(-1., -1., 3);
                    }
                } else {
                    self.app.update(&mut |runtime| {
                        runtime.focus(focused);
                        if !focused {
                            runtime.pointer(-1., -1., 3);
                        }
                    })?;
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.shift = modifiers.state().shift_key();
                self.alt = modifiers.state().alt_key();
                self.command = if cfg!(target_os = "macos") {
                    modifiers.state().super_key()
                } else {
                    modifiers.state().control_key()
                };
            }
            WindowEvent::Ime(Ime::Preedit(text, _)) => {
                self.composing = !text.is_empty();
                self.app
                    .update(&mut |runtime| runtime.text_preedit(&text))?;
            }
            WindowEvent::Ime(Ime::Commit(text)) => {
                self.composing = false;
                self.app.update(&mut |runtime| {
                    runtime.text_insert(&text);
                })?;
            }
            WindowEvent::Ime(Ime::Disabled) => {
                self.composing = false;
                self.app
                    .update(&mut |runtime| runtime.text_preedit(""))?;
            }
            WindowEvent::KeyboardInput { event, .. } => self.keyboard_input(event)?,
            WindowEvent::Occluded(false) => {
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

impl<A: NativeApp + 'static> ApplicationHandler for NativeHost<A> {
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
        let setup = || -> Result<(Arc<Window>, softbuffer::Surface<Arc<Window>, Arc<Window>>), String> {
            let window = Arc::new(
                event_loop
                    .create_window(
                        Window::default_attributes()
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
            Ok((window, surface))
        };
        match setup() {
            Ok((window, surface)) => {
                window.request_redraw();
                self.window = Some(window);
                self.surface = Some(surface);
                self.sync_window_title();
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
                    println!("Выбор бэкенда запуска Forma: нажмите [1] egui или [2] svet в окне (или клавиши 1 / 2; F6: платформа, F7: бэкенд)");
                }
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
        if let Err(error) = self.event(event, event_loop) {
            eprintln!("{error}");
            event_loop.exit();
        }
    }
}

/// Runs a Forma application in a real native window using the CPU renderer.
pub fn run<A: NativeApp + 'static>(
    app: A,
    config: NativeWindowConfig,
) -> Result<(), String> {
    let backend = config.backend;
    let platform = config.platform;
    let chooser = if backend.is_none() {
        Some(build_chooser_runtime()?)
    } else {
        None
    };
    let event_loop = EventLoop::new().map_err(|e| e.to_string())?;
    event_loop.set_control_flow(ControlFlow::Wait);
    event_loop
        .run_app(&mut NativeHost {
            app,
            config,
            backend,
            platform,
            chooser,
            window: None,
            surface: None,
            pointer: (0., 0.),
            touch_last: None,
            shift: false,
            command: false,
            alt: false,
            composing: false,
            last_frame: Instant::now(),
        })
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_backend_parses_and_toggles_between_egui_and_svet() {
        assert_eq!(UiBackend::from_arg("egui"), Some(UiBackend::Egui));
        assert_eq!(UiBackend::from_arg("EGUI"), Some(UiBackend::Egui));
        assert_eq!(UiBackend::from_arg("1"), Some(UiBackend::Egui));
        assert_eq!(UiBackend::from_arg("svet"), Some(UiBackend::Svet));
        assert_eq!(UiBackend::from_arg("SVET"), Some(UiBackend::Svet));
        assert_eq!(UiBackend::from_arg("2"), Some(UiBackend::Svet));
        assert_eq!(UiBackend::from_arg("unknown"), None);
        assert_eq!(UiBackend::Egui.toggle(), UiBackend::Svet);
        assert_eq!(UiBackend::Svet.toggle(), UiBackend::Egui);
        assert_eq!(UiBackend::Egui.as_str(), "egui");
        assert_eq!(UiBackend::Svet.as_str(), "svet");
    }

    #[test]
    fn chooser_and_two_buttons_runtimes_compile_and_click() {
        let mut chooser = build_chooser_runtime().expect("compile backend chooser .ui");
        assert_eq!(chooser.control_count(), 2);
        chooser.activate_control(0);
        assert_eq!(chooser.key(), "choose_egui");
        chooser.activate_control(1);
        assert_eq!(chooser.key(), "choose_svet");

        let mut two =
            crate::Runtime::from_sources(crate::TWO_BUTTONS_EXAMPLE, crate::BUTTON_COMPONENT)
                .expect("compile TwoButtons.ui");
        assert_eq!(two.control_count(), 2);
        two.activate_control(0);
        assert_eq!(two.key(), "first");
        assert_eq!(two.action(), "actions.first");
        two.activate_control(1);
        assert_eq!(two.key(), "second");
        assert_eq!(two.action(), "actions.second");
    }

    #[test]
    fn target_platform_parses_cycles_and_renders_mobile_chrome() {
        assert_eq!(TargetPlatform::from_arg("desktop"), Some(TargetPlatform::Desktop));
        assert_eq!(TargetPlatform::from_arg("ios"), Some(TargetPlatform::Ios));
        assert_eq!(TargetPlatform::from_arg("iPhone"), Some(TargetPlatform::Ios));
        assert_eq!(TargetPlatform::from_arg("android"), Some(TargetPlatform::Android));
        assert_eq!(TargetPlatform::from_arg("Pixel"), Some(TargetPlatform::Android));
        assert_eq!(TargetPlatform::from_arg("unknown"), None);

        assert_eq!(TargetPlatform::Desktop.next(), TargetPlatform::Ios);
        assert_eq!(TargetPlatform::Ios.next(), TargetPlatform::Android);
        assert_eq!(TargetPlatform::Android.next(), TargetPlatform::Desktop);

        assert_eq!(TargetPlatform::Ios.viewport_size(800.0, 600.0), (393.0, 852.0));
        assert_eq!(TargetPlatform::Android.viewport_size(800.0, 600.0), (412.0, 915.0));
        assert_eq!(TargetPlatform::Desktop.viewport_size(800.0, 600.0), (800.0, 600.0));

        let mut buf = vec![0x111520u32; 393 * 852];
        apply_mobile_chrome(&mut buf, 393, 852, 1.0, TargetPlatform::Ios);
        assert!(buf.contains(&0x090B10), "iOS Dynamic Island should be painted");
        assert!(buf.contains(&0xE2E8F5), "iOS Home Indicator should be painted");

        let mut android_buf = vec![0x111520u32; 412 * 915];
        apply_mobile_chrome(&mut android_buf, 412, 915, 1.0, TargetPlatform::Android);
        assert!(android_buf.contains(&0x090B10), "Android camera cutout should be painted");
        assert!(android_buf.contains(&0x76DFB1), "Android gesture pill should be painted");
    }
}
