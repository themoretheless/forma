use super::*;
fn expr(path: &str) -> Value {
    Value::Expr(path.into())
}
fn string(value: &str) -> Value {
    Value::Text(value.into())
}
fn binary(op: &str, left: Value, right: Value) -> Value {
    Value::Expression(Box::new(Expression::Binary(op.into(), left, right)))
}
fn node(kind: &str, properties: Vec<(&str, Value)>, children: Vec<Node>) -> Node {
    Node {
        kind: kind.into(),
        props: props(properties),
        children,
        action: None,
    }
}
fn document(root: Node) -> Document {
    Document {
        name: "Test".into(),
        root,
        definitions: BTreeMap::from([(
            "Control".into(),
            Definition {
                defaults: props(vec![
                    ("width", number(80.)),
                    ("height", number(30.)),
                    ("text", string("")),
                    ("disabled", Value::Bool(false)),
                ]),
                nodes: vec![node(
                    "Rectangle",
                    vec![],
                    vec![
                        node("Text", vec![("text", expr("props.text"))], vec![]),
                        Node {
                            action: Some("events.clicked".into()),
                            ..node("PointerArea", vec![], vec![])
                        },
                    ],
                )],
            },
        )]),
        fallback: Properties::new(),
        images: BTreeMap::new(),
        states: vec![],
    }
}
fn root(children: Vec<Node>) -> Node {
    node(
        "Frame",
        vec![
            ("width", number(300.)),
            ("height", number(120.)),
            ("padding", number(0.)),
        ],
        children,
    )
}
fn record(id: &str, title: &str) -> Value {
    Value::Object(props(vec![("id", string(id)), ("title", string(title))]))
}
#[test]
fn pure_expressions_short_circuit_and_interpolate() {
    let state = props(vec![
        ("count", number(3.)),
        ("busy", Value::Bool(false)),
        ("item", record("a", "Title")),
    ]);
    let empty = Properties::new();
    assert_eq!(
        binary("&&", Value::Bool(false), expr("state.missing"))
            .evaluate(&empty, &state)
            .unwrap(),
        Value::Bool(false)
    );
    assert_eq!(
        binary("??", Value::Null, number(4.))
            .evaluate(&empty, &state)
            .unwrap(),
        number(4.)
    );
    assert_eq!(
        Value::Interpolation(vec![
            string("Найдено: "),
            binary("+", expr("state.count"), number(2.))
        ])
        .evaluate(&empty, &state)
        .unwrap(),
        string("Найдено: 5")
    );
    assert_eq!(
        expr("item.title").evaluate(&empty, &state).unwrap(),
        string("Title")
    );
    assert!(
        binary("/", number(1.), number(0.))
            .evaluate(&empty, &state)
            .is_err()
    );
    assert!(
        binary("+", string("a"), number(1.))
            .evaluate(&empty, &state)
            .is_err()
    );
    let optional = Value::Expression(Box::new(Expression::Member(
        Value::Null,
        string("title"),
        true,
    )));
    assert_eq!(optional.evaluate(&empty, &state).unwrap(), Value::Null);
    let computed = Value::Expression(Box::new(Expression::Member(
        expr("state.missing"),
        expr("state.absent_index"),
        true,
    )));
    assert_eq!(computed.evaluate(&empty, &state).unwrap(), Value::Null);
    assert!(
        binary(
            "??",
            binary("+", expr("state.missing"), number(1.)),
            number(42.)
        )
        .evaluate(&empty, &state)
        .is_err()
    );
}
#[test]
fn standalone_primitive_wrappers_keep_nested_nodes_primitive() {
    let mut doc = document(root(vec![
        node("Text", vec![("text", expr("state.caption"))], vec![]),
        node("TextInput", vec![("value", expr("state.value"))], vec![]),
    ]));
    doc.definitions.insert(
        "Text".into(),
        Definition {
            defaults: props(vec![
                ("text", string("")),
                ("width", number(80.)),
                ("height", number(30.)),
            ]),
            nodes: vec![node(
                "Rectangle",
                vec![],
                vec![node("Text", vec![("text", expr("props.text"))], vec![])],
            )],
        },
    );
    doc.definitions.insert(
        "TextInput".into(),
        Definition {
            defaults: props(vec![
                ("value", string("")),
                ("width", number(80.)),
                ("height", number(30.)),
            ]),
            nodes: vec![node(
                "Rectangle",
                vec![],
                vec![
                    node("TextInput", vec![("value", expr("props.value"))], vec![]),
                    Node {
                        action: Some("events.clicked".into()),
                        ..node("PointerArea", vec![], vec![])
                    },
                ],
            )],
        },
    );
    doc.states = vec![props(vec![
        ("caption", string("First")),
        ("value", string("seed")),
    ])];
    let (source, templates, mapping) = doc.render_scene().unwrap();
    assert_eq!(mapping.len(), 2);
    assert!(templates.contains("ContentText"));
    assert!(templates.contains("ContentInput"));
    let runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
    assert_eq!(runtime.text_value(1), "seed");
    doc.states[0].insert("caption".into(), string("Second"));
    assert!(doc.render().unwrap().1.contains("Second"));
}
#[test]
fn contracts_check_required_properties_and_enum_members() {
    let mut doc = document(root(vec![]));
    doc.fallback.insert(
        "title".into(),
        string("Engine fallback does not satisfy required"),
    );
    let definition = doc.definitions.get_mut("Control").unwrap();
    definition.defaults.insert(
        "__contracts".into(),
        Value::Object(props(vec![
            (
                "title",
                Value::Object(props(vec![
                    ("type", string("String")),
                    ("required", Value::Bool(true)),
                ])),
            ),
            ("tone", Value::Object(props(vec![("type", string("Tone"))]))),
        ])),
    );
    definition.defaults.insert(
        "__enums".into(),
        Value::Object(props(vec![(
            "Tone",
            Value::List(vec![string("Accent"), string("Danger")]),
        )])),
    );
    definition
        .defaults
        .insert("tone".into(), expr("Tone.Accent"));
    assert!(
        doc.instantiate(
            &node("Control", vec![], vec![]),
            true,
            &Properties::new(),
            &[]
        )
        .unwrap_err()
        .contains("required")
    );
    let valid = node(
        "Control",
        vec![("title", string("Hello")), ("tone", expr("Tone.Danger"))],
        vec![],
    );
    assert_eq!(
        doc.instantiate(&valid, true, &Properties::new(), &[])
            .unwrap()
            .1["tone"],
        string("Tone.Danger")
    );
    let bad = node(
        "Control",
        vec![("title", string("Hello")), ("tone", expr("Tone.Wrong"))],
        vec![],
    );
    assert!(
        doc.instantiate(&bad, true, &Properties::new(), &[])
            .unwrap_err()
            .contains("Unknown enum variant")
    );
}
#[test]
fn root_scope_reacts_and_reaches_nested_scene_controls() {
    let mut doc = document(root(vec![node(
        "Control",
        vec![("text", expr("props.caption"))],
        vec![],
    )]));
    doc.root.props.insert(
        "__scope".into(),
        Value::Object(props(vec![(
            "caption",
            Value::Interpolation(vec![string("Count: "), expr("state.count")]),
        )])),
    );
    doc.states = vec![props(vec![("count", number(2.))])];
    assert!(doc.render().unwrap().0.contains("Count: 2"));
    doc.states[0].insert("count".into(), number(4.));
    assert!(doc.render().unwrap().0.contains("Count: 4"));
}
#[test]
fn grouped_match_keeps_explicit_properties_last() {
    let node = node(
        "Text",
        vec![
            (
                "__matches",
                Value::List(vec![Value::Match(
                    Box::new(expr("state.danger")),
                    vec![
                        (
                            Value::Bool(true),
                            Value::Object(props(vec![
                                ("color", string("#ff0000")),
                                ("text", expr("state.missing")),
                            ])),
                        ),
                        (
                            expr("_"),
                            Value::Object(props(vec![
                                ("color", string("#00ff00")),
                                ("text", string("OK")),
                            ])),
                        ),
                    ],
                )]),
            ),
            ("text", string("Override")),
        ],
        vec![],
    );
    let resolved = properties(
        &node,
        &Properties::new(),
        &props(vec![("danger", Value::Bool(true))]),
    )
    .unwrap();
    assert_eq!(resolved["color"], string("#ff0000"));
    assert_eq!(resolved["text"], string("Override"));
}
#[test]
fn sibling_control_snapshots_share_root_until_mutated() {
    let files = BTreeMap::from([("App.ui".into(), "component App { state query: String = 'seed'; state payload: List = []; Column { TextInput { key:'a'; value <-> state.query; } TextInput { key:'b'; value <-> state.query; } } }".into())]);
    let payload = Value::List((0..256).map(|i| string(&format!("payload-{i}"))).collect());
    let mut doc =
        Document::from_ui_files(&files, "App.ui", &props(vec![("payload", payload)])).unwrap();
    let (_, _, mut controls) = doc.render_scene().unwrap();
    assert_eq!(controls.len(), 2);
    assert!(
        controls[0].state.shares_storage(&controls[1].state),
        "siblings must share the expanded root map"
    );
    let before = serde_json::to_string(&controls[1]).unwrap();
    controls[0]
        .state
        .insert("query".into(), string("snapshot-edit"));
    doc.states[0].insert("query".into(), string("model-edit"));
    assert_eq!(serde_json::to_string(&controls[1]).unwrap(), before);
    assert_eq!(controls[0].state["query"], string("snapshot-edit"));
    assert_eq!(doc.states[0]["query"], string("model-edit"));
}

#[test]
fn shared_property_snapshots_detach_on_every_mutable_access() {
    let mut original = props(vec![
        (
            "object",
            Value::Object(props(vec![("caption", string("before"))])),
        ),
        ("list", Value::List(vec![string("first")])),
    ]);
    let snapshot = original.clone();
    assert!(original.shares_storage(&snapshot));
    let expected_json = serde_json::to_string(&snapshot).unwrap();
    let Some(Value::Object(object)) = original.get_mut("object") else {
        panic!("object")
    };
    object.insert("caption".into(), string("after"));
    let Some(Value::List(list)) = original.get_mut("list") else {
        panic!("list")
    };
    list.push(string("second"));
    assert!(!original.shares_storage(&snapshot));
    assert_eq!(serde_json::to_string(&snapshot).unwrap(), expected_json);
    assert_ne!(original, snapshot);
    let restored: Properties = serde_json::from_str(&expected_json).unwrap();
    assert_eq!(restored, snapshot);
    let mut detached = snapshot.clone();
    for (_, value) in &mut detached {
        *value = Value::Null;
    }
    assert_eq!(serde_json::to_string(&snapshot).unwrap(), expected_json);
    let consumed: Properties = snapshot.clone().into_iter().collect();
    assert_eq!(consumed, snapshot);
}

#[test]
fn scene_default_lookup_preserves_overlay_and_missing_state_evaluation() {
    let mut doc = document(root(vec![]));
    doc.fallback.insert("seed".into(), string("fallback"));
    doc.fallback.insert("text".into(), expr("props.seed"));
    let definition = doc.definitions.get_mut("Control").unwrap();
    definition
        .defaults
        .insert("seed".into(), string("definition"));
    definition
        .defaults
        .insert("text".into(), expr("props.seed"));
    let instance = node("Control", vec![("text", expr("state.absent"))], vec![]);
    let resolved = doc
        .scene_properties(&instance, &Properties::new(), &Properties::new())
        .unwrap();
    assert_eq!(resolved["text"], string("definition"));
    let plain = node("Text", vec![("text", expr("state.absent"))], vec![]);
    assert_eq!(
        doc.scene_properties(&plain, &Properties::new(), &Properties::new())
            .unwrap()["text"],
        string("fallback")
    );
    let state = props(vec![("absent", string("live"))]);
    assert_eq!(
        doc.scene_properties(&instance, &Properties::new(), &state)
            .unwrap()["text"],
        string("live")
    );
}

