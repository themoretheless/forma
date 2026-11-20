//! Collects dotted identifier references from parsed `.ui` AST JSON values.

use serde_json::Value;
use std::collections::BTreeSet;

pub(crate) fn is_dotted_ref(raw: &str) -> bool {
    let trimmed = raw.strip_prefix('!').unwrap_or(raw);
    let mut segments = trimmed.split('.');
    let Some(first) = segments.next() else {
        return false;
    };
    let mut count = 0usize;
    for seg in std::iter::once(first).chain(segments) {
        let mut chars = seg.chars();
        let Some(head) = chars.next() else {
            return false;
        };
        if !(head.is_ascii_alphabetic() || head == '_')
            || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return false;
        }
        count += 1;
    }
    count >= 2
}

pub(crate) fn expression_references(value: &Value) -> Vec<String> {
    fn walk(value: &Value, seen: &mut BTreeSet<String>, out: &mut Vec<String>) {
        match value {
            Value::Array(items) => {
                for item in items {
                    walk(item, seen, out);
                }
            }
            Value::Object(map) => {
                if let Some(raw) = map.get("expr").and_then(Value::as_str)
                    && is_dotted_ref(raw)
                {
                    let reference = raw.strip_prefix('!').unwrap_or(raw);
                    if seen.insert(reference.to_string()) {
                        out.push(reference.to_string());
                    }
                }
                for (key, child) in map {
                    if key != "expr" {
                        walk(child, seen, out);
                    }
                }
            }
            _ => {}
        }
    }
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    walk(value, &mut seen, &mut out);
    out
}
