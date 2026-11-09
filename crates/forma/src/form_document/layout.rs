use super::{Node, Properties, Value, boolean, font_family, num, number, text};

fn is_color_property(key: &str) -> bool {
    key.as_bytes()
        .windows(5)
        .any(|w| w.eq_ignore_ascii_case(b"color"))
        || [
            "background",
            "hoverBackground",
            "pressedBackground",
            "disabledBackground",
            "hover",
            "pressed",
            "disabled",
            "focus",
        ]
        .contains(&key)
}

pub(super) fn write_property_literal(
    key: &str,
    value: &Value,
    out: &mut String,
) -> Result<(), String> {
    if let Value::Text(text) = value {
        if is_color_property(key)
            && text.starts_with('#')
            && [4, 7, 9].contains(&text.len())
            && text[1..].bytes().all(|b| b.is_ascii_hexdigit())
        {
            out.push_str(text);
            return Ok(());
        }
        if key == "overflow" && ["visible", "hidden"].contains(&text.as_str()) {
            out.push_str(text);
            return Ok(());
        }
    }
    value.write_literal(out)
}

pub(super) fn serialize_into(node: &Node, out: &mut String) -> Result<(), String> {
    out.push_str(&node.kind);
    out.push_str(" { ");
    let mut first = true;
    for (k, v) in &node.props {
        if k.as_str() == "key"
            || k.starts_with("__")
            || ["minWidth", "maxWidth", "minHeight", "maxHeight"].contains(&k.as_str())
        {
            continue;
        }
        if !first {
            out.push(' ');
        }
        first = false;
        out.push_str(k);
        out.push_str(": ");
        write_property_literal(k, v, out)?;
        out.push(';');
    }
    out.push(' ');
    if let Some(a) = &node.action {
        out.push_str("clicked -> ");
        out.push_str(a);
        out.push_str("();");
    }
    out.push(' ');
    for (i, child) in node.children.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        serialize_into(child, out)?;
    }
    out.push_str(" }");
    Ok(())
}

pub(super) fn insets(value: Option<&Value>) -> Result<[f64; 4], String> {
    match value {
        None => Ok([0.; 4]),
        Some(Value::List(v)) => match v.as_slice() {
            [a] => Ok([a.number()?; 4]),
            [a, b] => {
                let (a, b) = (a.number()?, b.number()?);
                Ok([a, b, a, b])
            }
            [a, b, c] => {
                let (a, b, c) = (a.number()?, b.number()?, c.number()?);
                Ok([a, b, c, b])
            }
            [a, b, c, d] => Ok([a.number()?, b.number()?, c.number()?, d.number()?]),
            _ => Err("Expected 1..4 insets".into()),
        },
        Some(v) => Ok([v.number()?; 4]),
    }
}

pub(super) fn gaps(p: &Properties) -> Result<[f64; 2], String> {
    match p.get("gap") {
        None => Ok([0.; 2]),
        Some(Value::List(v)) if v.len() == 2 => Ok([v[0].number()?, v[1].number()?]),
        Some(v) => Ok([v.number()?; 2]),
    }
}

pub(super) fn tracks(p: &Properties, key: &str) -> Vec<Value> {
    match p.get(key) {
        None => vec![Value::Expr("*".into())],
        Some(Value::List(v)) => v.clone(),
        Some(v) => vec![v.clone()],
    }
}

pub(super) fn tracks_slice<'a>(p: &'a Properties, key: &str) -> &'a [Value] {
    static DEFAULT_STAR: std::sync::LazyLock<[Value; 1]> =
        std::sync::LazyLock::new(|| [Value::Expr("*".into())]);
    match p.get(key) {
        None => &*DEFAULT_STAR,
        Some(Value::List(v)) => v.as_slice(),
        Some(v) => std::slice::from_ref(v),
    }
}

pub(super) fn cell(n: &Node, axis: usize, count: usize) -> Result<Option<usize>, String> {
    let value = if let Some(Value::List(values)) = n.props.get("cell") {
        if values.len() != 2 || n.props.contains_key("row") || n.props.contains_key("column") {
            return Err("Invalid cell".into());
        }
        Some(&values[1 - axis])
    } else {
        n.props.get(if axis == 0 { "column" } else { "row" })
    };
    let Some(value) = value else { return Ok(None) };
    let n = value.number()?;
    if n < 1. || n.fract() != 0. || n > count as f64 {
        return Err("Cell outside grid".into());
    }
    Ok(Some(n as usize - 1))
}

pub(super) fn constraint(n: &Node, axis: usize, value: f64) -> Result<f64, String> {
    let min = num(
        &n.props,
        if axis == 0 { "minWidth" } else { "minHeight" },
        0.,
    )?;
    let max = num(
        &n.props,
        if axis == 0 { "maxWidth" } else { "maxHeight" },
        f64::MAX,
    )?;
    if min < 0. || max < min {
        return Err("Invalid minimum/maximum size".into());
    }
    if !value.is_finite() || value < 0. {
        return Err("Invalid layout size".into());
    }
    Ok(value.clamp(min, max))
}

