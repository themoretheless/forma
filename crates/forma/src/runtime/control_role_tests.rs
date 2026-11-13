//! What the standard choice and range controls do today, before control_state models are bound
//! to them: a checkbox and a switch report clicks and leave their value to the host, a slider
//! keeps a fraction of its own.

use super::*;

/// The light theme of `controls/tokens.js`, substituted the way `materializeTheme` does it there:
/// the library names theme values instead of carrying them.
const THEME: &[(&str, &str)] = &[
    ("theme.surfaceRaised", "#FFFFFF"),
    ("theme.surfaceHover", "#EEE9DF"),
    ("theme.surfacePressed", "#EBE5DC"),
    ("theme.surface", "#FBF9F4"),
    ("theme.canvas", "#F4F1EA"),
    ("theme.backdrop", "#2B2A2666"),
    ("theme.borderStrong", "#C8BFB0"),
    ("theme.border", "#DED7CA"),
    ("theme.textMuted", "#6B665C"),
    ("theme.textDisabled", "#A39C90"),
    ("theme.text", "#2B2A26"),
    ("theme.focus", "#A6470C"),
    ("theme.accentWash", "#F5E5D5"),
    ("theme.accentInk", "#93420E"),
    ("theme.accent", "#DE6A19"),
    ("theme.reveal", "#DE6A19F2"),
    ("theme.primaryHover", "#A6470C"),
    ("theme.primaryPressed", "#913F0B"),
    ("theme.primaryText", "#FFFFFF"),
    ("theme.primary", "#B9510F"),
    ("theme.dangerHover", "#AA2E20"),
    ("theme.dangerPressed", "#92261B"),
    ("theme.dangerText", "#FFFFFF"),
    ("theme.dangerInk", "#A42E22"),
    ("theme.dangerWash", "#F9E4DF"),
    ("theme.danger", "#C23625"),
    ("theme.successWash", "#E7F0E8"),
    ("theme.success", "#306448"),
    ("theme.warningWash", "#F5E8CE"),
    ("theme.warning", "#825219"),
    ("theme.track", "#E3DCCE"),
    ("motion.control", "180ms"),
    ("motion.reveal", "280ms"),
];

fn materialize(source: &str) -> String {
    THEME.iter().fold(source.to_owned(), |text, (name, value)| {
        text.replace(&format!("state.{name}"), value)
    })
}

/// A scene compiled together with the standard component library of `controls/components`.
fn library(body: &str) -> Result<Runtime, String> {
    let source = format!("component App {{ {body} }}");
    let mut files = std::collections::BTreeMap::from([("App.ui".to_owned(), source)]);
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("controls/components");
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap().to_owned();
        let source = materialize(&std::fs::read_to_string(path).unwrap());
        files.insert(format!("components/{name}"), source);
    }
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("controls/assets");
    for entry in std::fs::read_dir(assets).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|extension| extension == "svg") {
            let name = path.file_name().unwrap().to_str().unwrap().to_owned();
            files.insert(
                format!("assets/{name}"),
                std::fs::read_to_string(path).unwrap(),
            );
        }
    }
    Runtime::from_ui_files(&files, "App.ui", &Default::default())
}

/// A scene that uses only what the runtime builds in.
fn builtin(body: &str) -> Runtime {
    let source = format!("component App {{ {body} }}");
    let files = std::collections::BTreeMap::from([("App.ui".to_owned(), source)]);
    Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap()
}

fn state(runtime: &Runtime) -> serde_json::Value {
    serde_json::from_str(&runtime.ui_state_json()).unwrap()
}

fn click(runtime: &mut Runtime, index: usize) {
    let bounds = runtime.control_bounds(index);
    let (x, y) = (bounds[0] + bounds[2] / 2., bounds[1] + bounds[3] / 2.);
    runtime.pointer(x, y, 0);
    runtime.pointer(x, y, 1);
    runtime.pointer(x, y, 2);
}

const BOUND_SLIDER: &str = "state level: Number = LEVEL; Column { width: 240; height: 60; \
     Slider { key: 'level'; width: 216; height: 24; value <-> state.level; } }";

#[test]
fn a_checkbox_and_a_switch_count_clicks_and_leave_their_value_to_the_host() {
    let mut runtime = library(
        "state on: Bool = false; Column { width: 320; height: 200; \
         Checkbox { key: 'check'; checked: state.on; } \
         Switch { key: 'switch'; checked: state.on; } }",
    )
    .unwrap();
    assert_eq!(runtime.control_count(), 2);
    click(&mut runtime, 0);
    click(&mut runtime, 1);
    assert_eq!(
        (runtime.control_clicks(0), runtime.control_clicks(1)),
        (1, 1)
    );
    assert_eq!(state(&runtime)["on"], false, "nothing toggled the value");
    assert_eq!(runtime.sync_ui_bindings_native(), Ok(false));
    assert_eq!(state(&runtime)["on"], false);

    // Space activates the focused checkbox the same way: one more click, no change of value.
    assert!(runtime.focus_control(0));
    runtime.key_event(1, true, false);
    runtime.key_event(1, false, false);
    assert_eq!(runtime.control_clicks(0), 2);
    assert_eq!(state(&runtime)["on"], false);
    // Neither is a range control.
    assert!(runtime.range_value(0).is_nan() && runtime.range_value(1).is_nan());
}

