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
        // Handle `bitcast<T>(arg)`
        if expr[i..].starts_with("bitcast<") {
            let prev_ok = i == 0 || (!bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1] != b'_');
            if prev_ok {
                let after_bc = &expr[i + 7..]; // starts at `<`
                if let Some(close_angle) = find_matching_delim(after_bc, '<', '>') {
                    let ty_str = &after_bc[1..close_angle];
                    let msl_ty = wgsl_type_str_to_msl(module, ty_str);
                    let after_angle = after_bc[close_angle + 1..].trim_start();
                    if after_angle.starts_with('(') {
                        if let Some(close_paren) = find_matching_delim(after_angle, '(', ')') {
                            let arg = &after_angle[1..close_paren];
                            let arg_msl =
                                translate_expr_with_ctx(module, arg, func_globals, binding_map);
                            out.push_str(&format!("as_type<{msl_ty}>({arg_msl})"));
                            i = expr.len() - after_angle.len() + close_paren + 1;
                            continue;
                        }
                    }
                }
            }
        }

        // Handle generic type constructors: `vec2<f32>(...)`, `vec3<u32>(...)`, `mat4x4<f32>(...)`, `array<T, N>(...)`
        let prev_b = if i > 0 { bytes[i - 1] } else { b' ' };
        if let Some((msl_ctor, is_array_ctor, after_type_offset)) =
            try_match_generic_type_ctor(module, &expr[i..], prev_b)
        {
            let rest = &expr[i + after_type_offset..];
            let trimmed = rest.trim_start();
            if trimmed.starts_with('(') {
                if let Some(close_paren) = find_matching_delim(trimmed, '(', ')') {
                    let inner_args = &trimmed[1..close_paren];
                    let translated_args: Vec<String> = split_comma_top_level(inner_args)
                        .into_iter()
                        .filter(|a| !a.trim().is_empty())
                        .map(|a| {
                            translate_expr_with_ctx(module, a.trim(), func_globals, binding_map)
                        })
                        .collect();
                    if is_array_ctor {
                        out.push_str(&format!(
                            "{msl_ctor}{{{{ {} }}}}",
                            translated_args.join(", ")
                        ));
                    } else {
                        out.push_str(&format!("{msl_ctor}({})", translated_args.join(", ")));
                    }
                    i = expr.len() - trimmed.len() + close_paren + 1;
                    continue;
                }
            }
        }

        // Handle identifiers & function calls
        let b = bytes[i];
        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            let is_member =
                start > 0 && expr[..start].trim_end().as_bytes().last().copied() == Some(b'.');
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let ident = &expr[start..i];
            if is_member {
                out.push_str(ident);
                continue;
            }

            let after_ident = &expr[i..];
            let trimmed_after = after_ident.trim_start();
            if trimmed_after.starts_with('(') {
                if let Some(close_paren) = find_matching_delim(trimmed_after, '(', ')') {
                    let raw_args = &trimmed_after[1..close_paren];
                    let args: Vec<String> = if raw_args.trim().is_empty() {
                        Vec::new()
                    } else {
                        split_comma_top_level(raw_args)
                            .into_iter()
                            .filter(|a| !a.trim().is_empty())
                            .map(|a| {
                                translate_expr_with_ctx(module, a.trim(), func_globals, binding_map)
                            })
                            .collect()
                    };
                    let call_msl =
                        translate_call(module, ident, &args, raw_args, func_globals, binding_map);
                    out.push_str(&call_msl);
                    i = expr.len() - trimmed_after.len() + close_paren + 1;
                    continue;
                }
            }

            // Note: in WGSL, if `p` is a `ptr<function, array<f32, N>>` passed to a function, WGSL writes `(*p)[i]` or `*p = v`.
            // Wait: in MSL, if the parameter is declared as `thread _wgpu_arr<float, N>* p`, then `(*p)[i]` and `*p = v` work directly in C++/MSL!
            // What if `var<workgroup> wg_arr: array<f32, 256>;` is passed to a helper function or indexed directly?
            // Both `threadgroup float wg_arr[256]` and `_wgpu_arr` support `wg_arr[i]`!
            out.push_str(msl_escape_ident(ident));
            continue;
        }

        // Handle `&arr[idx]` or `&global` when taking pointer to atomic or workgroup/function variable
        let Some(ch) = expr[i..].chars().next() else {
            break;
        };
        out.push(ch);
        i += ch.len_utf8();
    }

    out
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

