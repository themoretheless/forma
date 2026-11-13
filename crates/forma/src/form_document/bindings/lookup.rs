//! Reading a bound value by path.

use super::*;

// Borrow real model fields instead of cloning a whole collection to inspect a
// member. Synthetic length properties are intentionally not collection targets.
pub(in crate::form_document) fn lookup_binding_ref<'a>(
    values: &'a Properties,
    path: &str,
) -> Option<&'a Value> {
    if let Some(value) = values.get(path) {
        return Some(value);
    }
    let (head, tail) = path.split_once('.')?;
    lookup_binding_value_ref(values.get(head)?, tail)
}

pub(super) fn lookup_binding_value_ref<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    match value {
        Value::Object(object) => lookup_binding_ref(object, path),
        Value::List(items) => {
            let (index, tail) = path
                .split_once('.')
                .map_or((path, None), |(index, tail)| (index, Some(tail)));
            let value = items.get(index.parse::<usize>().ok()?)?;
            match tail {
                Some(tail) => lookup_binding_value_ref(value, tail),
                None => Some(value),
            }
        }
        _ => None,
    }
}

pub(in crate::form_document) fn lookup_binding_mut<'a>(
    values: &'a mut Properties,
    path: &str,
) -> Option<&'a mut Value> {
    if values.contains_key(path) {
        return values.get_mut(path);
    }
    let (head, tail) = path.split_once('.')?;
    lookup_binding_value_mut(values.get_mut(head)?, tail)
}

pub(super) fn lookup_binding_value_mut<'a>(
    value: &'a mut Value,
    path: &str,
) -> Option<&'a mut Value> {
    match value {
        Value::Object(object) => lookup_binding_mut(object, path),
        Value::List(items) => {
            let (index, tail) = path
                .split_once('.')
                .map_or((path, None), |(index, tail)| (index, Some(tail)));
            let value = items.get_mut(index.parse::<usize>().ok()?)?;
            match tail {
                Some(tail) => lookup_binding_value_mut(value, tail),
                None => Some(value),
            }
        }
        _ => None,
    }
}
