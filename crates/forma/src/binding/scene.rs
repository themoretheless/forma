//! Writing inputs and control overrides into the document before a render.

use super::*;

/// Copies the document's inputs into the state of the nodes that declare them. An absent input
/// is written only when it is a list, which a `For` must still be able to iterate.
pub(super) fn write_inputs_into_states(document: &mut DocumentState) {
    for (node, path, input) in &document.inputs {
        while document.program.states.len() <= *node {
            document.program.states.push(Default::default());
        }
        if input.present() {
            document.program.states[*node].insert(path.clone(), input.value());
        } else {
            let val = input.value();
            if matches!(val, crate::form_document::Value::List(_)) {
                document.program.states[*node].insert(path.clone(), val);
            }
        }
    }
}

/// Writes each control's properties and bound values onto the document node it overrides.
pub(super) fn write_control_overrides(document: &mut DocumentState, controls: &[Rc<ControlState>]) {
    // Controls that override each document node, in control order.
    let mut overrides = std::collections::HashMap::<usize, Vec<_>>::new();
    for control in controls {
        if let Some(index) = document.nodes.get(&control.index) {
            overrides.entry(*index).or_default().push(control);
        }
    }
    let winners = override_targets(&document.program.root);
    for_each_node_post_order(
        &mut document.program.root,
        &mut 0,
        &mut 0,
        &mut |id, position, node| {
            if winners.get(&id) != Some(&position) {
                return;
            }
            for control in overrides.get(&id).map_or(&[][..], Vec::as_slice) {
                for (name, input) in control.properties.borrow().iter() {
                    node.props.insert(name.clone(), input.value());
                }
                for (target, binding) in control.bindings.borrow().iter() {
                    node.props
                        .insert(target.property_name().into(), binding.node_value());
                    if node.kind == "Slider" && *target == Target::Value {
                        node.props.insert(
                            "__range_normalized".into(),
                            crate::form_document::Value::Bool(true),
                        );
                    }
                }
            }
        },
    );
}

/// Checks the arguments each rendered control passes to its events against the declared types.
pub(super) fn check_event_arguments(
    document: &DocumentState,
    scene: &[crate::form_document::SceneControl],
) -> Result<(), String> {
    for item in scene {
        for ((node, event), types) in &document.event_types {
            if *node != item.node {
                continue;
            }
            let empty_args = Vec::new();
            let arguments = item.event_args.get(event).unwrap_or(&empty_args);
            if arguments.len() != types.len() {
                return Err(format!("Event {event} expects {} arguments", types.len()));
            }
            for (value, kind) in arguments.iter().zip(types) {
                if !event_argument_matches(kind, value) {
                    return Err(format!("Event {event} expects {kind}, got {value:?}"));
                }
            }
        }
    }
    Ok(())
}

/// Whether `value` is of the declared event argument type `kind`; a trailing `?` admits null.
pub(super) fn event_argument_matches(kind: &str, value: &crate::form_document::Value) -> bool {
    use crate::form_document::Value;
    if kind.ends_with('?') && matches!(value, Value::Null) {
        return true;
    }
    match kind.trim_end_matches('?') {
        "String" => matches!(value, Value::Text(_)),
        "Bool" | "Boolean" => matches!(value, Value::Bool(_)),
        "Number" | "Float" => value.number().is_ok(),
        "Int" => i64::from_value(value).is_ok(),
        _ => false,
    }
}

/// The id a node answers to: its explicit `__node` property, else its preorder position.
pub(super) fn node_id(node: &crate::form_document::Node, preorder: usize) -> usize {
    node.props
        .get("__node")
        .and_then(|value| value.number().ok())
        .map_or(preorder, |v| v as usize)
}

/// Visits every node after its children, passing its id and its post-order position.
pub(super) fn for_each_node_post_order(
    node: &mut crate::form_document::Node,
    preorder: &mut usize,
    postorder: &mut usize,
    visit: &mut impl FnMut(usize, usize, &mut crate::form_document::Node),
) {
    let id = node_id(node, *preorder);
    *preorder += 1;
    for child in &mut node.children {
        for_each_node_post_order(child, preorder, postorder, visit);
    }
    let position = *postorder;
    *postorder += 1;
    visit(id, position, node);
}

/// For each node id, the post-order position of the node that receives control overrides.
/// When several nodes share an id, the last one in post-order wins.
pub(super) fn override_targets(
    root: &crate::form_document::Node,
) -> std::collections::HashMap<usize, usize> {
    fn walk(
        node: &crate::form_document::Node,
        preorder: &mut usize,
        postorder: &mut usize,
        out: &mut std::collections::HashMap<usize, usize>,
    ) {
        let id = node_id(node, *preorder);
        *preorder += 1;
        for child in &node.children {
            walk(child, preorder, postorder, out);
        }
        out.insert(id, *postorder);
        *postorder += 1;
    }
    let mut out = std::collections::HashMap::new();
    walk(root, &mut 0, &mut 0, &mut out);
    out
}
