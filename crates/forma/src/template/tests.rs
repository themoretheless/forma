use super::*;

const SOURCE: &str = "component Button { Rectangle { radius: props.radius; background: Brush { color: props.background; hover: #a8baff; pressed: #6a83da; disabled: #596273; transition: 140ms; }; Border { width: 1; background: Brush { color: #bed0ff; focus: #ffffff; transition: 100ms; }; } Text { text: props.text; color: props.color; font.size: props.font.size; } PointerArea { clicked -> events.clicked(); } } }";
fn props() -> ButtonSpec {
    ButtonSpec {
        text: "Найти".into(),
        key: "run".into(),
        ..ButtonSpec::default()
    }
}
#[test]
fn component_defaults_and_explicit_instance_overrides() {
    let source = include_str!("../../examples/Button.component.ui");
    let scene = crate::markup::parse("component Demo { Frame { Button {} } }").unwrap();
    let t = parse(source, &scene.button).unwrap();
    assert_eq!((t.props.width, t.props.height), (100., 40.));
    assert_eq!(t.text.unwrap().text, "Кнопка");
    assert_eq!(t.border.unwrap().brush.duration_ms, 140.);
    let scene = crate::markup::parse("component Demo { Frame { Button { width: 60; fontSize: 22; text: 'Моя'; borderWidth: 0; hoverBackground: #123456; transitionDuration: 80ms; disabled: false; } } }").unwrap();
    let t = parse(
        &source.replace("disabled: false;", "disabled: true;"),
        &scene.button,
    )
    .unwrap();
    assert_eq!(t.props.width, 60.);
    assert!(!t.props.disabled);
    assert_eq!(t.text.as_ref().unwrap().font_size, 22.);
    assert_eq!(t.text.unwrap().text, "Моя");
    assert_eq!(t.border.unwrap().width, 0.);
    let fill = t.fill.unwrap();
    assert_eq!(fill.hover, Some([18, 52, 86, 255]));
    assert_eq!(fill.duration_ms, 80.);
}

#[test]
fn invalid_component_defaults_are_diagnostics() {
    for fields in [
        "width: 0;",
        "width: 10; width: 20;",
        "fontSize: 12; font.size: 14;",
        "width: props.width;",
        "unknown: 1;",
        "disabled: 1;",
        "transitionDuration: 20;",
    ] {
        assert!(
            parse(
                &format!("component Button {{ {fields} Rectangle {{}} }}"),
                &props()
            )
            .is_err(),
            "{fields}"
        );
    }
}
fn body(source: &str) -> Result<Template, String> {
    parse(
        &format!("component Button {{ Rectangle {{ {source} }} }}"),
        &props(),
    )
}
#[test]
fn shape_points_boundaries_and_string_escapes() {
    let shape = |points: &str| {
        body(&format!(
            "ContentShape {{ points: '{points}'; color: #ffffff; }}"
        ))
    };
    assert_eq!(
        shape("0 0 10 0 10 10").unwrap().content[0],
        Content::Shape {
            points: vec![[0., 0.], [10., 0.], [10., 10.]],
            color: [255; 4]
        }
    );
    assert_eq!(
        shape("-1.5 -2.25 3 4 5.125 6").unwrap().content[0],
        Content::Shape {
            points: vec![[-1.5, -2.25], [3., 4.], [5.125, 6.]],
            color: [255; 4]
        }
    );
    // Tab and newline separators must not change the accepted grammar.
    assert_eq!(
        shape("0 0\t10\t0\n10 10").unwrap().content[0],
        shape("0 0 10 0 10 10").unwrap().content[0]
    );
    for bad in [
        "0 0 10 0",
        "0 0 10 0 10",
        "0 0 10 0 10 x",
        "0 0 10 0 10 100001",
        "0 0 10 0 10 nan",
        &(0..8194)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(" "),
    ] {
        assert!(shape(bad).is_err(), "{bad}");
    }
    let big = shape(
        &(0..8192)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(" "),
    )
    .unwrap();
    let Content::Shape { points, .. } = &big.content[0] else {
        panic!("expected a shape")
    };
    assert_eq!(points.len(), 4096);
    // Escaped literals keep working; the escape-free path borrows the source.
    let text = body(
        r#"ContentText { x: 0; y: 0; width: 10; height: 10; text: 'a\'b\nc'; color: #ffffff; }"#,
    )
    .unwrap();
    assert_eq!(
        text.content[0],
        Content::Text {
            bounds: [0., 0., 10., 10.],
            text: Text {
                text: "a'b\nc".into(),
                color: [255; 4],
                font_size: 16.
            }
        }
    );
    assert!(
        body(
            r#"ContentText { x: 0; y: 0; width: 10; height: 10; text: 'unclosed; color: #ffffff; }"#
        )
        .is_err()
    );
    assert!(body("ContentShape { points: \"0 0 1 0 1 1\"; color: #ffffff; }").is_ok());
}

