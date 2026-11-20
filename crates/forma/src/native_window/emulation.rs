//! Demo presentation shell and device/platform chrome emulation, separated from the
//! native OS window event loop.

use super::*;

/// Emulated or compiled target platform for Forma native runtime (`desktop`, `ios`, `android`).
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

    pub(super) fn resolve(args: impl IntoIterator<Item = String>, env: Option<&str>) -> Self {
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
            let island_w = ((122.0 * s) as u32).min(width.saturating_sub(24));
            let island_h = ((26.0 * s) as u32).clamp(8, height / 6);
            let island_x = (width.saturating_sub(island_w)) / 2;
            let island_y = ((10.0 * s) as u32).min(height.saturating_sub(island_h));
            fill_rect(
                buffer, island_x, island_y, island_w, island_h, 0x090B10, true,
            );
            let lens_size = ((8.0 * s) as u32).clamp(3, island_h.saturating_sub(4));
            let lens_x = island_x + island_w.saturating_sub(lens_size + (8.0 * s) as u32);
            let lens_y = island_y + (island_h.saturating_sub(lens_size)) / 2;
            fill_rect(buffer, lens_x, lens_y, lens_size, lens_size, 0x1E2942, true);

            let home_w = ((134.0 * s) as u32).min(width.saturating_sub(32));
            let home_h = ((5.0 * s) as u32).clamp(3, 12);
            let home_x = (width.saturating_sub(home_w)) / 2;
            let home_y = height.saturating_sub(home_h + ((8.0 * s) as u32).max(4));
            fill_rect(buffer, home_x, home_y, home_w, home_h, 0xE2E8F5, true);
        }
        TargetPlatform::Android => {
            let hole_size = ((14.0 * s) as u32).clamp(6, height / 10);
            let hole_x = (width.saturating_sub(hole_size)) / 2;
            let hole_y = ((10.0 * s) as u32).min(height.saturating_sub(hole_size));
            fill_rect(buffer, hole_x, hole_y, hole_size, hole_size, 0x090B10, true);

            let nav_w = ((108.0 * s) as u32).min(width.saturating_sub(32));
            let nav_h = ((4.0 * s) as u32).clamp(3, 10);
            let nav_x = (width.saturating_sub(nav_w)) / 2;
            let nav_y = height.saturating_sub(nav_h + ((8.0 * s) as u32).max(4));
            fill_rect(buffer, nav_x, nav_y, nav_w, nav_h, 0x76DFB1, true);
        }
        TargetPlatform::Desktop => {}
    }
}

/// Feed input to the backend chooser; returns the backend picked when that input clicked a choice.
pub(crate) fn chooser_pick(
    chooser: &mut crate::Runtime,
    input: impl FnOnce(&mut crate::Runtime),
) -> Option<UiBackend> {
    let before = chooser.clicks();
    input(chooser);
    (chooser.clicks() != before).then(|| {
        if chooser.key() == "choose_egui" {
            UiBackend::Egui
        } else {
            UiBackend::Svet
        }
    })
}

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

/// The egui accent bar marks the demo's compatibility profile; an application frame
/// is presented as painted.
pub(super) fn shows_frame_bar(mode: HostMode, backend: Option<UiBackend>) -> bool {
    mode == HostMode::Demo && backend == Some(UiBackend::Egui)
}

/// Window title. An application gets its own title untouched; the demo appends the
/// active backend, the emulated platform and the function-key hints.
pub(super) fn window_title(
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

/// Encapsulates demo backend chooser, emulated platform selection, and software chrome overlays
/// so the OS window host loop stays free of platform-emulation concerns.
pub(super) struct HostShell {
    pub(super) mode: HostMode,
    pub(super) backend: Option<UiBackend>,
    pub(super) platform: TargetPlatform,
    pub(super) chooser: Option<crate::Runtime>,
}

impl HostShell {
    pub(super) fn from_config(config: &NativeWindowConfig) -> Result<Self, String> {
        let backend = config.resolved_backend();
        let chooser = if backend.is_none() {
            Some(build_chooser_runtime()?)
        } else {
            None
        };
        Ok(Self {
            mode: config.mode,
            backend,
            platform: config.platform,
            chooser,
        })
    }

    #[cfg(test)]
    pub(super) fn windowless(config: &NativeWindowConfig) -> Self {
        Self {
            mode: config.mode,
            backend: config.resolved_backend(),
            platform: config.platform,
            chooser: None,
        }
    }

    pub(super) fn initial_viewport(&self, config: &NativeWindowConfig) -> (f32, f32) {
        let (plat_w, plat_h) = self.platform.viewport_size(config.width, config.height);
        self.chooser.as_ref().map_or((plat_w, plat_h), |c| {
            (c.render_width().max(plat_w), c.render_height().max(plat_h))
        })
    }

    pub(super) fn viewport(&self, config: &NativeWindowConfig) -> (f32, f32) {
        self.platform.viewport_size(config.width, config.height)
    }

    pub(super) fn uses_gpu(&self, ios: bool) -> bool {
        uses_gpu(ios, self.backend, self.platform, self.chooser.is_some())
    }

    pub(super) fn title(&self, base_title: &str, gpu_active: bool) -> String {
        window_title(
            self.mode,
            base_title,
            self.backend,
            self.platform,
            gpu_active,
        )
    }

    pub(super) fn announce_startup(&self) {
        if self.mode == HostMode::Application {
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

    pub(super) fn select_platform(
        &mut self,
        platform: TargetPlatform,
        config: &NativeWindowConfig,
    ) -> (f32, f32) {
        self.platform = platform;
        println!(
            "Активна платформа: {} ({}) · F6 — переключить на лету",
            platform.as_str(),
            platform.label()
        );
        self.viewport(config)
    }

    pub(super) fn select_backend(
        &mut self,
        backend: UiBackend,
        config: &NativeWindowConfig,
    ) -> (f32, f32) {
        self.backend = Some(backend);
        self.chooser = None;
        println!("Активен бэкенд: {} ({})", backend.as_str(), backend.label());
        self.viewport(config)
    }

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

    pub(super) fn decorate_software_frame(
        &self,
        buffer: &mut [u32],
        width: u32,
        height: u32,
        scale: f32,
    ) {
        if shows_frame_bar(self.mode, self.backend) {
            apply_egui_frame_bar(buffer, width, height);
        }
        if emulates_device_chrome() {
            apply_mobile_chrome(buffer, width, height, scale, self.platform);
        }
    }
}