pub(super) fn normalize_layout(n: &Node) -> Result<Node, String> {
    normalize_layout_owned(n.clone())
}

pub(super) fn normalize_layout_owned(mut n: Node) -> Result<Node, String> {
    match n.kind.as_str() {
        "Row" | "Column" => {
            let row = n.kind == "Row";
            let axis_key = if row { "width" } else { "height" };
            let tracks = n
                .children
                .iter()
                .map(|c| {
                    c.props
                        .get(axis_key)
                        .cloned()
                        .unwrap_or(Value::Expr("auto".into()))
                })
                .collect();
            n.props.insert(
                if row { "columns" } else { "rows" }.into(),
                Value::List(tracks),
            );
            n.props.insert(
                if row { "rows" } else { "columns" }.into(),
                Value::Expr("*".into()),
            );
            for (i, c) in n.children.iter_mut().enumerate() {
                c.props.remove("cell");
                c.props
                    .insert("column".into(), number(if row { i + 1 } else { 1 } as f64));
                c.props
                    .insert("row".into(), number(if row { 1 } else { i + 1 } as f64));
            }
            n.props
                .insert("__linear_axis".into(), number(if row { 0. } else { 1. }));
            n.kind = "Frame".into();
        }
        "Grid" => {
            let columns = tracks_slice(&n.props, "columns").len().max(1);
            let mut rows = tracks(&n.props, "rows");
            if !n.props.contains_key("rows") {
                rows = vec![Value::Expr("auto".into())];
            }
            let mut occupied = std::collections::BTreeSet::new();
            for c in &n.children {
                let column = cell(c, 0, columns)?;
                let row = cell(c, 1, usize::MAX)?;
                if let (Some(column), Some(row)) = (column, row) {
                    if !occupied.insert((row, column)) {
                        return Err("Grid cell is occupied twice".into());
                    }
                }
            }
            for c in &mut n.children {
                let column = cell(c, 0, columns)?;
                let row = cell(c, 1, usize::MAX)?;
                let (row, column) = match (row, column) {
                    (Some(row), Some(column)) => (row, column),
                    (Some(row), None) => (
                        row,
                        (0..columns)
                            .find(|column| !occupied.contains(&(row, *column)))
                            .ok_or("Grid row is full")?,
                    ),
                    (None, Some(column)) => (
                        (0..)
                            .find(|row| !occupied.contains(&(*row, column)))
                            .unwrap(),
                        column,
                    ),
                    (None, None) => (0..)
                        .map(|i| (i / columns, i % columns))
                        .find(|position| !occupied.contains(position))
                        .unwrap(),
                };
                occupied.insert((row, column));
                while rows.len() <= row {
                    rows.push(Value::Expr("auto".into()));
                }
                c.props.remove("cell");
                c.props.insert("row".into(), number((row + 1) as f64));
                c.props.insert("column".into(), number((column + 1) as f64));
            }
            n.props.insert("rows".into(), Value::List(rows));
            n.kind = "Frame".into();
        }
        "Stack" => {
            n.kind = "Frame".into();
            n.props.insert("columns".into(), Value::Expr("*".into()));
            n.props.insert("rows".into(), Value::Expr("*".into()));
        }
        _ => {}
    }
    Ok(n)
}

pub(super) fn natural(n: &Node, axis: usize) -> Result<f64, String> {
    let key = if axis == 0 { "width" } else { "height" };
    if let Some(value) = n.props.get(key) {
        if let Ok(value) = value.number() {
            return constraint(n, axis, value);
        }
    }
    let owned_normalized;
    let n = if matches!(n.kind.as_str(), "Row" | "Column" | "Grid" | "Stack") {
        owned_normalized = normalize_layout(n)?;
        &owned_normalized
    } else {
        n
    };
    let measured = match n.kind.as_str() {
        "Text" => {
            let fs = num(&n.props, "fontSize", num(&n.props, "font.size", 16.)?)?;
            let text = text(&n.props, "text", "")?;
            let wrap = boolean(&n.props, "wrap", false)?;
            let family = font_family(&n.props)?;
            let max_w = n
                .props
                .get("width")
                .and_then(|v| v.number().ok())
                .unwrap_or(0.);
            if wrap && max_w > 0. {
                let lines =
                    crate::text::wrap_text_ranges_in(family, &text, fs as f32, max_w as f32);
                if axis == 1 {
                    lines.len().max(1) as f64 * fs * 1.5
                } else {
                    lines.iter().map(|(_, w)| *w as f64).fold(0., f64::max)
                }
            } else if axis == 1 && text.contains('\n') {
                text.split('\n').count() as f64 * fs * 1.5
            } else {
                text.split('\n')
                    .map(|line| crate::text::measure_line_in(family, line, fs as f32)[axis] as f64)
                    .fold(0., f64::max)
            }
        }
        "TextInput" => {
            if axis == 0 {
                80.
            } else {
                num(&n.props, "fontSize", 15.)? * 1.5
            }
        }
        "Image" => 20.,
        "Rectangle" => n
            .children
            .iter()
            .map(|c| natural(c, axis))
            .try_fold(0.0_f64, |a, b| b.map(|b| a.max(b)))?,
        "Frame" | "Scroll" => {
            let ts = tracks_slice(&n.props, if axis == 0 { "columns" } else { "rows" });
            let pad = insets(n.props.get("padding"))?;
            let mut total = pad[1 - axis]
                + pad[3 - axis]
                + gaps(&n.props)?[1 - axis] * ts.len().saturating_sub(1) as f64;
            for (i, t) in ts.iter().enumerate() {
                total += if let Ok(v) = t.number() {
                    v
                } else {
                    let mut size: f64 = 0.;
                    for c in &n.children {
                        if ts.len() == 1 || cell(c, axis, ts.len())? == Some(i) {
                            size = size.max(natural(c, axis)?);
                        }
                    }
                    size
                };
            }
            total
        }
        _ => return Err(format!("Unknown visual {}", n.kind)),
    };
    constraint(n, axis, measured)
}

