use super::*;

#[test]
fn touch_scroll_does_not_activate_the_pressed_control_on_release() {
    for (delta, steps) in [(-20_f32, 1), (20., 1), (0.25, 1), (-20., 80)] {
        let runtime=crate::Runtime::from_source("component Demo { Frame { width:200; height:100; Scroll { Button { key:'large'; width:180; height:400; } } } }").unwrap();
        let b = runtime.control_bounds(0);
        let (x, y) = (b[0] + 20., b[1] + 60.);
        let mut host = NativeHost {
            app: runtime,
            config: NativeWindowConfig::default(),
            backend: Some(UiBackend::Svet),
            platform: TargetPlatform::Desktop,
            chooser: None,
            accessibility: None,
            waker: None,
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
        };
        let device = winit::event::DeviceId::dummy();
        host.touch_input(device, 1, TouchPhase::Started, x, y)
            .unwrap();
        for step in 1..=steps {
            host.touch_input(
                device,
                1,
                TouchPhase::Moved,
                x,
                y + delta * step as f32 / steps as f32,
            )
            .unwrap();
        }
        assert_eq!(host.app.scroll_offset()[1] > 0., delta < 0.);
        if delta < 0. {
            assert!(
                (host.app.scroll_offset()[1] + delta).abs() <= 0.5,
                "only a subthreshold residual may remain"
            );
        }
        host.touch_input(device, 1, TouchPhase::Ended, x, y + delta)
            .unwrap();
        let expected = u32::from(delta.abs() <= 0.5);
        assert_eq!(
            host.app.control_clicks(0),
            expected,
            "pan at an edge must cancel too; subthreshold motion remains a tap"
        );
        host.touch_input(device, 2, TouchPhase::Started, x, y + delta)
            .unwrap();
        host.touch_input(device, 2, TouchPhase::Ended, x, y + delta)
            .unwrap();
        assert_eq!(
            host.app.control_clicks(0),
            expected + 1,
            "new stationary tap still activates"
        );
    }
}

#[test]
fn secondary_touches_cannot_steal_or_end_the_primary_gesture() {
    let files = std::collections::BTreeMap::from([(
        "App.ui".into(),
        include_str!("../../tests/fixtures/svet-native-ime/App.ui").into(),
    )]);
    let runtime = crate::Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
    let point = |index| {
        let b = runtime.control_bounds(index);
        (b[0] + b[2] * 0.5, b[1] + b[3] * 0.5)
    };
    let (ax, ay) = point(1);
    let (bx, by) = point(2);
    let mut host = NativeHost {
        app: runtime,
        config: NativeWindowConfig::default(),
        backend: Some(UiBackend::Svet),
        platform: TargetPlatform::Desktop,
        chooser: None,
        accessibility: None,
        waker: None,
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
    };
    let device = winit::event::DeviceId::dummy();
    host.touch_input(device, 1, TouchPhase::Started, ax, ay)
        .unwrap();
    assert_eq!(host.app.focused_index(), 1);
    for phase in [
        TouchPhase::Started,
        TouchPhase::Moved,
        TouchPhase::Ended,
        TouchPhase::Cancelled,
    ] {
        host.touch_input(device, 2, phase, bx, by).unwrap();
        assert_eq!(host.touch_owner, Some((device, 1)));
        assert_eq!(host.touch_last, Some((ax, ay)));
        assert_eq!(host.pointer, (ax, ay));
        assert_eq!(host.app.focused_index(), 1);
    }
    host.touch_input(device, 1, TouchPhase::Ended, ax, ay)
        .unwrap();
    assert_eq!(host.app.control_clicks(1), 1);
    assert_eq!(host.app.control_clicks(2), 0);
    assert!(host.touch_owner.is_none());
    assert!(host.touch_last.is_none());
    host.touch_input(device, 2, TouchPhase::Started, bx, by)
        .unwrap();
    assert_eq!(host.app.focused_index(), 2);
    host.focus_changed(false).unwrap();
    assert!(host.touch_owner.is_none());
    assert!(host.touch_last.is_none());
    for phase in [TouchPhase::Moved, TouchPhase::Ended] {
        host.touch_input(device, 2, phase, bx, by).unwrap();
    }
    assert_eq!(host.app.control_clicks(2), 0);
    assert_eq!(host.app.focused_index(), -1);
    host.focus_changed(true).unwrap();
    host.touch_input(device, 3, TouchPhase::Started, bx, by)
        .unwrap();
    host.touch_input(device, 3, TouchPhase::Cancelled, bx, by)
        .unwrap();
    assert!(host.touch_owner.is_none());
    assert!(host.touch_last.is_none());
    host.touch_input(device, 3, TouchPhase::Ended, bx, by)
        .unwrap();
    assert_eq!(host.app.control_clicks(2), 0);
}

#[test]
fn activation_focus_does_not_overwrite_new_pointer_selection() {
    let files = std::collections::BTreeMap::from([(
        "App.ui".into(),
        include_str!("../../tests/fixtures/svet-native-ime/App.ui").into(),
    )]);
    let mut runtime = crate::Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
    assert!(runtime.focus_control(1));
    let mut host = NativeHost {
        app: runtime,
        config: NativeWindowConfig::default(),
        backend: Some(UiBackend::Svet),
        platform: TargetPlatform::Desktop,
        chooser: None,
        accessibility: None,
        waker: None,
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
    };
    host.focus_changed(false).unwrap();
    let bounds = host.app.control_bounds(2);
    let (x, y) = (bounds[0] + bounds[2] * 0.5, bounds[1] + bounds[3] * 0.5);
    host.focus_sensitive_input(&mut |runtime| {
        runtime.pointer(x, y, 0);
        runtime.pointer(x, y, 1);
        runtime.pointer(x, y, 2);
    })
    .unwrap();
    assert_eq!(host.app.focused_index(), 2);
    host.focus_changed(true).unwrap();
    assert_eq!(
        host.app.focused_index(),
        2,
        "activation must respect a newer pointer selection"
    );
    host.focus_changed(true).unwrap();
    assert_eq!(host.app.focused_index(), 2);
    assert!(host.blurred_editor.is_none());
    host.app.text_key("End", false, false);
    host.app.text_insert("-clicked");
    host.app.sync_ui_bindings_native().unwrap();
    let state: serde_json::Value = serde_json::from_str(&host.app.ui_state_json()).unwrap();
    assert_eq!(state["first"], "seed-A");
    assert_eq!(state["second"], "seed-B-clicked");
}