#[test]
fn definition_matches_select_from_one_property_snapshot() {
    let mut doc = document(root(vec![]));
    let definition = doc.definitions.get_mut("Control").unwrap();
    definition
        .defaults
        .insert("flag".into(), Value::Bool(false));
    definition.defaults.insert(
        "__matches".into(),
        Value::List(vec![
            Value::Match(
                Box::new(Value::Bool(true)),
                vec![(
                    expr("_"),
                    Value::Object(props(vec![("flag", Value::Bool(true))])),
                )],
            ),
            Value::Match(
                Box::new(expr("props.flag")),
                vec![
                    (
                        Value::Bool(true),
                        Value::Object(props(vec![("text", string("changed"))])),
                    ),
                    (
                        expr("_"),
                        Value::Object(props(vec![("text", string("original"))])),
                    ),
                ],
            ),
        ]),
    );
    let (_, resolved) = doc
        .instantiate(
            &node("Control", vec![], vec![]),
            true,
            &Properties::new(),
            &[],
        )
        .unwrap();
    assert_eq!(resolved["text"], string("original"));
    let (_, explicit) = doc
        .instantiate(
            &node(
                "Control",
                vec![("flag", Value::Bool(false)), ("text", string("explicit"))],
                vec![],
            ),
            true,
            &Properties::new(),
            &[],
        )
        .unwrap();
    assert_eq!(explicit["flag"], Value::Bool(false));
    assert_eq!(explicit["text"], string("explicit"));
    let (_, metadata) = doc
        .instantiate(
            &node(
                "Control",
                vec![
                    ("flag", Value::Bool(false)),
                    ("text", expr("props.__state.caption")),
                    (
                        "__state",
                        Value::Object(props(vec![("caption", string("snapshot"))])),
                    ),
                ],
                vec![],
            ),
            true,
            &Properties::new(),
            &[],
        )
        .unwrap();
    assert_eq!(metadata["text"], string("snapshot"));
}
#[test]
fn keyed_collection_reorders_focus_and_evaluates_event_arguments() {
    let mut doc = document(root(vec![node(
        "For",
        vec![
            ("item", string("item")),
            ("collection", expr("state.items")),
            ("key", expr("item.id")),
        ],
        vec![node(
            "Control",
            vec![
                ("text", expr("item.title")),
                (
                    "__event_args",
                    Value::Object(props(vec![("clicked", Value::List(vec![expr("item.id")]))])),
                ),
            ],
            vec![],
        )],
    )]));
    doc.states = vec![props(vec![(
        "items",
        Value::List(vec![record("a", "Alpha"), record("b", "Beta")]),
    )])];
    let (source, templates, first) = doc.render_scene().unwrap();
    assert_eq!(first.len(), 2);
    assert_eq!(first[0].node, 2);
    assert_eq!(first[0].event_args["clicked"], vec![string("a")]);
    let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
    runtime.focus_control(0);
    doc.states[0].insert(
        "items".into(),
        Value::List(vec![record("b", "Beta"), record("a", "Alpha")]),
    );
    let (source, templates, second) = doc.render_scene().unwrap();
    assert_eq!(first[0].key, second[1].key);
    assert_eq!(second[1].state["__index.item"], number(1.));
    let mut next = crate::Runtime::from_sources(&source, &templates).unwrap();
    next.preserve_binding_interaction(&runtime);
    assert_eq!(next.focused_index(), 1);
    doc.states[0].insert(
        "items".into(),
        Value::List(vec![record("a", "Alpha"), record("a", "Again")]),
    );
    assert!(
        doc.render_scene()
            .unwrap_err()
            .contains("Duplicate collection key")
    );
}
#[test]
fn collection_binding_activation_follows_key_after_reorder_or_removal() {
    for (remove, cached) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut doc = document(root(vec![node(
            "For",
            vec![
                ("item", string("item")),
                ("collection", expr("state.items")),
                ("key", expr("item.id")),
            ],
            vec![node(
                "Control",
                vec![
                    ("text", expr("item.title")),
                    (
                        "__bindings",
                        Value::Object(props(vec![("checked", string("item.done"))])),
                    ),
                ],
                vec![],
            )],
        )]));
        let item = |id: &str| {
            Value::Object(props(vec![
                ("id", string(id)),
                ("title", string(id)),
                ("done", Value::Bool(false)),
            ]))
        };
        doc.states = vec![props(vec![(
            "items",
            Value::List(vec![item("a"), item("b")]),
        )])];
        let (source, templates, controls) = doc.render_scene().unwrap();
        let runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
        let items = if remove {
            vec![item("b")]
        } else {
            vec![item("b"), item("a")]
        };
        doc.states[0].insert("items".into(), Value::List(items));
        let changed = if cached {
            doc.sync_runtime_bindings_from_scene(&runtime, Some(0), &controls)
                .unwrap()
        } else {
            doc.sync_runtime_bindings(&runtime, Some(0)).unwrap()
        };
        assert_eq!(changed, if remove { 0 } else { 1 });
        let repeated = doc
            .sync_runtime_binding_events(&runtime, &[0, 0], &controls)
            .unwrap();
        assert_eq!(repeated, if remove { 0 } else { 2 });
        let Value::List(items) = &doc.states[0]["items"] else {
            panic!("items");
        };
        let Value::Object(b) = &items[0] else {
            panic!("b");
        };
        assert_eq!(
            b["done"],
            Value::Bool(false),
            "the old first index must not target b"
        );
        if !remove {
            let Value::Object(a) = &items[1] else {
                panic!("a");
            };
            assert_eq!(a["done"], Value::Bool(true));
        }
    }
}
#[test]
fn collection_editor_values_follow_keys_and_do_not_overwrite_new_items() {
    for (mode, cached) in (0..4).flat_map(|mode| [(mode, false), (mode, true)]) {
        let mut doc = document(root(vec![node(
            "For",
            vec![
                ("item", string("item")),
                ("collection", expr("state.items")),
                ("key", expr("item.id")),
            ],
            vec![node(
                "Control",
                vec![
                    ("value", expr("item.title")),
                    (
                        "__bindings",
                        Value::Object(props(vec![("value", string("item.title"))])),
                    ),
                ],
                vec![],
            )],
        )]));
        let definition = doc.definitions.get_mut("Control").unwrap();
        definition.defaults.insert("value".into(), string(""));
        definition.nodes = vec![node(
            "Rectangle",
            vec![],
            vec![node(
                "TextInput",
                vec![("value", expr("props.value"))],
                vec![],
            )],
        )];
        doc.states = vec![props(vec![(
            "items",
            Value::List(vec![record("a", "Alpha"), record("b", "Beta")]),
        )])];
        let (source, templates, controls) = doc.render_scene().unwrap();
        let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
        runtime.set_binding_value(0, "edited-a", true).unwrap();
        let items = match mode {
            0 => vec![record("b", "Beta"), record("a", "Alpha")],
            1 => vec![record("b", "Beta")],
            2 => vec![
                record("c", "New"),
                record("b", "model-b"),
                record("a", "Alpha"),
            ],
            _ => vec![record("b", "Beta"), record("b", "Duplicate")],
        };
        doc.states[0].insert("items".into(), Value::List(items));
        let before = doc.states.clone();
        let result = if cached {
            doc.sync_runtime_bindings_from_scene(&runtime, None, &controls)
        } else {
            doc.sync_runtime_bindings(&runtime, None)
        };
        if mode == 3 {
            assert!(result.unwrap_err().contains("Duplicate collection"));
            assert_eq!(doc.states, before);
            continue;
        }
        result.unwrap();
        let Value::List(items) = &doc.states[0]["items"] else {
            panic!("items");
        };
        for item in items {
            let Value::Object(item) = item else {
                panic!("record");
            };
            let expected = match item["id"].text().unwrap().as_str() {
                "a" => "edited-a",
                "b" => {
                    if mode == 2 {
                        "model-b"
                    } else {
                        "Beta"
                    }
                }
                "c" => "New",
                _ => unreachable!(),
            };
            assert_eq!(item["title"], string(expected));
        }
    }
}
#[test]
fn ambiguous_scene_and_runtime_binding_keys_refuse_before_writing() {
    let files = BTreeMap::from([("App.ui".to_string(),
            "component App { state a: String = 'one'; state b: String = 'two'; Column { width:260; height:100; TextInput { key:'a'; width:200; height:32; value <-> state.a; } TextInput { key:'b'; width:200; height:32; value <-> state.b; } } }".to_string())]);
    let mut doc = Document::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
    let (source, templates, controls) = doc.render_scene().unwrap();
    let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
    runtime.set_binding_value(0, "edited", true).unwrap();
    let before = doc.states.clone();
    let mut ambiguous = controls.clone();
    ambiguous[1].key = ambiguous[0].key.clone();
    assert!(
        doc.sync_runtime_bindings_from_scene(&runtime, None, &ambiguous)
            .unwrap_err()
            .contains("Ambiguous binding")
    );
    assert_eq!(doc.states, before);
    let source = source.replace("key:'b'", "key:'a'");
    assert!(
        crate::Runtime::from_sources(&source, &templates)
            .err()
            .unwrap()
            .contains("Duplicate control key")
    );
    assert_eq!(doc.states, before);
}
#[test]
fn unchanged_widgets_do_not_overwrite_newer_model_values() {
    let files = BTreeMap::from([("App.ui".to_string(),
            "component App { state a: String = 'one'; state b: String = 'two'; Column { width:260; height:100; TextInput { width:200; height:32; value <-> state.a; } TextInput { width:200; height:32; value <-> state.b; } } }".to_string())]);
    let mut doc = Document::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
    let (source, templates, _) = doc.render_scene().unwrap();
    let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
    runtime.set_binding_value(0, "edited", true).unwrap();
    doc.states[0].insert("a".into(), string("concurrent-model-a"));
    doc.states[0].insert("b".into(), string("new-model-b"));
    assert_eq!(doc.sync_runtime_bindings(&runtime, None).unwrap(), 1);
    assert_eq!(doc.states[0]["a"], string("edited"));
    assert_eq!(doc.states[0]["b"], string("new-model-b"));
    // A committed widget must not replay its value in a later model update.
    doc.states[0].insert("a".into(), string("new-model-a"));
    assert_eq!(doc.sync_runtime_bindings(&runtime, None).unwrap(), 0);
    assert_eq!(doc.states[0]["a"], string("new-model-a"));
}
#[test]
fn unchanged_range_preserves_model_and_committed_input_does_not_replay() {
    let mut doc = document(root(vec![node(
        "Control",
        vec![
            ("value", expr("state.level")),
            (
                "__bindings",
                Value::Object(props(vec![("value", string("state.level"))])),
            ),
        ],
        vec![],
    )]));
    doc.states = vec![props(vec![("level", number(0.5))])];
    let definition = doc.definitions.get_mut("Control").unwrap();
    definition.defaults.insert("value".into(), number(0.5));
    let range = node(
        "RangeInput",
        vec![("value", expr("props.value"))],
        vec![
            node(
                "Minimum",
                vec![],
                vec![node(
                    "ContentShape",
                    vec![
                        ("points", string("0 0 10 0 10 10")),
                        ("color", string("#ffffff")),
                    ],
                    vec![],
                )],
            ),
            node(
                "Maximum",
                vec![],
                vec![node(
                    "ContentShape",
                    vec![
                        ("points", string("0 0 80 0 80 10")),
                        ("color", string("#ffffff")),
                    ],
                    vec![],
                )],
            ),
        ],
    );
    definition.nodes[0].children.remove(0);
    definition.nodes[0].children.insert(0, range);
    let (source, templates, _) = doc.render_scene().unwrap();
    let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
    doc.states[0].insert("level".into(), number(0.8));
    assert_eq!(doc.sync_runtime_bindings(&runtime, None).unwrap(), 0);
    assert_eq!(doc.states[0]["level"], number(0.8));
    assert!(runtime.focus_control(0));
    assert!(runtime.range_key("ArrowRight"));
    let edited = runtime.range_value(0) as f64;
    assert_eq!(doc.sync_runtime_bindings(&runtime, None).unwrap(), 1);
    assert_eq!(doc.states[0]["level"], number(edited));
    doc.states[0].insert("level".into(), number(0.7));
    assert_eq!(doc.sync_runtime_bindings(&runtime, None).unwrap(), 0);
    assert_eq!(doc.states[0]["level"], number(0.7));
}
#[test]
fn collection_key_changes_revalidate_following_bindings() {
    let files = BTreeMap::from([("App.ui".to_string(),
            "component App { state items: List = []; Column { width:260; height:100; for item in state.items key item.id { TextInput { width:200; height:32; value <-> item.id; } } } }".to_string())]);
    let state = props(vec![(
        "items",
        Value::List(vec![
            Value::Object(props(vec![("id", string("a"))])),
            Value::Object(props(vec![("id", string("b"))])),
        ]),
    )]);
    let mut doc = Document::from_ui_files(&files, "App.ui", &state).unwrap();
    let (source, templates, _) = doc.render_scene().unwrap();
    let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
    let before = doc.states.clone();
    runtime.set_binding_value(0, "b", true).unwrap();
    assert!(
        doc.sync_runtime_bindings(&runtime, None)
            .unwrap_err()
            .contains("Duplicate collection")
    );
    assert_eq!(
        doc.states, before,
        "later lookup must not use the pre-write key index"
    );
    runtime.set_binding_value(0, "a-new", true).unwrap();
    assert_eq!(doc.sync_runtime_bindings(&runtime, None).unwrap(), 1);
    let Value::List(items) = &doc.states[0]["items"] else {
        panic!("items");
    };
    let Value::Object(first) = &items[0] else {
        panic!("item");
    };
    assert_eq!(first["id"], string("a-new"));
}
#[test]
fn collection_indices_handle_distinct_outer_local_scopes() {
    for groups in [2, 10] {
        let files = BTreeMap::from([("App.ui".to_string(),
            "component App { state items: List = []; state groups: List = []; Column { width:260; height:220; for outer in state.groups key outer.id { for item in state.items key outer.id + item.id { TextInput { width:200; height:32; value <-> item.title; } } } } }".to_string())]);
        let state = props(vec![
            (
                "items",
                Value::List(vec![record("a", "Alpha"), record("b", "Beta")]),
            ),
            (
                "groups",
                Value::List(
                    (0..groups)
                        .map(|i| record(&format!("g{i}"), "Group"))
                        .collect(),
                ),
            ),
        ]);
        let mut doc = Document::from_ui_files(&files, "App.ui", &state).unwrap();
        let (source, templates, controls) = doc.render_scene().unwrap();
        let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
        assert_eq!(runtime.control_count(), groups * 2);
        runtime
            .set_binding_value(groups * 2 - 1, "edited-b", true)
            .unwrap();
        assert_eq!(doc.sync_runtime_bindings(&runtime, None).unwrap(), 1);
        let Value::List(items) = &doc.states[0]["items"] else {
            panic!("items");
        };
        let Value::Object(second) = &items[1] else {
            panic!("item");
        };
        assert_eq!(second["title"], string("edited-b"));
        if groups == 2 {
            doc.states[0].insert(
                "items".into(),
                Value::List(vec![record("x", "Oversized"); 16_385]),
            );
            assert!(
                resolve_binding_item(&controls[0], "item", &doc.states[0])
                    .unwrap_err()
                    .contains("16384 items")
            );
        }
    }
}
#[test]
fn unchanged_virtual_collection_member_does_not_become_a_model_write() {
    let files = BTreeMap::from([("App.ui".to_string(),
            "component App { state items: List = []; Column { width:260; height:100; for item in state.items key item.id { TextInput { width:200; height:32; value:'5'; } } } }".to_string())]);
    let state = props(vec![("items", Value::List(vec![record("a", "Alpha")]))]);
    let mut doc = Document::from_ui_files(&files, "App.ui", &state).unwrap();
    doc.root.children[0].children[0].props.insert(
        "__bindings".into(),
        Value::Object(props(vec![("value", string("item.title.length"))])),
    );
    let (source, templates, _) = doc.render_scene().unwrap();
    let runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
    let before = doc.states.clone();
    assert_eq!(doc.sync_runtime_bindings(&runtime, None).unwrap(), 0);
    assert_eq!(doc.states, before);
}
#[test]
fn let_collection_aliases_preserve_origin_and_do_not_infer_it_from_values() {
    for collection in ["chosen", "state.chosen"] {
        for derived in [false, true] {
            for cached in [false, true] {
                let initial_items = Value::List(vec![record("a", "Alpha"), record("b", "Beta")]);
                let mut doc = document(root(vec![
                    node(
                        "Let",
                        vec![
                            ("name", string("rows")),
                            (
                                "value",
                                if derived {
                                    initial_items.clone()
                                } else {
                                    expr("state.items")
                                },
                            ),
                        ],
                        vec![],
                    ),
                    node(
                        "Let",
                        vec![("name", string("chosen")), ("value", expr("rows"))],
                        vec![],
                    ),
                    node(
                        "For",
                        vec![
                            ("item", string("item")),
                            ("collection", expr(collection)),
                            ("key", expr("item.id")),
                        ],
                        vec![node(
                            "Control",
                            vec![
                                ("value", expr("item.title")),
                                (
                                    "__bindings",
                                    Value::Object(props(vec![("value", string("item.title"))])),
                                ),
                            ],
                            vec![],
                        )],
                    ),
                ]));
                let definition = doc.definitions.get_mut("Control").unwrap();
                definition.defaults.insert("value".into(), string(""));
                definition.nodes = vec![node(
                    "Rectangle",
                    vec![],
                    vec![node(
                        "TextInput",
                        vec![("value", expr("props.value"))],
                        vec![],
                    )],
                )];
                doc.states = vec![props(vec![
                    ("items", initial_items),
                    ("chosen", Value::List(vec![record("other", "Model shadow")])),
                ])];
                let (source, templates, controls) = doc.render_scene().unwrap();
                let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
                runtime.set_binding_value(0, "Edited", true).unwrap();
                let Value::List(items) = doc.states[0].get_mut("items").unwrap() else {
                    panic!("items")
                };
                items.reverse();
                let changed = if cached {
                    doc.sync_runtime_bindings_from_scene(&runtime, None, &controls)
                } else {
                    doc.sync_runtime_bindings(&runtime, None)
                };
                assert_eq!(changed.unwrap(), usize::from(!derived));
                assert_eq!(
                    lookup(&doc.states[0], "items.1.title"),
                    Some(string(if derived { "Alpha" } else { "Edited" }))
                );
                assert_eq!(
                    lookup(&doc.states[0], "items.0.title"),
                    Some(string("Beta"))
                );
                assert_eq!(
                    lookup(&doc.states[0], "chosen.0.title"),
                    Some(string("Model shadow"))
                );
            }
        }
    }
}
#[test]
fn failed_let_replay_rolls_back_earlier_widget_writes_and_retries() {
    for cached in [false, true] {
        let editor = |value: Value, path: &str| {
            node(
                "Control",
                vec![
                    ("value", value),
                    (
                        "__bindings",
                        Value::Object(props(vec![("value", string(path))])),
                    ),
                ],
                vec![],
            )
        };
        let mut doc = document(root(vec![
            editor(expr("state.query"), "state.query"),
            node(
                "Let",
                vec![("name", string("prefix")), ("value", expr("state.prefix"))],
                vec![],
            ),
            node(
                "For",
                vec![
                    ("item", string("item")),
                    ("collection", expr("state.items")),
                    (
                        "key",
                        Value::Expression(Box::new(Expression::Binary(
                            "+".into(),
                            expr("prefix"),
                            expr("item.id"),
                        ))),
                    ),
                ],
                vec![editor(expr("item.title"), "item.title")],
            ),
        ]));
        let definition = doc.definitions.get_mut("Control").unwrap();
        definition.defaults.insert("value".into(), string(""));
        definition.nodes = vec![node(
            "Rectangle",
            vec![],
            vec![node(
                "TextInput",
                vec![("value", expr("props.value"))],
                vec![],
            )],
        )];
        doc.states = vec![props(vec![
            ("query", string("seed")),
            ("prefix", string("p/")),
            ("items", Value::List(vec![record("a", "Alpha")])),
        ])];
        let (source, templates, controls) = doc.render_scene().unwrap();
        let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
        runtime.set_binding_value(0, "Edited query", true).unwrap();
        runtime.set_binding_value(1, "Edited item", true).unwrap();
        doc.states[0].remove("prefix");
        let before = doc.states.clone();
        for _ in 0..2 {
            let result = if cached {
                doc.sync_runtime_bindings_from_scene(&runtime, None, &controls)
            } else {
                doc.sync_runtime_bindings(&runtime, None)
            };
            assert!(result.unwrap_err().contains("Missing state.prefix"));
            assert_eq!(doc.states, before, "earlier editor write must roll back");
        }
        doc.states[0].insert("prefix".into(), string("p/"));
        let mut corrupt = controls.clone();
        let Some(Value::Object(active)) = corrupt[1].state.get_mut("__binding_active") else {
            panic!("active frames")
        };
        active.insert("item".into(), number(-1.));
        let before = doc.states.clone();
        assert!(
            doc.sync_runtime_bindings_from_scene(&runtime, None, &corrupt)
                .unwrap_err()
                .contains("Invalid active binding frame")
        );
        assert_eq!(
            doc.states, before,
            "invalid frame must roll back the earlier editor write"
        );
        assert_eq!(
            doc.sync_runtime_bindings_from_scene(&runtime, None, &controls)
                .unwrap(),
            2
        );
        assert_eq!(doc.states[0]["query"], string("Edited query"));
        assert_eq!(
            lookup(&doc.states[0], "items.0.title"),
            Some(string("Edited item"))
        );
        assert_eq!(
            doc.sync_runtime_bindings_from_scene(&runtime, None, &controls)
                .unwrap(),
            0
        );
        doc.states[0].insert("query".into(), string("Model query"));
        assert_eq!(
            doc.sync_runtime_bindings_from_scene(&runtime, None, &controls)
                .unwrap(),
            0
        );
        assert_eq!(doc.states[0]["query"], string("Model query"));
    }
}
#[test]
fn let_key_local_shadows_root_model_during_binding_recovery() {
    for dynamic in [false, true] {
        for cached in [false, true] {
            let mut doc = document(root(vec![
                node(
                    "Let",
                    vec![
                        ("name", string("prefix")),
                        (
                            "value",
                            if dynamic {
                                expr("state.prefix")
                            } else {
                                string("lexical/")
                            },
                        ),
                    ],
                    vec![],
                ),
                node(
                    "Let",
                    vec![("name", string("suffix")), ("value", expr("prefix"))],
                    vec![],
                ),
                node(
                    "For",
                    vec![
                        ("item", string("item")),
                        ("collection", expr("state.items")),
                        (
                            "key",
                            Value::Expression(Box::new(Expression::Binary(
                                "+".into(),
                                expr("suffix"),
                                expr("item.id"),
                            ))),
                        ),
                    ],
                    vec![node(
                        "Control",
                        vec![
                            ("value", expr("item.title")),
                            (
                                "__bindings",
                                Value::Object(props(vec![("value", string("item.title"))])),
                            ),
                        ],
                        vec![],
                    )],
                ),
            ]));
            let definition = doc.definitions.get_mut("Control").unwrap();
            definition.defaults.insert("value".into(), string(""));
            definition.nodes = vec![node(
                "Rectangle",
                vec![],
                vec![node(
                    "TextInput",
                    vec![("value", expr("props.value"))],
                    vec![],
                )],
            )];
            doc.states = vec![props(vec![
                ("prefix", string("model/")),
                (
                    "items",
                    Value::List(vec![record("a", "Alpha"), record("b", "Beta")]),
                ),
            ])];
            let (source, templates, controls) = doc.render_scene().unwrap();
            let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
            runtime.set_binding_value(0, "Edited", true).unwrap();
            doc.states[0].insert("prefix".into(), string("new-model/"));
            let Value::List(items) = doc.states[0].get_mut("items").unwrap() else {
                panic!("items")
            };
            items.reverse();
            let changed = if cached {
                doc.sync_runtime_bindings_from_scene(&runtime, None, &controls)
            } else {
                doc.sync_runtime_bindings(&runtime, None)
            };
            assert_eq!(changed.unwrap(), usize::from(!dynamic));
            assert_eq!(
                lookup(&doc.states[0], "items.1.title"),
                Some(string(if dynamic { "Alpha" } else { "Edited" }))
            );
            assert_eq!(doc.states[0]["prefix"], string("new-model/"));
        }
    }
}
#[test]
fn lexical_parent_frames_must_point_to_earlier_frames() {
    let frames = vec![Value::Object(props(vec![
        ("__item", string("item")),
        ("__collection.item", string("state.items")),
        (
            "__parents",
            Value::Object(props(vec![("item", number(0.))])),
        ),
    ]))];
    assert!(
        resolve_binding_frame(
            &frames,
            0,
            &Properties::new(),
            &mut BindingKeyIndexes::new(),
            &mut BTreeMap::new(),
            0
        )
        .unwrap_err()
        .contains("parent frame")
    );
}
#[test]
fn later_loop_can_use_earlier_local_after_an_unrelated_shadow() {
    for cached in [false, true] {
        let files = BTreeMap::from([("App.ui".into(),
                "component App { state groups: List = []; state other: List = []; Column { width:260; height:400; for item in state.groups key item.id { for branch in item.items key branch.id { for item in state.other key item.id { for leaf in branch.items key item.id + leaf.id { TextInput { width:200; height:32; value <-> leaf.title; } } } } } } }".into())]);
        let group = |id: &str, children| {
            Value::Object(props(vec![
                ("id", string(id)),
                ("items", Value::List(children)),
            ]))
        };
        let state = props(vec![
            (
                "groups",
                Value::List(vec![
                    group("g1", vec![group("branch", vec![record("leaf", "First")])]),
                    group("g2", vec![group("branch", vec![record("leaf", "Second")])]),
                ]),
            ),
            ("other", Value::List(vec![record("inner", "Unused")])),
        ]);
        let mut doc = Document::from_ui_files(&files, "App.ui", &state).unwrap();
        let (source, templates, controls) = doc.render_scene().unwrap();
        let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
        runtime.set_binding_value(0, "Edited First", true).unwrap();
        let Value::List(groups) = doc.states[0].get_mut("groups").unwrap() else {
            panic!("groups")
        };
        groups.reverse();
        let changed = if cached {
            doc.sync_runtime_bindings_from_scene(&runtime, None, &controls)
        } else {
            doc.sync_runtime_bindings(&runtime, None)
        };
        assert_eq!(changed.unwrap(), 1);
        assert_eq!(
            lookup(&doc.states[0], "groups.1.items.0.items.0.title"),
            Some(string("Edited First"))
        );
        assert_eq!(
            lookup(&doc.states[0], "groups.0.items.0.items.0.title"),
            Some(string("Second"))
        );
    }
}
#[test]
fn three_shadowed_for_levels_preserve_parent_identity() {
    for middle in ["item", "branch"] {
        for cached in [false, true] {
            let source = "component App { state groups: List = []; Column { width:260; height:400; for item in state.groups key item.id { for middle in item.items key middle.id { for item in middle.items key item.id { TextInput { width:200; height:32; value <-> item.title; } } } } } }".replace("middle", middle);
            let files = BTreeMap::from([("App.ui".into(), source)]);
            let group = |id: &str, children| {
                Value::Object(props(vec![
                    ("id", string(id)),
                    ("items", Value::List(children)),
                ]))
            };
            let state = props(vec![(
                "groups",
                Value::List(vec![
                    group("root", vec![group("mid", vec![record("leaf", "Before")])]),
                    group(
                        "other",
                        vec![group("mid", vec![record("leaf", "Unchanged")])],
                    ),
                ]),
            )]);
            let mut doc = Document::from_ui_files(&files, "App.ui", &state).unwrap();
            let (source, templates, controls) = doc.render_scene().unwrap();
            let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
            runtime.set_binding_value(0, "After", true).unwrap();
            let Value::List(groups) = doc.states[0].get_mut("groups").unwrap() else {
                panic!("groups")
            };
            groups.reverse();
            let changed = if cached {
                doc.sync_runtime_bindings_from_scene(&runtime, None, &controls)
            } else {
                doc.sync_runtime_bindings(&runtime, None)
            };
            assert_eq!(changed.unwrap(), 1);
            assert_eq!(
                lookup(&doc.states[0], "groups.1.items.0.items.0.title"),
                Some(string("After"))
            );
            assert_eq!(
                lookup(&doc.states[0], "groups.0.items.0.items.0.title"),
                Some(string("Unchanged"))
            );
        }
    }
}
#[test]
fn cyclic_collection_parent_metadata_refuses_before_recursing_unbounded() {
    let control = SceneControl {
        node: 0,
        key: String::new(),
        event_args: BTreeMap::new(),
        state: props(vec![("__collection.item", string("item.items"))]),
    };
    assert!(
        resolve_binding_item(&control, "item", &Properties::new())
            .unwrap_err()
            .contains("parent depth")
    );
}
#[test]
fn source_scroll_viewport_extents_preserve_frame_and_clip_input() {
    for (width, height, padding, expected) in [
        ("320", "300", "0", [20., 50., 320., 300.]),
        ("50%", "50%", "0", [90., 100., 180., 200.]),
        ("50%", "50%", "20", [100., 110., 160., 180.]),
    ] {
        let source = format!(
            "component App {{ state items: List = []; Column {{ width:360; height:400; padding:{padding}; Scroll {{ width:{width}; height:{height}; Column {{ width:100; height:512; for item in state.items key item.id {{ TextInput {{ width:100; height:32; value <-> item.title; }} }} }} }} }} }}"
        );
        let files = BTreeMap::from([("App.ui".into(), source)]);
        let state = props(vec![(
            "items",
            Value::List((0..16).map(|i| record(&format!("id{i}"), "Row")).collect()),
        )]);
        let mut runtime = crate::Runtime::from_ui_files(&files, "App.ui", &state).unwrap();
        assert!(runtime.scrollable());
        assert_eq!(runtime.viewport(), expected.map(|v| v as f32).to_vec());
        assert_eq!(runtime.render_width(), 360.);
        assert_eq!(runtime.render_height(), 400.);
        runtime.scroll(0., 10_000.);
        assert_eq!(runtime.scroll_offset()[1], 512. - expected[3] as f32);
        runtime.pointer(
            expected[0] as f32 + expected[2] as f32 / 2.,
            expected[1] as f32 - 1.,
            0,
        );
        assert!((0..16).all(|i| !runtime.control_hovered(i)));
        runtime.pointer(
            expected[0] as f32 + expected[2] as f32 / 2.,
            expected[1] as f32 + 10.,
            0,
        );
        assert!((0..16).any(|i| runtime.control_hovered(i)));
    }
    let files = BTreeMap::from([("App.ui".into(),
            "component App { Column { width:360; height:400; Scroll { width:500; TextInput { width:100; height:32; } } } }".into())]);
    let doc = Document::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
    assert!(
        doc.render_scene()
            .unwrap_err()
            .contains("viewport must fit")
    );
}
#[test]
fn empty_scroll_layout_retains_logical_content_height() {
    for (content, expected) in [(512, 212.), (900, 600.), (200, 0.)] {
        let source = format!(
            "component App {{ Column {{ width:360; height:400; Scroll {{ width:320; height:300; Column {{ width:100; height:{content}; }} }} }} }}"
        );
        let files = BTreeMap::from([("App.ui".into(), source)]);
        let mut runtime =
            crate::Runtime::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
        assert_eq!(runtime.control_count(), 0);
        runtime.scroll(0., 10_000.);
        assert_eq!(runtime.scroll_offset()[1], expected);
        assert_eq!(runtime.viewport(), vec![20., 50., 320., 300.]);
    }
}

