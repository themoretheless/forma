//! A state update addressed by path.

use super::*;

/// Application state on its way into a retained document's root scope.
/// Values use the same JSON conversion as the initial state of a UI project.
pub(crate) struct StatePush {
    pub(super) entries: Properties,
    pub(super) paths: bool,
}

// A path is walked recursively, so its depth is bounded like scene composition.
pub(super) const MAX_STATE_PATH_SEGMENTS: usize = 64;

impl StatePush {
    /// Every entry replaces one root state field as a whole.
    pub(crate) fn fields(entries: Properties) -> Self {
        Self {
            entries,
            paths: false,
        }
    }
    /// `paths` reads each key as a dotted path (`user.name`, `items.2.done`).
    pub(crate) fn from_json(json: &str, paths: bool) -> Result<Self, String> {
        let value: serde_json::Value =
            serde_json::from_str(json).map_err(|e| format!("Invalid state JSON: {e}"))?;
        if !value.is_object() {
            return Err("State must be a JSON object".into());
        }
        let entries = json_to_doc_props(&value)?;
        if paths {
            for path in entries.keys() {
                let mut segments = 0usize;
                for segment in path.split('.') {
                    segments += 1;
                    if segment.is_empty() {
                        return Err(format!("Invalid state path '{path}'"));
                    }
                }
                if segments > MAX_STATE_PATH_SEGMENTS {
                    return Err(format!(
                        "State path exceeds {MAX_STATE_PATH_SEGMENTS} segments"
                    ));
                }
            }
        }
        Ok(Self { entries, paths })
    }
    /// The caller applies to a copy: a refused path may follow accepted ones.
    pub(crate) fn apply(self, root: &mut Properties) -> Result<(), String> {
        for (key, value) in self.entries {
            if self.paths {
                push_state_path(root, &key, &key, value)?;
            } else {
                root.insert(key, value);
            }
        }
        Ok(())
    }
}

// Unlike binding writes, a pushed path that cannot be reached is an error:
// the application named it, so a silent miss would hide a stale screen.
pub(super) fn push_state_path(
    target: &mut Properties,
    path: &str,
    full: &str,
    value: Value,
) -> Result<(), String> {
    let Some((head, tail)) = path.split_once('.') else {
        target.insert(path.into(), value);
        return Ok(());
    };
    match target.get_mut(head) {
        Some(Value::Object(object)) => push_state_path(object, tail, full, value),
        Some(Value::List(items)) => push_state_item(items, tail, full, value),
        Some(Value::Null) | None => {
            let mut nested = Properties::new();
            push_state_path(&mut nested, tail, full, value)?;
            target.insert(head.into(), Value::Object(nested));
            Ok(())
        }
        Some(_) => Err(format!(
            "State path '{full}': '{head}' is not an object or list"
        )),
    }
}

pub(super) fn push_state_item(
    items: &mut [Value],
    path: &str,
    full: &str,
    value: Value,
) -> Result<(), String> {
    let (index, tail) = path
        .split_once('.')
        .map_or((path, None), |(index, tail)| (index, Some(tail)));
    let Some(target) = index
        .parse::<usize>()
        .ok()
        .and_then(|index| items.get_mut(index))
    else {
        return Err(format!("State path '{full}': no list item '{index}'"));
    };
    match (tail, target) {
        (None, target) => {
            *target = value;
            Ok(())
        }
        (Some(tail), Value::Object(object)) => push_state_path(object, tail, full, value),
        (Some(tail), Value::List(items)) => push_state_item(items, tail, full, value),
        (Some(_), _) => Err(format!(
            "State path '{full}': item '{index}' is not an object or list"
        )),
    }
}