#[test]
fn window_refocus_restores_stable_editor_key_after_reorder() {
    let source = |reverse: bool| {
        let a = "TextInput { key:'a'; width:180; height:32; value <-> state.a; }";
        let b = "TextInput { key:'b'; width:180; height:32; value <-> state.b; }";
        std::collections::BTreeMap::from([(
            "App.ui".into(),
            format!(
                "component App {{ state a: String = 'first'; state b: String = 'second'; Column {{ width:220; height:160; {} {} }} }}",
                if reverse { b } else { a },
                if reverse { a } else { b }
            ),
        )])
    };
    let mut runtime =
        crate::Runtime::from_ui_files(&source(false), "App.ui", &Default::default()).unwrap();
    assert!(runtime.focus_control(1));
    let mut host = NativeHost {
        app: runtime,
        config: NativeWindowConfig::default(),
        backend: Some(UiBackend::Svet),
        platform: TargetPlatform::Desktop,
        chooser: None,
        accessibility: None,
        waker: None,
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
    };
    host.focus_changed(false).unwrap();
    host.focus_changed(false).unwrap();
    assert!(!host.app.text_editing());
    host.focus_changed(true).unwrap();
    assert_eq!(host.app.focused_index(), 1);
    host.focus_changed(false).unwrap();
    host.app
        .load_ui_files_json_native(
            &serde_json::to_string(&source(true)).unwrap(),
            "App.ui",
            "{}",
        )
        .unwrap();
    host.focus_changed(true).unwrap();
    assert_eq!(host.app.focused_index(), 0);
    assert_eq!(host.app.text_value(0), "second");
}

#[test]
fn accessibility_mutations_cancel_same_editor_ime_but_refusals_preserve_it() {
    for action in [
        accesskit::Action::SetValue,
        accesskit::Action::SetTextSelection,
    ] {
        for empty in [false, true] {
            for valid in [false, true] {
                let files=std::collections::BTreeMap::from([("App.ui".into(),"component App { state query: String = 'seed'; Column { TextInput { key:'editor'; value <-> state.query; } } }".into())]);
                let mut runtime =
                    crate::Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
                runtime.focus_control(0);
                let mut host = NativeHost {
                    app: runtime,
                    config: NativeWindowConfig::default(),
                    backend: Some(UiBackend::Svet),
                    platform: TargetPlatform::Desktop,
                    chooser: None,
                    accessibility: None,
                    waker: None,
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
                };
                host.preedit_ime("未確定").unwrap();
                if empty {
                    host.preedit_ime("").unwrap();
                }
                let update = host.accessibility_ids.update(&host.app).unwrap();
                let (target, parent) = update
                    .nodes
                    .iter()
                    .find(|(_, node)| node.role() == accesskit::Role::TextInput)
                    .unwrap();
                let data = if !valid {
                    Some(accesskit::ActionData::NumericValue(0.))
                } else if action == accesskit::Action::SetValue {
                    Some(accesskit::ActionData::Value("AX".into()))
                } else {
                    let run = parent.children()[0];
                    Some(accesskit::ActionData::SetTextSelection(
                        accesskit::TextSelection {
                            anchor: accesskit::TextPosition {
                                node: run,
                                character_index: 1,
                            },
                            focus: accesskit::TextPosition {
                                node: run,
                                character_index: 3,
                            },
                        },
                    ))
                };
                let request = accesskit::ActionRequest {
                    action,
                    target_tree: accesskit::TreeId::ROOT,
                    target_node: *target,
                    data,
                };
                assert_eq!(host.accessibility_action(&request).unwrap(), valid);
                let after = host.app.text_value(0);
                host.commit_ime("late").unwrap();
                if valid {
                    assert_eq!(
                        host.app.text_value(0),
                        after,
                        "{action:?}, empty={empty}: stale commit"
                    );
                    host.commit_ime("late-again").unwrap();
                    assert_eq!(host.app.text_value(0), after);
                    host.preedit_ime("fresh").unwrap();
                    host.commit_ime("fresh").unwrap();
                    assert!(host.app.text_value(0).contains("fresh"));
                } else {
                    assert!(
                        host.app.text_value(0).contains("late"),
                        "refused AX action must retain IME"
                    );
                }
            }
        }
    }
}

#[test]
fn cancelled_composition_rejects_late_commits_until_a_new_preedit() {
    let files = std::collections::BTreeMap::from([(
        "App.ui".into(),
        include_str!("../../tests/fixtures/svet-native-ime/App.ui").into(),
    )]);
    for (cause, empty) in ["escape", "tab", "blur", "pointer"]
        .into_iter()
        .flat_map(|cause| [false, true].map(|empty| (cause, empty)))
    {
        let mut runtime =
            crate::Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
        runtime.focus_control(1);
        let mut host = NativeHost {
            app: runtime,
            config: NativeWindowConfig::default(),
            backend: Some(UiBackend::Svet),
            platform: TargetPlatform::Desktop,
            chooser: None,
            accessibility: None,
            waker: None,
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
        };
        host.preedit_ime("未確定").unwrap();
        if empty {
            host.preedit_ime("").unwrap();
            assert!(!host.composing);
        }
        match cause {
            "escape" => host
                .navigation_key(&Key::Named(NamedKey::Escape), true, false)
                .unwrap(),
            "tab" => host
                .navigation_key(&Key::Named(NamedKey::Tab), true, false)
                .unwrap(),
            "blur" => {
                host.focus_changed(false).unwrap();
                host.focus_changed(true).unwrap();
            }
            _ => {
                host.focus_sensitive_input(&mut |runtime| {
                    runtime.focus_control(2);
                })
                .unwrap();
            }
        }
        assert!(host.ime_cancelled, "{cause}");
        host.preedit_ime("").unwrap();
        host.ime_presentation().unwrap();
        host.commit_ime("late").unwrap();
        host.commit_ime("late-again").unwrap();
        assert_eq!(host.app.text_value(1), "seed-A", "{cause}");
        assert_eq!(host.app.text_value(2), "seed-B", "{cause}");
        host.app.focus_control(2);
        host.app.text_key("End", false, false);
        host.preedit_ime("新").unwrap();
        assert!(!host.ime_cancelled);
        host.preedit_ime("").unwrap();
        host.commit_ime("確定").unwrap();
        assert_eq!(
            host.app.text_value(2),
            "seed-B確定",
            "{cause}: new composition works"
        );
        host.ime_event(Ime::Disabled).unwrap();
        host.commit_ime("disabled-late").unwrap();
        assert_eq!(host.app.text_value(2), "seed-B確定");
        host.ime_event(Ime::Enabled).unwrap();
        host.ime_event(Ime::Preedit(String::new(), None)).unwrap();
        host.ime_event(Ime::Commit("direct".into())).unwrap();
        assert_eq!(host.app.text_value(2), "seed-B確定direct");
    }
}