#[test]
fn structural_binding_maps_match_rendered_maps_across_layouts_and_branches() {
    let body = "let prefix = 'p/'; if state.enabled { for item in state.items key prefix + item.id { TextInput { width:100; height:32; value <-> item.title; } } } else { TextInput { width:100; height:32; value <-> state.query; } } Text { text:state.marker; }";
    let mut layouts: Vec<String> = ["Frame", "Column", "Row", "Grid", "Stack"]
        .iter()
        .map(|kind| format!("{kind} {{ width:360; height:400; {body} }}"))
        .collect();
    layouts.push(format!("Column {{ width:360; height:400; Scroll {{ width:360; height:300; Column {{ width:320; height:600; {body} }} }} }}"));
    for layout in layouts {
        let source = format!(
            "component App {{ state enabled: Bool = true; state query: String = 'seed'; state marker: String = 'snapshot'; state items: List = []; {layout} }}"
        );
        let files = BTreeMap::from([("App.ui".into(), source)]);
        let state = props(vec![(
            "items",
            Value::List(vec![record("a", "Alpha"), record("b", "Beta")]),
        )]);
        let mut doc = Document::from_ui_files(&files, "App.ui", &state).unwrap();
        fn annotate_events(node: &mut Node) {
            if node.kind == "TextInput" {
                node.props.insert(
                    "__event_args".into(),
                    Value::Object(props(vec![(
                        "snapshot",
                        Value::List(vec![expr("state.marker")]),
                    )])),
                );
            }
            for child in &mut node.children {
                annotate_events(child);
            }
        }
        annotate_events(&mut doc.root);
        for enabled in [true, false, true] {
            doc.states[0].insert("enabled".into(), Value::Bool(enabled));
            let Value::List(items) = doc.states[0].get_mut("items").unwrap() else {
                panic!("items")
            };
            items.reverse();
            let (source, templates, rendered) = doc.render_scene().unwrap();
            crate::Runtime::from_sources(&source, &templates).unwrap();
            let structural = doc.binding_scene_controls().unwrap();
            assert_eq!(structural.len(), rendered.len());
            for (actual, expected) in structural.iter().zip(&rendered) {
                assert_eq!(actual.node, expected.node);
                assert_eq!(actual.key, expected.key);
                assert_eq!(actual.state, expected.state);
                assert_eq!(actual.event_args, expected.event_args);
            }
            assert_eq!(
                structural[0].event_args["snapshot"],
                vec![string("snapshot")]
            );
        }
    }
}
#[test]
fn source_match_let_aliases_and_guard_switch_preserve_binding_identity() {
    let files = BTreeMap::from([("App.ui".into(),
            "component App { state mode: String = 'a'; state enabled: Bool = true; state prefix: String = 'root'; state other: String = 'Other'; state items: List = []; Column { width:260; height:300; match state.mode { 'a' if state.enabled => { let prefix = 'branch/'; let rows = state.items; for item in rows key prefix + item.id { TextInput { width:200; height:32; value <-> item.title; } } } _ => { let prefix = 'fallback'; let suffix = prefix; TextInput { width:200; height:32; value <-> state.other; } } } Text { text:state.prefix; } } }".into())]);
    let state = props(vec![("items", Value::List(vec![record("a", "Alpha")]))]);
    let mut doc = Document::from_ui_files(&files, "App.ui", &state).unwrap();
    let (source, templates, first) = doc.render_scene().unwrap();
    assert_eq!(first[0].state["prefix"], string("branch/"));
    assert_eq!(first[1].state["prefix"], string("root"));
    let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
    runtime
        .set_binding_value(0, "Old branch input", true)
        .unwrap();
    doc.states[0].insert("enabled".into(), Value::Bool(false));
    assert_eq!(doc.sync_runtime_bindings(&runtime, None).unwrap(), 0);
    assert_eq!(
        lookup(&doc.states[0], "items.0.title"),
        Some(string("Alpha"))
    );
    assert_eq!(doc.states[0]["other"], string("Other"));
    let (source, templates, fallback) = doc.render_scene().unwrap();
    assert_ne!(first[0].key, fallback[0].key);
    assert_eq!(fallback[0].state["suffix"], string("fallback"));
    assert!(!fallback[1].state.contains_key("suffix"));
    let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
    runtime
        .set_binding_value(0, "Edited fallback", true)
        .unwrap();
    assert_eq!(doc.sync_runtime_bindings(&runtime, None).unwrap(), 1);
    assert_eq!(doc.states[0]["other"], string("Edited fallback"));
    doc.states[0].insert("enabled".into(), Value::Bool(true));
    assert_eq!(doc.sync_runtime_bindings(&runtime, None).unwrap(), 0);
    let (_, _, again) = doc.render_scene().unwrap();
    assert_eq!(again[0].key, first[0].key);
    assert_eq!(doc.states[0]["prefix"], string("root"));
}
#[test]
fn source_if_let_sequence_is_visible_only_in_selected_branch() {
    let files = BTreeMap::from([("App.ui".into(),
            "component App { state prefix: String = 'root'; state enabled: Bool = true; Column { width:260; height:300; if state.enabled { let prefix = 'yes'; let suffix = prefix; Text { text:suffix; } } else { let prefix = 'no'; let suffix = prefix; Text { text:suffix; } } Text { text:state.prefix; } } }".into())]);
    let mut doc = Document::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
    for (enabled, expected) in [(true, "yes"), (false, "no"), (true, "yes")] {
        doc.states[0].insert("enabled".into(), Value::Bool(enabled));
        let (source, templates, controls) = doc.render_scene().unwrap();
        crate::Runtime::from_sources(&source, &templates).unwrap();
        assert_eq!(controls.len(), 2);
        assert_eq!(controls[0].state["suffix"], string(expected));
        assert_eq!(controls[1].state["prefix"], string("root"));
        assert!(!controls[1].state.contains_key("suffix"));
        assert_eq!(doc.states[0]["prefix"], string("root"));
    }
}
#[test]
fn source_for_let_siblings_are_visible_and_do_not_leak_between_iterations() {
    let files = BTreeMap::from([("App.ui".into(),
            "component App { state prefix: String = 'root/'; state items: List = []; Column { width:260; height:300; for item in state.items key state.prefix + item.id { let prefix = item.id; TextInput { width:200; height:32; value <-> item.title; } } Text { text:state.prefix; } } }".into())]);
    let state = props(vec![(
        "items",
        Value::List(vec![record("a", "Alpha"), record("b", "Beta")]),
    )]);
    let mut doc = Document::from_ui_files(&files, "App.ui", &state).unwrap();
    let (_, _, first) = doc.render_scene().unwrap();
    assert_eq!(first[0].state["prefix"], string("a"));
    assert_eq!(first[1].state["prefix"], string("b"));
    assert_eq!(first[2].state["prefix"], string("root/"));
    let Value::List(items) = doc.states[0].get_mut("items").unwrap() else {
        panic!("items")
    };
    items.reverse();
    let (_, _, second) = doc.render_scene().unwrap();
    assert_eq!(first[0].key, second[1].key);
    assert_eq!(first[1].key, second[0].key);
    assert_eq!(doc.states[0]["prefix"], string("root/"));
}
#[test]
fn nested_for_bindings_follow_parent_and_child_keys() {
    for alias in [false, true] {
        for shadow in [false, true] {
            for cached in [false, true] {
                for mode in [0, 1, 2] {
                    let mut files: BTreeMap<String, String> = BTreeMap::from([("App.ui".into(),
                    "component App { state groups: List = []; Column { width:260; height:400; for outer in state.groups key outer.id { for item in outer.items key item.id { TextInput { width:200; height:32; value <-> item.title; } } } } }".into())]);
                    if shadow {
                        let source = files.get_mut("App.ui").unwrap();
                        *source = source.replace("outer", "item");
                    }
                    if alias {
                        let source = files.get_mut("App.ui").unwrap();
                        let parent = if shadow { "item" } else { "outer" };
                        *source = source.replace(
                            &format!("for item in {parent}.items"),
                            &format!("let rows = {parent}.items; for item in rows"),
                        );
                    }
                    let group = |id: &str, children| {
                        Value::Object(props(vec![
                            ("id", string(id)),
                            ("items", Value::List(children)),
                        ]))
                    };
                    let initial = props(vec![(
                        "groups",
                        Value::List(vec![
                            group("g1", vec![record("a", "Alpha"), record("b", "Beta")]),
                            group("g2", vec![record("a", "Other Alpha")]),
                        ]),
                    )]);
                    let mut doc = Document::from_ui_files(&files, "App.ui", &initial).unwrap();
                    let (source, templates, controls) = doc.render_scene().unwrap();
                    let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
                    runtime.set_binding_value(0, "edited-g1-a", true).unwrap();
                    let groups = match mode {
                        0 => vec![
                            group("g2", vec![record("a", "Other Alpha")]),
                            group("g1", vec![record("b", "Beta"), record("a", "Alpha")]),
                        ],
                        1 => vec![group("g2", vec![record("a", "Other Alpha")])],
                        _ => vec![
                            group("g1", vec![record("a", "Alpha")]),
                            group("g1", vec![record("a", "Duplicate")]),
                        ],
                    };
                    doc.states[0].insert("groups".into(), Value::List(groups));
                    let before = doc.states.clone();
                    let result = if cached {
                        doc.sync_runtime_bindings_from_scene(&runtime, None, &controls)
                    } else {
                        doc.sync_runtime_bindings(&runtime, None)
                    };
                    if mode == 2 {
                        assert!(result.unwrap_err().contains("Duplicate collection"));
                        assert_eq!(doc.states, before);
                    } else {
                        assert_eq!(result.unwrap(), usize::from(mode == 0));
                        assert_eq!(
                            lookup(&doc.states[0], "groups.0.items.0.title"),
                            Some(string("Other Alpha"))
                        );
                        if mode == 0 {
                            assert_eq!(
                                lookup(&doc.states[0], "groups.1.items.1.title"),
                                Some(string("edited-g1-a"))
                            );
                            assert_eq!(
                                lookup(&doc.states[0], "groups.1.items.0.title"),
                                Some(string("Beta"))
                            );
                        }
                    }
                }
            }
        }
    }
}
#[test]
fn nested_list_paths_read_and_write_without_replacing_lists() {
    let mut state = props(vec![(
        "groups",
        Value::List(vec![Value::Object(props(vec![(
            "items",
            Value::List(vec![record("a", "Alpha"), record("b", "Beta")]),
        )]))]),
    )]);
    assert_eq!(
        lookup(&state, "groups.0.items.1.title"),
        Some(string("Beta"))
    );
    assert_eq!(
        lookup_binding_ref(&state, "groups.0.items.1.title"),
        Some(&string("Beta"))
    );
    assert_eq!(lookup(&state, "groups.0.items.length"), Some(number(2.)));
    assert!(lookup_binding_ref(&state, "groups.0.items.length").is_none());
    assert!(write_collection_item_path(
        &mut state,
        "groups.0.items",
        1,
        "title",
        string("Edited")
    ));
    assert_eq!(
        lookup(&state, "groups.0.items.1.title"),
        Some(string("Edited"))
    );
    assert!(write_state_path(
        &mut state,
        "groups.0.items.0.title",
        string("First")
    ));
    let before = state.clone();
    for path in [
        "groups.9.items.0.title",
        "groups.bad.items",
        "groups.0.items.length",
    ] {
        assert!(!write_state_path(&mut state, path, string("invalid")));
        assert_eq!(state, before);
    }
}
#[test]
fn bindings_route_root_backed_collections_through_list_indices() {
    let mut doc = document(root(vec![node(
        "For",
        vec![
            ("item", string("item")),
            ("collection", expr("state.groups.0.items")),
            ("key", expr("item.id")),
        ],
        vec![node(
            "Control",
            vec![
                ("value", expr("item.title")),
                (
                    "__bindings",
                    Value::Object(props(vec![("value", string("item.title"))])),
                ),
            ],
            vec![],
        )],
    )]));
    let definition = doc.definitions.get_mut("Control").unwrap();
    definition.defaults.insert("value".into(), string(""));
    definition.nodes = vec![node(
        "Rectangle",
        vec![],
        vec![node(
            "TextInput",
            vec![("value", expr("props.value"))],
            vec![],
        )],
    )];
    doc.states = vec![props(vec![(
        "groups",
        Value::List(vec![Value::Object(props(vec![(
            "items",
            Value::List(vec![record("a", "Alpha"), record("b", "Beta")]),
        )]))]),
    )])];
    let (source, templates, controls) = doc.render_scene().unwrap();
    let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
    runtime.set_binding_value(0, "edited-a", true).unwrap();
    let Some(Value::List(items)) = lookup_binding_mut(&mut doc.states[0], "groups.0.items") else {
        panic!("items")
    };
    items.reverse();
    assert_eq!(
        doc.sync_runtime_bindings_from_scene(&runtime, None, &controls)
            .unwrap(),
        1
    );
    assert_eq!(
        lookup(&doc.states[0], "groups.0.items.1.title"),
        Some(string("edited-a"))
    );
    assert_eq!(
        lookup(&doc.states[0], "groups.0.items.0.title"),
        Some(string("Beta"))
    );
}
#[test]
fn collection_key_encoding_preserves_types_and_utf8_lengths() {
    for (value, expected) in [
        (string(""), "s0:"),
        (string("я🙂"), "s6:я🙂"),
        (string("12"), "s2:12"),
        (number(12.), "n2:12"),
        (number(-1.5), "n4:-1.5"),
    ] {
        assert_eq!(key_part(&value).unwrap(), expected);
    }
    for value in [
        number(f64::NAN),
        number(f64::INFINITY),
        Value::Bool(true),
        Value::Null,
    ] {
        assert!(key_part(&value).is_err());
    }
}
#[test]
fn borrowed_collection_keys_preserve_computed_and_exact_field_semantics() {
    let item = |id: Value, raw: &str| Value::Object(props(vec![("id", id), ("raw", string(raw))]));
    let mut state = props(vec![(
        "items",
        Value::List(vec![
            item(string("a"), "unused"),
            item(expr("item.raw"), "b"),
        ]),
    )]);
    let control = SceneControl {
        node: 0,
        key: String::new(),
        event_args: BTreeMap::new(),
        state: props(vec![
            ("__collection.item", string("state.items")),
            (
                "__binding_key.item",
                string(&key_part(&string("b")).unwrap()),
            ),
            ("__binding_key_expr.item", expr("item.id")),
        ]),
    };
    assert_eq!(
        resolve_binding_item(&control, "item", &state).unwrap(),
        Some(("items".into(), 1))
    );
    // Exact dotted root fields take precedence over item members, causing
    // duplicate keys here; the borrowed path must not bypass this refusal.
    state.insert("item.id".into(), string("override"));
    assert!(
        resolve_binding_item(&control, "item", &state)
            .unwrap_err()
            .contains("Duplicate")
    );
    state.remove("item.id");
    let Value::List(items) = state.get_mut("items").unwrap() else {
        panic!("items")
    };
    items.reverse();
    assert_eq!(
        resolve_binding_item(&control, "item", &state).unwrap(),
        Some(("items".into(), 0))
    );
}
#[test]
fn reused_for_scope_preserves_nested_shadowing_and_owned_snapshots() {
    let control = || node("Control", vec![("text", expr("item.title"))], vec![]);
    let loop_node = |collection: &str, children| {
        node(
            "For",
            vec![
                ("item", string("item")),
                ("collection", expr(collection)),
                ("key", expr("item.id")),
            ],
            children,
        )
    };
    let mut doc = document(root(vec![loop_node(
        "state.items",
        vec![loop_node("state.inner", vec![control()]), control()],
    )]));
    doc.states = vec![props(vec![
        (
            "items",
            Value::List(vec![record("a", "Alpha"), record("b", "Beta")]),
        ),
        (
            "inner",
            Value::List(vec![record("x", "Ex"), record("y", "Why")]),
        ),
    ])];
    let (_, _, snapshots) = doc.render_scene().unwrap();
    let titles: Vec<_> = snapshots
        .iter()
        .map(|c| {
            let Value::Object(item) = &c.state["item"] else {
                panic!("item snapshot")
            };
            item["title"].text().unwrap()
        })
        .collect();
    assert_eq!(titles, ["Ex", "Why", "Alpha", "Ex", "Why", "Beta"]);
    for (i, key) in ["x", "y", "a", "x", "y", "b"].iter().enumerate() {
        assert_eq!(
            snapshots[i].state["__binding_key.item"],
            string(&key_part(&string(key)).unwrap())
        );
    }
    doc.states[0].insert("items".into(), Value::List(vec![record("c", "Changed")]));
    let (_, _, next) = doc.render_scene().unwrap();
    assert_eq!(next.len(), 3);
    assert_eq!(
        snapshots[5].state["__binding_key.item"],
        string(&key_part(&string("b")).unwrap())
    );
    assert!(!doc.states[0].contains_key("item"));
}
#[test]
fn borrowed_root_alias_keys_match_general_replay_on_reorder_and_key_errors() {
    let files = BTreeMap::from([("App.ui".into(), "component App { state items: List = []; Column { let visible = state.items; let rows = visible; for item in rows key item.id { TextInput { width:100; height:30; value <-> item.title; } } } }".into())]);
    let item =
        |id: Value, title: &str| Value::Object(props(vec![("id", id), ("title", string(title))]));
    let mut doc = Document::from_ui_files(
        &files,
        "App.ui",
        &props(vec![(
            "items",
            Value::List(vec![item(string("a"), "Alpha"), item(string("b"), "Beta")]),
        )]),
    )
    .unwrap();
    let (_, _, controls) = doc.render_scene().unwrap();
    let mut reference = controls[0].clone();
    let Value::List(frames) = reference.state.get_mut("__binding_frames").unwrap() else {
        panic!("frames")
    };
    let Value::Object(frame) = &mut frames[0] else {
        panic!("frame")
    };
    let Value::List(steps) = frame.get_mut("__let_steps").unwrap() else {
        panic!("steps")
    };
    // An unrelated literal Let requires general replay without changing the key.
    steps.push(Value::Object(props(vec![
        ("name", string("unused_marker")),
        ("expression", string("marker")),
    ])));
    let Value::Object(lexicals) = frame.get_mut("__lexicals").unwrap() else {
        panic!("lexicals")
    };
    lexicals.insert("unused_marker".into(), string("marker"));
    let compare = |state: &Properties| {
        let actual = resolve_binding_item(&controls[0], "item", state);
        let expected = resolve_binding_item(&reference, "item", state);
        assert_eq!(actual, expected);
        actual
    };
    assert_eq!(compare(&doc.states[0]).unwrap(), Some(("items".into(), 0)));
    let Value::List(items) = doc.states[0].get_mut("items").unwrap() else {
        panic!("items")
    };
    items.reverse();
    assert_eq!(compare(&doc.states[0]).unwrap(), Some(("items".into(), 1)));
    doc.states[0].insert("item.id".into(), string("override"));
    assert!(compare(&doc.states[0]).unwrap_err().contains("Duplicate"));
    doc.states[0].remove("item.id");
    let Value::List(items) = doc.states[0].get_mut("items").unwrap() else {
        panic!("items")
    };
    let Value::Object(first) = &mut items[0] else {
        panic!("item")
    };
    first.insert("id".into(), expr("item.title"));
    assert_eq!(compare(&doc.states[0]).unwrap(), Some(("items".into(), 1)));
}

