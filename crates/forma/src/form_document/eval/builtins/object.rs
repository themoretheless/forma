//! Builtins over objects (keyed records).

use super::*;

pub(super) fn call(name: &str, arguments: &[Value]) -> Result<Value, String> {
    match name {
        "keys" if arguments.len() == 1 => {
            let Value::Object(o) = &arguments[0] else {
                return Err("keys expects an object".into());
            };
            Ok(Value::List(
                o.keys().map(|k| Value::Text(k.clone())).collect(),
            ))
        }
        "values" if arguments.len() == 1 => {
            let Value::Object(o) = &arguments[0] else {
                return Err("values expects an object".into());
            };
            Ok(Value::List(o.values().cloned().collect()))
        }
        "entries" if arguments.len() == 1 => {
            let Value::Object(o) = &arguments[0] else {
                return Err("entries expects an object".into());
            };
            Ok(Value::List(
                o.iter()
                    .map(|(k, v)| Value::List(vec![Value::Text(k.clone()), v.clone()]))
                    .collect(),
            ))
        }
        "fromEntries" if arguments.len() == 1 => {
            let Value::List(a) = &arguments[0] else {
                return Err("fromEntries expects a list of pairs".into());
            };
            let mut out = Properties::new();
            for item in a {
                match item {
                    Value::List(pair) if pair.len() == 2 => {
                        out.insert(pair[0].text()?, pair[1].clone());
                    }
                    Value::Object(obj) if obj.contains_key("key") && obj.contains_key("value") => {
                        out.insert(obj["key"].text()?, obj["value"].clone());
                    }
                    _ => return Err("fromEntries expects [key, value] pairs".into()),
                }
            }
            Ok(Value::Object(out))
        }
        "merge" if arguments.len() >= 2 => {
            let mut out = Properties::new();
            for arg in arguments {
                let Value::Object(o) = arg else {
                    return Err("merge expects objects".into());
                };
                out.extend(o.clone());
            }
            Ok(Value::Object(out))
        }
        "pick" | "omit" if arguments.len() == 2 => {
            let Value::Object(o) = &arguments[0] else {
                return Err(format!("{name} expects an object"));
            };
            let Value::List(ks) = &arguments[1] else {
                return Err(format!("{name} expects a list of keys"));
            };
            let mut key_set = std::collections::BTreeSet::new();
            for k in ks {
                key_set.insert(k.text()?);
            }
            let mut out = Properties::new();
            for (k, v) in o {
                let hit = key_set.contains(k);
                if (name == "pick" && hit) || (name == "omit" && !hit) {
                    out.insert(k.clone(), v.clone());
                }
            }
            Ok(Value::Object(out))
        }
        _ => Err(unknown(name)),
    }
}