#[test]
fn empty_preedit_preserves_commit_address_across_runtime_replacement() {
    let files = std::collections::BTreeMap::from([(
        "App.ui".into(),
        include_str!("../../tests/fixtures/svet-native-ime/App.ui").into(),
    )]);
    for replace in [false, true] {
        let mut runtime =
            crate::Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
        runtime.focus_control(1);
        runtime.text_key("End", false, false);
        let mut host = NativeHost {
            app: runtime,
            config: NativeWindowConfig::default(),
            backend: Some(UiBackend::Svet),
            platform: TargetPlatform::Desktop,
            chooser: None,
            accessibility: None,
            waker: None,
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
        };
        let clean = host.app.pixels(640, 360, 1.);
        host.preedit_ime("未確定").unwrap();
        let owner = host.composition_owner;
        assert!(owner.is_some());
        host.preedit_ime("").unwrap();
        assert!(!host.composing);
        assert_eq!(host.app.pixels(640, 360, 1.), clean);
        host.ime_presentation().unwrap();
        assert_eq!(host.composition_owner, owner);
        if replace {
            host.app =
                crate::Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
            host.app.focus_control(1);
            host.ime_presentation().unwrap();
        }
        host.commit_ime("確定").unwrap();
        assert_eq!(
            host.app.text_value(1),
            if replace { "seed-A" } else { "seed-A確定" }
        );
        assert!(host.composition_owner.is_none());
    }
}

#[test]
fn ime_commit_checks_editor_owner_at_dispatch() {
    let files = std::collections::BTreeMap::from([(
        "App.ui".into(),
        include_str!("../../tests/fixtures/svet-native-ime/App.ui").into(),
    )]);
    for replacement in [false, true] {
        let mut runtime =
            crate::Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
        assert!(runtime.focus_control(1));
        runtime.text_preedit("未確定");
        let owner = runtime.native_editor_session();
        let mut host = NativeHost {
            app: runtime,
            config: NativeWindowConfig::default(),
            backend: Some(UiBackend::Svet),
            platform: TargetPlatform::Desktop,
            chooser: None,
            accessibility: None,
            waker: None,
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
            composing: true,
            ime_cancelled: false,
            composition_owner: owner,
            blurred_editor: None,
            clicks: Default::default(),
            last_frame: Instant::now(),
        };
        if replacement {
            host.app =
                crate::Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
            host.app.focus_control(1);
        } else {
            host.app.focus_control(2);
        }
        host.commit_ime("stale").unwrap();
        host.commit_ime("stale-again").unwrap();
        assert!(!host.composing);
        assert!(host.composition_owner.is_none());
        assert_eq!(host.app.text_value(1), "seed-A");
        assert_eq!(host.app.text_value(2), "seed-B");
        host.app.focus_control(1);
        host.app.text_key("End", false, false);
        host.preedit_ime("pending").unwrap();
        host.commit_ime("確定").unwrap();
        assert_eq!(host.app.text_value(1), "seed-A確定");
        host.commit_ime("direct").unwrap();
        assert_eq!(host.app.text_value(1), "seed-A確定direct");
        host.app.text_key("a", false, true);
        let selected = host.app.text_selected();
        host.preedit_ime("cancelled").unwrap();
        host.commit_ime("").unwrap();
        assert_eq!(
            host.app.text_value(1),
            "seed-A確定direct",
            "empty commit must not delete selection"
        );
        assert_eq!(host.app.text_selected(), selected);
        host.app.sync_ui_bindings_native().unwrap();
        assert!(!host.app.ui_state_json().contains("stale"));
    }
}

#[test]
fn presentation_paints_retained_runtime_without_dispatching_update() {
    use std::cell::Cell;
    struct App {
        runtime: crate::Runtime,
        updates: usize,
        inspections: Cell<usize>,
        fail: bool,
    }
    impl NativeApp for App {
        fn inspect(&self, observer: &mut dyn FnMut(&crate::Runtime)) -> Result<(), String> {
            self.inspections.set(self.inspections.get() + 1);
            if self.fail {
                return Err("inspect refused".into());
            }
            observer(&self.runtime);
            Ok(())
        }
        fn update(&mut self, input: &mut dyn FnMut(&mut crate::Runtime)) -> Result<(), String> {
            self.updates += 1;
            input(&mut self.runtime);
            // Simulate a model update after the input/tick callback.
            self.runtime = crate::Runtime::from_ui_files(
                &std::collections::BTreeMap::from([(
                    "App.ui".into(),
                    include_str!("../../tests/fixtures/svet-native-window/App.ui").into(),
                )]),
                "App.ui",
                &Default::default(),
            )?;
            Ok(())
        }
    }
    let mut app = App {
        runtime: crate::Runtime::new(),
        updates: 0,
        inspections: Cell::new(0),
        fail: false,
    };
    app.update(&mut |runtime| {
        runtime.tick(16.);
    })
    .unwrap();
    let mut expected = vec![0; 640 * 360];
    app.runtime
        .paint_native(&mut expected, 640, 360, 1.)
        .unwrap();
    let mut pixels = vec![0; 640 * 360];
    assert_eq!(
        paint_app(&app, &mut pixels, 640, 360, 1.).unwrap(),
        app.runtime.is_animating()
    );
    assert_eq!(pixels, expected);
    assert_eq!(app.updates, 1);
    assert_eq!(app.inspections.get(), 1);
    assert!(paint_app(&app, &mut [], 640, 360, 1.).is_err());
    app.fail = true;
    assert_eq!(
        paint_app(&app, &mut pixels, 640, 360, 1.).unwrap_err(),
        "inspect refused"
    );
    assert_eq!(app.updates, 1);
}

#[test]
fn composition_reconciles_replacement_after_input_callback() {
    struct ReplacingApp {
        runtime: crate::Runtime,
        replace: bool,
        updates: usize,
        fail: bool,
    }
    impl NativeApp for ReplacingApp {
        fn inspect(&self, observer: &mut dyn FnMut(&crate::Runtime)) -> Result<(), String> {
            observer(&self.runtime);
            if self.fail {
                Err("inspect failed".into())
            } else {
                Ok(())
            }
        }
        fn update(&mut self, input: &mut dyn FnMut(&mut crate::Runtime)) -> Result<(), String> {
            self.updates += 1;
            input(&mut self.runtime);
            if self.replace {
                self.replace = false;
                self.runtime = crate::Runtime::from_ui_files(
                    &std::collections::BTreeMap::from([(
                        "App.ui".into(),
                        include_str!("../../tests/fixtures/svet-native-window/App.ui").into(),
                    )]),
                    "App.ui",
                    &Default::default(),
                )?;
                self.runtime.focus_control(1);
                self.runtime.text_preedit("新しい");
            }
            if self.fail {
                Err("sync failed".into())
            } else {
                Ok(())
            }
        }
    }
    let mut runtime = crate::Runtime::from_ui_files(
        &std::collections::BTreeMap::from([(
            "App.ui".into(),
            include_str!("../../tests/fixtures/svet-native-window/App.ui").into(),
        )]),
        "App.ui",
        &Default::default(),
    )
    .unwrap();
    assert!(runtime.focus_control(1));
    runtime.text_preedit("未確定");
    let owner = runtime.native_editor_session();
    let mut host = NativeHost {
        app: ReplacingApp {
            runtime,
            replace: true,
            updates: 0,
            fail: false,
        },
        config: NativeWindowConfig::default(),
        backend: Some(UiBackend::Svet),
        platform: TargetPlatform::Desktop,
        chooser: None,
        accessibility: None,
        waker: None,
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
        composing: true,
        ime_cancelled: false,
        composition_owner: owner,
        blurred_editor: None,
        clicks: Default::default(),
        last_frame: Instant::now(),
    };
    host.focus_sensitive_input(&mut |_| {}).unwrap();
    assert_eq!(host.app.updates, 1);
    let pixels = host.app.runtime.pixels(640, 360, 1.);
    let expected_caret = host.app.runtime.text_caret_bounds();
    let (editing, caret) = host.ime_presentation().unwrap();
    assert_eq!(
        host.app.updates, 1,
        "presentation must not run an application update"
    );
    assert!(editing);
    assert_eq!(caret, expected_caret);
    assert_eq!(
        host.app.runtime.pixels(640, 360, 1.),
        pixels,
        "inspection preserves replacement preedit pixels"
    );
    assert!(!host.composing);
    assert_eq!(host.composition_owner, owner);
    assert!(!host.app.runtime.ui_state_json().contains("未確定"));
    host.composing = true;
    host.composition_owner = owner;
    host.app.fail = true;
    assert_eq!(host.ime_presentation().unwrap_err(), "inspect failed");
    assert!(
        !host.composing,
        "retirement survives a later application error"
    );
    assert_eq!(host.composition_owner, owner);
    host.app.fail = false;
    let current = host.app.runtime.native_editor_session();
    host.composing = true;
    host.composition_owner = current;
    host.ime_presentation().unwrap();
    assert!(host.composing, "unchanged editor preserves composition");
    host.composing = false;
    host.ime_presentation().unwrap();
    assert_eq!(
        host.composition_owner, current,
        "pending commit keeps its address after empty preedit"
    );
}