#[test]
fn direct_alias_replay_preserves_shadowing_and_readonly_origins() {
    for (lets, target) in [
        ("let visible = state.items;", Some("items")),
        ("let rows = state.items; let visible = rows;", Some("items")),
        (
            "let items = state.other; let visible = items;",
            Some("other"),
        ),
        (
            "let visible = state.items; let items = state.other;",
            Some("items"),
        ),
        (
            "let visible = state.items; let visible = state.other;",
            Some("other"),
        ),
        (
            "let marker = 'literal'; let visible = state.items;",
            Some("items"),
        ),
        ("let visible = true ? state.items : state.other;", None),
    ] {
        let source = format!(
            "component App {{ state items: List = []; state other: List = []; Column {{ {lets} for item in visible key item.id {{ TextInput {{ width:100; height:30; value <-> item.title; }} }} }} }}"
        );
        let files = BTreeMap::from([("App.ui".into(), source)]);
        let list = |prefix: &str| {
            Value::List(
                ["a", "b"]
                    .into_iter()
                    .map(|id| {
                        Value::Object(props(vec![
                            ("id", string(id)),
                            ("title", string(&format!("{prefix}-{id}"))),
                        ]))
                    })
                    .collect(),
            )
        };
        let mut doc = Document::from_ui_files(
            &files,
            "App.ui",
            &props(vec![("items", list("items")), ("other", list("other"))]),
        )
        .unwrap();
        let (_, _, controls) = doc.render_scene().unwrap();
        let mut reference = controls[0].clone();
        let Value::List(frames) = reference.state.get_mut("__binding_frames").unwrap() else {
            panic!("frames")
        };
        let Value::Object(frame) = &mut frames[0] else {
            panic!("frame")
        };
        let Value::List(steps) = frame.get_mut("__let_steps").unwrap() else {
            panic!("steps")
        };
        steps.push(Value::Object(props(vec![
            ("name", string("unused_reference_marker")),
            ("expression", string("marker")),
        ])));
        let Value::Object(lexicals) = frame.get_mut("__lexicals").unwrap() else {
            panic!("lexicals")
        };
        lexicals.insert("unused_reference_marker".into(), string("marker"));
        for reversed in [false, true] {
            if reversed {
                for name in ["items", "other"] {
                    let Value::List(items) = doc.states[0].get_mut(name).unwrap() else {
                        panic!("items")
                    };
                    items.reverse();
                }
            }
            let actual = resolve_binding_item(&controls[0], "item", &doc.states[0]);
            let expected = resolve_binding_item(&reference, "item", &doc.states[0]);
            assert_eq!(actual, expected, "{lets}");
            assert_eq!(
                actual.unwrap(),
                target.map(|target| (target.into(), usize::from(reversed))),
                "{lets}"
            );
        }
    }
}