/// WGSL value constructors and the Metal type each one builds.
const CONSTRUCTORS: &[(&str, &str)] = &[
    ("u32", "uint"),
    ("i32", "int"),
    ("f32", "float"),
    ("f16", "half"),
    ("bool", "bool"),
    ("vec2f", "float2"),
    ("vec2<f32>", "float2"),
    ("vec3f", "float3"),
    ("vec3<f32>", "float3"),
    ("vec4f", "float4"),
    ("vec4<f32>", "float4"),
    ("vec2u", "uint2"),
    ("vec2<u32>", "uint2"),
    ("vec3u", "uint3"),
    ("vec3<u32>", "uint3"),
    ("vec4u", "uint4"),
    ("vec4<u32>", "uint4"),
    ("vec2i", "int2"),
    ("vec2<i32>", "int2"),
    ("vec3i", "int3"),
    ("vec3<i32>", "int3"),
    ("vec4i", "int4"),
    ("vec4<i32>", "int4"),
    ("mat2x2f", "float2x2"),
    ("mat2x2<f32>", "float2x2"),
    ("mat2x2", "float2x2"),
    ("mat3x3f", "float3x3"),
    ("mat3x3<f32>", "float3x3"),
    ("mat3x3", "float3x3"),
    ("mat4x4f", "float4x4"),
    ("mat4x4<f32>", "float4x4"),
    ("mat4x4", "float4x4"),
];

/// Atomic built-ins that take a pointer and a value and map onto one Metal function.
const ATOMIC_UPDATES: &[(&str, &str)] = &[
    ("atomicStore", "atomic_store_explicit"),
    ("atomicAdd", "atomic_fetch_add_explicit"),
    ("atomicSub", "atomic_fetch_sub_explicit"),
    ("atomicMax", "atomic_fetch_max_explicit"),
    ("atomicMin", "atomic_fetch_min_explicit"),
    ("atomicAnd", "atomic_fetch_and_explicit"),
    ("atomicOr", "atomic_fetch_or_explicit"),
    ("atomicXor", "atomic_fetch_xor_explicit"),
    ("atomicExchange", "atomic_exchange_explicit"),
];

/// Built-ins that only change name in Metal.
const RENAMED_BUILTINS: &[(&str, &str)] = &[
    ("inverseSqrt", "rsqrt"),
    ("round", "rint"),
    ("countLeadingZeros", "clz"),
    ("countTrailingZeros", "ctz"),
    ("countOneBits", "popcount"),
    ("reverseBits", "reverse_bits"),
    ("degrees", "_wgpu_degrees"),
    ("frexp", "_wgpu_frexp"),
    ("modf", "_wgpu_modf"),
    ("radians", "_wgpu_radians"),
    ("dpdx", "dfdx"),
    ("dpdy", "dfdy"),
];

/// Built-ins that wrap their single argument, with the argument used when it is missing.
const WRAPPED_BUILTINS: &[(&str, &str, &str, &str)] = &[
    (
        "unpack2x16float",
        "float2(as_type<half2>(uint(",
        ")))",
        "0u",
    ),
    (
        "pack2x16float",
        "as_type<uint>(half2(float2(",
        ")))",
        "float2(0.0)",
    ),
    (
        "unpack4x8unorm",
        "unpack_unorm4x8_to_float(uint(",
        "))",
        "0u",
    ),
    (
        "pack4x8unorm",
        "pack_float_to_unorm4x8(float4(",
        "))",
        "float4(0.0)",
    ),
];

fn lookup<'a>(table: &'a [(&str, &str)], name: &str) -> Option<&'a str> {
    table
        .iter()
        .find(|(wgsl, _)| *wgsl == name)
        .map(|(_, msl)| *msl)
}