#[test]
fn composition_retires_when_callback_replaces_editor_at_same_index() {
    let source = |value: &str| {
        serde_json::to_string(&std::collections::BTreeMap::from([("App.ui",format!("component App {{ state query: String = '{value}'; Column {{ width:220; height:100; TextInput {{ key:'editor'; width:180; height:32; value <-> state.query; }} }} }}"))])).unwrap()
    };
    let mut runtime = crate::Runtime::new();
    runtime
        .load_ui_files_json_native(&source("old"), "App.ui", "{}")
        .unwrap();
    assert!(runtime.focus_control(0));
    runtime.text_preedit("未確定");
    let mut host = NativeHost {
        app: runtime,
        config: NativeWindowConfig::default(),
        backend: Some(UiBackend::Svet),
        platform: TargetPlatform::Desktop,
        chooser: None,
        accessibility: None,
        waker: None,
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
        composing: true,
        ime_cancelled: false,
        composition_owner: None,
        blurred_editor: None,
        clicks: Default::default(),
        last_frame: Instant::now(),
    };
    host.focus_sensitive_input(&mut |runtime| {
        runtime
            .load_ui_files_json_native(&source("new"), "App.ui", "{}")
            .unwrap();
        assert!(runtime.focus_control(0));
    })
    .unwrap();
    assert!(
        !host.composing,
        "same index/key in a replacement scene is a new editor session"
    );
    assert_eq!(host.app.focused_index(), 0);
    assert_eq!(host.app.text_value(0), "new");
    host.app.text_insert("!");
    host.app.sync_ui_bindings_native().unwrap();
    assert!(!host.app.ui_state_json().contains("未確定"));
}

#[test]
fn window_refocus_rejects_retired_or_ineligible_editor_keys() {
    let files = |body: &str| {
        std::collections::BTreeMap::from([
            (
                "App.ui".into(),
                format!(
                    "component App {{ state a: String = 'first'; state b: String = 'second'; Column {{ width:220; height:200; {body} }} }}"
                ),
            ),
            ("Button.component.ui".into(), crate::BUTTON_COMPONENT.into()),
        ])
    };
    let first = "TextInput { key:'a'; width:180; height:32; value <-> state.a; }";
    let second = "TextInput { key:'b'; width:180; height:32; value <-> state.b; }";
    let duplicate = crate::Runtime::from_ui_files(
        &files(&format!("{first} {second} {second}")),
        "App.ui",
        &Default::default(),
    );
    assert!(matches!(duplicate, Err(error) if error.contains("Повторный key b")));
    for (case, replacement) in [
        ("removed", ""),
        (
            "disabled",
            "TextInput { key:'b'; disabled:true; width:180; height:32; value <-> state.b; }",
        ),
        (
            "changed kind",
            "Button { key:'b'; width:180; height:32; text:'button'; }",
        ),
    ] {
        let mut runtime = crate::Runtime::from_ui_files(
            &files(&format!("{first} {second}")),
            "App.ui",
            &Default::default(),
        )
        .unwrap();
        assert!(runtime.focus_control(1));
        let mut host = NativeHost {
            app: runtime,
            config: NativeWindowConfig::default(),
            backend: Some(UiBackend::Svet),
            platform: TargetPlatform::Desktop,
            chooser: None,
            accessibility: None,
            waker: None,
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
        };
        host.focus_changed(false).unwrap();
        host.app = crate::Runtime::from_ui_files(
            &files(&format!("{first} {replacement}")),
            "App.ui",
            &Default::default(),
        )
        .unwrap();
        host.focus_changed(true).unwrap();
        assert_eq!(
            host.app.focused_index(),
            0,
            "{case}: ordinary focus fallback"
        );
        assert!(
            host.blurred_editor.is_none(),
            "{case}: retired key consumed"
        );
        host.app.text_insert("!");
        assert!(
            host.app.text_value(0).contains('!'),
            "{case}: input reaches fallback editor"
        );
    }
}

