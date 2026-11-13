use super::*;
use crate::{frame_stats::FrameRate, native_gpu::Presentation};
use std::time::Duration;

const PROJECT_UI: &str = "component App { state a: String = 'first'; Column { width:240; height:140; TextInput { key:'a'; width:200; height:32; value <-> state.a; } } }";

/// A fresh directory under the system temp dir, removed when dropped.
struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("forma-dev-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, relative: &str, content: &str) -> String {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
        path.to_string_lossy().into_owned()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn title(change: impl FnOnce(&mut DevTitle<'_>)) -> String {
    let mut state = DevTitle {
        platform: TargetPlatform::Desktop,
        choosing: false,
        backend: Some(UiBackend::Svet),
        gpu: true,
        error: None,
        surface: Presentation::Presented,
        rate: FrameRate::Waiting,
        continuous: false,
        clicks: 0,
    };
    change(&mut state);
    dev_title(&state)
}

#[test]
fn title_reports_backend_renderer_cadence_and_clicks() {
    assert_eq!(
        title(|_| {}),
        "Forma [svet] · GPU · ожидание кадра · F6: iOS/Android · F7: бэкенд"
    );
    assert_eq!(
        title(|t| {
            t.rate = FrameRate::Active {
                fps: 59.94,
                frame_ms: 16.683,
            };
            t.continuous = true;
            t.clicks = 3;
        }),
        "Forma [svet] · GPU · 59.9 FPS · 16.68 ms · FPS-тест · кликов: 3"
    );
    assert!(title(|t| t.rate = FrameRate::Sampling).contains(" · замер… · "));
    assert!(title(|t| t.rate = FrameRate::Idle).contains(" · idle · "));
    // The egui profile is painted on the CPU even when a GPU surface exists.
    assert!(title(|t| t.backend = Some(UiBackend::Egui)).starts_with("Forma [egui] · CPU · "));
    assert!(title(|t| t.gpu = false).starts_with("Forma [svet] · CPU · "));
    assert!(title(|t| t.backend = None).starts_with("Forma [svet] · GPU · "));
}

#[test]
fn title_prefers_an_error_then_the_surface_state_over_the_cadence() {
    let active = FrameRate::Active {
        fps: 60.,
        frame_ms: 16.,
    };
    assert!(
        title(|t| {
            t.error = Some("boom");
            t.surface = Presentation::Occluded;
            t.rate = active;
        })
        .contains(" · ошибка: boom · ")
    );
    for (surface, text) in [
        (Presentation::Occluded, " · occluded · "),
        (Presentation::ZeroSize, " · нет поверхности · "),
        (Presentation::Retry, " · ожидание поверхности · "),
    ] {
        assert!(
            title(|t| {
                t.surface = surface;
                t.rate = active;
            })
            .contains(text),
            "{surface:?}"
        );
    }
}

#[test]
fn title_names_the_emulated_platform_and_the_chooser() {
    assert_eq!(
        title(|t| t.platform = TargetPlatform::Ios),
        "Forma [svet · iOS] · Metal (iOS) · ожидание кадра · F6: iOS/Android · F7: бэкенд"
    );
    assert!(
        title(|t| t.platform = TargetPlatform::Android)
            .starts_with("Forma [svet · Android] · Vulkan (Android) · ")
    );
    assert_eq!(
        title(|t| {
            t.choosing = true;
            t.backend = None;
            t.clicks = 2;
        }),
        "Forma · Выберите бэкенд: [1] egui / [2] svet · F6: платформа"
    );
    assert_eq!(
        title(|t| {
            t.choosing = true;
            t.platform = TargetPlatform::Android;
        }),
        "Forma · Android · Выберите бэкенд: [1] egui / [2] svet · F6: платформа"
    );
}

#[test]
fn inspect_report_lists_every_node_and_control_and_the_selection() {
    let mut runtime =
        crate::Runtime::from_sources(crate::TWO_BUTTONS_EXAMPLE, crate::BUTTON_COMPONENT).unwrap();
    let report: serde_json::Value = serde_json::from_str(&inspect_report(&runtime)).unwrap();
    assert_eq!(report["selected"], -1);
    assert_eq!(
        report["nodes"].as_array().unwrap().len(),
        runtime.nodes().len()
    );
    let controls = report["controls"].as_array().unwrap();
    assert_eq!(controls.len(), runtime.control_count());
    assert_eq!(controls[1]["index"], 1);
    assert_eq!(controls[1]["disabled"], false);
    let bounds: Vec<f64> = runtime
        .control_bounds(1)
        .iter()
        .map(|v| f64::from(*v))
        .collect();
    assert_eq!(controls[1]["bounds"], serde_json::json!(bounds));
    let node = &report["nodes"][0];
    assert!(node["id"].is_number() && node["kind"].is_string());
    assert!(node["parent"].is_null());

    runtime.focus_control(1);
    let report: serde_json::Value = serde_json::from_str(&inspect_report(&runtime)).unwrap();
    assert_eq!(report["selected"], 1);
}

#[test]
fn function_keys_of_the_dev_window_fire_on_the_first_press_only() {
    let key = |named| Key::Named(named);
    assert_eq!(
        dev_shortcut(&key(NamedKey::F8), true, false),
        Some(DevShortcut::FpsTest)
    );
    assert_eq!(
        dev_shortcut(&key(NamedKey::F9), true, false),
        Some(DevShortcut::Inspect)
    );
    assert_eq!(
        dev_shortcut(&key(NamedKey::F10), true, false),
        Some(DevShortcut::Popup)
    );
    assert_eq!(dev_shortcut(&key(NamedKey::F8), false, false), None);
    assert_eq!(dev_shortcut(&key(NamedKey::F9), true, true), None);
    // F6 and F7 belong to the window host, everything else to the scene.
    for named in [NamedKey::F6, NamedKey::F7, NamedKey::F11, NamedKey::Enter] {
        assert_eq!(dev_shortcut(&key(named), true, false), None);
    }
}

#[test]
fn project_files_skip_build_folders_and_the_entry_is_the_first_plain_ui_file() {
    let dir = TempDir::new("entry");
    dir.write("Main.ui", PROJECT_UI);
    dir.write("App.design.ui", "design");
    dir.write("components/Card.component.ui", "card");
    dir.write("assets/icon.svg", "<svg/>");
    dir.write("notes.txt", "ignored");
    for skipped in ["target", "node_modules", "dist", ".git"] {
        dir.write(&format!("{skipped}/Skipped.ui"), "skipped");
    }
    let files = collect_ui_project_files(&dir.0).unwrap();
    assert_eq!(
        files.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            "App.design.ui",
            "Main.ui",
            "assets/icon.svg",
            "components/Card.component.ui"
        ]
    );
    let (root, entry) = resolve_ui_project_root_and_entry(&dir.0).unwrap();
    assert_eq!(root, dir.0.canonicalize().unwrap());
    assert_eq!(entry, "Main.ui");

    // A file inside a conventional source folder belongs to the project one level up.
    let nested = dir.write("ui/Page.ui", PROJECT_UI);
    let (root, entry) = resolve_ui_project_root_and_entry(std::path::Path::new(&nested)).unwrap();
    assert_eq!(root, dir.0.canonicalize().unwrap());
    assert_eq!(entry, "ui/Page.ui");
    let (root, entry) = resolve_ui_project_root_and_entry(&dir.0.join("Main.ui")).unwrap();
    assert_eq!(root, dir.0.canonicalize().unwrap());
    assert_eq!(entry, "Main.ui");

    let only_components = TempDir::new("entry-components");
    only_components.write("components/Card.component.ui", "card");
    assert_eq!(
        resolve_ui_project_root_and_entry(&only_components.0)
            .unwrap()
            .1,
        "components/Card.component.ui"
    );
    assert!(resolve_ui_project_root_and_entry(&TempDir::new("entry-empty").0).is_none());
}

#[test]
fn without_arguments_the_two_button_demo_loads_and_nothing_is_watched() {
    let mut project = DevProject::load(None, None).unwrap();
    assert_eq!(project.source, crate::TWO_BUTTONS_EXAMPLE);
    assert_eq!(project.template, crate::BUTTON_COMPONENT);
    assert_eq!(project.runtime.control_count(), 2);
    assert!(project.hot.is_demo());
    assert_eq!(project.hot.deadline(), None);
    let later = Instant::now() + Duration::from_secs(5);
    assert!(project.hot.poll(later, &project.runtime).is_none());
}

#[test]
fn a_scene_file_reloads_when_it_changes_and_survives_a_broken_edit() {
    let dir = TempDir::new("scene");
    let scene = "component Demo { Frame { width:200; height:100; Button { key:'one'; } } }";
    let path = dir.write("Demo.ui", scene);
    let mut project = DevProject::load(Some(path.clone()), None).unwrap();
    assert_eq!(project.source, scene);
    assert_eq!(project.template, crate::BUTTON_COMPONENT);
    assert_eq!(project.runtime.control_count(), 1);
    assert!(!project.hot.is_demo());
    let due = project.hot.deadline().unwrap();
    assert!(due > Instant::now());

    // Not due yet, then due but unchanged.
    assert!(
        project
            .hot
            .poll(due - Duration::from_millis(50), &project.runtime)
            .is_none()
    );
    assert!(project.hot.poll(due, &project.runtime).is_none());
    assert_eq!(
        project.hot.deadline(),
        Some(due + Duration::from_millis(120))
    );

    let two = "component Demo { Frame { width:200; height:100; Button { key:'one'; } Button { key:'two'; } } }";
    dir.write("Demo.ui", two);
    let now = due + Duration::from_secs(1);
    let (next, message) = project.hot.poll(now, &project.runtime).unwrap();
    assert_eq!(next.control_count(), 2);
    assert_eq!(message, "Hot reload: векторная сцена обновлена на лету");
    assert!(
        project
            .hot
            .poll(now + Duration::from_secs(1), &next)
            .is_none()
    );

    // A scene that does not compile, and an emptied file, keep the running one.
    dir.write("Demo.ui", "component Demo { Frame { width: ");
    assert!(
        project
            .hot
            .poll(now + Duration::from_secs(2), &next)
            .is_none()
    );
    dir.write("Demo.ui", "  ");
    assert!(
        project
            .hot
            .poll(now + Duration::from_secs(3), &next)
            .is_none()
    );
    dir.write("Demo.ui", scene);
    let (again, _) = project
        .hot
        .poll(now + Duration::from_secs(4), &next)
        .unwrap();
    assert_eq!(again.control_count(), 1);
}

#[test]
fn a_scene_with_its_template_reloads_when_either_file_changes() {
    let dir = TempDir::new("template");
    let scene = dir.write("Scene.ui", crate::TWO_BUTTONS_EXAMPLE);
    let template = dir.write("Button.component.ui", crate::BUTTON_COMPONENT);
    let mut project = DevProject::load(Some(scene), Some(template)).unwrap();
    assert_eq!(project.runtime.control_count(), 2);
    let now = Instant::now() + Duration::from_secs(1);
    assert!(project.hot.poll(now, &project.runtime).is_none());
    dir.write(
        "Button.component.ui",
        &format!("{}\n", crate::BUTTON_COMPONENT),
    );
    let (next, _) = project
        .hot
        .poll(now + Duration::from_secs(1), &project.runtime)
        .unwrap();
    assert_eq!(next.control_count(), 2);
}

#[test]
fn a_project_folder_reloads_with_its_state_when_a_file_changes() {
    let dir = TempDir::new("project");
    dir.write("App.ui", PROJECT_UI);
    let folder = dir.0.to_string_lossy().into_owned();
    let mut project = DevProject::load(Some(folder), None).unwrap();
    assert!(project.source.is_empty() && project.template.is_empty());
    assert!(!project.hot.is_demo());
    project.runtime.sync_ui_bindings_native().unwrap();
    assert_eq!(project.runtime.text_value(0), "first");
    project.runtime.focus_control(0);
    project.runtime.text_key("End", false, false);
    project.runtime.text_insert("!");
    project.runtime.sync_ui_bindings_native().unwrap();

    let now = Instant::now() + Duration::from_secs(1);
    assert!(project.hot.poll(now, &project.runtime).is_none());
    dir.write("App.ui", &PROJECT_UI.replace("width:200", "width:180"));
    let (mut next, message) = project
        .hot
        .poll(now + Duration::from_secs(1), &project.runtime)
        .unwrap();
    assert_eq!(
        message,
        "Hot reload: многофайловый .ui проект перекомпилирован в Rust"
    );
    next.sync_ui_bindings_native().unwrap();
    assert_eq!(next.text_value(0), "first!", "state survives the reload");
    assert_eq!(next.control_bounds(0)[2], 180.);

    // A broken project keeps the running one and is retried on the next check.
    dir.write("App.ui", "component App {");
    assert!(
        project
            .hot
            .poll(now + Duration::from_secs(2), &next)
            .is_none()
    );
    dir.write("App.ui", PROJECT_UI);
    assert!(
        project
            .hot
            .poll(now + Duration::from_secs(3), &next)
            .is_some()
    );
}

#[test]
fn a_file_that_is_not_a_single_scene_loads_as_a_project() {
    let dir = TempDir::new("fallback");
    let path = dir.write("App.ui", PROJECT_UI);
    let project = DevProject::load(Some(path), None).unwrap();
    assert_eq!(project.runtime.control_count(), 1);
    assert_eq!(project.hot.project_entry.as_deref(), Some("App.ui"));
    assert!(project.hot.source_path.is_none());
    assert!(
        DevProject::load(
            Some(dir.0.join("Missing.ui").to_string_lossy().into_owned()),
            None
        )
        .is_err()
    );
}

/// The dev window as `run_dev` builds it, before the loop creates the window.
fn dev_host(project: DevProject, environment: DevEnvironment) -> NativeHost<DevApp> {
    let backend = Some(UiBackend::Svet);
    let config = NativeWindowConfig {
        title: "Forma".into(),
        width: project.runtime.render_width(),
        height: project.runtime.render_height(),
        resizable: true,
        backend,
        platform: TargetPlatform::Desktop,
        mode: HostMode::Demo,
    };
    NativeHost {
        extension: DevApp::extension,
        ..super::tests::windowless_host(DevApp::new(project, backend, environment), config)
    }
}

fn demo_host() -> NativeHost<DevApp> {
    dev_host(DevProject::load(None, None).unwrap(), Default::default())
}

fn control_center(runtime: &crate::Runtime, index: usize) -> (f32, f32) {
    let b = runtime.control_bounds(index);
    (b[0] + b[2] * 0.5, b[1] + b[3] * 0.5)
}

fn click(host: &mut NativeHost<DevApp>, index: usize) {
    host.pointer = control_center(&host.app.runtime, index);
    host.primary_button(true, Instant::now()).unwrap();
    host.primary_button(false, Instant::now()).unwrap();
}

#[test]
fn a_slow_touch_drag_scrolls_the_dev_window_by_its_whole_distance() {
    // F-4: the dev window used to remember the touch position on every move, so a drag in
    // steps below half a pixel never scrolled. It now runs on the host that accumulates them.
    let mut project = DevProject::load(None, None).unwrap();
    project.runtime = crate::Runtime::from_source("component Demo { Frame { width:200; height:100; Scroll { Button { key:'large'; width:180; height:400; } } } }").unwrap();
    let mut host = dev_host(project, Default::default());
    let b = host.app.runtime.control_bounds(0);
    let (x, y) = (b[0] + 20., b[1] + 60.);
    let device = winit::event::DeviceId::dummy();
    host.touch_input(device, 1, TouchPhase::Started, x, y)
        .unwrap();
    for step in 1..=80 {
        host.touch_input(device, 1, TouchPhase::Moved, x, y - step as f32 * 0.25)
            .unwrap();
    }
    let scrolled = host.app.runtime.scroll_offset()[1];
    assert!(
        (19.5..=20.).contains(&scrolled),
        "80 quarter-pixel moves scroll 20 px less a pending remainder, got {scrolled}"
    );
    host.touch_input(device, 1, TouchPhase::Ended, x, y - 20.)
        .unwrap();
    assert_eq!(host.app.runtime.control_clicks(0), 0, "a drag is not a tap");
}

#[test]
fn a_click_is_printed_and_counted_in_the_title() {
    let mut host = demo_host();
    host.sync_window_title();
    assert_eq!(
        host.app.title(&host.status(), Instant::now()),
        "Forma [svet] · CPU · ожидание кадра · F6: iOS/Android · F7: бэкенд"
    );
    click(&mut host, 1);
    let runtime = &host.app.runtime;
    assert_eq!(runtime.control_clicks(1), 1);
    assert_eq!(
        host.app.printed,
        [format!(
            "{}.clicked → {} (событие; обработчик приложения пока не подключён)",
            runtime.key(),
            runtime.action()
        )]
    );
    assert!(runtime.key() == "second" && !runtime.action().is_empty());
    let status = host.status();
    assert!(
        host.app
            .title(&status, Instant::now())
            .ends_with(" · кликов: 1")
    );
}

#[test]
fn the_element_picker_selects_instead_of_clicking_and_prints_what_changed() {
    let mut host = demo_host();
    host.app.toggle_inspect();
    assert!(host.picks_elements());
    assert_eq!(host.app.printed, ["F9: выбор элемента включён"]);

    click(&mut host, 1);
    assert_eq!(host.app.runtime.clicks(), 0);
    assert_eq!(host.app.runtime.focused_index(), 1);
    let printed = host.app.printed.clone();
    assert_eq!(printed.len(), 3, "{printed:?}");
    assert_eq!(
        printed[1],
        format!("FORMA_INSPECT {}", inspect_report(&host.app.runtime))
    );
    assert!(printed[1].contains("\"selected\":1"));
    assert_eq!(
        printed[2],
        format!("FORMA_A11Y {}", host.app.runtime.accessibility_tree_json())
    );
    // The same selection again prints nothing.
    click(&mut host, 1);
    assert_eq!(host.app.printed.len(), 3);
    click(&mut host, 0);
    assert!(host.app.printed[3].contains("\"selected\":0"));

    host.app.toggle_inspect();
    assert!(!host.picks_elements());
    assert_eq!(host.app.runtime.focused_index(), -1);
    assert_eq!(
        host.app.printed.last().unwrap(),
        "F9: выбор элемента выключен"
    );
    click(&mut host, 0);
    assert_eq!(host.app.runtime.control_clicks(0), 1);
}

#[test]
fn the_frame_rate_test_keeps_the_window_redrawing_and_shows_in_the_title() {
    let mut host = demo_host();
    assert!(!host.app.redraws_continuously());
    host.app.on_frame_presented(NativeRenderer::Cpu).unwrap();
    let status = host.status();
    assert!(
        host.app
            .title(&status, Instant::now())
            .contains(" · замер… · F6: iOS/Android · F7: бэкенд")
    );
    host.app.toggle_fps_test();
    assert!(host.app.redraws_continuously());
    assert!(
        host.app
            .title(&status, Instant::now())
            .ends_with(" · ожидание кадра · FPS-тест"),
        "switching the test restarts the measurement"
    );
    host.app.toggle_fps_test();
    assert!(!host.app.redraws_continuously());

    let environment = DevEnvironment {
        fps_test: true,
        fps_log: true,
        cpu_only: true,
    };
    let mut host = dev_host(DevProject::load(None, None).unwrap(), environment);
    assert!(host.app.redraws_continuously());
    assert!(!host.app.allows_gpu());
    assert!(demo_host().app.allows_gpu());
    // FORMA_FPS_LOG prints the title when it changes, not on every look at it.
    let status = host.status();
    let now = Instant::now();
    host.app.title(&status, now);
    host.app.title(&status, now);
    assert_eq!(host.app.printed.len(), 1);
    assert!(host.app.printed[0].starts_with("Frame stats: Forma [svet] · CPU · "));
}

#[test]
fn a_backend_change_reaches_the_title_and_a_binding_error_replaces_the_cadence() {
    let mut host = demo_host();
    host.select_backend(UiBackend::Egui);
    let status = host.status();
    let now = Instant::now();
    assert_eq!(
        host.app.title(&status, now),
        "Forma [egui] · CPU · ожидание кадра · F6: iOS/Android · F7: бэкенд"
    );
    host.select_platform(TargetPlatform::Ios);
    let status = host.status();
    assert!(
        host.app
            .title(&status, now)
            .starts_with("Forma [egui · iOS] · Metal (iOS) · ")
    );
    host.app
        .set_binding_error(Some("state.a is not a number".into()));
    assert!(
        host.app
            .title(&status, now)
            .contains(" · ошибка: state.a is not a number · ")
    );
    // The next successful update clears it.
    host.app.update(&mut |_| {}).unwrap();
    assert!(!host.app.title(&status, now).contains("ошибка"));
    host.occluded = true;
    let status = host.status();
    assert!(host.app.title(&status, now).contains(" · occluded · "));
}

#[test]
fn the_idle_dev_window_reloads_changed_files_and_refreshes_its_title_on_schedule() {
    let dir = TempDir::new("idle");
    let scene = "component Demo { Frame { width:200; height:100; Button { key:'one'; } } }";
    let path = dir.write("Demo.ui", scene);
    let mut host = dev_host(
        DevProject::load(Some(path), None).unwrap(),
        Default::default(),
    );
    assert!(host.app.runtime.focus_control(0));
    let status = host.status();
    let now = Instant::now() + Duration::from_secs(1);

    let first = host.app.idle(&status, now);
    assert!(!first.redraw);
    assert_eq!(
        first.title.as_deref(),
        Some("Forma [svet] · CPU · ожидание кадра · F6: iOS/Android · F7: бэкенд")
    );
    // Nothing was presented, so only the file check wakes the loop.
    assert_eq!(first.deadline, Some(now + Duration::from_millis(120)));
    let again = host.app.idle(&status, now + Duration::from_millis(10));
    assert_eq!(again.title, None, "the title is looked at every 250 ms");
    assert!(!again.redraw);

    dir.write(
        "Demo.ui",
        "component Demo { Frame { width:200; height:100; Button { key:'one'; } Button { key:'two'; } } }",
    );
    let (redraw, deadline) = host.extension_idle(now + Duration::from_secs(1));
    assert!(redraw);
    assert!(deadline.is_some());
    assert_eq!(host.app.runtime.control_count(), 2);
    assert_eq!(host.app.runtime.focused_index(), 0, "focus survives");
    assert_eq!(
        host.app.printed,
        ["Hot reload: векторная сцена обновлена на лету"]
    );

    // After a frame the title is refreshed until the meter reports idle, then wakeups stop.
    let mut host = demo_host();
    let now = Instant::now();
    host.app.on_frame_presented(NativeRenderer::Gpu).unwrap();
    let request = host.app.idle(&status, now);
    assert!(request.title.is_some());
    let due = request.deadline.unwrap();
    assert!(due > now && due <= now + Duration::from_millis(250));
    let idle = host.app.idle(&status, now + Duration::from_secs(2));
    assert!(idle.title.unwrap().contains(" · idle · "));
    assert_eq!(idle.deadline, None);
}

#[test]
fn the_built_in_demo_follows_the_screen_size_and_marks_the_pressed_button() {
    let mut host = demo_host();
    assert!(host.app.runtime.focus_control(1));
    host.app.fit_demo(393., 852.);
    assert_eq!(
        (host.app.runtime.width(), host.app.runtime.height()),
        (393., 852.)
    );
    assert_eq!(host.app.runtime.focused_index(), 1);
    let revision = host.app.runtime.visual_revision();
    host.app.fit_demo(393.4, 852.);
    assert_eq!(host.app.runtime.visual_revision(), revision);

    click(&mut host, 0);
    host.app.restyle_demo("first");
    assert_eq!(
        (host.app.runtime.width(), host.app.runtime.height()),
        (393., 852.)
    );
    assert!(
        host.app
            .runtime
            .accessibility_tree_json()
            .contains("Первая кнопка · нажата (1)")
    );

    // A scene from disk is never replaced by the demo.
    let dir = TempDir::new("fit");
    let path = dir.write(
        "Demo.ui",
        "component Demo { Frame { width:200; height:100; Button { key:'one'; } } }",
    );
    let mut host = dev_host(
        DevProject::load(Some(path), None).unwrap(),
        Default::default(),
    );
    host.app.fit_demo(393., 852.);
    host.app.restyle_demo("one");
    assert_eq!(host.app.runtime.width(), 200.);
    assert_eq!(host.app.runtime.control_count(), 1);
}