#[test]
fn a_slider_holds_a_fraction_and_steps_by_fixed_amounts() {
    let mut runtime = builtin(&BOUND_SLIDER.replace("LEVEL", "0.5"));
    assert_eq!(runtime.range_value(0), 0.5);
    assert!(
        !runtime.range_key("ArrowRight"),
        "an unfocused slider ignores keys"
    );
    assert!(runtime.focus_control(0));
    let mut steps = Vec::new();
    for key in [
        "ArrowRight",
        "ArrowUp",
        "PageUp",
        "ArrowLeft",
        "ArrowDown",
        "PageDown",
    ] {
        assert!(runtime.range_key(key), "{key}");
        steps.push((runtime.range_value(0) * 100.).round() as i32);
    }
    assert_eq!(
        steps,
        [51, 52, 62, 61, 60, 50],
        "one hundredth by arrows, a tenth by pages"
    );
    assert!(runtime.range_key("Home"));
    assert_eq!(runtime.range_value(0), 0.);
    assert!(runtime.range_key("ArrowLeft"));
    assert_eq!(runtime.range_value(0), 0., "clamped at the start");
    assert!(runtime.range_key("End"));
    assert_eq!(runtime.range_value(0), 1.);
    assert!(runtime.range_key("PageUp"));
    assert_eq!(runtime.range_value(0), 1., "clamped at the end");
    assert!(!runtime.range_key("x"), "other keys are not the slider's");
    assert_eq!(runtime.range_value(0), 1.);
}

#[test]
fn dragging_a_slider_maps_the_pointer_between_its_eight_pixel_insets() {
    let mut runtime = builtin(&BOUND_SLIDER.replace("LEVEL", "0.5"));
    let bounds = runtime.control_bounds(0);
    assert_eq!(
        bounds[2], 216.,
        "the track is the width less eight pixels on each side"
    );
    let (start, y) = (bounds[0] + 8., bounds[1] + 5.);
    runtime.pointer(start + 50., y, 0);
    runtime.pointer(start + 50., y, 1);
    assert_eq!(runtime.range_value(0), 0.25);
    runtime.pointer(start + 150., y, 0);
    assert_eq!(runtime.range_value(0), 0.75);
    runtime.pointer(start + 400., y, 0);
    assert_eq!(runtime.range_value(0), 1., "past the end clamps");
    runtime.pointer(start + 150., y, 0);
    runtime.pointer(start + 150., y, 2);
    assert_eq!(runtime.sync_ui_bindings_native(), Ok(true));
    assert_eq!(
        state(&runtime)["level"],
        0.75,
        "the bound state receives the fraction"
    );
}

fn drag_to(runtime: &mut Runtime, fraction: f32) {
    let bounds = runtime.control_bounds(0);
    let (x, y) = (
        bounds[0] + 8. + fraction * (bounds[2] - 16.),
        bounds[1] + 5.,
    );
    runtime.pointer(x, y, 0);
    runtime.pointer(x, y, 1);
    runtime.pointer(x, y, 2);
}

const SCALED_SLIDER: &str = "state level: Number = LEVEL; Column { width: 240; height: 60; \
     Slider { key: 'level'; width: 216; height: 24; min: 0; max: 100; STEP \
     value <-> state.level; } }";

/// F-9: without `min` and `max` the bound number is the fraction itself, so a value of 40 used
/// to show a full slider and be replaced by a fraction on the first drag.
#[test]
fn a_slider_with_a_scale_shows_and_writes_values_on_that_scale() {
    let source = SCALED_SLIDER.replace("LEVEL", "40").replace("STEP", "");
    let mut runtime = builtin(&source);
    assert!((runtime.range_value(0) - 0.4).abs() < 1e-6, "40 of 100");
    drag_to(&mut runtime, 0.75);
    assert_eq!(runtime.sync_ui_bindings_native(), Ok(true));
    assert_eq!(
        state(&runtime)["level"],
        75.0,
        "the state stays on the scale of the slider"
    );
    assert!((runtime.range_value(0) - 0.75).abs() < 1e-6);
}