#[test]
fn pointer_focus_switch_retires_composition_but_hover_preserves_it() {
    let files=std::collections::BTreeMap::from([("App.ui".into(),"component App { state a: String = 'first'; state b: String = 'second'; Column { width:220; height:160; TextInput { width:180; height:32; value <-> state.a; } TextInput { width:180; height:32; value <-> state.b; } } }".into())]);
    let mut runtime = crate::Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
    assert!(runtime.focus_control(0));
    runtime.text_preedit("未確定");
    let bounds = runtime.control_bounds(1);
    let (x, y) = (bounds[0] + bounds[2] * 0.5, bounds[1] + bounds[3] * 0.5);
    let mut host = NativeHost {
        app: runtime,
        config: NativeWindowConfig::default(),
        backend: Some(UiBackend::Svet),
        platform: TargetPlatform::Desktop,
        chooser: None,
        accessibility: None,
        waker: None,
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
        pointer: (x, y),
        touch_last: None,
        touch_owner: None,
        shift: false,
        command: false,
        control: false,
        alt: false,
        composing: true,
        ime_cancelled: false,
        composition_owner: None,
        blurred_editor: None,
        clicks: Default::default(),
        last_frame: Instant::now(),
    };
    host.focus_sensitive_input(&mut |runtime| runtime.pointer(x, y, 0))
        .unwrap();
    assert!(host.composing);
    assert_eq!(host.app.focused_index(), 0);
    host.focus_sensitive_input(&mut |runtime| runtime.pointer(x, y, 1))
        .unwrap();
    assert_eq!(host.app.focused_index(), 1);
    assert!(
        !host.composing,
        "pointer focus switch must retire the old IME owner"
    );
    host.focus_sensitive_input(&mut |runtime| runtime.pointer(x, y, 2))
        .unwrap();
    assert_eq!(host.app.text_value(0), "first");
    assert_eq!(host.app.text_value(1), "second");
    assert!(host.app.text_insert("-next"));
    assert!(host.app.sync_ui_bindings_native().unwrap());
    let state: serde_json::Value = serde_json::from_str(&host.app.ui_state_json()).unwrap();
    assert_eq!(state["a"], "first");
    assert!(state["b"].as_str().unwrap().contains("-next"));
    assert!(!state["b"].as_str().unwrap().contains("未確定"));
    host.app.text_preedit("pending-again");
    host.composing = true;
    host.focus_sensitive_input(&mut |runtime| runtime.pointer(210., 140., 0))
        .unwrap();
    assert!(host.composing);
    assert_eq!(host.app.focused_index(), 1);
    host.focus_sensitive_input(&mut |runtime| runtime.pointer(210., 140., 1))
        .unwrap();
    assert!(!host.composing);
    assert!(!host.app.text_editing());
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&host.app.ui_state_json()).unwrap(),
        state
    );
}

#[test]
fn tab_and_escape_end_composition_without_committing_pending_text() {
    for key in [NamedKey::Tab, NamedKey::Escape] {
        let files = std::collections::BTreeMap::from([("App.ui".into(), "component App { state a: String = 'first'; state b: String = 'second'; Column { TextInput { width:180; height:32; value <-> state.a; } TextInput { width:180; height:32; value <-> state.b; } } }".into())]);
        let mut runtime =
            crate::Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
        assert!(runtime.focus_control(0));
        let normal_pixels = runtime.pixels(360, 120, 1.);
        runtime.text_preedit("未確定");
        let mut host = NativeHost {
            app: runtime,
            config: NativeWindowConfig::default(),
            backend: Some(UiBackend::Svet),
            platform: TargetPlatform::Desktop,
            chooser: None,
            accessibility: None,
            waker: None,
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
            composing: true,
            ime_cancelled: false,
            composition_owner: None,
            blurred_editor: None,
            clicks: Default::default(),
            last_frame: Instant::now(),
        };
        for (pressed, repeat) in [(false, false), (true, true)] {
            host.navigation_key(&Key::Named(key), pressed, repeat)
                .unwrap();
            assert!(
                host.composing,
                "release/repeat must preserve the current composition"
            );
            assert_eq!(host.app.focused_index(), 0);
        }
        host.navigation_key(&Key::Named(key), true, false).unwrap();
        assert!(
            !host.composing,
            "{key:?} must retire the old composition session"
        );
        if key == NamedKey::Escape {
            assert_eq!(
                host.app.pixels(360, 120, 1.),
                normal_pixels,
                "Escape must remove pending preedit pixels"
            );
        }
        assert_eq!(host.app.text_value(0), "first");
        assert_eq!(host.app.text_value(1), "second");
        assert_eq!(
            host.app.focused_index(),
            if key == NamedKey::Tab { 1 } else { 0 }
        );
        assert!(host.app.text_insert("-next"));
        assert!(host.app.sync_ui_bindings_native().unwrap());
        let state: serde_json::Value = serde_json::from_str(&host.app.ui_state_json()).unwrap();
        let field = if key == NamedKey::Tab { "b" } else { "a" };
        assert!(state[field].as_str().unwrap().contains("-next"));
        assert!(!state[field].as_str().unwrap().contains("未確定"));
        assert_eq!(
            state[if key == NamedKey::Tab { "a" } else { "b" }],
            if key == NamedKey::Tab {
                "first"
            } else {
                "second"
            }
        );
    }
}

#[test]
fn focus_loss_clears_composition_and_modifiers_without_committing_preedit() {
    let files = std::collections::BTreeMap::from([(
        "App.ui".into(),
        include_str!("../../tests/fixtures/svet-native-window/App.ui").into(),
    )]);
    let mut runtime = crate::Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
    let editor = (0..runtime.control_count())
        .find(|index| runtime.control_editable(*index))
        .unwrap();
    assert!(runtime.focus_control(editor));
    runtime.text_preedit("未確定");
    let mut host = NativeHost {
        app: runtime,
        config: NativeWindowConfig::default(),
        backend: Some(UiBackend::Svet),
        platform: TargetPlatform::Desktop,
        chooser: None,
        accessibility: None,
        waker: None,
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
        pointer: (10., 10.),
        touch_last: Some((10., 10.)),
        touch_owner: None,
        shift: true,
        command: true,
        control: true,
        alt: true,
        composing: true,
        ime_cancelled: false,
        composition_owner: None,
        blurred_editor: None,
        clicks: Default::default(),
        last_frame: Instant::now(),
    };
    host.focus_changed(false).unwrap();
    assert!(
        !host.composing,
        "focus loss must retire the old IME session"
    );
    assert!(
        !host.shift && !host.command && !host.control && !host.alt,
        "stale modifiers must not affect refocused input"
    );
    assert!(host.touch_last.is_none());
    assert_eq!(host.app.text_value(editor), "seed");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&host.app.ui_state_json()).unwrap()["query"],
        "seed"
    );
    host.focus_changed(true).unwrap();
    assert!(!host.composing);
    assert!(host.app.focus_control(editor));
    assert!(host.app.text_insert("-after"));
    assert!(host.app.sync_ui_bindings_native().unwrap());
    assert!(host.app.text_value(editor).contains("-after"));
}

