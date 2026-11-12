//! The MSL back end: a [`Module`] as Metal Shading Language.

use crate::front::wgsl::{find_matching_delim, find_top_level_char, split_comma_top_level};
use crate::proc::compute_type_layout;
use crate::valid::ModuleInfo;
use crate::{
    AddressSpace, ArraySize, Binding, BuiltIn, EntryPoint, Function, GlobalVariable, Handle,
    ImageClass, ImageDimension, Interpolation, Module, Scalar, ScalarKind, ShaderStage,
    StorageAccess, Type, TypeInner, VectorSize,
};
use std::collections::{BTreeMap, BTreeSet};

mod constants;
mod entry;
mod expressions;
mod statements;
mod types;

pub use constants::*;
use entry::*;
pub use expressions::*;
use statements::*;
pub use types::*;

#[derive(Debug, Clone)]
pub struct Options {
    pub lang_version: (u8, u8),
}

impl Default for Options {
    fn default() -> Self {
        Self {
            lang_version: (2, 4),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PipelineOptions {
    pub entry_point: Option<(ShaderStage, String)>,
}

#[derive(Debug, Clone)]
pub struct BindingMap {
    pub buffers: BTreeMap<(u32, u32), u32>,
    pub textures: BTreeMap<(u32, u32), u32>,
    pub samplers: BTreeMap<(u32, u32), u32>,
    pub immediate_slot: Option<u32>,
    pub array_length_slots: BTreeMap<String, (u32, u32, u32, u32)>, // global_name -> (msl_buf_slot, base_offset, elem_stride, group_binding_packed)
}

#[derive(Debug, Clone)]
pub struct TranslatedMsl {
    pub source: String,
    pub entry_point_name: String,
    pub workgroup_size: [u32; 3],
    pub bindings: BindingMap,
}

#[inline]
pub const fn msl_buffer_slot(group: u32, binding: u32) -> u32 {
    group * 8 + binding
}

#[inline]
pub const fn msl_texture_slot(group: u32, binding: u32) -> u32 {
    group * 8 + binding
}

#[inline]
pub const fn msl_sampler_slot(group: u32, binding: u32) -> u32 {
    group * 4 + (binding & 3)
}

pub const MSL_VERTEX_BUFFER_BASE_SLOT: u32 = 24;

pub const MSL_IMMEDIATE_BUFFER_SLOT: u32 = 29;

pub const MSL_ARRAY_LENGTHS_BUFFER_SLOT: u32 = 30;

/// Stands in for an `arrayLength` whose buffer could not be found, so that the translation is
/// rejected rather than given a made-up length.
const UNRESOLVED_ARRAY_LENGTH: &str = "_wgpu_unresolved_array_length";

pub fn write_string(
    module: &Module,
    _info: &ModuleInfo,
    _options: &Options,
    pipeline_options: &PipelineOptions,
) -> Result<(String, BindingMap), String> {
    let mut out = String::with_capacity(module.raw_source.len() + 2048);
    out.push_str("#include <metal_stdlib>\n#include <simd/simd.h>\nusing namespace metal;\n\n");

    // Helper templates & structs for WGSL intrinsics in MSL
    out.push_str(
        r#"struct _wgpu_atomic_cmp_exch_u32 {
    uint old_value;
    bool exchanged;
};
inline _wgpu_atomic_cmp_exch_u32 _wgpu_atomic_compare_exchange_weak(device atomic_uint* ptr, uint cmp, uint val) {
    bool ok = atomic_compare_exchange_weak_explicit(ptr, &cmp, val, memory_order_relaxed, memory_order_relaxed);
    return _wgpu_atomic_cmp_exch_u32{cmp, ok};
}
inline _wgpu_atomic_cmp_exch_u32 _wgpu_atomic_compare_exchange_weak(threadgroup atomic_uint* ptr, uint cmp, uint val) {
    bool ok = atomic_compare_exchange_weak_explicit(ptr, &cmp, val, memory_order_relaxed, memory_order_relaxed);
    return _wgpu_atomic_cmp_exch_u32{cmp, ok};
}
inline _wgpu_atomic_cmp_exch_u32 _wgpu_atomic_compare_exchange_weak(device atomic_int* ptr, int cmp, int val) {
    bool ok = atomic_compare_exchange_weak_explicit(ptr, &cmp, val, memory_order_relaxed, memory_order_relaxed);
    return _wgpu_atomic_cmp_exch_u32{uint(cmp), ok};
}
inline float _wgpu_degrees(float x) { return x * 57.29577951308232f; }
inline float2 _wgpu_degrees(float2 x) { return x * 57.29577951308232f; }
inline float3 _wgpu_degrees(float3 x) { return x * 57.29577951308232f; }
inline float4 _wgpu_degrees(float4 x) { return x * 57.29577951308232f; }
inline float _wgpu_radians(float x) { return x * 0.017453292519943295f; }
inline float2 _wgpu_radians(float2 x) { return x * 0.017453292519943295f; }
inline float3 _wgpu_radians(float3 x) { return x * 0.017453292519943295f; }
inline float4 _wgpu_radians(float4 x) { return x * 0.017453292519943295f; }
template<typename F, typename E>
struct _wgpu_frexp_result {
    F fract;
    E exp;
};
template<typename F>
struct _wgpu_modf_result {
    F fract;
    F whole;
};
inline _wgpu_frexp_result<float, int> _wgpu_frexp(float x) { int e; float f = frexp(x, e); return {f, e}; }
inline _wgpu_frexp_result<float2, int2> _wgpu_frexp(float2 x) { int2 e; float2 f = frexp(x, e); return {f, e}; }
inline _wgpu_frexp_result<float3, int3> _wgpu_frexp(float3 x) { int3 e; float3 f = frexp(x, e); return {f, e}; }
inline _wgpu_frexp_result<float4, int4> _wgpu_frexp(float4 x) { int4 e; float4 f = frexp(x, e); return {f, e}; }
template<typename F>
inline _wgpu_modf_result<F> _wgpu_modf(F x) { F w; F f = modf(x, w); return {f, w}; }
template<typename T, uint N>
struct _wgpu_arr {
    T inner[N];
    thread T& operator[](uint i) thread { return inner[i]; }
    const thread T& operator[](uint i) const thread { return inner[i]; }
    thread T& operator[](int i) thread { return inner[i]; }
    const thread T& operator[](int i) const thread { return inner[i]; }
    device T& operator[](uint i) device { return inner[i]; }
    const device T& operator[](uint i) const device { return inner[i]; }
    device T& operator[](int i) device { return inner[i]; }
    const device T& operator[](int i) const device { return inner[i]; }
    constant T& operator[](uint i) constant { return inner[i]; }
    constant T& operator[](int i) constant { return inner[i]; }
    threadgroup T& operator[](uint i) threadgroup { return inner[i]; }
    const threadgroup T& operator[](uint i) const threadgroup { return inner[i]; }
    threadgroup T& operator[](int i) threadgroup { return inner[i]; }
    const threadgroup T& operator[](int i) const threadgroup { return inner[i]; }
};
"#,
    );
    out.push('\n');

    // Emit type aliases
    for (alias_name, target) in &module.aliases {
        let msl_ty = wgsl_type_str_to_msl(module, target);
        out.push_str(&format!("using {alias_name} = {msl_ty};\n"));
    }
    if !module.aliases.is_empty() {
        out.push('\n');
    }

    // Emit structs in declaration order with exact WGSL byte-for-byte padding
    let mut packed_vec3_fields: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for &struct_handle in &module.struct_order {
        let ty = &module.types[struct_handle];
        let Some(struct_name) = &ty.name else {
            continue;
        };
        let TypeInner::Struct { members, span } = &ty.inner else {
            continue;
        };
        let is_io_struct = members.iter().any(|m| m.binding.is_some());
        if is_io_struct {
            // Stage I/O struct (vertex output / fragment input / fragment output)
            // Keep attributes per entry point or emit standard Metal stage I/O struct
            out.push_str(&format!("struct {struct_name} {{\n"));
            for m in members {
                let m_name = m.name.as_deref().unwrap_or("field");
                let m_ty = msl_type_for_handle(module, m.ty, false);
                let attr = match m.binding {
                    Some(Binding::BuiltIn(BuiltIn::Position)) => " [[position]]".to_string(),
                    Some(Binding::BuiltIn(BuiltIn::FragDepth)) => " [[depth(any)]]".to_string(),
                    Some(Binding::BuiltIn(BuiltIn::FrontFacing)) => " [[front_facing]]".to_string(),
                    Some(Binding::Location {
                        location,
                        interpolation,
                        ..
                    }) => {
                        let flat_str = if interpolation == Some(Interpolation::Flat) {
                            " [[flat]]"
                        } else {
                            ""
                        };
                        // Note: for fragment return structs `@location(0)` is `[[color(0)]]`, while for vertex output / fragment input it's `[[user(locN)]]`.
                        // Determine whether this struct is used only as fragment return:
                        if is_vertex_input_struct(module, struct_handle) {
                            format!(" [[attribute({location})]]")
                        } else if is_fragment_return_struct(module, struct_handle) {
                            format!(" [[color({location})]]")
                        } else {
                            format!(" [[user(loc{location})]]{flat_str}")
                        }
                    }
                    _ => String::new(),
                };
                out.push_str(&format!("    {m_ty} {m_name}{attr};\n"));
            }
            out.push_str("};\n\n");
        } else {
            // Buffer/value struct: enforce 100% WGSL byte layout
            out.push_str(&format!("struct {struct_name} {{\n"));
            let mut cursor = 0u32;
            let mut pad_idx = 0u32;
            for (mi, m) in members.iter().enumerate() {
                let m_name = m.name.as_deref().unwrap_or("field");
                let layout = compute_type_layout(&module.types, m.ty);
                if m.offset > cursor {
                    let gap = m.offset - cursor;
                    emit_padding_fields(&mut out, gap, &mut pad_idx);
                    cursor = m.offset;
                }
                // Check if this field is a `vec3<T>`: in Metal, `float3` has sizeof == 16, whereas in WGSL `vec3<f32>` has size 12, align 16!
                // If we use `packed_float3` / `packed_uint3` / `packed_int3` whenever the next field is at offset `m.offset + 12` (or always use `packed_*3` + explicit padding),
                // then `sizeof` and every field offset in Metal is guaranteed to match WGSL!
                let next_offset = members.get(mi + 1).map(|nm| nm.offset).unwrap_or(*span);
                let is_vec3 = matches!(
                    module.types[m.ty].inner,
                    TypeInner::Vector {
                        size: VectorSize::Tri,
                        ..
                    }
                );
                if is_vec3 && next_offset < m.offset + 16 {
                    let m_ty = msl_type_for_handle(module, m.ty, true);
                    out.push_str(&format!("    {m_ty} {m_name};\n"));
                    packed_vec3_fields
                        .entry(struct_name.clone())
                        .or_default()
                        .insert(m_name.to_string());
                    cursor += 12;
                } else if is_vec3 {
                    let m_ty = msl_type_for_handle(module, m.ty, false);
                    out.push_str(&format!("    {m_ty} {m_name};\n"));
                    cursor += 16;
                } else {
                    let m_ty = msl_type_for_handle(module, m.ty, false);
                    out.push_str(&format!("    {m_ty} {m_name};\n"));
                    cursor += layout.size;
                }
            }
            if *span > cursor {
                emit_padding_fields(&mut out, *span - cursor, &mut pad_idx);
            }
            out.push_str("};\n\n");
        }
    }

    // Emit module-scope constants
    for (_, c) in module.constants.iter() {
        let Some(name) = &c.name else { continue };
        let ty_str = if let Some(th) = c.ty {
            msl_type_for_handle(module, th, false)
        } else if c.expr_src.trim_start().starts_with("array<") {
            let open = c.expr_src.find('<').unwrap();
            let close = find_matching_delim(&c.expr_src[open..], '<', '>').unwrap() + open;
            wgsl_type_str_to_msl(module, &c.expr_src[..=close])
        } else if c.expr_src.contains('.') {
            "float".to_string()
        } else {
            "uint".to_string()
        };
        let expr_msl = translate_expr(module, &c.expr_src, &BTreeSet::new(), &BTreeMap::new());
        if c.is_override {
            let mac = override_macro(name);
            out.push_str(&format!(
                "#ifndef {mac}\n#define {mac} {expr_msl}\n#endif\nconstant {ty_str} {name} = {ty_str}({mac});\n"
            ));
        } else {
            out.push_str(&format!("constant {ty_str} {name} = {expr_msl};\n"));
        }
    }
    if !module.constants.is_empty() {
        out.push('\n');
    }

    // Build global binding map
    let mut binding_map = BindingMap {
        buffers: BTreeMap::new(),
        textures: BTreeMap::new(),
        samplers: BTreeMap::new(),
        immediate_slot: None,
        array_length_slots: BTreeMap::new(),
    };

    // Textures and samplers take consecutive slots in (group, binding) order. Metal has 16
    // sampler slots per stage, so a formula over group/binding numbers would collide.
    let mut handle_bindings: Vec<(u32, u32, bool)> = module
        .global_variables
        .iter()
        .filter_map(|(_, gv)| {
            let rb = gv.binding?;
            match module.types[gv.ty].inner {
                TypeInner::Image { .. } => Some((rb.group, rb.binding, false)),
                TypeInner::Sampler { .. } => Some((rb.group, rb.binding, true)),
                _ => None,
            }
        })
        .collect();
    handle_bindings.sort_unstable();
    for (group, binding, sampler) in handle_bindings {
        let map = if sampler {
            &mut binding_map.samplers
        } else {
            &mut binding_map.textures
        };
        let slot = map.len() as u32;
        map.insert((group, binding), slot);
    }

    for (_, gv) in module.global_variables.iter() {
        match gv.space {
            AddressSpace::Uniform | AddressSpace::Storage { .. } => {
                if let Some(rb) = gv.binding {
                    let slot = msl_buffer_slot(rb.group, rb.binding);
                    binding_map.buffers.insert((rb.group, rb.binding), slot);
                    if let Some(gname) = &gv.name {
                        if let Some((base_off, stride)) = runtime_array_info(module, gv.ty) {
                            binding_map.array_length_slots.insert(
                                gname.clone(),
                                (slot, base_off, stride.max(1), (rb.group << 16) | rb.binding),
                            );
                        }
                    }
                }
            }
            AddressSpace::Immediate | AddressSpace::PushConstant => {
                binding_map.immediate_slot = Some(MSL_IMMEDIATE_BUFFER_SLOT);
            }
            _ => {}
        }
    }

    // Forward-declare and emit helper functions
    // Note: in WGSL, helper functions can access module-scope globals (`var<storage>`, `var<uniform>`, `var<workgroup>`, textures, samplers)
    // and can call functions declared later in the file.
    // In MSL, globals must be passed as parameters from the entry point to any helper function that uses them,
    // and we emit forward declarations so declaration order does not matter!
    let func_transitive_globals = compute_all_function_globals(module);

    for (fh, func) in module.functions.iter() {
        emit_helper_function_prototype(
            &mut out,
            module,
            func,
            &func_transitive_globals,
            fh,
            &binding_map,
        );
    }
    if !module.functions.is_empty() {
        out.push('\n');
    }

    for (fh, func) in module.functions.iter() {
        emit_helper_function(
            &mut out,
            module,
            func,
            &func_transitive_globals,
            fh,
            &binding_map,
        );
    }

    // Emit entry points
    for ep in &module.entry_points {
        if let Some((req_stage, req_name)) = &pipeline_options.entry_point {
            if &ep.stage != req_stage || &ep.name != req_name {
                continue;
            }
        }
        emit_entry_point(&mut out, module, ep, &func_transitive_globals, &binding_map);
    }

    if let Some(found) = out.find(UNRESOLVED_ARRAY_LENGTH) {
        let argument = out[found + UNRESOLVED_ARRAY_LENGTH.len()..]
            .trim_start_matches('(')
            .split(')')
            .next()
            .unwrap_or_default();
        return Err(format!(
            "arrayLength(&{argument}): `{argument}` is not a runtime-sized array in a bound buffer"
        ));
    }
    Ok((out, binding_map))
}

fn emit_padding_fields(out: &mut String, mut gap: u32, pad_idx: &mut u32) {
    while gap >= 4 {
        out.push_str(&format!("    uint _pad_{};\n", *pad_idx));
        *pad_idx += 1;
        gap -= 4;
    }
    while gap > 0 {
        out.push_str(&format!("    uchar _padb_{};\n", *pad_idx));
        *pad_idx += 1;
        gap -= 1;
    }
}
