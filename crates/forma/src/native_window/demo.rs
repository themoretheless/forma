//! The demo shell: the backend chooser, the two-button scene and the simulated platform chrome.

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

    pub(super) fn resolve(
        args: impl IntoIterator<Item = String>,
        env: Option<&str>,
    ) -> Option<Self> {
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