#[test]
fn surface_backoff_and_occlusion_block_acquisition_until_ready() {
    let runtime = crate::Runtime::new();
    let mut host = NativeHost {
        app: runtime,
        config: NativeWindowConfig::default(),
        backend: Some(UiBackend::Svet),
        platform: TargetPlatform::Desktop,
        chooser: None,
        accessibility: None,
        waker: None,
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
    };
    let deadline = Instant::now() + std::time::Duration::from_secs(30);
    host.retry_at = Some(deadline);
    // A real draw would fail because no native window exists. Repeated
    // redraws during backoff must not reach window lookup or acquisition.
    for _ in 0..10 {
        assert!(host.draw().is_ok());
        assert_eq!(host.retry_at, Some(deadline));
    }
    host.retry_at = Some(Instant::now() - std::time::Duration::from_millis(1));
    assert!(
        host.draw()
            .unwrap_err()
            .contains("window is not initialized")
    );
    assert_eq!(host.retry_at, None, "expired retry must be consumed");
    host.occluded = true;
    for _ in 0..10 {
        assert!(host.draw().is_ok());
    }
    host.occluded = false;
    assert!(
        host.draw()
            .unwrap_err()
            .contains("window is not initialized"),
        "unoccluded draw must resume presentation attempts"
    );
    let now = Instant::now();
    let animation = now + std::time::Duration::from_millis(16);
    let retry = now + std::time::Duration::from_millis(50);
    host.animation_at = Some(animation);
    host.retry_at = Some(retry);
    assert_eq!(host.scheduled_redraw(now), (false, Some(animation)));
    assert_eq!(host.scheduled_redraw(animation), (true, Some(retry)));
    assert_eq!(host.scheduled_redraw(retry), (true, None));
    assert_eq!(
        host.scheduled_redraw(retry),
        (false, None),
        "expired animation must not spin"
    );
    host.animation_at = Some(animation);
    assert!(
        host.draw()
            .unwrap_err()
            .contains("window is not initialized"),
        "input redraw must bypass animation pacing"
    );
    host.occluded = true;
    host.retry_at = Some(retry);
    host.animation_at = Some(animation);
    assert_eq!(host.scheduled_redraw(now), (false, None));
    assert!(host.retry_at.is_none() && host.animation_at.is_none());
    // Occlusion suppresses painting, but cannot discard completion service
    // for already-submitted GPU work.
    host.gpu_health_at = Some(animation);
    assert_eq!(host.scheduled_redraw(now), (false, Some(animation)));
    assert_eq!(host.scheduled_redraw(animation), (false, Some(animation)));
    host.gpu_health_at = None;
    assert_eq!(
        host.scheduled_redraw(animation),
        (false, None),
        "completed queue must return to unbounded Wait"
    );
}

#[test]
fn application_config_ignores_environment_and_never_asks_for_a_backend() {
    // `application` takes nothing but the title, so no argument or variable can
    // reach it; the demo resolvers below are the only readers of either.
    let config = NativeWindowConfig::application("Shelf");
    assert_eq!(config.title, "Shelf");
    assert_eq!(config.mode, HostMode::Application);
    assert_eq!(config.backend, Some(UiBackend::Svet));
    assert_eq!(config.platform, TargetPlatform::compiled());
    assert_eq!(config.resolved_backend(), Some(UiBackend::Svet));
    assert_eq!(
        (config.width, config.height, config.resizable),
        (800., 600., true)
    );

    // The demo resolution keeps honouring the same arguments and variables.
    let args = |list: &[&str]| list.iter().map(|arg| arg.to_string()).collect::<Vec<_>>();
    assert_eq!(UiBackend::resolve(args(&[]), None), None);
    assert_eq!(
        UiBackend::resolve(args(&["--egui"]), Some("svet")),
        Some(UiBackend::Egui)
    );
    assert_eq!(
        UiBackend::resolve(args(&["-b", "svet"]), None),
        Some(UiBackend::Svet)
    );
    assert_eq!(
        UiBackend::resolve(args(&[]), Some("egui")),
        Some(UiBackend::Egui)
    );
    assert_eq!(UiBackend::resolve(args(&["--ask"]), Some("egui")), None);
    assert_eq!(UiBackend::resolve(args(&[]), Some("ask")), None);
    assert_eq!(
        TargetPlatform::resolve(args(&[]), None),
        TargetPlatform::compiled()
    );
    assert_eq!(
        TargetPlatform::resolve(args(&["--platform", "android"]), Some("ios")),
        TargetPlatform::Android
    );
    assert_eq!(
        TargetPlatform::resolve(args(&[]), Some("ios")),
        TargetPlatform::Ios
    );
    assert_eq!(NativeWindowConfig::default().mode, HostMode::Demo);

    // No backend means "show the chooser" for the demo only.
    let unset = |mode| NativeWindowConfig {
        backend: None,
        mode,
        ..NativeWindowConfig::application("Shelf")
    };
    assert_eq!(unset(HostMode::Demo).resolved_backend(), None);
    assert_eq!(
        unset(HostMode::Application).resolved_backend(),
        Some(UiBackend::Svet)
    );
    let egui = NativeWindowConfig {
        backend: Some(UiBackend::Egui),
        ..NativeWindowConfig::application("Shelf")
    };
    assert_eq!(egui.resolved_backend(), Some(UiBackend::Egui));
}

#[test]
fn application_title_is_verbatim_and_demo_title_keeps_its_suffix() {
    for (backend, platform, gpu) in [
        (Some(UiBackend::Svet), TargetPlatform::Desktop, true),
        (Some(UiBackend::Egui), TargetPlatform::Ios, false),
        (None, TargetPlatform::Android, false),
    ] {
        assert_eq!(
            window_title(HostMode::Application, "Shelf", backend, platform, gpu),
            "Shelf"
        );
    }
    let demo =
        |backend, platform, gpu| window_title(HostMode::Demo, "Forma", backend, platform, gpu);
    assert_eq!(
        demo(Some(UiBackend::Svet), TargetPlatform::Desktop, true),
        "Forma [svet/GPU] · F6: платформа · F7: бэкенд"
    );
    assert_eq!(
        demo(Some(UiBackend::Svet), TargetPlatform::Desktop, false),
        "Forma [svet] · F6: платформа · F7: бэкенд"
    );
    assert_eq!(
        demo(Some(UiBackend::Svet), TargetPlatform::Ios, true),
        "Forma [svet · iOS] · F6: платформа · F7: бэкенд"
    );
    assert_eq!(
        demo(Some(UiBackend::Egui), TargetPlatform::Desktop, true),
        "Forma [egui] · F6: платформа · F7: бэкенд"
    );
    assert_eq!(
        demo(None, TargetPlatform::Android, false),
        "Forma · Android · Выберите бэкенд: [1] egui / [2] svet"
    );
}

#[test]
fn function_keys_belong_to_the_demo_host_only() {
    let key = |named| Key::Named(named);
    let demo = |named, pressed, repeat, hooks| {
        host_shortcut(HostMode::Demo, &key(named), pressed, repeat, hooks)
    };
    assert_eq!(
        demo(NamedKey::F6, true, false, false),
        Some(HostShortcut::NextPlatform)
    );
    assert_eq!(
        demo(NamedKey::F7, true, false, false),
        Some(HostShortcut::ToggleBackend)
    );
    assert_eq!(demo(NamedKey::F8, true, false, false), None);
    assert_eq!(
        demo(NamedKey::F8, true, false, true),
        Some(HostShortcut::InjectDeviceLoss)
    );
    // Releases, repeats and other keys were never captured.
    assert_eq!(demo(NamedKey::F6, false, false, true), None);
    assert_eq!(demo(NamedKey::F7, true, true, true), None);
    assert_eq!(demo(NamedKey::F5, true, false, true), None);

    for named in [NamedKey::F6, NamedKey::F7, NamedKey::F8] {
        assert_eq!(
            host_shortcut(HostMode::Application, &key(named), true, false, true),
            None,
            "{named:?} must reach the application"
        );
    }
}