#[test]
fn a_slider_with_a_step_writes_values_on_the_step_grid() {
    let source = SCALED_SLIDER
        .replace("LEVEL", "40")
        .replace("STEP", "step: 5;");
    let mut runtime = builtin(&source);
    drag_to(&mut runtime, 0.73);
    assert_eq!(runtime.sync_ui_bindings_native(), Ok(true));
    assert_eq!(
        state(&runtime)["level"],
        75.0,
        "73 moves to the nearest multiple of five"
    );
    // The scene is rebuilt from the state, so the thumb settles on the grid as well.
    assert!((runtime.range_value(0) - 0.75).abs() < 1e-6);
    drag_to(&mut runtime, 1.);
    runtime.sync_ui_bindings_native().unwrap();
    assert_eq!(
        state(&runtime)["level"],
        100.0,
        "the end of the scale is always reachable"
    );
}

#[test]
fn the_keys_and_the_pointer_of_a_stepped_slider_move_along_the_step_grid() {
    let source = SCALED_SLIDER
        .replace("LEVEL", "40")
        .replace("STEP", "step: 25;");
    let mut runtime = builtin(&source);
    assert!(runtime.focus_control(0));
    let mut positions = Vec::new();
    for key in [
        "ArrowRight",
        "ArrowRight",
        "ArrowLeft",
        "PageUp",
        "End",
        "Home",
    ] {
        assert!(runtime.range_key(key), "{key}");
        positions.push((runtime.range_value(0) * 100.).round() as i32);
    }
    // 40 is off the grid: the first press moves a step and lands on the grid.
    assert_eq!(positions, [75, 100, 75, 100, 100, 0]);

    // A press on the track shows the thumb on the grid at once, before any sync.
    let bounds = runtime.control_bounds(0);
    let (x, y) = (bounds[0] + 8. + 0.6 * (bounds[2] - 16.), bounds[1] + 5.);
    runtime.pointer(x, y, 0);
    runtime.pointer(x, y, 1);
    assert_eq!(runtime.range_value(0), 0.5, "60 of 100 on a grid of 25");
    runtime.pointer(x, y, 2);
    runtime.sync_ui_bindings_native().unwrap();
    assert_eq!(state(&runtime)["level"], 50.0);

    // A page is never shorter than a tenth of the scale.
    let fine = SCALED_SLIDER
        .replace("LEVEL", "40")
        .replace("STEP", "step: 2;");
    let mut runtime = builtin(&fine);
    assert!(runtime.focus_control(0));
    runtime.range_key("ArrowRight");
    assert!((runtime.range_value(0) - 0.42).abs() < 1e-6);
    runtime.range_key("PageUp");
    assert!((runtime.range_value(0) - 0.52).abs() < 1e-6);
}

#[test]
fn a_slider_without_a_scale_still_treats_the_bound_number_as_a_fraction() {
    let mut runtime = builtin(&BOUND_SLIDER.replace("LEVEL", "0.25"));
    assert_eq!(runtime.range_value(0), 0.25);
    drag_to(&mut runtime, 0.5);
    assert_eq!(runtime.sync_ui_bindings_native(), Ok(true));
    assert_eq!(state(&runtime)["level"], 0.5);
}

#[test]
fn a_bound_value_outside_the_scale_is_shown_at_the_nearest_end() {
    for (level, expected) in [("40", 1.), ("-3", 0.)] {
        let runtime = builtin(&BOUND_SLIDER.replace("LEVEL", level));
        assert_eq!(
            runtime.range_value(0),
            expected,
            "{level} on the default scale"
        );
    }
    let source = SCALED_SLIDER.replace("LEVEL", "250").replace("STEP", "");
    assert_eq!(builtin(&source).range_value(0), 1.);
    // The state is not rewritten until the user moves the slider.
    assert_eq!(state(&builtin(&source))["level"], 250.0);
}

#[test]
fn a_scale_that_is_empty_or_reversed_falls_back_to_zero_to_one() {
    for scale in ["min: 5; max: 5;", "min: 10; max: 0;"] {
        let source = format!(
            "state level: Number = 0.5; Column {{ width: 240; height: 60; \
             Slider {{ key: 'level'; width: 216; height: 24; {scale} value <-> state.level; }} }}"
        );
        assert_eq!(builtin(&source).range_value(0), 0.5, "{scale}");
    }
}