#[test]
fn direct_alias_shortcut_does_not_hide_an_unused_let_replay_error() {
    let files = BTreeMap::from([("App.ui".into(), "component App { state items: List = []; state unused: List = []; Column { let ignored = state.unused; let visible = state.items; for item in visible key item.id { TextInput { width:100; height:30; value <-> item.title; } } } }".into())]);
    let mut doc = Document::from_ui_files(
        &files,
        "App.ui",
        &props(vec![(
            "items",
            Value::List(vec![Value::Object(props(vec![
                ("id", string("a")),
                ("title", string("Alpha")),
            ]))]),
        )]),
    )
    .unwrap();
    let (_, _, controls) = doc.render_scene().unwrap();
    doc.states[0].remove("unused");
    let before = doc.states.clone();
    assert!(
        resolve_binding_item(&controls[0], "item", &doc.states[0])
            .unwrap_err()
            .contains("Missing state.unused")
    );
    assert_eq!(doc.states, before);
}

#[test]
fn later_let_metadata_is_retained_when_not_represented_in_collection_frame() {
    let files = BTreeMap::from([("App.ui".into(), "component App { state items: List = []; state suffix: String = 'tail'; Column { let visible = state.items; for item in visible key item.id { let extra = state.suffix; TextInput { width:100; height:30; value <-> item.title; } } } }".into())]);
    let state = props(vec![(
        "items",
        Value::List(vec![Value::Object(props(vec![
            ("id", string("a")),
            ("title", string("Alpha")),
        ]))]),
    )]);
    let doc = Document::from_ui_files(&files, "App.ui", &state).unwrap();
    let (_, _, controls) = doc.render_scene().unwrap();
    assert_eq!(controls.len(), 1);
    assert_eq!(controls[0].state["extra"], string("tail"));
    let Value::Object(lexicals) = &controls[0].state["__binding_lexicals"] else {
        panic!("lexicals")
    };
    assert_eq!(lexicals["extra"], string("tail"));
    assert!(controls[0].state.contains_key("__binding_let_steps"));
}
#[test]
fn compact_lexical_frames_preserve_model_values_and_legacy_map_recovery() {
    let files = BTreeMap::from([("App.ui".into(), "component App { state items: List = []; Column { let visible = state.items; for item in visible key item.id { TextInput { width:100; height:30; value <-> item.title; } } } }".into())]);
    let item = |id: &str, title: &str| {
        Value::Object(props(vec![("id", string(id)), ("title", string(title))]))
    };
    let mut doc = Document::from_ui_files(
        &files,
        "App.ui",
        &props(vec![(
            "items",
            Value::List(vec![item("a", "Alpha"), item("b", "Beta")]),
        )]),
    )
    .unwrap();
    let mut legacy_doc = doc.clone();
    let (source, templates, controls) = doc.render_scene().unwrap();
    let mut legacy = controls.clone();
    for control in &mut legacy {
        let Value::List(frames) = control.state.get_mut("__binding_frames").unwrap() else {
            panic!("frames")
        };
        let Value::Object(frame) = &mut frames[0] else {
            panic!("frame")
        };
        let Value::Object(lexicals) = frame["__lexicals"].clone() else {
            panic!("lexicals")
        };
        let Value::Object(locals) = frame.get_mut("__locals").unwrap() else {
            panic!("locals")
        };
        assert!(!locals.contains_key("visible"));
        locals.extend(lexicals);
        let replay_lexicals = frame["__lexicals"].clone();
        let replay_steps = frame["__let_steps"].clone();
        control
            .state
            .insert("__binding_lexicals".into(), replay_lexicals);
        control
            .state
            .insert("__binding_let_steps".into(), replay_steps);
    }
    assert!(!controls[0].state.contains_key("__binding_lexicals"));
    assert!(!controls[0].state.contains_key("__binding_let_steps"));
    assert!(matches!(&controls[0].state["visible"], Value::List(items) if items.len() == 2));
    assert!(matches!(&controls[0].state["items"], Value::List(items) if items.len() == 2));
    let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
    runtime.set_binding_value(0, "edited", true).unwrap();
    let mut legacy_runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
    legacy_runtime.set_binding_value(0, "edited", true).unwrap();
    assert_eq!(
        doc.sync_runtime_bindings_from_scene(&runtime, None, &controls)
            .unwrap(),
        1
    );
    assert_eq!(
        legacy_doc
            .sync_runtime_bindings_from_scene(&legacy_runtime, None, &legacy)
            .unwrap(),
        1
    );
    assert_eq!(doc.states, legacy_doc.states);
}
#[test]
fn computed_item_member_keys_keep_captured_props_scope_during_recovery() {
    let files = BTreeMap::from([("App.ui".into(), "component App { prop prefix: String = 'stable'; state items: List = []; Column { let visible = state.items; let chosen = visible; for item in chosen key item.id { TextInput { width:100; height:30; value <-> item.title; } } } }".into())]);
    let item =
        |id: Value, title: &str| Value::Object(props(vec![("id", id), ("title", string(title))]));
    let mut doc = Document::from_ui_files(
        &files,
        "App.ui",
        &props(vec![(
            "items",
            Value::List(vec![
                item(expr("props.prefix"), "Alpha"),
                item(string("other"), "Beta"),
            ]),
        )]),
    )
    .unwrap();
    let (source, templates, controls) = doc.render_scene().unwrap();
    let frames = controls[0].state["__binding_frames"].clone();
    let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
    runtime.set_binding_value(0, "edited", true).unwrap();
    let Value::List(items) = doc.states[0].get_mut("items").unwrap() else {
        panic!("items")
    };
    items.swap(0, 1);
    assert_eq!(
        doc.sync_runtime_bindings_from_scene(&runtime, None, &controls)
            .unwrap(),
        1
    );
    let Value::List(items) = &doc.states[0]["items"] else {
        panic!("items")
    };
    let Value::Object(first) = &items[0] else {
        panic!("first")
    };
    let Value::Object(second) = &items[1] else {
        panic!("second")
    };
    assert_eq!(first["title"], string("Beta"));
    assert_eq!(second["title"], string("edited"));
    assert_eq!(controls[0].state["__binding_frames"], frames);
}
#[test]
fn flat_frame_owned_control_snapshots_survive_subsequent_model_replacement() {
    let files = BTreeMap::from([("App.ui".into(), "component App { state items: List = []; Frame { width:260; height:300; for item in state.items key item.id { TextInput { width:200; height:32; value <-> item.title; } } } }".into())]);
    let item = |id: &str, title: &str| {
        Value::Object(props(vec![("id", string(id)), ("title", string(title))]))
    };
    let mut doc = Document::from_ui_files(
        &files,
        "App.ui",
        &props(vec![(
            "items",
            Value::List(vec![item("a", "Alpha"), item("b", "Beta")]),
        )]),
    )
    .unwrap();
    let first = doc.render_scene_artifact().unwrap();
    assert_eq!(first.controls.len(), 2);
    doc.states[0].insert("items".into(), Value::List(vec![item("c", "Changed")]));
    let second = doc.render_scene_artifact().unwrap();
    assert_eq!(second.controls.len(), 1);
    for (control, title) in first.controls.iter().zip(["Alpha", "Beta"]) {
        let Value::Object(local) = &control.state["item"] else {
            panic!("item snapshot")
        };
        assert_eq!(local["title"], string(title));
        let Value::List(items) = &control.state["items"] else {
            panic!("root snapshot")
        };
        assert_eq!(items.len(), 2);
    }
    let runtime = crate::Runtime::from_scene_artifact(&first).unwrap();
    assert_eq!(runtime.control_count(), 2);
    assert_eq!(runtime.text_value(0), "Alpha");
    let runtime = crate::Runtime::from_scene_artifact(&second).unwrap();
    assert_eq!(runtime.control_count(), 1);
    assert_eq!(runtime.text_value(0), "Changed");
}
#[test]
fn scene_snapshot_transfer_matches_borrowed_instantiation_and_restores_on_error() {
    let mut doc = document(root(vec![]));
    let state = props(vec![
        ("caption", string("snapshot")),
        ("items", Value::List(vec![number(1.), number(2.)])),
    ]);
    let mut instance = node(
        "Control",
        vec![
            ("text", expr("props.__state.caption")),
            ("__state", Value::Object(state.clone())),
        ],
        vec![],
    );
    let original = instance.clone();
    let expected = doc.instantiate(&instance, true, &state, &[]).unwrap();
    let actual = doc.instantiate_scene(&mut instance).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(instance, original);
    doc.definitions.get_mut("Control").unwrap().nodes =
        vec![node("Text", vec![("text", string("invalid root"))], vec![])];
    assert!(
        doc.instantiate_scene(&mut instance)
            .unwrap_err()
            .contains("Rectangle root")
    );
    assert_eq!(
        instance, original,
        "failed evaluation must return the owned state snapshot"
    );
    doc.definitions.get_mut("Control").unwrap().nodes = vec![node("Rectangle", vec![], vec![])];
    doc.definitions
        .get_mut("Control")
        .unwrap()
        .defaults
        .insert("__state".into(), Value::Object(state));
    let mut missing_snapshot = node("Control", vec![("text", expr("state.caption"))], vec![]);
    assert!(
        doc.instantiate(&missing_snapshot, true, &Properties::new(), &[])
            .is_err()
    );
    assert!(doc.instantiate_scene(&mut missing_snapshot).is_err());
    assert!(
        !missing_snapshot.props.contains_key("__state"),
        "definition defaults must not become the caller state"
    );
}
#[test]
fn borrowed_definition_content_is_instantiated_without_mutating_shared_templates() {
    let mut doc = document(root(vec![]));
    doc.definitions.get_mut("Control").unwrap().nodes = vec![node(
        "Rectangle",
        vec![],
        vec![node(
            "ContentPresenter",
            vec![("key", string("content"))],
            vec![],
        )],
    )];
    let original = doc.definitions["Control"].nodes.clone();
    let first = doc
        .instantiate(
            &node(
                "Control",
                vec![],
                vec![node("Text", vec![("text", string("first"))], vec![])],
            ),
            true,
            &Properties::new(),
            &[],
        )
        .unwrap()
        .0;
    let second = doc
        .instantiate(
            &node(
                "Control",
                vec![],
                vec![node("Text", vec![("text", string("second"))], vec![])],
            ),
            true,
            &Properties::new(),
            &[],
        )
        .unwrap()
        .0;
    assert_eq!(first.children[0].children[0].props["text"], string("first"));
    assert_eq!(
        second.children[0].children[0].props["text"],
        string("second")
    );
    assert_eq!(doc.definitions["Control"].nodes, original);
    let empty = doc
        .instantiate(
            &node("Control", vec![], vec![]),
            true,
            &Properties::new(),
            &[],
        )
        .unwrap()
        .0;
    assert!(empty.children[0].children.is_empty());
    assert_eq!(doc.definitions["Control"].nodes, original);
}
#[test]
fn visual_sequence_borrowing_preserves_lexical_shadowing_and_owned_results() {
    let doc = document(root(vec![]));
    let label = || node("Text", vec![("text", expr("state.local"))], vec![]);
    let binding = |value: &str| {
        node(
            "Let",
            vec![("name", string("local")), ("value", string(value))],
            vec![],
        )
    };
    let nodes = vec![
        label(),
        binding("later"),
        label(),
        node("Frame", vec![], vec![binding("inner"), label()]),
        label(),
    ];
    let mut state = props(vec![("local", string("parent"))]);
    let expanded = doc
        .expand_sequence(&nodes, &Properties::new(), &state, &[], 0)
        .unwrap();
    assert_eq!(expanded[0].props["text"], string("parent"));
    assert_eq!(expanded[1].props["text"], string("later"));
    assert_eq!(expanded[2].children[0].props["text"], string("inner"));
    assert_eq!(expanded[3].props["text"], string("later"));
    assert_eq!(state["local"], string("parent"));
    state.insert("local".into(), string("changed"));
    assert_eq!(expanded[0].props["text"], string("parent"));
}
#[test]
fn scene_scope_writes_are_local_and_returned_snapshots_remain_owned() {
    let control = || node("Control", vec![("text", expr("state.local"))], vec![]);
    let binding = |value: &str| {
        node(
            "Let",
            vec![("name", string("local")), ("value", string(value))],
            vec![],
        )
    };
    let mut doc = document(root(vec![
        control(),
        control(),
        binding("later"),
        control(),
        node(
            "Frame",
            vec![("width", number(180.)), ("height", number(30.))],
            vec![binding("inner"), control()],
        ),
        control(),
    ]));
    doc.states = vec![Properties::new(); 9];
    doc.states[0].insert("local".into(), string("parent"));
    doc.states[2].insert("local".into(), string("node"));
    let (_, _, snapshots) = doc.render_scene().unwrap();
    let values: Vec<_> = snapshots
        .iter()
        .map(|control| control.state["local"].text().unwrap())
        .collect();
    assert_eq!(values, ["parent", "node", "later", "inner", "later"]);
    assert_eq!(doc.states[0]["local"], string("parent"));
    doc.states[0].insert("local".into(), string("changed"));
    let (_, _, next) = doc.render_scene().unwrap();
    assert_eq!(next[0].state["local"], string("changed"));
    assert_eq!(snapshots[0].state["local"], string("parent"));
    assert_eq!(next[1].state["local"], string("node"));
    assert_eq!(next[4].state["local"], string("later"));
}
#[test]
fn flex_does_not_freeze_conflicting_minimum_and_maximum_together() {
    let row = normalize_layout(&node(
        "Row",
        vec![],
        vec![
            node(
                "Text",
                vec![("width", expr("*")), ("minWidth", number(80.))],
                vec![],
            ),
            node(
                "Text",
                vec![("width", expr("*")), ("maxWidth", number(40.))],
                vec![],
            ),
        ],
    ))
    .unwrap();
    assert_eq!(sizes(&row, 0, 100.).unwrap(), vec![80., 20.]);
}
#[test]
fn conditional_layout_removes_the_cell_and_grid_adds_implicit_rows() {
    let mut grid = node(
        "Grid",
        vec![("columns", Value::List(vec![expr("*"), expr("*")]))],
        vec![
            node("Text", vec![], vec![]),
            node("Text", vec![], vec![]),
            node("Text", vec![], vec![]),
        ],
    );
    grid = normalize_layout(&grid).unwrap();
    assert_eq!(tracks(&grid.props, "rows").len(), 2);
    assert_eq!(grid.children[2].props["row"], number(2.));
    let mut doc = document(root(vec![node(
        "Row",
        vec![],
        vec![
            node(
                "If",
                vec![("condition", expr("state.show"))],
                vec![node("Control", vec![("width", number(30.))], vec![])],
            ),
            node(
                "Control",
                vec![("width", expr("*")), ("maxWidth", number(100.))],
                vec![],
            ),
            node("Control", vec![("width", expr("*"))], vec![]),
        ],
    )]));
    doc.states = vec![props(vec![("show", Value::Bool(false))])];
    let (source, templates, mapping) = doc.render_scene().unwrap();
    assert_eq!(mapping.len(), 2);
    let runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
    assert_eq!(runtime.control_bounds(0)[2], 100.);
    assert_eq!(runtime.control_bounds(1)[2], 200.);
    doc.states[0].insert("show".into(), Value::Bool(true));
    let (source, templates, mapping) = doc.render_scene().unwrap();
    assert_eq!(mapping.len(), 3);
    let runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
    assert_eq!(runtime.control_bounds(2)[2], 170.);
}

