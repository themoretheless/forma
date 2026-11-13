//! The dev window: what the `forma` binary shows on top of the shared host.

use super::*;
use crate::{
    frame_stats::{FrameRate, FrameStats},
    native_gpu::Presentation,
};
use std::cell::{Cell, RefCell};

/// What the dev window's title reports.
pub(super) struct DevTitle<'a> {
    pub(super) platform: TargetPlatform,
    /// The backend chooser is shown instead of the scene.
    pub(super) choosing: bool,
    pub(super) backend: Option<UiBackend>,
    pub(super) gpu: bool,
    pub(super) error: Option<&'a str>,
    pub(super) surface: Presentation,
    pub(super) rate: FrameRate,
    pub(super) continuous: bool,
    pub(super) clicks: u32,
}

pub(super) fn dev_title(state: &DevTitle<'_>) -> String {
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
pub(super) fn inspect_report(runtime: &crate::Runtime) -> String {
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
pub(super) enum DevShortcut {
    /// F8: redraw continuously to measure the frame rate.
    FpsTest,
    /// F9: pick elements with the pointer and print their description.
    Inspect,
    /// F10: open or close the detached popup window.
    Popup,
}

pub(super) fn dev_shortcut(key: &Key, pressed: bool, repeat: bool) -> Option<DevShortcut> {
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

const TITLE_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);

/// The application behind the `forma` binary's window: a scene loaded from disk, reloaded
/// when its files change, with a diagnostic title, an element picker and a frame rate test.
pub(super) struct DevApp {
    pub(super) runtime: crate::Runtime,
    hot: HotReload,
    backend: Option<UiBackend>,
    inspect_mode: bool,
    inspect_report: String,
    a11y_report: String,
    binding_error: Option<String>,
    // Presentation is reported through `&self`.
    frame_stats: RefCell<FrameStats>,
    title_due: Cell<Instant>,
    last_title: String,
    continuous_redraw: bool,
    log_fps: bool,
    cpu_only: bool,
    windows: AuxiliaryWindows,
    #[cfg(test)]
    pub(super) printed: Vec<String>,
}

/// What the environment asks of the dev window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct DevEnvironment {
    /// `FORMA_FPS_TEST=1`: start with the frame rate test on.
    pub(super) fps_test: bool,
    /// `FORMA_FPS_LOG=1`: print the title whenever it changes.
    pub(super) fps_log: bool,
    /// `FORMA_RENDERER=cpu`: never create the GPU surface.
    pub(super) cpu_only: bool,
}

impl DevEnvironment {
    fn read() -> Self {
        let is = |name: &str, value: &str| std::env::var(name).as_deref() == Ok(value);
        Self {
            fps_test: is("FORMA_FPS_TEST", "1"),
            fps_log: is("FORMA_FPS_LOG", "1"),
            cpu_only: is("FORMA_RENDERER", "cpu"),
        }
    }
}

impl DevApp {
    pub(super) fn new(
        project: DevProject,
        backend: Option<UiBackend>,
        environment: DevEnvironment,
    ) -> Self {
        let DevProject {
            mut runtime, hot, ..
        } = project;
        runtime.set_svet_document_scheduling(backend == Some(UiBackend::Svet));
        Self {
            runtime,
            hot,
            backend,
            inspect_mode: false,
            inspect_report: String::new(),
            a11y_report: String::new(),
            binding_error: None,
            frame_stats: RefCell::default(),
            title_due: Cell::new(Instant::now()),
            last_title: String::new(),
            continuous_redraw: environment.fps_test,
            log_fps: environment.fps_log,
            cpu_only: environment.cpu_only,
            windows: AuxiliaryWindows::default(),
            #[cfg(test)]
            printed: Vec::new(),
        }
    }

    pub(super) fn extension(app: &mut Self) -> Option<&mut dyn HostExtension> {
        Some(app)
    }

    fn print(&mut self, line: String) {
        println!("{line}");
        #[cfg(test)]
        self.printed.push(line);
    }

    fn reset_frame_stats(&self) {
        *self.frame_stats.borrow_mut() = FrameStats::default();
        self.title_due.set(Instant::now());
    }

    /// A binding error is shown in the title and the window stays open.
    pub(super) fn set_binding_error(&mut self, error: Option<String>) {
        if self.binding_error != error {
            if let Some(error) = &error {
                eprintln!("Ошибка bindings: {error}");
            }
            self.binding_error = error;
            self.title_due.set(Instant::now());
        }
    }

    /// Replaces the scene with a rebuilt one, keeping focus, hover and scroll.
    fn replace_runtime(&mut self, mut next: crate::Runtime) {
        next.preserve_interaction(&self.runtime);
        next.set_svet_document_scheduling(self.backend == Some(UiBackend::Svet));
        self.runtime = next;
    }

    pub(super) fn toggle_inspect(&mut self) {
        self.inspect_mode = !self.inspect_mode;
        self.runtime.focus(false);
        self.inspect_report.clear();
        self.a11y_report.clear();
        self.print(format!(
            "F9: выбор элемента {}",
            if self.inspect_mode {
                "включён"
            } else {
                "выключен"
            }
        ));
    }

    pub(super) fn toggle_fps_test(&mut self) {
        self.continuous_redraw = !self.continuous_redraw;
        self.reset_frame_stats();
    }

    /// Prints the picker's description of the scene whenever it changed.
    fn print_inspection(&mut self) {
        let report = inspect_report(&self.runtime);
        if report != self.inspect_report {
            self.print(format!("FORMA_INSPECT {report}"));
            self.inspect_report = report;
        }
        let a11y = self.runtime.accessibility_tree_json();
        if a11y != self.a11y_report {
            self.print(format!("FORMA_A11Y {a11y}"));
            self.a11y_report = a11y;
        }
    }

    fn report_click(&mut self) {
        let key = self.runtime.key();
        self.print(format!(
            "{}.clicked → {} (событие; обработчик приложения пока не подключён)",
            key,
            self.runtime.action()
        ));
        if cfg!(target_os = "ios") {
            self.restyle_demo(&key);
        }
        self.title_due.set(Instant::now());
    }

    /// The built-in demo fills the screen it runs on.
    pub(super) fn fit_demo(&mut self, width: f32, height: f32) {
        if !self.hot.is_demo()
            || ((self.runtime.width() - width).abs() <= 1.
                && (self.runtime.height() - height).abs() <= 1.)
        {
            return;
        }
        let source = two_buttons_ui_for_viewport(width, height);
        if let Ok(next) = crate::Runtime::from_sources(&source, crate::BUTTON_COMPONENT) {
            self.replace_runtime(next);
        }
    }

    /// The built-in demo shows which button was pressed last and how often.
    pub(super) fn restyle_demo(&mut self, clicked_key: &str) {
        if !self.hot.is_demo() {
            return;
        }
        let clicks = (
            self.runtime.control_clicks(0),
            self.runtime.control_clicks(1),
        );
        let source = two_buttons_ui_for_viewport_with_state(
            self.runtime.width(),
            self.runtime.height(),
            Some(clicked_key),
            Some(clicks),
        );
        if let Ok(next) = crate::Runtime::from_sources(&source, crate::BUTTON_COMPONENT) {
            self.replace_runtime(next);
        }
    }

    fn title_text(&self, status: &HostStatus, now: Instant) -> String {
        dev_title(&DevTitle {
            platform: status.platform,
            choosing: status.choosing,
            backend: status.backend,
            gpu: status.gpu,
            error: self.binding_error.as_deref(),
            surface: status.surface,
            rate: self.frame_stats.borrow().snapshot(now),
            continuous: self.continuous_redraw,
            clicks: self.runtime.clicks(),
        })
    }

    /// Computes the title and schedules the next look at it. `None` when it did not change.
    fn refresh_title(&mut self, status: &HostStatus, now: Instant) -> Option<String> {
        let title = self.title_text(status, now);
        self.title_due.set(now + TITLE_INTERVAL);
        if title == self.last_title {
            return None;
        }
        if self.log_fps && !status.choosing {
            self.print(format!("Frame stats: {title}"));
        }
        self.last_title.clone_from(&title);
        Some(title)
    }
}

impl NativeApp for DevApp {
    fn update(&mut self, input: &mut dyn FnMut(&mut crate::Runtime)) -> Result<(), String> {
        let clicks = self.runtime.clicks();
        let dismissals = self.runtime.dismiss_count();
        input(&mut self.runtime);
        if self.inspect_mode {
            self.print_inspection();
        }
        let binding_error = self.runtime.sync_ui_bindings_native().err();
        self.set_binding_error(binding_error);
        if self.runtime.clicks() != clicks {
            self.report_click();
        }
        if self.runtime.dismiss_count() != dismissals {
            let layer = self.runtime.dismissed_index().max(0) as usize;
            self.print(format!(
                "{}.dismissed → {} (модальный слой закрыт: Escape или щелчок по backdrop)",
                self.runtime.modal_key(layer),
                self.runtime.modal_action(layer)
            ));
        }
        Ok(())
    }

    fn inspect(&self, observer: &mut dyn FnMut(&crate::Runtime)) -> Result<(), String> {
        observer(&self.runtime);
        Ok(())
    }

    fn on_frame_presented(&self, _renderer: NativeRenderer) -> Result<(), String> {
        self.frame_stats.borrow_mut().presented(Instant::now());
        Ok(())
    }

    fn on_gpu_fallback(&self, _error: &str) -> Result<(), String> {
        // Do not mix the cadences of two renderers.
        self.reset_frame_stats();
        Ok(())
    }

    fn on_occlusion_changed(&self, _occluded: bool) -> Result<(), String> {
        self.title_due.set(Instant::now());
        Ok(())
    }

    fn on_resized(&mut self, width: f32, height: f32, _scale: f32) -> Result<(), String> {
        if cfg!(target_os = "ios") {
            self.fit_demo(width.round(), height.round());
        }
        Ok(())
    }
}

impl HostExtension for DevApp {
    fn window_created(&mut self, window: &Window) {
        window.focus_window();
        self.print(
            "F8: непрерывный FPS-тест; обычный режим перерисовывает окно после ввода и во время анимаций"
                .into(),
        );
    }

    fn allows_gpu(&self) -> bool {
        !self.cpu_only
    }

    fn key(
        &mut self,
        key: &Key,
        pressed: bool,
        repeat: bool,
        event_loop: &ActiveEventLoop,
    ) -> bool {
        match dev_shortcut(key, pressed, repeat) {
            Some(DevShortcut::Popup) => self.windows.toggle_popup(event_loop, !self.cpu_only),
            Some(DevShortcut::Inspect) => self.toggle_inspect(),
            Some(DevShortcut::FpsTest) => self.toggle_fps_test(),
            None => return false,
        }
        true
    }

    fn picks_elements(&self) -> bool {
        self.inspect_mode
    }

    fn redraws_continuously(&self) -> bool {
        self.continuous_redraw
    }

    fn backend_selected(&mut self, backend: UiBackend) {
        self.backend = Some(backend);
        self.runtime
            .set_svet_document_scheduling(backend == UiBackend::Svet);
        self.reset_frame_stats();
    }

    fn platform_selected(&mut self, _platform: TargetPlatform) {
        self.reset_frame_stats();
    }

    fn title(&mut self, status: &HostStatus, now: Instant) -> String {
        self.refresh_title(status, now);
        self.last_title.clone()
    }

    fn idle(&mut self, status: &HostStatus, now: Instant) -> IdleRequest {
        let mut request = IdleRequest::default();
        if let Some((next, message)) = self.hot.poll(now, &self.runtime) {
            self.replace_runtime(next);
            self.binding_error = None;
            self.title_due.set(now);
            self.inspect_report.clear();
            self.a11y_report.clear();
            self.print(message.into());
            request.redraw = true;
        }
        let idle_deadline = self.frame_stats.borrow().idle_deadline();
        if now >= self.title_due.get() || idle_deadline.is_some_and(|deadline| deadline <= now) {
            request.title = self.refresh_title(status, now);
        }
        // Updating the title never causes a redraw. Stop all meter wakeups at idle.
        let meter_deadline = idle_deadline
            .filter(|deadline| *deadline > now)
            .map(|deadline| deadline.min(self.title_due.get()));
        request.deadline = meter_deadline.into_iter().chain(self.hot.deadline()).min();
        request
    }

    fn window_event(&mut self, window: WindowId, event: WindowEvent) {
        self.windows.window_event(window, event);
    }
}

/// Runs the dev window of the `forma` binary: the scene of `project` on the shared native
/// host, with hot reload, the diagnostic title and the F8, F9 and F10 tools. `backend` of
/// `None` starts with the backend chooser.
pub fn run_dev(
    project: DevProject,
    backend: Option<UiBackend>,
    platform: TargetPlatform,
) -> Result<(), String> {
    let config = NativeWindowConfig {
        title: "Forma".into(),
        width: project.runtime.render_width(),
        height: project.runtime.render_height(),
        resizable: true,
        backend,
        platform,
        mode: HostMode::Demo,
    };
    let app = DevApp::new(project, backend, DevEnvironment::read());
    run_host(app, config, DevApp::extension)
}