pub(super) fn translate_call(
    module: &Module,
    ident: &str,
    args: &[String],
    raw_args: &str,
    func_globals: &BTreeMap<String, Vec<Handle<GlobalVariable>>>,
    binding_map: &BindingMap,
) -> String {
    // Check if `ident` is an alias (e.g. `Value(0)` or `D64(lo, hi)`)
    let resolved_ident = module
        .aliases
        .get(ident)
        .map(|s| s.as_str())
        .unwrap_or(ident);

    if let Some(call) = translate_constructor(resolved_ident, args)
        .or_else(|| translate_synchronization(resolved_ident, args))
        .or_else(|| translate_numeric_builtin(resolved_ident, args, func_globals))
        .or_else(|| translate_texture_builtin(module, resolved_ident, args))
    {
        return call;
    }
    if resolved_ident == "arrayLength" {
        return translate_array_length(raw_args, binding_map);
    }
    translate_user_call(module, ident, resolved_ident, args, func_globals)
}

fn translate_constructor(name: &str, args: &[String]) -> Option<String> {
    let ty = match name {
        "vec2" => infer_vec_ctor_type(2, args),
        "vec3" => infer_vec_ctor_type(3, args),
        "vec4" => infer_vec_ctor_type(4, args),
        _ => lookup(CONSTRUCTORS, name)?,
    };
    Some(format!("{ty}({})", args.join(", ")))
}

/// Barriers and atomics.
fn translate_synchronization(name: &str, args: &[String]) -> Option<String> {
    let arg = |i: usize, missing: &'static str| args.get(i).map(|s| s.as_str()).unwrap_or(missing);
    Some(match name {
        "workgroupBarrier" => "threadgroup_barrier(mem_flags::mem_threadgroup)".to_string(),
        "storageBarrier" => "threadgroup_barrier(mem_flags::mem_device)".to_string(),
        "textureBarrier" => "threadgroup_barrier(mem_flags::mem_texture)".to_string(),
        "atomicLoad" => format!("atomic_load_explicit({}, memory_order_relaxed)", arg(0, "")),
        "atomicCompareExchangeWeak" => format!(
            "_wgpu_atomic_compare_exchange_weak({}, {}, {})",
            arg(0, ""),
            arg(1, "0"),
            arg(2, "0")
        ),
        _ => {
            let update = lookup(ATOMIC_UPDATES, name)?;
            format!(
                "{update}({}, {}, memory_order_relaxed)",
                arg(0, ""),
                arg(1, "0")
            )
        }
    })
}

/// Math, bit and packing built-ins.
fn translate_numeric_builtin(
    name: &str,
    args: &[String],
    func_globals: &BTreeMap<String, Vec<Handle<GlobalVariable>>>,
) -> Option<String> {
    // A user function may be called `distance`; only the built-in needs the namespace.
    if name == "distance" && !func_globals.contains_key("distance") {
        return Some(format!("metal::distance({})", args.join(", ")));
    }
    if let Some(renamed) = lookup(RENAMED_BUILTINS, name) {
        return Some(format!("{renamed}({})", args.join(", ")));
    }
    let (_, open, close, missing) = WRAPPED_BUILTINS.iter().find(|entry| entry.0 == name)?;
    let value = args.first().map(|s| s.as_str()).unwrap_or(missing);
    Some(format!("{open}{value}{close}"))
}

fn translate_texture_builtin(module: &Module, name: &str, args: &[String]) -> Option<String> {
    Some(match name {
        "textureDimensions" => translate_texture_dimensions(module, args),
        "textureNumLevels" => {
            let tex = args.first().map(|s| s.as_str()).unwrap_or("tex");
            format!("uint({tex}.get_num_mip_levels())")
        }
        "textureSample" => {
            let tex = args.first().map(|s| s.as_str()).unwrap_or("tex");
            let samp = args.get(1).map(|s| s.as_str()).unwrap_or("samp");
            let uv = args.get(2).map(|s| s.as_str()).unwrap_or("float2(0.0)");
            format!("{tex}.sample({samp}, {uv})")
        }
        "textureSampleLevel" => {
            let tex = args.first().map(|s| s.as_str()).unwrap_or("tex");
            let samp = args.get(1).map(|s| s.as_str()).unwrap_or("samp");
            let uv = args.get(2).map(|s| s.as_str()).unwrap_or("float2(0.0)");
            let lod = args.get(3).map(|s| s.as_str()).unwrap_or("0.0");
            format!("{tex}.sample({samp}, {uv}, level({lod}))")
        }
        "textureSampleCompare" => {
            let tex = args.first().map(|s| s.as_str()).unwrap_or("tex");
            let samp = args.get(1).map(|s| s.as_str()).unwrap_or("samp");
            let uv = args.get(2).map(|s| s.as_str()).unwrap_or("float2(0.0)");
            let dref = args.get(3).map(|s| s.as_str()).unwrap_or("0.0");
            format!("{tex}.sample_compare({samp}, {uv}, {dref})")
        }
        "textureLoad" => translate_texture_load(module, args),
        "textureStore" => translate_texture_store(module, args),
        _ => return None,
    })
}

