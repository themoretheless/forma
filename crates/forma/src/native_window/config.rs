//! How a window is configured and the shortcuts the host itself handles.

use super::*;

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

/// Function keys the demo host keeps for itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HostShortcut {
    NextPlatform,
    ToggleBackend,
    InjectDeviceLoss,
}

/// Decides whether a key belongs to the host. In application mode none does, so every
/// key reaches the application and the runtime.
pub(super) fn host_shortcut(
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
