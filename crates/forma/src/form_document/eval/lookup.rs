//! Scoped lookups (`props.*`, `tokens.*`, `i18n.*`, `state.*`, enum variants, and lexical `let`).

use super::error::EvalError;
use super::{Properties, Value, eval_owned_value, number};

pub(in crate::form_document) fn lookup(values: &Properties, path: &str) -> Option<Value> {
    if let Some(value) = values.get(path) {
        return Some(value.clone());
    }
    let (head, tail) = path.split_once('.')?;
    lookup_value(values.get(head)?, tail)
}

pub(in crate::form_document) fn lookup_value(value: &Value, path: &str) -> Option<Value> {
    match value {
        Value::Object(object) => lookup(object, path),
        Value::List(items) if path == "length" => Some(number(items.len() as f64)),
        Value::List(items) => {
            let (index, tail) = path
                .split_once('.')
                .map_or((path, None), |(index, tail)| (index, Some(tail)));
            let value = items.get(index.parse::<usize>().ok()?)?;
            match tail {
                Some(tail) => lookup_value(value, tail),
                None => Some(value.clone()),
            }
        }
        Value::Text(value) if path == "length" => Some(number(value.encode_utf16().count() as f64)),
        _ => None,
    }
}

pub(in crate::form_document) fn enum_value(path: &str, scope: &Properties) -> Option<Value> {
    let (kind, variant) = path.split_once('.')?;
    let Value::Object(enums) = scope.get("__enums")? else {
        return None;
    };
    let Value::List(variants) = enums.get(kind)? else {
        return None;
    };
    variants
        .iter()
        .any(|v| v.text().ok().as_deref() == Some(variant))
        .then(|| Value::Text(path.into()))
}

pub(super) fn eval_expr_path<'a>(
    path: &'a str,
    scope: &'a Properties,
    state: &'a Properties,
    stack: &mut Vec<&'a str>,
) -> Result<Value, EvalError> {
    let (negative, path) = path.strip_prefix('!').map_or((false, path), |p| (true, p));
    let result = if let Some(key) = path.strip_prefix("props.") {
        let key = if key == "font.size" { "fontSize" } else { key };
        if stack.contains(&key) {
            return Err(EvalError::CyclicProperty(key.to_owned()));
        }
        let v = lookup(scope, key).ok_or_else(|| EvalError::UnknownProperty(path.to_owned()))?;
        stack.push(key);
        let result = eval_owned_value(&v, scope, state, stack);
        stack.pop();
        result?
    } else if let Some(key) = path.strip_prefix("tokens.") {
        match scope.get("__tokens") {
            Some(Value::Object(tokens)) => {
                lookup(tokens, key).ok_or_else(|| EvalError::UnknownToken(path.to_owned()))?
            }
            _ => return Err(EvalError::UnknownToken(path.to_owned())),
        }
    } else if let Some(key) = path.strip_prefix("i18n.") {
        match scope.get("__i18n") {
            Some(Value::Object(i18n)) => {
                lookup(i18n, key).ok_or_else(|| EvalError::UnknownI18nKey(path.to_owned()))?
            }
            _ => return Err(EvalError::UnknownI18nKey(path.to_owned())),
        }
    } else if let Some(key) = path.strip_prefix("state.") {
        lookup(state, key).ok_or_else(|| EvalError::MissingState(key.to_owned()))?
    } else if path == "null" {
        Value::Null
    } else if let Some(value) = enum_value(path, scope)
        .or_else(|| lookup(state, path))
        .or_else(|| match scope.get("__lets") {
            Some(Value::Object(lets)) => lookup(lets, path),
            _ => None,
        })
    {
        if matches!(
            value,
            Value::Expression(_) | Value::Expr(_) | Value::Interpolation(_) | Value::Match(..)
        ) {
            eval_owned_value(&value, scope, state, stack)?
        } else {
            value
        }
    } else if path.split_once('.').is_some_and(|(head, _)| {
        matches!(scope.get("__enums"), Some(Value::Object(enums)) if enums.contains_key(head))
    }) {
        return Err(EvalError::UnknownEnumVariant(path.to_owned()));
    } else if path
        .split_once('.')
        .is_some_and(|(head, _)| state.contains_key(head))
    {
        return Err(EvalError::UnknownMember(path.to_owned()));
    } else {
        Value::Expr(path.into())
    };
    if negative {
        Ok(Value::Bool(
            !result.boolean().map_err(EvalError::TypeError)?,
        ))
    } else {
        Ok(result)
    }
}