#[test]
fn brushes_are_property_values_with_color_shorthand() {
    let t = body("background: #123456; Border { background: #ffffff; }").unwrap();
    assert_eq!(t.fill.unwrap().color, [18, 52, 86, 255]);
    assert_eq!(t.border.unwrap().brush.color, [255; 4]);
    assert!(body("background: Brush { color:#fff; }").is_err());
    assert!(body("Border { Brush { color:#fff; } }").is_err());
}
#[test]
fn reveal_is_an_optional_independent_primitive_with_validated_fields() {
    let t = body("Reveal {}").unwrap();
    assert_eq!(t.reveal, Some(crate::reveal::Reveal::default()));
    assert!(!t.clickable);
    let t = body("Reveal { width:2; color:#abcdef80; radius:100px; stop:0.5; activeRadius:1200; activeStop:1; transition:0ms; }").unwrap();
    let r = t.reveal.unwrap();
    assert_eq!(r.width, 2.);
    assert_eq!(r.color, [171, 205, 239, 128]);
    assert_eq!(
        (
            r.radius,
            r.stop,
            r.active_radius,
            r.active_stop,
            r.duration_ms
        ),
        (100., 0.5, 1200., 1., 0.)
    );
    for bad in [
        "stop:0;",
        "stop:1.1;",
        "activeStop:60px;",
        "radius:0;",
        "width:-1;",
        "transition:280;",
        "unknown:1;",
        "color:#ff0000; color:#ffffff;",
    ] {
        assert!(body(&format!("Reveal {{ {bad} }}")).is_err(), "{bad}");
    }
    assert!(body("Reveal {} Reveal {}").is_err());
}

#[test]
fn evaluates_the_complete_primitive_template() {
    let props = props();
    let template = parse(SOURCE, &props).unwrap();
    assert_eq!(template.radius, props.radius);
    assert!(template.clickable);
    let fill = template.fill.unwrap();
    assert_eq!(fill.color, props.background);
    assert_eq!(fill.hover, Some([168, 186, 255, 255]));
    assert_eq!(fill.pressed, Some([106, 131, 218, 255]));
    assert_eq!(fill.disabled, Some([89, 98, 115, 255]));
    assert_eq!(fill.duration_ms, 140.);
    assert_eq!(fill.focus, None);
    let border = template.border.unwrap();
    assert_eq!(border.width, 1.);
    assert_eq!(border.brush.color, [190, 208, 255, 255]);
    assert_eq!(border.brush.focus, Some([255; 4]));
    assert_eq!(border.brush.duration_ms, 100.);
    let text = template.text.unwrap();
    assert_eq!(text.text, "Найти");
    assert_eq!(text.color, props.color);
    assert_eq!(text.font_size, props.font_size);
}

#[test]
fn literals_comments_and_css_colors_work() {
    let template = body("// shape\nradius: 12px; /* a /* nested */ comment */ background: Brush { color: #AbC; hover: #12345678; transition: 0ms; }; Text { text: \"Кнопка \\\"ОК\\\"\\n\"; font.size: 2e1; color: #000; }").unwrap();
    assert_eq!(template.radius, 12.);
    assert_eq!(template.fill.as_ref().unwrap().color, [170, 187, 204, 255]);
    assert_eq!(template.fill.unwrap().hover, Some([18, 52, 86, 120]));
    assert_eq!(template.text.as_ref().unwrap().text, "Кнопка \"ОК\"\n");
    assert_eq!(template.text.unwrap().font_size, 20.);
}

#[test]
fn omitted_primitives_are_absent_and_do_not_get_painted() {
    assert_eq!(
        body("").unwrap(),
        Template {
            props: props(),
            content: Vec::new(),
            inputs: Vec::new(),
            range: None,
            radius: 0.,
            fill: None,
            border: None,
            reveal: None,
            text: None,
            clickable: false
        }
    );
    assert!(!body("PointerArea {}").unwrap().clickable);
    assert!(
        body("PointerArea { clicked -> events.clicked(); }")
            .unwrap()
            .clickable
    );
    let border = body("Border { background: Brush { color: #fff; }; }")
        .unwrap()
        .border
        .unwrap();
    assert_eq!(border.width, 1.);
    assert_eq!(border.brush.duration_ms, 0.);
}