#[test]
fn rust_ui_compiler_resolves_inheritance_overrides_tokens_svg_and_state() {
    let mut files: BTreeMap<String, String> = BTreeMap::new();
    files.insert(
            "theme.ui".into(),
            "component Theme { tokens { brand: #3b82f6; surface: #1e293b; } enum Tone { Primary, Danger } }".into(),
        );
    files.insert(
            "icon.svg".into(),
            "<svg viewBox=\"0 0 16 16\"><circle cx=\"8\" cy=\"8\" r=\"6\" fill=\"currentColor\"/></svg>".into(),
        );
    files.insert(
            "BaseButton.component.ui".into(),
            "component BaseButton { use 'theme.ui'; prop text: String = 'Click'; prop tone: Tone = Tone.Primary; width: 120; height: 36; background: tokens.surface; match props.tone { Tone.Primary => { background: tokens.brand; } Tone.Danger => { background: #ef4444; } } Rectangle { slot: 'frame'; Row { padding: 4; gap: 6; Image { slot: 'icon'; source: 'icon.svg'; width: 16; height: 16; color: #ffffff; } Text { slot: 'label'; text: props.text; color: #ffffff; } } PointerArea { clicked -> events.clicked(); } } }".into(),
        );
    files.insert(
            "DangerButton.component.ui".into(),
            "component DangerButton : BaseButton { tone: Tone.Danger; override label { color: #ffe4e6; } }".into(),
        );
    files.insert(
            "App.ui".into(),
            "component App { state count: Number = 3; Column { width: 240; height: 120; padding: 12; gap: 10; BaseButton { key: 'primary'; text: 'Count: ${state.count}'; clicked -> actions.increment(); } DangerButton { key: 'danger'; text: 'Reset'; clicked -> actions.reset(); } } }".into(),
        );
    let doc = Document::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
    assert!(doc.images.contains_key("icon.svg"));
    let (scene_src, templates, controls) = doc.render_scene().unwrap();
    assert_eq!(controls.len(), 2);
    assert_eq!(controls[0].key, "primary");
    assert_eq!(controls[1].key, "danger");
    assert!(templates.contains("ContentText { x:"));
    assert!(templates.contains("ContentShape {"));
    let runtime = crate::Runtime::from_sources(&scene_src, &templates).unwrap();
    assert_eq!(runtime.control_count(), 2);
}

