use super::{Properties, Value, boolean, expression_number, text};

fn valid_unit(value: &str, units: &[&str]) -> bool {
    units.iter().any(|unit| {
        value.strip_suffix(unit).is_some_and(|raw| {
            !raw.is_empty()
                && raw.bytes().all(|b| b.is_ascii_digit() || b == b'.')
                && raw.parse::<f64>().is_ok_and(|v| v.is_finite() && v >= 0.)
        })
    })
}

fn valid_signed_unit(value: &str, units: &[&str]) -> bool {
    let value = value
        .strip_prefix('-')
        .or_else(|| value.strip_prefix('+'))
        .unwrap_or(value);
    valid_unit(value, units)
}

fn split_top_level(s: &str, sep: u8) -> Option<Vec<&str>> {
    let mut depth = 0i32;
    let mut parts = Vec::new();
    let mut last = 0usize;
    let bytes = s.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'<' {
            depth += 1;
        } else if b == b'>' {
            depth -= 1;
        } else if b == sep && depth == 0 {
            parts.push(s[last..i].trim());
            last = i + 1;
        }
    }
    if parts.is_empty() {
        None
    } else {
        parts.push(s[last..].trim());
        Some(parts)
    }
}

pub(super) fn validate_contract_type_name(ty: &str, enums: Option<&Value>) -> Result<(), String> {
    let ty = ty.trim();
    if let Some(parts) = split_top_level(ty, b'|') {
        for p in parts {
            validate_contract_type_name(p, enums)?;
        }
        return Ok(());
    }
    let name = ty.strip_suffix('?').unwrap_or(ty).trim();
    if let Some(inner) = name.strip_prefix("List<").and_then(|s| s.strip_suffix('>')) {
        return validate_contract_type_name(inner, enums);
    }
    if let Some(inner) = name.strip_prefix("Map<").and_then(|s| s.strip_suffix('>')) {
        return validate_contract_type_name(inner, enums);
    }
    if let Some(inner) = name
        .strip_prefix("Record<")
        .and_then(|s| s.strip_suffix('>'))
    {
        let Some(parts) = split_top_level(inner, b',') else {
            return Err(format!("Unknown property type {ty}"));
        };
        if parts.len() != 2 {
            return Err(format!("Unknown property type {ty}"));
        }
        validate_contract_type_name(parts[0], enums)?;
        return validate_contract_type_name(parts[1], enums);
    }
    if [
        "String", "Bool", "Boolean", "Number", "Int", "Float", "Color", "Length", "Duration",
        "Angle", "Asset", "List", "Object", "Map", "Any",
    ]
    .contains(&name)
    {
        return Ok(());
    }
    if let Some(Value::Object(enums)) = enums {
        if enums.contains_key(name) {
            return Ok(());
        }
    }
    Err(format!("Unknown property type {ty}"))
}

pub(super) fn matches_contract_type(value: &Value, ty: &str, enums: Option<&Value>) -> bool {
    let ty = ty.trim();
    if let Some(parts) = split_top_level(ty, b'|') {
        return parts
            .into_iter()
            .any(|p| matches_contract_type(value, p, enums));
    }
    if let Some(ty) = ty.strip_suffix('?') {
        return value == &Value::Null || matches_contract_type(value, ty.trim(), enums);
    }
    if let Some(inner) = ty.strip_prefix("List<").and_then(|s| s.strip_suffix('>')) {
        return matches!(value, Value::List(items) if items.iter().all(|v| matches_contract_type(v, inner, enums)));
    }
    if let Some(inner) = ty.strip_prefix("Map<").and_then(|s| s.strip_suffix('>')) {
        return matches!(value, Value::Object(obj) if obj.values().all(|v| matches_contract_type(v, inner, enums)));
    }
    if let Some(inner) = ty.strip_prefix("Record<").and_then(|s| s.strip_suffix('>')) {
        let Some(parts) = split_top_level(inner, b',') else {
            return false;
        };
        if parts.len() != 2 {
            return false;
        }
        let val_ty = parts[1];
        return matches!(value, Value::Object(obj) if obj.values().all(|v| matches_contract_type(v, val_ty, enums)));
    }
    match ty {
        "Any" => true,
        "String" | "Asset" => matches!(value, Value::Text(_)),
        "Color" => value.text().ok().is_some_and(|v| {
            v.starts_with('#')
                && [4, 7, 9].contains(&v.len())
                && v[1..].bytes().all(|b| b.is_ascii_hexdigit())
        }),
        "Bool" | "Boolean" => matches!(value, Value::Bool(_)),
        "Number" | "Float" => matches!(value, Value::Number(n) if n.is_finite()),
        "Int" => {
            matches!(value, Value::Number(n) if n.is_finite() && n.fract() == 0. && n.abs() <= 9_007_199_254_740_991.)
        }
        "Length" => {
            matches!(value, Value::Number(n) if n.is_finite() && *n >= 0.)
                || matches!(value, Value::Text(v) | Value::Expr(v) if valid_unit(v, &["px", "%", "rem", "em", "vw", "vh", "ch", "fr"]))
        }
        "Duration" => {
            matches!(value, Value::Text(v) | Value::Expr(v) if valid_unit(v, &["ms", "s"]))
        }
        "Angle" => {
            matches!(value, Value::Number(n) if n.is_finite())
                || matches!(value, Value::Text(v) | Value::Expr(v) if valid_signed_unit(v, &["deg", "rad", "turn"]))
        }
        "List" => matches!(value, Value::List(_)),
        "Object" | "Map" => matches!(value, Value::Object(_)),
        _ => {
            let Some(Value::Object(enums_map)) = enums else {
                return false;
            };
            match enums_map.get(ty) {
                Some(Value::List(variants)) => variants.iter().any(|v| {
                    v.text().ok().is_some_and(|variant| {
                        matches!(value, Value::Text(actual) if actual.len() == ty.len() + 1 + variant.len()
                            && actual.starts_with(ty)
                            && actual.as_bytes()[ty.len()] == b'.'
                            && &actual[ty.len() + 1..] == variant)
                    })
                }),
                Some(Value::Object(fields)) => {
                    let Value::Object(obj) = value else {
                        return false;
                    };
                    fields.iter().all(|(field, fty)| {
                        let Ok(fty) = fty.text() else {
                            return false;
                        };
                        match obj.get(field) {
                            Some(v) => matches_contract_type(v, &fty, enums),
                            None => fty.ends_with('?'),
                        }
                    })
                }
                _ => false,
            }
        }
    }
}