#[test]
fn accent_bar_is_painted_for_the_demo_egui_profile_only() {
    assert!(shows_frame_bar(HostMode::Demo, Some(UiBackend::Egui)));
    assert!(!shows_frame_bar(HostMode::Demo, Some(UiBackend::Svet)));
    assert!(!shows_frame_bar(HostMode::Demo, None));
    assert!(!shows_frame_bar(
        HostMode::Application,
        Some(UiBackend::Egui)
    ));
    assert!(!shows_frame_bar(
        HostMode::Application,
        Some(UiBackend::Svet)
    ));
}

/// Host without a window, as the loop builds it before `resumed`.
fn windowless_host<A: NativeApp + 'static>(app: A, config: NativeWindowConfig) -> NativeHost<A> {
    NativeHost {
        app,
        backend: config.resolved_backend(),
        platform: config.platform,
        config,
        chooser: None,
        accessibility: None,
        waker: None,
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
    }
}

/// Application that counts what the host asks of it.
struct Recorder {
    runtime: crate::Runtime,
    updates: u32,
    /// Keys seen, as (logical key, pressed, modifiers, composing).
    keys: Vec<(Key, bool, NativeModifiers, bool)>,
    /// The application's shortcut: command+K is handled, everything else is not.
    shortcut_hits: u32,
    pointer: Vec<NativePointerEvent>,
    sizes: Vec<(f32, f32, f32)>,
    unsaved: bool,
}

impl Recorder {
    fn new() -> Self {
        Self::with_runtime(crate::Runtime::new())
    }

    fn with_runtime(runtime: crate::Runtime) -> Self {
        Self {
            runtime,
            updates: 0,
            keys: Vec::new(),
            shortcut_hits: 0,
            pointer: Vec::new(),
            sizes: Vec::new(),
            unsaved: false,
        }
    }
}

impl NativeApp for Recorder {
    fn update(&mut self, input: &mut dyn FnMut(&mut crate::Runtime)) -> Result<(), String> {
        self.updates += 1;
        input(&mut self.runtime);
        Ok(())
    }

    fn inspect(&self, observer: &mut dyn FnMut(&crate::Runtime)) -> Result<(), String> {
        observer(&self.runtime);
        Ok(())
    }

    fn on_key_event(&mut self, event: &NativeKeyEvent<'_>) -> Result<bool, String> {
        self.keys.push((
            event.logical_key.clone(),
            event.pressed,
            event.modifiers,
            event.composing,
        ));
        let shortcut = event.pressed
            && event.modifiers.command
            && matches!(event.logical_key, Key::Character(c) if c.as_str() == "k");
        self.shortcut_hits += u32::from(shortcut);
        Ok(shortcut)
    }

    fn on_resized(&mut self, width: f32, height: f32, scale: f32) -> Result<(), String> {
        self.sizes.push((width, height, scale));
        Ok(())
    }

    fn on_close_requested(&mut self) -> Result<bool, String> {
        Ok(!self.unsaved)
    }

    fn on_pointer_event(&mut self, event: &NativePointerEvent) -> Result<(), String> {
        self.pointer.push(*event);
        Ok(())
    }
}

#[test]
fn default_hooks_leave_existing_applications_unchanged() {
    // `Runtime` implements only `update` and `inspect`, like any earlier implementor.
    let mut app = crate::Runtime::new();
    let key = Key::Named(NamedKey::F6);
    let physical = PhysicalKey::Code(winit::keyboard::KeyCode::F6);
    let event = NativeKeyEvent {
        logical_key: &key,
        physical_key: &physical,
        text: None,
        pressed: true,
        repeat: false,
        modifiers: NativeModifiers::default(),
        composing: false,
    };
    assert_eq!(
        app.on_key_event(&event),
        Ok(false),
        "keys go on to the runtime"
    );
    assert_eq!(app.on_close_requested(), Ok(true), "the window closes");
    assert_eq!(app.on_resized(640., 480., 2.), Ok(()));
    let pointer = NativePointerEvent {
        kind: NativePointerKind::DoubleClick,
        x: 1.,
        y: 2.,
        modifiers: NativeModifiers::default(),
    };
    assert_eq!(app.on_pointer_event(&pointer), Ok(()));
}

#[test]
fn application_sees_keys_first_and_decides_whether_the_runtime_gets_them() {
    let mut host = windowless_host(Recorder::new(), NativeWindowConfig::application("Shelf"));
    let physical = PhysicalKey::Code(winit::keyboard::KeyCode::KeyK);
    let k = Key::Character("k".into());

    // Unhandled: the host goes on to the runtime.
    assert_eq!(
        host.offer_key(&k, &physical, Some("k"), true, false),
        Ok(false)
    );

    host.command = true;
    host.shift = true;
    host.composing = true;
    assert_eq!(host.offer_key(&k, &physical, None, true, false), Ok(true));
    assert_eq!(host.offer_key(&k, &physical, None, false, false), Ok(false));
    assert_eq!(host.app.shortcut_hits, 1);

    // A function key the demo would capture is an ordinary key here.
    let f7 = Key::Named(NamedKey::F7);
    assert_eq!(
        host_shortcut(host.config.mode, &f7, true, false, true),
        None
    );
    let physical = PhysicalKey::Code(winit::keyboard::KeyCode::F7);
    assert_eq!(host.offer_key(&f7, &physical, None, true, false), Ok(false));

    let held = NativeModifiers {
        shift: true,
        control: false,
        alt: false,
        command: true,
    };
    assert_eq!(
        host.app.keys,
        vec![
            (k.clone(), true, NativeModifiers::default(), false),
            (k.clone(), true, held, true),
            (k, false, held, true),
            (f7, true, held, true),
        ]
    );
    assert_eq!(host.app.updates, 0, "offering a key dispatches nothing");
}

#[test]
fn close_request_can_be_vetoed_by_the_application() {
    let mut app = Recorder::new();
    assert_eq!(app.on_close_requested(), Ok(true));
    app.unsaved = true;
    assert_eq!(app.on_close_requested(), Ok(false));
}

#[test]
fn secondary_and_middle_buttons_reach_the_application_with_position() {
    assert_eq!(
        button_pointer_kind(MouseButton::Right, true),
        Some(NativePointerKind::Secondary { pressed: true })
    );
    assert_eq!(
        button_pointer_kind(MouseButton::Middle, false),
        Some(NativePointerKind::Middle { pressed: false })
    );
    for button in [
        MouseButton::Left,
        MouseButton::Back,
        MouseButton::Forward,
        MouseButton::Other(9),
    ] {
        assert_eq!(button_pointer_kind(button, true), None);
    }

    let mut host = windowless_host(Recorder::new(), NativeWindowConfig::application("Shelf"));
    host.pointer = (12., 34.);
    host.alt = true;
    host.pointer_event(NativePointerKind::Secondary { pressed: true })
        .unwrap();
    assert_eq!(
        host.app.pointer,
        vec![NativePointerEvent {
            kind: NativePointerKind::Secondary { pressed: true },
            x: 12.,
            y: 34.,
            modifiers: NativeModifiers {
                alt: true,
                ..Default::default()
            },
        }]
    );
    assert_eq!(host.app.updates, 0, "the runtime receives no pointer input");
}