pub(super) fn dimension(n: &Node, axis: usize, available: f64) -> Result<f64, String> {
    let key = if axis == 0 { "width" } else { "height" };
    let value = match n.props.get(key) {
        None => available,
        Some(Value::Expr(v) | Value::Text(v)) if ["auto", "content", "-"].contains(&v.as_str()) => {
            natural(n, axis)?
        }
        Some(Value::Expr(v) | Value::Text(v)) if v.ends_with('*') => available,
        Some(Value::Expr(v) | Value::Text(v)) if v.ends_with('%') => {
            available
                * v[..v.len() - 1]
                    .parse::<f64>()
                    .map_err(|_| "Invalid percentage")?
                / 100.
        }
        Some(v) => v.number()?,
    };
    constraint(n, axis, value)
}

pub(super) fn sizes(n: &Node, axis: usize, available: f64) -> Result<Vec<f64>, String> {
    let ts = tracks_slice(&n.props, if axis == 0 { "columns" } else { "rows" });
    let mut values = Vec::with_capacity(ts.len());
    let mut weights = Vec::with_capacity(ts.len());
    for (i, t) in ts.iter().enumerate() {
        let mut weight = 0.;
        let size = match t {
            Value::Number(v) => *v,
            Value::Expr(s) | Value::Text(s) if ["-", "content", "auto"].contains(&s.as_str()) => {
                let mut size: f64 = 0.;
                for c in &n.children {
                    if ts.len() == 1 || cell(c, axis, ts.len())? == Some(i) {
                        size = size.max(natural(c, axis)?);
                    }
                }
                size
            }
            Value::Expr(s) | Value::Text(s) if s.ends_with('*') => {
                weight = if s == "*" {
                    1.
                } else {
                    s[..s.len() - 1]
                        .parse()
                        .map_err(|_| "Invalid grid weight")?
                };
                if weight <= 0. {
                    return Err("Grid weight must be positive".into());
                }
                0.
            }
            Value::Expr(s) | Value::Text(s) if s.ends_with('%') => {
                available
                    * s[..s.len() - 1]
                        .parse::<f64>()
                        .map_err(|_| "Invalid percentage")?
                    / 100.
            }
            _ => return Err("Invalid grid track".into()),
        };
        if !size.is_finite() || size < 0. {
            return Err("Invalid grid size".into());
        }
        values.push(size);
        weights.push(weight);
    }
    let mut remaining = (available
        - gaps(&n.props)?[1 - axis] * ts.len().saturating_sub(1) as f64
        - values.iter().sum::<f64>())
    .max(0.);
    let mut pending = (0..weights.len())
        .filter(|i| weights[*i] > 0.)
        .collect::<Vec<_>>();
    while !pending.is_empty() {
        let total = pending.iter().map(|i| weights[*i]).sum::<f64>();
        let mut frozen = Vec::new();
        for &index in &pending {
            let share = remaining * weights[index] / total;
            let mut min = 0.0_f64;
            let mut max = f64::MAX;
            if n.props.get("__linear_axis") == Some(&number(axis as f64)) {
                if let Some(child) = n.children.get(index) {
                    min = num(
                        &child.props,
                        if axis == 0 { "minWidth" } else { "minHeight" },
                        0.,
                    )?;
                    max = num(
                        &child.props,
                        if axis == 0 { "maxWidth" } else { "maxHeight" },
                        f64::MAX,
                    )?;
                    if min < 0. || max < min {
                        return Err("Invalid minimum/maximum size".into());
                    }
                }
            }
            if share < min || share > max {
                frozen.push((index, share.clamp(min, max), share.clamp(min, max) - share));
            }
        }
        if frozen.is_empty() {
            for index in pending {
                values[index] = remaining * weights[index] / total;
            }
            break;
        }
        let violation = frozen.iter().map(|(_, _, delta)| delta).sum::<f64>();
        for (index, value, delta) in frozen {
            if violation > 0. && delta < 0. || violation < 0. && delta > 0. {
                continue;
            }
            values[index] = value;
            remaining = (remaining - value).max(0.);
            pending.retain(|i| *i != index);
        }
    }
    Ok(values)
}
