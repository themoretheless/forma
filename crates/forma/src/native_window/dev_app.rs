//! The dev window: what the `forma` binary shows on top of the shared host.

use super::*;
use crate::{frame_stats::FrameRate, native_gpu::Presentation};

/// What the dev window's title reports.
pub struct DevTitle<'a> {
    pub platform: TargetPlatform,
    /// The backend chooser is shown instead of the scene.
    pub choosing: bool,
    pub backend: Option<UiBackend>,
    pub gpu: bool,
    pub error: Option<&'a str>,
    pub surface: Presentation,
    pub rate: FrameRate,
    pub continuous: bool,
    pub clicks: u32,
}

pub fn dev_title(state: &DevTitle<'_>) -> String {
    let plat_suffix = if state.platform != TargetPlatform::Desktop {
        format!(" · {}", state.platform.short_label())
    } else {
        String::new()
    };
    if state.choosing {
        return format!(
            "Forma{plat_suffix} · Выберите бэкенд: [1] egui / [2] svet · F6: платформа"
        );
    }
    let ui_tag = state.backend.map_or("svet", UiBackend::as_str);
    let backend = match state.platform {
        TargetPlatform::Ios => "Metal (iOS)",
        TargetPlatform::Android => "Vulkan (Android)",
        TargetPlatform::Desktop => {
            if state.gpu && state.backend != Some(UiBackend::Egui) {
                "GPU"
            } else {
                "CPU"
            }
        }
    };
    let rate = if let Some(error) = state.error {
        format!("ошибка: {error}")
    } else if state.surface == Presentation::Occluded {
        "occluded".into()
    } else if state.surface == Presentation::ZeroSize {
        "нет поверхности".into()
    } else if state.surface == Presentation::Retry {
        "ожидание поверхности".into()
    } else {
        match state.rate {
            FrameRate::Waiting => "ожидание кадра".into(),
            FrameRate::Sampling => "замер…".into(),
            FrameRate::Idle => "idle".into(),
            FrameRate::Active { fps, frame_ms } => format!("{fps:.1} FPS · {frame_ms:.2} ms"),
        }
    };
    let mode = if state.continuous {
        "FPS-тест"
    } else {
        "F6: iOS/Android · F7: бэкенд"
    };
    let mut title = format!("Forma [{ui_tag}{plat_suffix}] · {backend} · {rate} · {mode}");
    if state.clicks > 0 {
        title.push_str(&format!(" · кликов: {}", state.clicks));
    }
    title
}

/// The node tree and control geometry the element picker prints as `FORMA_INSPECT`.
pub fn inspect_report(runtime: &crate::Runtime) -> String {
    let nodes = runtime
        .nodes()
        .iter()
        .map(|n| {
            format!(
                "{{\"id\":{},\"parent\":{},\"control\":{},\"kind\":\"{:?}\"}}",
                n.id,
                n.parent.map_or("null".into(), |v| v.to_string()),
                n.control.map_or("null".into(), |v| v.to_string()),
                n.kind
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let controls = (0..runtime.control_count())
        .map(|i| {
            format!(
                "{{\"index\":{},\"bounds\":{:?},\"disabled\":{}}}",
                i,
                runtime.control_bounds(i),
                runtime.control_disabled(i)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"selected\":{},\"nodes\":[{}],\"controls\":[{}]}}",
        runtime.focused_index(),
        nodes,
        controls
    )
}

/// Function keys of the dev window that the shared host does not know.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DevShortcut {
    /// F8: redraw continuously to measure the frame rate.
    FpsTest,
    /// F9: pick elements with the pointer and print their description.
    Inspect,
    /// F10: open or close the detached popup window.
    Popup,
}

pub fn dev_shortcut(key: &Key, pressed: bool, repeat: bool) -> Option<DevShortcut> {
    if !pressed || repeat {
        return None;
    }
    match key {
        Key::Named(NamedKey::F8) => Some(DevShortcut::FpsTest),
        Key::Named(NamedKey::F9) => Some(DevShortcut::Inspect),
        Key::Named(NamedKey::F10) => Some(DevShortcut::Popup),
        _ => None,
    }
}