#[test]
fn rust_ui_compiler_supports_apply_styles_let_match_guards_and_base_slot_wrapping() {
    let mut files: BTreeMap<String, String> = BTreeMap::new();
    files.insert(
            "Card.component.ui".into(),
            "component Card { style pill { radius: 12; } prop score: Number = 85; prop active: Bool = true; width: 180; height: 64; Rectangle { apply: pill; Column { slot: 'body'; let tier = props.score >= 80 ? 'gold' : 'silver'; match tier { 'gold' if props.active => { Text { text: 'Gold (${props.score})'; } } _ => { Text { text: 'Standard'; } } } } } }".into(),
        );
    files.insert(
            "BadgeCard.component.ui".into(),
            "component BadgeCard : Card { override body: Column { gap: 4; Text { text: 'Verified'; } }; }".into(),
        );
    files.insert(
            "App.ui".into(),
            "component App { Column { width: 220; height: 140; gap: 8; Card { key: 'card'; score: 92; active: true; } BadgeCard { key: 'badge'; score: 70; active: true; } } }".into(),
        );
    let doc = Document::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
    let (scene_src, templates, controls) = doc.render_scene().unwrap();
    assert_eq!(controls.len(), 2);
    assert_eq!(controls[0].key, "card");
    assert_eq!(controls[1].key, "badge");
    assert!(templates.contains("radius: 12;"));
    assert!(templates.contains("Gold (92)"));
    assert!(templates.contains("Verified"));
    let runtime = crate::Runtime::from_sources(&scene_src, &templates).unwrap();
    assert_eq!(runtime.control_count(), 2);
}