/// `textureDimensions(texture[, level])`: one component per dimension of the texture.
fn translate_texture_dimensions(module: &Module, args: &[String]) -> String {
    let tex = args.first().map(|s| s.as_str()).unwrap_or("tex");
    let lod = args.get(1).map(|s| s.as_str()).unwrap_or("");
    // A texture passed through a helper parameter has no global to inspect and is taken as 2D.
    let dim = global_image(module, tex).map_or(ImageDimension::D2, |(dim, _, _)| dim);
    match dim {
        ImageDimension::D1 => format!("uint({tex}.get_width({lod}))"),
        ImageDimension::D3 => format!(
            "uint3({tex}.get_width({lod}), {tex}.get_height({lod}), {tex}.get_depth({lod}))"
        ),
        ImageDimension::D2 | ImageDimension::Cube => {
            format!("uint2({tex}.get_width({lod}), {tex}.get_height({lod}))")
        }
    }
}

/// The dimension, arrayedness and class of the texture global called `name`.
fn global_image(module: &Module, name: &str) -> Option<(ImageDimension, bool, ImageClass)> {
    module
        .global_variables
        .iter()
        .find(|(_, g)| g.name.as_deref() == Some(name.trim()))
        .and_then(|(_, g)| match &module.types[g.ty].inner {
            TypeInner::Image {
                dim,
                arrayed,
                class,
            } => Some((*dim, *arrayed, *class)),
            _ => None,
        })
}

/// `textureStore(texture, coords, value)`, with the array layer before the value when arrayed.
fn translate_texture_store(module: &Module, args: &[String]) -> String {
    let arg = |i: usize| args.get(i).map(|s| s.as_str()).unwrap_or("0");
    let tex = args.first().map(|s| s.as_str()).unwrap_or("tex");
    let coords = arg(1);
    // A texture passed through a helper parameter has no global to inspect; the four-argument
    // form is the arrayed one.
    let (dim, arrayed) = global_image(module, tex).map_or(
        (ImageDimension::D2, args.len() == 4),
        |(dim, arrayed, _)| (dim, arrayed),
    );
    if arrayed {
        return format!("{tex}.write({}, uint2({coords}), uint({}))", arg(3), arg(2));
    }
    let coordinate = match dim {
        ImageDimension::D1 => "uint",
        ImageDimension::D3 => "uint3",
        ImageDimension::D2 | ImageDimension::Cube => "uint2",
    };
    format!("{tex}.write({}, {coordinate}({coords}))", arg(2))
}