pub(super) fn check_value_constraints(
    value: &Value,
    constraints: &Properties,
    label: &str,
) -> Result<(), String> {
    if value == &Value::Null {
        return Ok(());
    }
    if let Some(Value::List(range)) = constraints.get("range") {
        if range.len() == 2 {
            let empty = Properties::new();
            let min = expression_number(&range[0].evaluate(&empty, &empty)?)?;
            let max = expression_number(&range[1].evaluate(&empty, &empty)?)?;
            match value {
                Value::Number(n) if *n < min || *n > max => {
                    return Err(format!(
                        "Property {label} value {n} out of range {min}..={max}"
                    ));
                }
                Value::Text(s) => {
                    let len = s.chars().count() as f64;
                    if len < min || len > max {
                        return Err(format!(
                            "Property {label} length {len} out of range {min}..={max}"
                        ));
                    }
                }
                Value::List(a) => {
                    let len = a.len() as f64;
                    if len < min || len > max {
                        return Err(format!(
                            "Property {label} length {len} out of range {min}..={max}"
                        ));
                    }
                }
                _ => {}
            }
        }
    }
    if let Some(pat) = constraints.get("pattern") {
        let pat = pat.text()?;
        let s = value.text()?;
        let matched =
            themoretheless_tokenizer_forma::evaluate::matches_constraint_pattern(&s, &pat)
                .map_err(|e| e.to_string())?;
        if !matched {
            return Err(format!("Property {label} does not match pattern '{pat}'"));
        }
    }
    Ok(())
}

pub(super) fn check_contracts(
    kind: &str,
    contracts: Option<&Value>,
    enums: Option<&Value>,
    values: &Properties,
) -> Result<(), String> {
    let Some(Value::Object(contracts)) = contracts else {
        return Ok(());
    };
    for (name, contract) in contracts {
        let Value::Object(contract) = contract else {
            return Err("Invalid property contract".into());
        };
        let required = boolean(contract, "required", false)?;
        let ty = text(contract, "type", "")?;
        if !ty.is_empty() {
            validate_contract_type_name(&ty, enums)?;
        }
        let Some(value) = values.get(name) else {
            if required {
                return Err(format!("Missing required property {kind}.{name}"));
            }
            continue;
        };
        if ty.is_empty() {
            continue;
        }
        if !matches_contract_type(value, &ty, enums) {
            return Err(format!(
                "Property {kind}.{name} expects {ty}, got {value:?}"
            ));
        }
        if let Some(Value::Object(constraints)) = contract.get("constraints") {
            check_value_constraints(value, constraints, &format!("{kind}.{name}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::form_document::props;

    #[test]
    fn validates_unions_generics_optional_and_value_constraints() {
        validate_contract_type_name("List<String | Number>?", None).unwrap();
        validate_contract_type_name("Record<String, Color>", None).unwrap();
        assert!(validate_contract_type_name("Record<String>", None).is_err());
        assert!(validate_contract_type_name("UnknownType", None).is_err());

        assert!(matches_contract_type(&Value::Null, "List<Number>?", None));
        assert!(matches_contract_type(
            &Value::List(vec![Value::Number(1.), Value::Text("a".into())]),
            "List<Number | String>",
            None
        ));
        assert!(matches_contract_type(
            &Value::Text("#ff00aa".into()),
            "Color",
            None
        ));
        assert!(!matches_contract_type(
            &Value::Text("red".into()),
            "Color",
            None
        ));
    }

    #[test]
    fn check_contracts_enforces_required_and_range_bounds() {
        let contracts = Value::Object(props(vec![(
            "opacity",
            Value::Object(props(vec![
                ("required", Value::Bool(true)),
                ("type", Value::Text("Number".into())),
                (
                    "constraints",
                    Value::Object(props(vec![(
                        "range",
                        Value::List(vec![Value::Number(0.), Value::Number(1.)]),
                    )])),
                ),
            ])),
        )]));

        assert!(
            check_contracts("Badge", Some(&contracts), None, &Properties::new())
                .unwrap_err()
                .contains("Missing required property Badge.opacity")
        );

        let ok_vals = props(vec![("opacity", Value::Number(0.5))]);
        check_contracts("Badge", Some(&contracts), None, &ok_vals).unwrap();

        let bad_vals = props(vec![("opacity", Value::Number(1.5))]);
        assert!(
            check_contracts("Badge", Some(&contracts), None, &bad_vals)
                .unwrap_err()
                .contains("out of range")
        );
    }
}