#[test]
fn the_library_slider_takes_the_same_scale() {
    let runtime =
        library("Column { width: 320; height: 200; Slider { key: 'level'; value: 25%; } }")
            .unwrap();
    assert_eq!(runtime.range_value(0), 0.25);
    let mut runtime = library(
        "state level: Number = 40; Column { width: 320; height: 200; \
         Slider { key: 'level'; min: 0; max: 100; step: 10; value <-> state.level; } }",
    )
    .unwrap();
    assert!((runtime.range_value(0) - 0.4).abs() < 1e-6);
    drag_to(&mut runtime, 0.62);
    assert_eq!(runtime.sync_ui_bindings_native(), Ok(true));
    assert_eq!(state(&runtime)["level"], 60.0);
    // A value off the default scale used to fail with a message about a width.
    assert!(
        library(
            "state level: Number = 40; Column { width: 320; height: 200; \
             Slider { key: 'level'; value <-> state.level; } }"
        )
        .is_ok()
    );
}

#[test]
fn a_two_way_checked_binding_toggles_the_bound_state_on_activation() {
    let mut runtime = library(
        "state on: Bool = false; Column { width: 320; height: 200; \
         Switch { key: 'switch'; checked <-> state.on; } }",
    )
    .unwrap();
    click(&mut runtime, 0);
    assert_eq!(
        state(&runtime)["on"],
        false,
        "the write waits for the binding sync"
    );
    assert_eq!(runtime.sync_ui_bindings_native(), Ok(true));
    assert_eq!(state(&runtime)["on"], true);
    click(&mut runtime, 0);
    assert_eq!(runtime.sync_ui_bindings_native(), Ok(true));
    assert_eq!(state(&runtime)["on"], false);
    // Without an activation the sync has nothing to write.
    assert_eq!(runtime.sync_ui_bindings_native(), Ok(false));
}

#[test]
fn a_two_way_selected_binding_sets_the_bound_state_and_never_clears_it() {
    let mut runtime = library(
        "state pick: Bool = false; Column { width: 320; height: 200; \
         TabButton { key: 'tab'; selected <-> state.pick; } }",
    )
    .unwrap();
    click(&mut runtime, 0);
    assert_eq!(runtime.sync_ui_bindings_native(), Ok(true));
    assert_eq!(state(&runtime)["pick"], true);
    click(&mut runtime, 0);
    assert_eq!(
        runtime.sync_ui_bindings_native(),
        Ok(false),
        "already selected"
    );
    assert_eq!(state(&runtime)["pick"], true);
}

#[test]
fn a_radio_button_toggles_like_a_checkbox_because_nothing_groups_radios() {
    let mut runtime = library(
        "state a: Bool = false; state b: Bool = false; Column { width: 320; height: 200; \
         RadioButton { key: 'a'; checked <-> state.a; } \
         RadioButton { key: 'b'; checked <-> state.b; } }",
    )
    .unwrap();
    click(&mut runtime, 0);
    runtime.sync_ui_bindings_native().unwrap();
    click(&mut runtime, 1);
    runtime.sync_ui_bindings_native().unwrap();
    assert_eq!(
        state(&runtime),
        serde_json::json!({ "a": true, "b": true }),
        "both stay on"
    );
    click(&mut runtime, 0);
    runtime.sync_ui_bindings_native().unwrap();
    assert_eq!(
        state(&runtime)["a"],
        false,
        "a second click turns a radio off"
    );
}

/// F-10: the check mark of the library is a component named only as the value of a `match` arm.
/// Components were collected from child nodes alone, so it was never linked and a checked
/// checkbox did not load.
#[test]
fn a_checked_library_checkbox_loads_and_a_bound_one_survives_being_toggled() {
    let column = |control: &str| format!("Column {{ width: 320; height: 200; {control} }}");
    for control in [
        "Checkbox { key: 'c'; }",
        "Checkbox { key: 'c'; checked: true; }",
        "Checkbox { key: 'c'; indeterminate: true; }",
        "Switch { key: 'c'; checked: true; }",
        "RadioButton { key: 'c'; checked: true; }",
    ] {
        assert_eq!(
            library(&column(control)).map(|runtime| runtime.control_count()),
            Ok(1),
            "{control}"
        );
    }
    // The mark is drawn: a checked box paints something an empty one does not.
    let pixels = |control: &str| {
        let runtime = library(&column(control)).unwrap();
        let mut frame = vec![0; 320 * 200 * 4];
        runtime.paint_rgba(&mut frame, 320, 200, 1., true).unwrap();
        frame
    };
    assert_ne!(
        pixels("Checkbox { key: 'c'; checked: true; }"),
        pixels("Checkbox { key: 'c'; }"),
    );

    let mut runtime = library(&format!(
        "state on: Bool = false; {}",
        column("Checkbox { key: 'c'; checked <-> state.on; }")
    ))
    .unwrap();
    click(&mut runtime, 0);
    assert_eq!(runtime.sync_ui_bindings_native(), Ok(true));
    assert_eq!(state(&runtime)["on"], true);
    click(&mut runtime, 0);
    assert_eq!(runtime.sync_ui_bindings_native(), Ok(true));
    assert_eq!(state(&runtime)["on"], false);
}