fn translate_texture_load(module: &Module, args: &[String]) -> String {
    let tex = args.first().map(|s| s.as_str()).unwrap_or("tex");
    let coords = args.get(1).map(|s| s.as_str()).unwrap_or("uint2(0)");
    let image = global_image(module, tex);
    // A texture passed through a helper parameter has no global to inspect; WGSL's
    // only four-argument form is the arrayed one.
    let (dim, arrayed, class) = image.unwrap_or((
        ImageDimension::D2,
        args.len() == 4,
        ImageClass::Sampled {
            kind: ScalarKind::Float,
            multi: false,
        },
    ));
    let arg = |i: usize| args.get(i).map(|s| s.as_str()).unwrap_or("0");
    match (dim, arrayed, class) {
        (_, _, ImageClass::Storage { .. }) if arrayed => {
            format!("{tex}.read(uint2({coords}), uint({}))", arg(2))
        }
        (ImageDimension::D1, _, ImageClass::Storage { .. }) => {
            format!("{tex}.read(uint({coords}))")
        }
        (ImageDimension::D3, _, ImageClass::Storage { .. }) => {
            format!("{tex}.read(uint3({coords}))")
        }
        (_, _, ImageClass::Storage { .. }) => format!("{tex}.read(uint2({coords}))"),
        (_, _, ImageClass::Sampled { multi: true, .. } | ImageClass::Depth { multi: true }) => {
            format!("{tex}.read(uint2({coords}), uint({}))", arg(2))
        }
        (_, true, _) => format!(
            "{tex}.read(uint2({coords}), uint({}), uint({}))",
            arg(2),
            arg(3)
        ),
        (ImageDimension::D1, _, _) => {
            format!("{tex}.read(uint({coords}), uint({}))", arg(2))
        }
        (ImageDimension::D3, _, _) => {
            format!("{tex}.read(uint3({coords}), uint({}))", arg(2))
        }
        _ => format!("{tex}.read(uint2({coords}), uint({}))", arg(2)),
    }
}

/// Length of a runtime-sized array, read from the buffer sizes passed to the entry point.
fn translate_array_length(raw_args: &str, binding_map: &BindingMap) -> String {
    let arg = raw_args.trim().trim_start_matches('&').trim();
    let base_ident = arg.split('.').next().unwrap_or(arg).trim();
    if let Some(&(slot, base_off, stride, _)) = binding_map.array_length_slots.get(base_ident) {
        if base_off > 0 {
            format!(
                "((_wgpu_buffer_sizes[{slot}] > {base_off}u) ? ((_wgpu_buffer_sizes[{slot}] - {base_off}u) / {stride}u) : 0u)"
            )
        } else {
            format!("(_wgpu_buffer_sizes[{slot}] / {stride}u)")
        }
    } else {
        // No buffer to measure: the marker makes `write_string` fail instead of guessing.
        format!("{UNRESOLVED_ARRAY_LENGTH}({base_ident})")
    }
}

/// A struct constructor or a call to a user function.
fn translate_user_call(
    module: &Module,
    ident: &str,
    resolved_ident: &str,
    args: &[String],
    func_globals: &BTreeMap<String, Vec<Handle<GlobalVariable>>>,
) -> String {
    // Check if `ident` is a struct constructor
    let is_struct = module
        .types
        .iter()
        .any(|(_, t)| t.name.as_deref() == Some(resolved_ident));
    if is_struct {
        return format!("{resolved_ident}{{{}}}", args.join(", "));
    }

    // Check if `ident` is a user helper function that requires transitive globals
    let mut full_args = args.to_vec();
    if let Some(globals) = func_globals.get(ident) {
        for &gh in globals {
            if let Some(gname) = &module.global_variables[gh].name {
                full_args.push(gname.clone());
            }
        }
    }
    let callee_needs_arr_len = module
        .functions
        .iter()
        .any(|(_, f)| f.name.as_deref() == Some(ident) && f.body_src.contains("arrayLength"));
    if callee_needs_arr_len {
        full_args.push("_wgpu_buffer_sizes".to_string());
    }
    let escaped_ident = msl_escape_ident(ident);
    format!("{escaped_ident}({})", full_args.join(", "))
}

pub(super) fn infer_vec_ctor_type(dim: u32, args: &[String]) -> &'static str {
    let first = args.first().map(|s| s.as_str()).unwrap_or("");
    let is_uint = first.ends_with('u') || first.starts_with("uint");
    let is_int = !is_uint
        && (first.ends_with('i')
            || first.starts_with("int")
            || (first.parse::<i64>().is_ok() && !first.contains('.')));
    match (dim, is_uint, is_int) {
        (2, true, _) => "uint2",
        (3, true, _) => "uint3",
        (4, true, _) => "uint4",
        (2, _, true) => "int2",
        (3, _, true) => "int3",
        (4, _, true) => "int4",
        (2, _, _) => "float2",
        (3, _, _) => "float3",
        _ => "float4",
    }
}
