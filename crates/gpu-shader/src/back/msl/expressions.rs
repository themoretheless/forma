//! Expressions and calls.

use super::*;

pub fn translate_expr(
    module: &Module,
    expr: &str,
    _ptr_params: &BTreeSet<String>,
    func_globals: &BTreeMap<String, Vec<Handle<GlobalVariable>>>,
) -> String {
    let empty_map = BindingMap {
        buffers: BTreeMap::new(),
        textures: BTreeMap::new(),
        samplers: BTreeMap::new(),
        immediate_slot: None,
        array_length_slots: BTreeMap::new(),
    };
    translate_expr_with_ctx(module, expr, func_globals, &empty_map)
}

pub(super) fn translate_expr_with_ctx(
    module: &Module,
    expr: &str,
    func_globals: &BTreeMap<String, Vec<Handle<GlobalVariable>>>,
    binding_map: &BindingMap,
) -> String {
    let mut out = String::with_capacity(expr.len() + 32);
    let bytes = expr.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        if let Some(next) =
            try_translate_bitcast(module, expr, i, func_globals, binding_map, &mut out)
        {
            i = next;
            continue;
        }
        if let Some(next) =
            try_translate_generic_ctor(module, expr, i, func_globals, binding_map, &mut out)
        {
            i = next;
            continue;
        }
        let b = bytes[i];
        if b.is_ascii_alphabetic() || b == b'_' {
            i = translate_ident_or_call(module, expr, i, func_globals, binding_map, &mut out);
            continue;
        }
        let Some(ch) = expr[i..].chars().next() else {
            break;
        };
        out.push(ch);
        i += ch.len_utf8();
    }

    out
}

fn try_translate_bitcast(
    module: &Module,
    expr: &str,
    i: usize,
    func_globals: &BTreeMap<String, Vec<Handle<GlobalVariable>>>,
    binding_map: &BindingMap,
    out: &mut String,
) -> Option<usize> {
    if !expr[i..].starts_with("bitcast<") {
        return None;
    }
    let bytes = expr.as_bytes();
    let prev_ok = i == 0 || (!bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1] != b'_');
    if !prev_ok {
        return None;
    }
    let after_bc = &expr[i + 7..];
    let close_angle = find_matching_delim(after_bc, '<', '>')?;
    let msl_ty = wgsl_type_str_to_msl(module, &after_bc[1..close_angle]);
    let after_angle = after_bc[close_angle + 1..].trim_start();
    if !after_angle.starts_with('(') {
        return None;
    }
    let close_paren = find_matching_delim(after_angle, '(', ')')?;
    let arg_msl = translate_expr_with_ctx(
        module,
        &after_angle[1..close_paren],
        func_globals,
        binding_map,
    );
    out.push_str(&format!("as_type<{msl_ty}>({arg_msl})"));
    Some(expr.len() - after_angle.len() + close_paren + 1)
}

fn try_translate_generic_ctor(
    module: &Module,
    expr: &str,
    i: usize,
    func_globals: &BTreeMap<String, Vec<Handle<GlobalVariable>>>,
    binding_map: &BindingMap,
    out: &mut String,
) -> Option<usize> {
    let bytes = expr.as_bytes();
    let prev_b = if i > 0 { bytes[i - 1] } else { b' ' };
    let (msl_ctor, is_array_ctor, after_type_offset) =
        try_match_generic_type_ctor(module, &expr[i..], prev_b)?;
    let trimmed = expr[i + after_type_offset..].trim_start();
    if !trimmed.starts_with('(') {
        return None;
    }
    let close_paren = find_matching_delim(trimmed, '(', ')')?;
    let translated_args: Vec<String> = split_comma_top_level(&trimmed[1..close_paren])
        .into_iter()
        .filter(|a| !a.trim().is_empty())
        .map(|a| translate_expr_with_ctx(module, a.trim(), func_globals, binding_map))
        .collect();
    if is_array_ctor {
        out.push_str(&format!(
            "{msl_ctor}{{{{ {} }}}}",
            translated_args.join(", ")
        ));
    } else {
        out.push_str(&format!("{msl_ctor}({})", translated_args.join(", ")));
    }
    Some(expr.len() - trimmed.len() + close_paren + 1)
}

fn translate_ident_or_call(
    module: &Module,
    expr: &str,
    start: usize,
    func_globals: &BTreeMap<String, Vec<Handle<GlobalVariable>>>,
    binding_map: &BindingMap,
    out: &mut String,
) -> usize {
    let bytes = expr.as_bytes();
    let is_member = start > 0 && expr[..start].trim_end().as_bytes().last().copied() == Some(b'.');
    let mut i = start + 1;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    let ident = &expr[start..i];
    if is_member {
        out.push_str(ident);
        return i;
    }

    let trimmed_after = expr[i..].trim_start();
    if trimmed_after.starts_with('(') {
        if let Some(close_paren) = find_matching_delim(trimmed_after, '(', ')') {
            let raw_args = &trimmed_after[1..close_paren];
            let args: Vec<String> = if raw_args.trim().is_empty() {
                Vec::new()
            } else {
                split_comma_top_level(raw_args)
                    .into_iter()
                    .filter(|a| !a.trim().is_empty())
                    .map(|a| translate_expr_with_ctx(module, a.trim(), func_globals, binding_map))
                    .collect()
            };
            let call_msl =
                translate_call(module, ident, &args, raw_args, func_globals, binding_map);
            out.push_str(&call_msl);
            return expr.len() - trimmed_after.len() + close_paren + 1;
        }
    }

    out.push_str(msl_escape_ident(ident));
    i
}

pub(super) fn msl_escape_ident(ident: &str) -> &str {
    match ident {
        "half" => "_wgpu_ident_half",
        "kernel" => "_wgpu_ident_kernel",
        "vertex" => "_wgpu_ident_vertex",
        "fragment" => "_wgpu_ident_fragment",
        "device" => "_wgpu_ident_device",
        "constant" => "_wgpu_ident_constant",
        "thread" => "_wgpu_ident_thread",
        "threadgroup" => "_wgpu_ident_threadgroup",
        "sampler" => "_wgpu_ident_sampler",
        "texture" => "_wgpu_ident_texture",
        "address" => "_wgpu_ident_address",
        "filter" => "_wgpu_ident_filter",
        "coord" => "_wgpu_ident_coord",
        "compare_func" => "_wgpu_ident_compare_func",
        "access" => "_wgpu_ident_access",
        "distance" => "_wgpu_ident_distance",
        _ => ident,
    }
}

pub(super) fn try_match_generic_type_ctor(
    module: &Module,
    s: &str,
    prev_byte: u8,
) -> Option<(String, bool, usize)> {
    if prev_byte.is_ascii_alphanumeric() || prev_byte == b'_' || prev_byte == b'.' {
        return None;
    }
    for prefix in [
        "vec2<", "vec3<", "vec4<", "mat2x2<", "mat3x3<", "mat4x4<", "array<",
    ] {
        if s.starts_with(prefix) {
            let open_pos = prefix.len() - 1;
            let close_rel = find_matching_delim(&s[open_pos..], '<', '>')?;
            let end = open_pos + close_rel + 1;
            let full_ty = &s[..end];
            let msl_ty = wgsl_type_str_to_msl(module, full_ty);
            return Some((msl_ty, prefix == "array<", end));
        }
    }
    None
}