#[test]
fn references_are_typed_and_re_evaluated_for_each_instance() {
    let mut props = props();
    props.text = "Другая кнопка".into();
    props.radius = 8.;
    let template = parse(SOURCE, &props).unwrap();
    assert_eq!(template.text.unwrap().text, "Другая кнопка");
    assert_eq!(template.radius, 8.);
    assert_eq!(
        body("Text { text: props.key; }")
            .unwrap()
            .text
            .unwrap()
            .text,
        "run"
    );
    for source in [
        "radius: props.color;",
        "radius: state.radius;",
        "radius: props.missing;",
        "Text { text: props.radius; }",
        "background: Brush { color: props.text; };",
        "background: Brush { color: state.background; };",
        "Text { font.size: props.font.weight; }",
        "Text { text: props.text(); }",
    ] {
        assert!(body(source).is_err(), "accepted {source}");
    }
}

#[test]
fn rejects_duplicate_primitives_properties_and_missing_required_values() {
    for source in [
        "background: Brush { color:#fff; }; background: Brush { color:#000; };",
        "Text {} Text {}",
        "Border { background: Brush {color:#fff;}; } Border { background: Brush {color:#fff;}; }",
        "PointerArea {} PointerArea {}",
        "radius:1; radius:2;",
        "background: Brush { color:#fff; color:#000; };",
        "background: Brush { color:#fff; hover:#fff; hover:#000; };",
        "Border { width:1; width:2; background: Brush {color:#fff;}; }",
        "Border { background: Brush {color:#fff;}; background: Brush {color:#000;}; }",
        "Text { text:'a'; text:'b'; }",
        "background: Brush {};",
        "Border {}",
    ] {
        assert!(body(source).is_err(), "accepted {source}");
    }
}

#[test]
fn rejects_nonfinite_negative_oversize_and_wrong_unit_values() {
    for value in ["-1", "4097", "1e999", "NaN", "20ms", "10%", "*"] {
        assert!(
            body(&format!("radius:{value};")).is_err(),
            "accepted {value}"
        );
    }
    for value in ["-1ms", "2001ms", "140", "1px", "1s", "props.radius"] {
        assert!(
            body(&format!(
                "background: Brush {{ color:#fff; transition:{value}; }};"
            ))
            .is_err(),
            "accepted {value}"
        );
    }
    assert!(
        body(
            "radius:4096; Border { width:0; background: Brush { color:#fff; transition:2000ms; }; }"
        )
        .is_ok()
    );
    assert!(body("Text { font.size:0; }").is_err());
    assert!(body("Border { width:-1; background: Brush {color:#fff;}; }").is_err());
    let mut props = props();
    props.radius = f32::NAN;
    assert!(parse(SOURCE, &props).is_err());
    props.radius = 1.;
    props.font_size = f32::INFINITY;
    assert!(parse(SOURCE, &props).is_err());
}

#[test]
fn events_only_forward_the_declared_component_event() {
    for event in [
        "hovered -> events.clicked();",
        "clicked -> actions.search();",
        "clicked -> events.other();",
        "clicked -> events.clicked(1);",
        "clicked -> events.clicked(); clicked -> events.clicked();",
        "clicked => events.clicked();",
        "clicked: true;",
    ] {
        assert!(
            body(&format!("PointerArea {{ {event} }}")).is_err(),
            "accepted {event}"
        );
    }
}

#[test]
fn unsupported_primitives_and_fields_are_not_silent() {
    for source in [
        "Rectangle {}",
        "Frame {}",
        "width:200;",
        "Brush { color:#fff; }",
        "background: Brush { color:#fff; easing:'spring'; };",
        "Border { radius:4; background: Brush {color:#fff;}; }",
        "Text { font.weight:700; }",
        "PointerArea { enabled:true; }",
    ] {
        let error = body(source).unwrap_err();
        assert!(error.contains("Unsupported"), "{source}: {error}");
    }
}

#[test]
fn rejects_malformed_documents_and_colors() {
    for source in [
        "",
        "component Other { Rectangle {} }",
        "component Button {}",
        "component Button { Rectangle {} Rectangle {} }",
        "component Button { Rectangle {} } junk",
        "component Button { Rectangle {",
        "/* unclosed",
        "component Button { Rectangle {} } /* unclosed",
    ] {
        let error = parse(source, &props()).unwrap_err();
        assert!(error.contains("byte"), "no location: {error}");
    }
    for color in ["#ab", "#abcd", "#gggggg", "'#fff'", "rgb(0,0,0)"] {
        assert!(
            body(&format!("background: Brush {{ color:{color}; }};")).is_err(),
            "accepted {color}"
        );
    }
    assert!(body("Text { text:'unclosed; }").is_err());
    assert!(body("Text { text:'x' }").is_err());
}
