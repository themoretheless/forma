//! Writing a value back by path.

use super::*;

pub(in crate::form_document) fn write_collection_item_path(
    target: &mut Properties,
    col_path: &str,
    item_idx: usize,
    item_tail: &str,
    value: Value,
) -> bool {
    match lookup_binding_mut(target, col_path) {
        Some(Value::List(items)) => match items.get_mut(item_idx) {
            Some(Value::Object(item_obj)) => write_state_path(item_obj, item_tail, value),
            _ => false,
        },
        _ => false,
    }
}

pub(in crate::form_document) fn write_state_path(
    target: &mut Properties,
    path: &str,
    value: Value,
) -> bool {
    if let Some((head, tail)) = path.split_once('.') {
        match target.get_mut(head) {
            Some(Value::Object(obj)) => write_state_path(obj, tail, value),
            Some(Value::List(items)) => write_list_path(items, tail, value),
            _ => {
                let mut nested = Properties::new();
                write_state_path(&mut nested, tail, value);
                target.insert(head.into(), Value::Object(nested));
                true
            }
        }
    } else {
        if target.get(path) == Some(&value) {
            return false;
        }
        target.insert(path.into(), value);
        true
    }
}

pub(super) fn write_list_path(items: &mut [Value], path: &str, value: Value) -> bool {
    let (index, tail) = path
        .split_once('.')
        .map_or((path, None), |(index, tail)| (index, Some(tail)));
    let Some(target) = index
        .parse::<usize>()
        .ok()
        .and_then(|index| items.get_mut(index))
    else {
        return false;
    };
    match tail {
        None => {
            if *target == value {
                return false;
            }
            *target = value;
            true
        }
        Some(tail) => match target {
            Value::Object(object) => write_state_path(object, tail, value),
            Value::List(items) => write_list_path(items, tail, value),
            _ => false,
        },
    }
}