#[test]
fn rust_ui_compiler_evaluates_and_synchronizes_two_way_bindings() {
    let mut files: BTreeMap<String, String> = BTreeMap::new();
    files.insert(
            "Toggle.component.ui".into(),
            "component Toggle { prop checked: Bool = false; prop label: String = ''; width: 140; height: 32; Rectangle { Row { padding: 4; Text { text: props.checked ? '[x] ${props.label}' : '[ ] ${props.label}'; } } PointerArea { clicked -> events.clicked(); } } }".into(),
        );
    files.insert(
            "App.ui".into(),
            "component App { state query: String = 'forma'; state agreed: Bool = false; Column { width: 260; height: 140; padding: 10; gap: 8; TextInput { key: 'search'; width: 200; height: 32; value <-> state.query; } Toggle { key: 'agree'; label: 'Terms'; checked <-> state.agreed; } Text { key: 'summary'; text: '${state.query}:${state.agreed}'; } } }".into(),
        );
    let mut doc = Document::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
    let (scene_src, templates, controls) = doc.render_scene().unwrap();
    assert_eq!(controls.len(), 3);
    let mut runtime = crate::Runtime::from_sources(&scene_src, &templates).unwrap();
    assert_eq!(runtime.text_value(0), "forma");

    // Edit the TextInput and click the Toggle
    assert!(runtime.focus_control(0));
    assert!(runtime.text_key("End", false, false));
    assert!(runtime.text_insert("-ui"));
    assert_eq!(runtime.text_value(0), "forma-ui");
    runtime.activate_control(1);

    // Synchronize bindings back to Document
    let changed = doc.sync_runtime_bindings(&runtime, Some(1)).unwrap();
    assert_eq!(changed, 2);
    let state_json = doc.state_json().unwrap();
    assert!(state_json.contains("\"query\":\"forma-ui\""));
    assert!(state_json.contains("\"agreed\":true"));

    // Re-render and verify summary Text reflects updated bound state
    let (_, next_templates, _) = doc.render_scene().unwrap();
    assert!(next_templates.contains("forma-ui:true"));
    assert!(next_templates.contains("[x] Terms"));
}
#[test]
fn binding_sync_rejects_invalid_metadata_without_partial_model_writes() {
    for (cached, empty) in [(false, false), (true, false), (true, true)] {
        let files = BTreeMap::from([("App.ui".to_string(),
                "component App { state first: String = 'one'; state second: String = 'two'; Column { width:260; height:100; TextInput { width:200; height:32; value <-> state.first; } TextInput { width:200; height:32; value <-> state.second; } } }".to_string())]);
        let mut doc = Document::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
        let (source, templates, controls) = doc.render_scene().unwrap();
        let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
        runtime.set_binding_value(0, "edited", true).unwrap();
        if empty {
            doc.states.clear();
        }
        let before = doc.states.clone();
        fn second_binding(node: &mut Node, count: &mut usize, value: Value) -> bool {
            if let Some(Value::Object(bindings)) = node.props.get_mut("__bindings") {
                *count += 1;
                if *count == 2 {
                    bindings.insert("value".into(), value);
                    return true;
                }
            }
            node.children
                .iter_mut()
                .any(|node| second_binding(node, count, value.clone()))
        }
        assert!(second_binding(&mut doc.root, &mut 0, Value::List(vec![])));
        let result = if cached {
            doc.sync_runtime_bindings_from_scene(&runtime, None, &controls)
        } else {
            doc.sync_runtime_bindings(&runtime, None)
        };
        assert!(result.unwrap_err().contains("Expected scalar text"));
        assert_eq!(
            doc.states, before,
            "a later invalid binding must roll back earlier writes"
        );
        assert!(second_binding(
            &mut doc.root,
            &mut 0,
            Value::Text("state.second".into())
        ));
        let changed = if cached {
            doc.sync_runtime_bindings_from_scene(&runtime, None, &controls)
                .unwrap()
        } else {
            doc.sync_runtime_bindings(&runtime, None).unwrap()
        };
        assert_eq!(changed, if empty { 2 } else { 1 });
        assert_eq!(doc.states[0]["first"], Value::Text("edited".into()));
        assert_eq!(doc.states[0]["second"], Value::Text("two".into()));
    }
}

#[test]
fn recovery_binding_map_refuses_excessive_source_depth_before_writing() {
    let files = BTreeMap::from([("App.ui".to_string(),
            "component App { state value: String = 'seed'; Column { width:260; height:100; TextInput { width:200; height:32; value <-> state.value; } } }".to_string())]);
    let mut doc = Document::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
    let (source, templates, controls) = doc.render_scene().unwrap();
    let mut runtime = crate::Runtime::from_sources(&source, &templates).unwrap();
    runtime.set_binding_value(0, "edited", true).unwrap();
    let before = doc.states.clone();
    let mut nested = Node {
        kind: "Frame".into(),
        props: Properties::new(),
        children: vec![],
        action: None,
    };
    for _ in 0..66 {
        nested = Node {
            kind: "Frame".into(),
            props: Properties::new(),
            children: vec![nested],
            action: None,
        };
    }
    doc.root.children.push(nested);
    let error = doc
        .sync_runtime_bindings_from_scene(&runtime, None, &controls)
        .unwrap_err();
    assert!(error.contains("depth exceeded"));
    assert_eq!(doc.states, before);
    assert!(doc.render_scene().unwrap_err().contains("depth exceeded"));
    doc.root.children.pop();
    doc.root.children.extend((0..16_384).map(|_| Node {
        kind: "Frame".into(),
        props: Properties::new(),
        children: vec![],
        action: None,
    }));
    assert!(
        doc.sync_runtime_bindings_from_scene(&runtime, None, &controls)
            .unwrap_err()
            .contains("16384 nodes")
    );
    assert_eq!(doc.states, before);
    assert!(doc.render_scene().unwrap_err().contains("16384 nodes"));
}

#[test]
fn wrapped_text_component_emits_multiline_content_and_measures_height() {
    let files = BTreeMap::from([(
            "App.ui".to_string(),
            "component App { Column { width: 200; height: 160; Text { key: 'wrapped'; width: 120; height: 'auto'; wrap: true; text: 'Hello world from Forma soft wrapped text'; } } }".to_string(),
        )]);
    let doc = Document::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
    let (scene_src, templates, controls) = doc.render_scene().unwrap();
    assert_eq!(controls.len(), 1);
    let content_text_count = templates.matches("ContentText {").count();
    assert!(
        content_text_count >= 2,
        "expected wrapped Text to emit >= 2 ContentText lines, got {content_text_count}: {templates}"
    );
    let runtime = crate::Runtime::from_sources(&scene_src, &templates).unwrap();
    assert_eq!(runtime.control_count(), 1);
}

#[test]
fn let_inside_empty_for_branch_and_block_let_scoping_match_between_visual_and_scene() {
    let control = || node("Control", vec![("text", expr("state.msg"))], vec![]);
    let mut doc = document(root(vec![
        node(
            "Let",
            vec![("name", string("msg")), ("value", string("scoped"))],
            vec![control()],
        ),
        control(),
        node(
            "For",
            vec![
                ("item", string("item")),
                ("collection", expr("state.items")),
                ("key", expr("item.id")),
            ],
            vec![
                control(),
                node(
                    "Empty",
                    vec![],
                    vec![
                        node(
                            "Let",
                            vec![("name", string("msg")), ("value", string("empty-fallback"))],
                            vec![],
                        ),
                        control(),
                    ],
                ),
            ],
        ),
    ]));
    doc.states = vec![props(vec![
        ("msg", string("outer")),
        ("items", Value::List(vec![])),
    ])];
    let (_, _, snapshots) = doc.render_scene().unwrap();
    let scene_texts: Vec<_> = snapshots
        .iter()
        .map(|c| c.state["msg"].text().unwrap())
        .collect();
    assert_eq!(scene_texts, ["scoped", "outer", "empty-fallback"]);

    // Verify visual expansion follows the exact same scoping rules.
    let label = || node("Text", vec![("text", expr("state.msg"))], vec![]);
    let visual_nodes = vec![
        node(
            "Let",
            vec![("name", string("msg")), ("value", string("scoped"))],
            vec![label()],
        ),
        label(),
        node(
            "For",
            vec![
                ("item", string("item")),
                ("collection", expr("state.items")),
                ("key", expr("item.id")),
            ],
            vec![
                label(),
                node(
                    "Empty",
                    vec![],
                    vec![
                        node(
                            "Let",
                            vec![("name", string("msg")), ("value", string("empty-fallback"))],
                            vec![],
                        ),
                        label(),
                    ],
                ),
            ],
        ),
    ];
    let visual_expanded = doc
        .expand_sequence(&visual_nodes, &Properties::new(), &doc.states[0], &[], 0)
        .unwrap();
    let visual_texts: Vec<_> = visual_expanded
        .iter()
        .map(|n| n.props["text"].text().unwrap())
        .collect();
    assert_eq!(visual_texts, ["scoped", "outer", "empty-fallback"]);
}
