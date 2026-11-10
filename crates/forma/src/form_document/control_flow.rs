//! Shared zero-allocation control-flow selection and lexical binding helpers
//! for both visual expansion (`visual.rs`) and scene expansion (`scene.rs`).

use super::{Node, Properties, Value, pattern_matches, text};

/// Evaluates an `If` node's `condition` property into a boolean.
pub(super) fn evaluate_if_condition(
    node: &Node,
    props: &Properties,
    state: &Properties,
) -> Result<bool, String> {
    node.props
        .get("condition")
        .ok_or("If requires a condition")?
        .evaluate(props, state)?
        .boolean()
}

/// Evaluates a `For` node's `collection` property into a list and resolves its `item` variable name.
pub(super) fn evaluate_for_header(
    node: &Node,
    props: &Properties,
    state: &Properties,
) -> Result<(Vec<Value>, String), String> {
    let Value::List(items) = node
        .props
        .get("collection")
        .ok_or("For requires a collection")?
        .evaluate(props, state)?
    else {
        return Err("For expects a list".into());
    };
    let item = text(&node.props, "item", "item")?;
    Ok((items, item))
}

/// Evaluates a `Let` node's `name` and `value` properties if both are present and non-empty.
/// Returns `Ok(Some((name, evaluated_value, value_expr)))` or `Ok(None)` when the declaration is empty.
pub(super) fn evaluate_let_binding<'a>(
    node: &'a Node,
    props: &Properties,
    state: &Properties,
) -> Result<Option<(String, Value, &'a Value)>, String> {
    let name = text(&node.props, "name", "")?;
    if name.is_empty() {
        return Ok(None);
    }
    let Some(expression) = node.props.get("value") else {
        return Ok(None);
    };
    let value = expression.evaluate(props, state)?;
    Ok(Some((name, value, expression)))
}

/// Tests whether a `Branch` child of a `Match` node matches `subject` and passes its optional `guard`.
pub(super) fn branch_matches(
    branch: &Node,
    subject: &Value,
    props: &Properties,
    state: &Properties,
) -> Result<bool, String> {
    if branch.kind != "Branch" {
        return Ok(false);
    }
    let pat = branch
        .props
        .get("pattern")
        .ok_or("Match branch requires a pattern")?;
    if !pattern_matches(pat, subject, props, state, &mut Vec::new())? {
        return Ok(false);
    }
    match branch.props.get("guard") {
        Some(guard) => guard.evaluate(props, state)?.boolean(),
        None => Ok(true),
    }
}

/// Iterator over non-`Else` children of an `If` node without heap allocation.
pub(super) fn if_then_children(node: &Node) -> impl Iterator<Item = &Node> {
    node.children.iter().filter(|n| n.kind != "Else")
}

/// Finds the first `Else` child of an `If` node, if any.
pub(super) fn if_else_node(node: &Node) -> Option<&Node> {
    node.children.iter().find(|n| n.kind == "Else")
}

/// Iterator over non-`Empty` body children of a `For` node without heap allocation.
pub(super) fn for_body_children(node: &Node) -> impl Iterator<Item = &Node> {
    node.children.iter().filter(|n| n.kind != "Empty")
}

/// Finds the first `Empty` child of a `For` node, if any.
pub(super) fn for_empty_node(node: &Node) -> Option<&Node> {
    node.children.iter().find(|n| n.kind == "Empty")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::form_document::{Value, props};

    fn make_node(kind: &str, p: Vec<(&str, Value)>, children: Vec<Node>) -> Node {
        Node {
            kind: kind.into(),
            props: props(p),
            children,
            action: None,
        }
    }

    #[test]
    fn control_flow_helpers_select_branches_and_evaluate_let_without_allocating_child_vecs() {
        let state = props(vec![
            ("flag", Value::Bool(true)),
            (
                "items",
                Value::List(vec![Value::Number(1.), Value::Number(2.)]),
            ),
            ("x", Value::Text("ok".into())),
        ]);
        let scope = Properties::new();

        let if_node = make_node(
            "If",
            vec![("condition", Value::Expr("state.flag".into()))],
            vec![
                make_node("Text", vec![], vec![]),
                make_node("Else", vec![], vec![make_node("Rectangle", vec![], vec![])]),
            ],
        );
        assert!(evaluate_if_condition(&if_node, &scope, &state).unwrap());
        assert_eq!(if_then_children(&if_node).count(), 1);
        assert!(if_else_node(&if_node).is_some());

        let for_node = make_node(
            "For",
            vec![
                ("collection", Value::Expr("state.items".into())),
                ("item", Value::Text("row".into())),
            ],
            vec![
                make_node("Text", vec![], vec![]),
                make_node("Empty", vec![], vec![]),
            ],
        );
        let (items, item_name) = evaluate_for_header(&for_node, &scope, &state).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(item_name, "row");
        assert_eq!(for_body_children(&for_node).count(), 1);
        assert!(for_empty_node(&for_node).is_some());

        let let_node = make_node(
            "Let",
            vec![
                ("name", Value::Text("alias".into())),
                ("value", Value::Expr("state.x".into())),
            ],
            vec![],
        );
        let bound = evaluate_let_binding(&let_node, &scope, &state)
            .unwrap()
            .unwrap();
        assert_eq!(bound.0, "alias");
        assert_eq!(bound.1, Value::Text("ok".into()));
    }
}
