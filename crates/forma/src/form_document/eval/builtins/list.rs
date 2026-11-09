//! Builtins over lists, including lists of records.

use super::*;

pub(super) fn call(name: &str, arguments: &[Value]) -> Result<Value, String> {
    match name {
        "take" if arguments.len() == 2 => {
            let Value::List(a) = &arguments[0] else {
                return Err("take expects a list".into());
            };
            let n = expression_number(&arguments[1])?.max(0.) as usize;
            Ok(Value::List(a.iter().take(n).cloned().collect()))
        }
        "skip" if arguments.len() == 2 => {
            let Value::List(a) = &arguments[0] else {
                return Err("skip expects a list".into());
            };
            let n = expression_number(&arguments[1])?.max(0.) as usize;
            Ok(Value::List(a.iter().skip(n).cloned().collect()))
        }
        "range" if arguments.len() == 2 => {
            let st = expression_number(&arguments[0])? as i64;
            let en = expression_number(&arguments[1])? as i64;
            if en < st {
                return Ok(Value::List(vec![]));
            }
            if en - st > 4096 {
                return Err("range exceeds 4096 items".into());
            }
            Ok(Value::List((st..en).map(|i| number(i as f64)).collect()))
        }
        "unique" if arguments.len() == 1 || arguments.len() == 2 => {
            let Value::List(a) = &arguments[0] else {
                return Err("unique expects a list".into());
            };
            let key = if arguments.len() == 2 {
                Some(arguments[1].text()?)
            } else {
                None
            };
            let mut out: Vec<Value> = Vec::with_capacity(a.len());
            for item in a {
                let k = match (&key, item) {
                    (Some(f), Value::Object(o)) => o.get(f).unwrap_or(&Value::Null),
                    (Some(_), _) => &Value::Null,
                    (None, _) => item,
                };
                let exists = out.iter().any(|existing| {
                    let ek = match (&key, existing) {
                        (Some(f), Value::Object(o)) => o.get(f).unwrap_or(&Value::Null),
                        (Some(_), _) => &Value::Null,
                        (None, _) => existing,
                    };
                    ek == k
                });
                if !exists {
                    out.push(item.clone());
                }
            }
            Ok(Value::List(out))
        }
        "find" if arguments.len() == 3 => {
            let Value::List(a) = &arguments[0] else {
                return Err("find expects a list".into());
            };
            let key = arguments[1].text()?;
            let target = &arguments[2];
            let found = a
                .iter()
                .find(|item| matches!(item, Value::Object(o) if o.get(&key) == Some(target)))
                .cloned()
                .unwrap_or(Value::Null);
            Ok(found)
        }
        "compact" if arguments.len() == 1 => {
            let Value::List(a) = &arguments[0] else {
                return Err("compact expects a list".into());
            };
            Ok(Value::List(
                a.iter()
                    .filter(|v| {
                        !matches!(v, Value::Null | Value::Bool(false))
                            && !matches!(v, Value::Text(s) if s.is_empty())
                    })
                    .cloned()
                    .collect(),
            ))
        }
        "flatten" if arguments.len() == 1 => {
            let Value::List(a) = &arguments[0] else {
                return Err("flatten expects a list".into());
            };
            let mut out = Vec::with_capacity(a.len());
            for item in a {
                if let Value::List(sub) = item {
                    out.extend(sub.iter().cloned());
                } else {
                    out.push(item.clone());
                }
            }
            Ok(Value::List(out))
        }
        "chunk" if arguments.len() == 2 => {
            let Value::List(a) = &arguments[0] else {
                return Err("chunk expects a list".into());
            };
            let sz = expression_number(&arguments[1])? as usize;
            if sz == 0 {
                return Err("chunk size must be positive".into());
            }
            Ok(Value::List(
                a.chunks(sz).map(|c| Value::List(c.to_vec())).collect(),
            ))
        }
        "zip" if arguments.len() == 2 => {
            let (Value::List(a), Value::List(b)) = (&arguments[0], &arguments[1]) else {
                return Err("zip expects two lists".into());
            };
            Ok(Value::List(
                a.iter()
                    .zip(b.iter())
                    .map(|(x, y)| Value::List(vec![x.clone(), y.clone()]))
                    .collect(),
            ))
        }
        "first" if arguments.len() == 1 => {
            let Value::List(a) = &arguments[0] else {
                return Err("first expects a list".into());
            };
            Ok(a.first().cloned().unwrap_or(Value::Null))
        }
        "last" if arguments.len() == 1 => {
            let Value::List(a) = &arguments[0] else {
                return Err("last expects a list".into());
            };
            Ok(a.last().cloned().unwrap_or(Value::Null))
        }
        "groupBy" if arguments.len() == 2 => {
            let Value::List(a) = &arguments[0] else {
                return Err("groupBy expects a list".into());
            };
            let key = arguments[1].text()?;
            let mut groups: Vec<(Value, Vec<Value>)> = Vec::new();
            for item in a {
                let k = match item {
                    Value::Object(o) => o.get(&key).cloned().unwrap_or(Value::Null),
                    _ => Value::Null,
                };
                if let Some((_, items)) = groups.iter_mut().find(|(gk, _)| gk == &k) {
                    items.push(item.clone());
                } else {
                    groups.push((k, vec![item.clone()]));
                }
            }
            Ok(Value::List(
                groups
                    .into_iter()
                    .map(|(k, items)| {
                        Value::Object(props(vec![("key", k), ("items", Value::List(items))]))
                    })
                    .collect(),
            ))
        }
        "pluck" if arguments.len() == 2 => {
            let Value::List(a) = &arguments[0] else {
                return Err("pluck expects a list".into());
            };
            let key = arguments[1].text()?;
            Ok(Value::List(
                a.iter()
                    .map(|item| match item {
                        Value::Object(o) => o.get(&key).cloned().unwrap_or(Value::Null),
                        _ => Value::Null,
                    })
                    .collect(),
            ))
        }
        "sortBy" if arguments.len() == 1 || arguments.len() == 2 => {
            let Value::List(a) = &arguments[0] else {
                return Err("sortBy expects a list".into());
            };
            let key = if arguments.len() == 2 {
                Some(arguments[1].text()?)
            } else {
                None
            };
            let mut v = a.clone();
            v.sort_by(|x, y| {
                let kx = match (&key, x) {
                    (Some(f), Value::Object(o)) => o.get(f).unwrap_or(&Value::Null),
                    (Some(_), _) => &Value::Null,
                    (None, _) => x,
                };
                let ky = match (&key, y) {
                    (Some(f), Value::Object(o)) => o.get(f).unwrap_or(&Value::Null),
                    (Some(_), _) => &Value::Null,
                    (None, _) => y,
                };
                match (kx, ky) {
                    (Value::Text(a), Value::Text(b)) => a.cmp(b),
                    (Value::Number(a), Value::Number(b)) => {
                        a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
                    }
                    _ => std::cmp::Ordering::Equal,
                }
            });
            Ok(Value::List(v))
        }
        "any" if arguments.len() == 1 => {
            let Value::List(a) = &arguments[0] else {
                return Err("any expects a list".into());
            };
            let mut res = false;
            for item in a {
                if item.boolean()? {
                    res = true;
                    break;
                }
            }
            Ok(Value::Bool(res))
        }
        "all" if arguments.len() == 1 => {
            let Value::List(a) = &arguments[0] else {
                return Err("all expects a list".into());
            };
            let mut res = true;
            for item in a {
                if !item.boolean()? {
                    res = false;
                    break;
                }
            }
            Ok(Value::Bool(res))
        }
        _ => Err(unknown(name)),
    }
}