#[test]
fn double_click_needs_two_close_presses_in_time_and_space() {
    let start = Instant::now();
    let ms = |n| start + std::time::Duration::from_millis(n);
    let mut clicks = ClickTracker::default();
    assert!(!clicks.press(ms(0), 10., 10.));
    assert!(clicks.press(ms(200), 12., 9.));
    assert!(
        !clicks.press(ms(300), 12., 9.),
        "a third press starts a new pair"
    );
    assert!(clicks.press(ms(500), 12., 9.));

    assert!(!clicks.press(ms(1000), 10., 10.));
    assert!(!clicks.press(ms(1501), 10., 10.), "too slow");
    assert!(!clicks.press(ms(1600), 30., 10.), "too far");
    assert!(
        clicks.press(ms(2100), 30., 10.),
        "the interval is inclusive"
    );
}

#[test]
fn primary_double_click_is_reported_after_the_runtime_got_both_presses() {
    let runtime = crate::Runtime::from_source(
        "component Demo { Frame { width:200; height:100; Button { key:'open'; width:180; height:60; } } }",
    )
    .unwrap();
    let b = runtime.control_bounds(0);
    let mut host = windowless_host(
        Recorder::with_runtime(runtime),
        NativeWindowConfig::application("Shelf"),
    );
    host.pointer = (b[0] + 20., b[1] + 20.);
    let start = Instant::now();
    let ms = |n| start + std::time::Duration::from_millis(n);

    host.primary_button(true, ms(0)).unwrap();
    host.primary_button(false, ms(50)).unwrap();
    assert!(
        host.app.pointer.is_empty(),
        "a single click is the runtime's"
    );
    assert_eq!(host.app.runtime.control_clicks(0), 1);

    host.primary_button(true, ms(150)).unwrap();
    assert_eq!(host.app.pointer.len(), 1);
    assert_eq!(host.app.pointer[0].kind, NativePointerKind::DoubleClick);
    assert_eq!((host.app.pointer[0].x, host.app.pointer[0].y), host.pointer);
    host.primary_button(false, ms(200)).unwrap();
    assert_eq!(
        host.app.runtime.control_clicks(0),
        2,
        "the runtime still receives every press and release"
    );
    assert_eq!(host.app.pointer.len(), 1, "releases report nothing");
}

#[test]
fn resize_reports_logical_size_and_scale() {
    assert_eq!(logical_size(1600, 1200, 2.), (800., 600.));
    assert_eq!(logical_size(801, 600, 1.), (801., 600.));
    assert_eq!(logical_size(0, 0, 1.5), (0., 0.));
    assert_eq!(logical_size(300, 150, 0.), (300., 150.), "a bad scale is 1");

    // Without a window there is no size to report.
    let mut host = windowless_host(Recorder::new(), NativeWindowConfig::application("Shelf"));
    host.report_size().unwrap();
    assert!(host.app.sizes.is_empty());
}

#[test]
fn wake_updates_the_application_even_when_nothing_is_drawn() {
    let mut host = windowless_host(Recorder::new(), NativeWindowConfig::application("Shelf"));
    host.occluded = true;
    host.woken().unwrap();
    host.woken().unwrap();
    assert_eq!(host.app.updates, 2, "one update per served wake");

    // The demo chooser precedes the application: it is not updated behind it.
    host.chooser = Some(build_chooser_runtime().unwrap());
    host.woken().unwrap();
    assert_eq!(host.app.updates, 2);
}

#[test]
fn default_waker_hook_accepts_and_drops_the_handle() {
    // Compiles only while `on_waker` keeps a default body for existing implementors.
    fn hook<A: NativeApp>(app: &A, waker: NativeWaker) -> Result<(), String> {
        app.on_waker(waker)
    }
    let _ = hook::<Recorder>;
}

#[test]
fn waker_is_a_cloneable_cross_thread_handle() {
    fn assert_handle<T: Send + Sync + Clone + 'static>() {}
    assert_handle::<NativeWaker>();
}

#[test]
fn wakes_merge_until_the_loop_serves_them() {
    let pending = AtomicBool::new(false);
    assert!(begin_wake(&pending), "first wake posts an event");
    assert!(!begin_wake(&pending), "a queued wake covers later calls");
    assert!(!begin_wake(&pending));
    finish_wake(&pending);
    assert!(begin_wake(&pending), "a wake after service posts again");
}

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

    let mut two = crate::Runtime::from_sources(crate::TWO_BUTTONS_EXAMPLE, crate::BUTTON_COMPONENT)
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
    assert_eq!(
        TargetPlatform::from_arg("desktop"),
        Some(TargetPlatform::Desktop)
    );
    assert_eq!(TargetPlatform::from_arg("ios"), Some(TargetPlatform::Ios));
    assert_eq!(
        TargetPlatform::from_arg("iPhone"),
        Some(TargetPlatform::Ios)
    );
    assert_eq!(
        TargetPlatform::from_arg("android"),
        Some(TargetPlatform::Android)
    );
    assert_eq!(
        TargetPlatform::from_arg("Pixel"),
        Some(TargetPlatform::Android)
    );
    assert_eq!(TargetPlatform::from_arg("unknown"), None);

    assert_eq!(TargetPlatform::Desktop.next(), TargetPlatform::Ios);
    assert_eq!(TargetPlatform::Ios.next(), TargetPlatform::Android);
    assert_eq!(TargetPlatform::Android.next(), TargetPlatform::Desktop);

    assert_eq!(
        TargetPlatform::Ios.viewport_size(800.0, 600.0),
        (393.0, 852.0)
    );
    assert_eq!(
        TargetPlatform::Android.viewport_size(800.0, 600.0),
        (412.0, 915.0)
    );
    assert_eq!(
        TargetPlatform::Desktop.viewport_size(800.0, 600.0),
        (800.0, 600.0)
    );

    let mut buf = vec![0x111520u32; 393 * 852];
    apply_mobile_chrome(&mut buf, 393, 852, 1.0, TargetPlatform::Ios);
    assert!(
        buf.contains(&0x090B10),
        "iOS Dynamic Island should be painted"
    );
    assert!(
        buf.contains(&0xE2E8F5),
        "iOS Home Indicator should be painted"
    );

    let mut android_buf = vec![0x111520u32; 412 * 915];
    apply_mobile_chrome(&mut android_buf, 412, 915, 1.0, TargetPlatform::Android);
    assert!(
        android_buf.contains(&0x090B10),
        "Android camera cutout should be painted"
    );
    assert!(
        android_buf.contains(&0x76DFB1),
        "Android gesture pill should be painted"
    );
}
