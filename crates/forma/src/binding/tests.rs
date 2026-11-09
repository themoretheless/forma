
use super::*;

fn node(
    kind: &str,
    explicit_id: Option<f64>,
    children: Vec<crate::form_document::Node>,
) -> crate::form_document::Node {
    let mut props = crate::form_document::Properties::new();
    if let Some(id) = explicit_id {
        props.insert("__node".into(), crate::form_document::Value::Number(id));
    }
    crate::form_document::Node {
        kind: kind.into(),
        props,
        children,
        action: None,
    }
}

#[test]
fn control_overrides_reach_one_node_per_id_and_the_last_in_post_order_wins() {
    // Preorder ids: root 0, a 1, a1 2, b 3. `late` claims id 1 explicitly and comes later
    // in post-order than `a`, so it is the node an override for id 1 must land on.
    let mut root = node(
        "root",
        None,
        vec![
            node("a", None, vec![node("a1", None, vec![])]),
            node("b", None, vec![]),
            node("late", Some(1.), vec![]),
        ],
    );
    let winners = override_targets(&root);
    let mut visited = Vec::new();
    let mut chosen = Vec::new();
    for_each_node_post_order(&mut root, &mut 0, &mut 0, &mut |id, position, node| {
        visited.push((node.kind.clone(), id, position));
        if winners.get(&id) == Some(&position) {
            chosen.push((id, node.kind.clone()));
        }
    });
    assert_eq!(
        visited,
        [
            ("a1".to_owned(), 2, 0),
            ("a".to_owned(), 1, 1),
            ("b".to_owned(), 3, 2),
            ("late".to_owned(), 1, 3),
            ("root".to_owned(), 0, 4),
        ]
    );
    chosen.sort();
    assert_eq!(
        chosen,
        [
            (0, "root".to_owned()),
            (1, "late".to_owned()),
            (2, "a1".to_owned()),
            (3, "b".to_owned())
        ]
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn svet_bindings_drive_runtime_and_release_retained_control() {
    let mut form = Form::new(
            "component Demo { Frame { width:100; height:60; Button { width:80; height:40; } } }",
            "component Button { Rectangle { ContentInput { width:80; height:30; value:'seed'; } PointerArea { clicked -> events.clicked(); } } }",
        ).unwrap();
    let control = form.add_control(0, Context::new(Rc::new(()))).unwrap();
    let property = Property::new("first".to_string());
    control.bind_value(property.clone());
    form.set_svet_bindings(true);
    form.sync().unwrap();
    assert_eq!(form.runtime().text_value(0), "first");
    assert_eq!(form.svet_binding_telemetry().unwrap().nodes, 2);
    property.set("second".into());
    property.set("third".into());
    let binding = control
        .state
        .bindings
        .borrow()
        .get(&Target::Value)
        .unwrap()
        .clone();
    assert!(!binding.dirty(), "only graph drain may dirty this binding");
    form.sync().unwrap();
    assert_eq!(form.runtime().text_value(0), "third");
    form.update(|runtime| {
        runtime.focus_control(0);
        runtime.text_insert("!");
    })
    .unwrap();
    assert_eq!(property.get(), form.runtime().text_value(0));
    assert!(property.get().contains('!'));
    property.set("pending".into());
    form.set_svet_bindings(false);
    form.sync().unwrap();
    assert_eq!(form.runtime().text_value(0), "pending");
    form.set_svet_bindings(true);
    drop(binding);
    property.set("stale".into());
    let replacement = Property::new("replacement".to_string());
    control.bind_value(replacement.clone());
    assert_eq!(form.svet_binding_telemetry().unwrap().nodes, 2);
    form.sync().unwrap();
    assert_eq!(form.runtime().text_value(0), "replacement");
    property.set("retired".into());
    assert!(!form.is_dirty());
    let wake = form.wake.clone();
    drop(form);
    assert!(wake.native.borrow().graph.is_none());
    assert!(wake.native.borrow().fields.is_empty());
    assert!(replacement.0.listeners.borrow().is_empty());
    drop(control);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn svet_scheduler_deduplicates_and_isolates_forms() {
    let wake = Rc::new(Wake::default());
    let property = Property::new(0);
    let field = Field::explicit(property.clone(), wake.clone());
    wake.native.borrow_mut().enable(true);
    field.0.dirty.set(false);
    for value in 1..100 {
        property.set(value);
    }
    assert!(!field.0.dirty.get());
    assert_eq!(wake.native.borrow_mut().drain(), 1);
    assert!(field.0.dirty.get());
    field.0.dirty.set(false);
    property.set(100);
    assert_eq!(wake.native.borrow_mut().drain(), 1);
    let other = Rc::new(Wake::default());
    let other_field = Field::explicit(Property::new(0), other.clone());
    other.native.borrow_mut().enable(true);
    other_field.0.dirty.set(false);
    property.set(101);
    assert_eq!(other.native.borrow_mut().drain(), 0);
    drop(field);
    assert_eq!(wake.native.borrow_mut().drain(), 0);
    assert_eq!(
        wake.native
            .borrow()
            .graph
            .as_ref()
            .unwrap()
            .telemetry()
            .nodes,
        0
    );
    assert!(!other_field.0.dirty.get());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn svet_context_replacement_and_reentrant_wake_preserve_latest_value() {
    struct Model {
        value: Property<i32>,
    }
    let first = Rc::new(Model {
        value: Property::new(1),
    });
    let second = Rc::new(Model {
        value: Property::new(2),
    });
    let context = Context::new(first.clone());
    let wake = Rc::new(Wake::default());
    let field = Field::path(&context, |model| model.value.clone(), wake.clone());
    wake.native.borrow_mut().enable(true);
    field.0.dirty.set(false);
    wake.pending.set(false);
    let weak = Rc::downgrade(&wake);
    let property = first.value.clone();
    *wake.callback.borrow_mut() = Some(Rc::new(move || {
        // Arbitrary host notification must run outside the graph RefCell borrow.
        assert_eq!(
            weak.upgrade()
                .unwrap()
                .native
                .borrow()
                .graph
                .as_ref()
                .unwrap()
                .telemetry()
                .nodes,
            2
        );
        property.set(3);
    }));
    first.value.set(4);
    assert_eq!(wake.native.borrow_mut().drain(), 1);
    assert_eq!(field.get(), Some(3));
    context.set(second.clone());
    assert_eq!(wake.native.borrow_mut().drain(), 1);
    assert_eq!(field.get(), Some(2));
    field.0.dirty.set(false);
    first.value.set(10);
    assert_eq!(wake.native.borrow_mut().drain(), 0);
    assert!(!field.0.dirty.get());
    second.value.set(20);
    assert_eq!(wake.native.borrow_mut().drain(), 1);
    assert_eq!(field.get(), Some(20));
}

#[test]
fn property_notifications_allow_reentrant_writes_and_disconnect_on_drop() {
    let property = Property::new(0);
    let observed = Rc::new(RefCell::new(Vec::new()));
    let values = observed.clone();
    let subscription = property.subscribe(move |value| values.borrow_mut().push(value));
    assert!(!property.set(0));
    assert!(property.set(1));
    drop(subscription);
    property.set(2);
    assert_eq!(*observed.borrow(), vec![1]);

    let weak = Rc::downgrade(&property.0);
    let listener = property.subscribe(move |value| {
        if value == 3 {
            Property(weak.upgrade().unwrap()).set(4);
        }
    });
    property.set(3);
    assert_eq!(property.get(), 4);
    drop(listener);
}

#[test]
fn inherited_context_tracks_replacements_while_overridden_and_releases_sources() {
    struct Model;
    let first = Rc::new(Model);
    let second = Rc::new(Model);
    let local = Rc::new(Model);
    let first_weak = Rc::downgrade(&first);
    let second_weak = Rc::downgrade(&second);
    let root = Context::new(first.clone());
    let child = Context::child(&root);
    child.set(local.clone());
    root.set(second.clone());
    assert!(Rc::ptr_eq(&child.get().unwrap(), &local));
    child.inherit();
    assert!(Rc::ptr_eq(&child.get().unwrap(), &second));
    drop(first);
    assert!(first_weak.upgrade().is_none());
    drop(second);
    drop(root);
    drop(child);
    assert!(second_weak.upgrade().is_none());
}
