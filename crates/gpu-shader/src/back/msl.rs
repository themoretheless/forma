use crate::front::wgsl::{find_matching_delim, find_top_level_char, split_comma_top_level};
use crate::proc::compute_type_layout;
use crate::valid::ModuleInfo;
use crate::{
    AddressSpace, ArraySize, Binding, BuiltIn, EntryPoint, Function, GlobalVariable, Handle,
    ImageClass, Interpolation, Module, Scalar, ScalarKind, ShaderStage, StorageAccess, Type,
    TypeInner, VectorSize,
};
use std::collections::{BTreeMap, BTreeSet};

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
                        if is_fragment_return_struct(module, struct_handle) {
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
        out.push_str(&format!("constant {ty_str} {name} = {expr_msl};\n"));
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
            AddressSpace::Handle => {
                if let Some(rb) = gv.binding {
                    match module.types[gv.ty].inner {
                        TypeInner::Image { .. } => {
                            binding_map.textures.insert(
                                (rb.group, rb.binding),
                                msl_texture_slot(rb.group, rb.binding),
                            );
                        }
                        TypeInner::Sampler { .. } => {
                            binding_map.samplers.insert(
                                (rb.group, rb.binding),
                                msl_sampler_slot(rb.group, rb.binding),
                            );
                        }
                        _ => {}
                    }
                }
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

fn is_fragment_return_struct(module: &Module, handle: Handle<Type>) -> bool {
    for ep in &module.entry_points {
        if ep.stage == ShaderStage::Fragment {
            if let Some(res) = &ep.function.result {
                if res.ty == handle {
                    return true;
                }
            }
        }
    }
    false
}

fn runtime_array_info(module: &Module, ty: Handle<Type>) -> Option<(u32, u32)> {
    match &module.types[ty].inner {
        TypeInner::Array {
            size: ArraySize::Dynamic,
            stride,
            base,
        } => {
            let s = if *stride > 0 {
                *stride
            } else {
                compute_type_layout(&module.types, *base).size
            };
            Some((0, s.max(1)))
        }
        TypeInner::Struct { members, .. } => {
            let last = members.last()?;
            if let TypeInner::Array {
                size: ArraySize::Dynamic,
                stride,
                base,
            } = &module.types[last.ty].inner
            {
                let s = if *stride > 0 {
                    *stride
                } else {
                    compute_type_layout(&module.types, *base).size
                };
                Some((last.offset, s.max(1)))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn compute_all_function_globals(
    module: &Module,
) -> BTreeMap<String, Vec<Handle<GlobalVariable>>> {
    let mut map = BTreeMap::new();
    for (_, func) in module.functions.iter() {
        let Some(name) = &func.name else { continue };
        let mut visited = BTreeSet::new();
        visited.insert(name.clone());
        let mut globals = func.used_globals.clone();
        collect_callee_globals(module, func, &mut visited, &mut globals);
        map.insert(name.clone(), globals.into_iter().collect());
    }
    map
}

fn collect_callee_globals(
    module: &Module,
    func: &Function,
    visited: &mut BTreeSet<String>,
    out: &mut BTreeSet<Handle<GlobalVariable>>,
) {
    for callee in &func.called_functions {
        if visited.insert(callee.clone()) {
            for (_, f) in module.functions.iter() {
                if f.name.as_deref() == Some(callee.as_str()) {
                    out.extend(f.used_globals.iter().copied());
                    collect_callee_globals(module, f, visited, out);
                }
            }
        }
    }
}

fn msl_global_param_decl(module: &Module, gh: Handle<GlobalVariable>) -> String {
    let gv = &module.global_variables[gh];
    let gname = gv.name.as_deref().unwrap_or("g");
    match gv.space {
        AddressSpace::Uniform => {
            let ty = msl_type_for_handle(module, gv.ty, false);
            format!("constant {ty}& {gname}")
        }
        AddressSpace::Immediate | AddressSpace::PushConstant => {
            let ty = msl_type_for_handle(module, gv.ty, false);
            format!("constant {ty}& {gname}")
        }
        AddressSpace::Storage { access } => {
            let qual = if access == StorageAccess::Read {
                "const device"
            } else {
                "device"
            };
            match &module.types[gv.ty].inner {
                TypeInner::Array {
                    base,
                    size: ArraySize::Dynamic,
                    ..
                } => {
                    let elem_ty = msl_type_for_handle(module, *base, false);
                    format!("{qual} {elem_ty}* {gname}")
                }
                _ => {
                    let ty = msl_type_for_handle(module, gv.ty, false);
                    format!("{qual} {ty}& {gname}")
                }
            }
        }
        AddressSpace::WorkGroup => {
            match &module.types[gv.ty].inner {
                TypeInner::Array {
                    base,
                    size: ArraySize::Constant(_),
                    ..
                } => {
                    let elem_ty = msl_type_for_handle(module, *base, false);
                    format!("threadgroup {elem_ty}* {gname}")
                }
                _ => {
                    let ty = msl_type_for_handle(module, gv.ty, false);
                    format!("threadgroup {ty}& {gname}")
                }
            }
        }
        AddressSpace::Private => {
            let ty = msl_type_for_handle(module, gv.ty, false);
            format!("thread {ty}& {gname}")
        }
        AddressSpace::Handle => {
            let ty = msl_type_for_handle(module, gv.ty, false);
            format!("{ty} {gname}")
        }
        AddressSpace::Function => {
            let ty = msl_type_for_handle(module, gv.ty, false);
            format!("thread {ty}& {gname}")
        }
    }
}

fn emit_helper_function_prototype(
    out: &mut String,
    module: &Module,
    func: &Function,
    func_globals: &BTreeMap<String, Vec<Handle<GlobalVariable>>>,
    _fh: Handle<Function>,
    _binding_map: &BindingMap,
) {
    let raw_fname = func.name.as_deref().unwrap_or("helper");
    let fname = if raw_fname == "main" {
        "_wgpu_helper_main"
    } else {
        msl_escape_ident(raw_fname)
    };
    let ret_ty = func
        .result
        .as_ref()
        .map(|r| msl_type_for_handle(module, r.ty, false))
        .unwrap_or_else(|| "void".to_string());

    let mut params = Vec::new();
    for arg in &func.arguments {
        let raw_aname = arg.name.as_deref().unwrap_or("arg");
        let aname = msl_escape_ident(raw_aname);
        let aty = msl_param_type(module, arg.ty);
        params.push(format!("{aty} {aname}"));
    }
    if let Some(globals) = func_globals.get(raw_fname) {
        for &gh in globals {
            params.push(msl_global_param_decl(module, gh));
        }
    }
    if func.body_src.contains("arrayLength") {
        params.push("constant uint* _wgpu_buffer_sizes".to_string());
    }
    out.push_str(&format!("{ret_ty} {fname}({});\n", params.join(", ")));
}

fn emit_helper_function(
    out: &mut String,
    module: &Module,
    func: &Function,
    func_globals: &BTreeMap<String, Vec<Handle<GlobalVariable>>>,
    _fh: Handle<Function>,
    binding_map: &BindingMap,
) {
    let raw_fname = func.name.as_deref().unwrap_or("helper");
    let fname = if raw_fname == "main" {
        "_wgpu_helper_main"
    } else {
        msl_escape_ident(raw_fname)
    };
    let ret_ty = func
        .result
        .as_ref()
        .map(|r| msl_type_for_handle(module, r.ty, false))
        .unwrap_or_else(|| "void".to_string());

    let mut params = Vec::new();
    let mut ptr_params = BTreeSet::new();
    for arg in &func.arguments {
        let raw_aname = arg.name.as_deref().unwrap_or("arg");
        let aname = msl_escape_ident(raw_aname);
        if matches!(module.types[arg.ty].inner, TypeInner::Pointer { .. }) {
            ptr_params.insert(raw_aname.to_string());
        }
        let aty = msl_param_type(module, arg.ty);
        params.push(format!("{aty} {aname}"));
    }

    if let Some(globals) = func_globals.get(raw_fname) {
        for &gh in globals {
            params.push(msl_global_param_decl(module, gh));
        }
    }
    if func.body_src.contains("arrayLength") {
        params.push("constant uint* _wgpu_buffer_sizes".to_string());
    }

    out.push_str(&format!(
        "{ret_ty} {fname}({}) {{\n",
        params.join(", ")
    ));
    let body = translate_function_body(
        module,
        &func.body_src,
        &ptr_params,
        func_globals,
        binding_map,
        None,
    );
    out.push_str(&body);
    out.push_str("}\n\n");
}

fn emit_entry_point(
    out: &mut String,
    module: &Module,
    ep: &EntryPoint,
    func_globals: &BTreeMap<String, Vec<Handle<GlobalVariable>>>,
    binding_map: &BindingMap,
) {
    let func = &ep.function;
    let mut all_used_globals = func.used_globals.clone();
    let mut visited = BTreeSet::new();
    collect_callee_globals(module, func, &mut visited, &mut all_used_globals);

    // Check if any vertex attributes `@location(N)` are in the entry point arguments
    let has_vertex_input_attrs = ep.stage == ShaderStage::Vertex
        && func
            .arguments
            .iter()
            .any(|a| matches!(a.binding, Some(Binding::Location { .. })));

    let stage_in_struct_name = format!("_wgpu_{}_StageIn", ep.name);
    if has_vertex_input_attrs {
        out.push_str(&format!("struct {stage_in_struct_name} {{\n"));
        for (idx, arg) in func.arguments.iter().enumerate() {
            if let Some(Binding::Location { location, .. }) = arg.binding {
                let aname = arg.name.clone().unwrap_or_else(|| format!("arg_{idx}"));
                let aty = msl_type_for_handle(module, arg.ty, false);
                out.push_str(&format!("    {aty} {aname} [[attribute({location})]];\n"));
            }
        }
        out.push_str("};\n\n");
    }

    // Check if return value is a direct builtin or location scalar/vector (not a struct)
    let mut direct_out_struct_name: Option<String> = None;
    let ret_ty_str = if let Some(res) = &func.result {
        if let Some(binding) = res.binding {
            let sname = format!("_wgpu_{}_StageOut", ep.name);
            let inner_ty = msl_type_for_handle(module, res.ty, false);
            let attr = match binding {
                Binding::BuiltIn(BuiltIn::Position) => "[[position]]".to_string(),
                Binding::BuiltIn(BuiltIn::FragDepth) => "[[depth(any)]]".to_string(),
                Binding::Location { location, .. } => {
                    if ep.stage == ShaderStage::Fragment {
                        format!("[[color({location})]]")
                    } else {
                        format!("[[user(loc{location})]]")
                    }
                }
                _ => String::new(),
            };
            out.push_str(&format!(
                "struct {sname} {{\n    {inner_ty} value {attr};\n}};\n\n"
            ));
            direct_out_struct_name = Some(sname.clone());
            sname
        } else {
            msl_type_for_handle(module, res.ty, false)
        }
    } else {
        "void".to_string()
    };

    let stage_kw = match ep.stage {
        ShaderStage::Compute => "kernel",
        ShaderStage::Vertex => "vertex",
        ShaderStage::Fragment => "fragment",
    };

    let mut params = Vec::new();
    let mut preamble = String::new();

    if has_vertex_input_attrs {
        params.push(format!("{stage_in_struct_name} _stage_in [[stage_in]]"));
    }

    for (idx, arg) in func.arguments.iter().enumerate() {
        let aname = arg.name.clone().unwrap_or_else(|| format!("arg_{idx}"));
        let aty = msl_type_for_handle(module, arg.ty, false);
        match arg.binding {
            Some(Binding::Location { .. }) if has_vertex_input_attrs => {
                preamble.push_str(&format!("    {aty} {aname} = _stage_in.{aname};\n"));
            }
            Some(Binding::BuiltIn(b)) => {
                let attr = match b {
                    BuiltIn::GlobalInvocationId => "[[thread_position_in_grid]]",
                    BuiltIn::LocalInvocationId => "[[thread_position_in_threadgroup]]",
                    BuiltIn::LocalInvocationIndex => "[[thread_index_in_threadgroup]]",
                    BuiltIn::WorkGroupId => "[[threadgroup_position_in_grid]]",
                    BuiltIn::NumWorkGroups => "[[threadgroups_per_grid]]",
                    BuiltIn::VertexIndex => "[[vertex_id]]",
                    BuiltIn::InstanceIndex => "[[instance_id]]",
                    BuiltIn::Position => "[[position]]",
                    BuiltIn::FrontFacing => "[[front_facing]]",
                    BuiltIn::FragDepth => "[[depth(any)]]",
                };
                params.push(format!("{aty} {aname} {attr}"));
            }
            None => {
                // Struct parameter in vertex/fragment shader (`[[stage_in]]`)
                params.push(format!("{aty} {aname} [[stage_in]]"));
            }
            Some(Binding::Location { location, .. }) => {
                params.push(format!("{aty} {aname} [[user(loc{location})]]"));
            }
        }
    }

    // Bind used globals in entry point signature (or declare workgroup/private globals in preamble)
    for &gh in &all_used_globals {
        let gv = &module.global_variables[gh];
        let gname = gv.name.as_deref().unwrap_or("g");
        match gv.space {
            AddressSpace::Uniform => {
                if let Some(rb) = gv.binding {
                    let slot = msl_buffer_slot(rb.group, rb.binding);
                    let ty = msl_type_for_handle(module, gv.ty, false);
                    params.push(format!("constant {ty}& {gname} [[buffer({slot})]]"));
                }
            }
            AddressSpace::Immediate | AddressSpace::PushConstant => {
                let slot = MSL_IMMEDIATE_BUFFER_SLOT;
                let ty = msl_type_for_handle(module, gv.ty, false);
                params.push(format!("constant {ty}& {gname} [[buffer({slot})]]"));
            }
            AddressSpace::Storage { access } => {
                if let Some(rb) = gv.binding {
                    let slot = msl_buffer_slot(rb.group, rb.binding);
                    let qual = if access == StorageAccess::Read {
                        "const device"
                    } else {
                        "device"
                    };
                    match &module.types[gv.ty].inner {
                        TypeInner::Array {
                            base,
                            size: ArraySize::Dynamic,
                            ..
                        } => {
                            let elem_ty = msl_type_for_handle(module, *base, false);
                            params.push(format!("{qual} {elem_ty}* {gname} [[buffer({slot})]]"));
                        }
                        _ => {
                            let ty = msl_type_for_handle(module, gv.ty, false);
                            params.push(format!("{qual} {ty}& {gname} [[buffer({slot})]]"));
                        }
                    }
                }
            }
            AddressSpace::Handle => {
                if let Some(rb) = gv.binding {
                    match module.types[gv.ty].inner {
                        TypeInner::Image { .. } => {
                            let slot = msl_texture_slot(rb.group, rb.binding);
                            let ty = msl_type_for_handle(module, gv.ty, false);
                            params.push(format!("{ty} {gname} [[texture({slot})]]"));
                        }
                        TypeInner::Sampler { .. } => {
                            let slot = msl_sampler_slot(rb.group, rb.binding);
                            let ty = msl_type_for_handle(module, gv.ty, false);
                            params.push(format!("{ty} {gname} [[sampler({slot})]]"));
                        }
                        _ => {}
                    }
                }
            }
            AddressSpace::WorkGroup => {
                match &module.types[gv.ty].inner {
                    TypeInner::Array {
                        base,
                        size: ArraySize::Constant(n),
                        ..
                    } => {
                        let elem_ty = msl_type_for_handle(module, *base, false);
                        let count = n.get();
                        preamble.push_str(&format!(
                            "    threadgroup {elem_ty} {gname}[{count}];\n"
                        ));
                    }
                    _ => {
                        let ty = msl_type_for_handle(module, gv.ty, false);
                        preamble.push_str(&format!("    threadgroup {ty} {gname};\n"));
                    }
                }
            }
            AddressSpace::Private | AddressSpace::Function => {
                let ty = msl_type_for_handle(module, gv.ty, false);
                if let Some(init) = &gv.init {
                    let init_msl = translate_expr(module, init, &BTreeSet::new(), &BTreeMap::new());
                    preamble.push_str(&format!("    {ty} {gname} = {init_msl};\n"));
                } else {
                    preamble.push_str(&format!("    {ty} {gname} = {{}};\n"));
                }
            }
        }
    }

    let uses_array_length = func.body_src.contains("arrayLength")
        || func.called_functions.iter().any(|cf| {
            module
                .functions
                .iter()
                .any(|(_, f)| f.name.as_deref() == Some(cf.as_str()) && f.body_src.contains("arrayLength"))
        });
    if uses_array_length {
        params.push(format!(
            "constant uint* _wgpu_buffer_sizes [[buffer({MSL_ARRAY_LENGTHS_BUFFER_SLOT})]]"
        ));
    }

    let ep_fn_name = if ep.name == "main" {
        "_wgpu_entry_main"
    } else {
        ep.name.as_str()
    };
    out.push_str(&format!(
        "{stage_kw} {ret_ty_str} {ep_fn_name}({}) {{\n",
        params.join(", ")
    ));
    out.push_str(&preamble);

    let body = translate_function_body(
        module,
        &func.body_src,
        &BTreeSet::new(),
        func_globals,
        binding_map,
        direct_out_struct_name.as_deref(),
    );
    out.push_str(&body);
    out.push_str("}\n\n");
}

pub fn msl_type_for_handle(module: &Module, handle: Handle<Type>, packed_vec3: bool) -> String {
    if let Some(name) = &module.types[handle].name {
        return name.clone();
    }
    match &module.types[handle].inner {
        TypeInner::Scalar(s) => msl_scalar(*s).to_string(),
        TypeInner::Vector { size, scalar } => {
            let prefix = match (scalar.kind, scalar.width) {
                (ScalarKind::Float, 2) => "half",
                (ScalarKind::Float, _) => "float",
                (ScalarKind::Sint, _) => "int",
                (ScalarKind::Uint, _) => "uint",
                (ScalarKind::Bool, _) => "bool",
            };
            let n = size.as_u32();
            if packed_vec3 && n == 3 {
                format!("packed_{prefix}3")
            } else {
                format!("{prefix}{n}")
            }
        }
        TypeInner::Matrix {
            columns,
            rows,
            scalar,
        } => {
            let prefix = if scalar.width == 2 { "half" } else { "float" };
            format!("{prefix}{}x{}", columns.as_u32(), rows.as_u32())
        }
        TypeInner::Atomic(s) => {
            if s.kind == ScalarKind::Sint {
                "atomic_int".to_string()
            } else {
                "atomic_uint".to_string()
            }
        }
        TypeInner::Array { base, size, .. } => {
            let elem = msl_type_for_handle(module, *base, false);
            match size {
                ArraySize::Constant(n) => format!("_wgpu_arr<{elem}, {}>", n.get()),
                ArraySize::Dynamic => elem,
            }
        }
        TypeInner::Pointer { base, space } => {
            let elem = msl_type_for_handle(module, *base, false);
            let qual = match space {
                AddressSpace::WorkGroup => "threadgroup",
                AddressSpace::Storage {
                    access: StorageAccess::Read,
                } => "const device",
                AddressSpace::Storage { .. } => "device",
                AddressSpace::Uniform | AddressSpace::Immediate | AddressSpace::PushConstant => {
                    "constant"
                }
                _ => "thread",
            };
            format!("{qual} {elem}*")
        }
        TypeInner::Image { dim, class, .. } => {
            let dim_str = match dim {
                crate::ImageDimension::D1 => "1d",
                crate::ImageDimension::D2 => "2d",
                crate::ImageDimension::D3 => "3d",
                crate::ImageDimension::Cube => "cube",
            };
            match class {
                ImageClass::Depth { .. } => format!("depth{dim_str}<float>"),
                ImageClass::Sampled { kind, .. } => {
                    let s = match kind {
                        ScalarKind::Sint => "int",
                        ScalarKind::Uint => "uint",
                        _ => "float",
                    };
                    format!("texture{dim_str}<{s}>")
                }
                ImageClass::Storage { access, .. } => {
                    let acc = match access {
                        StorageAccess::Read => "access::read",
                        StorageAccess::Write => "access::write",
                        StorageAccess::ReadWrite => "access::read_write",
                    };
                    format!("texture{dim_str}<float, {acc}>")
                }
            }
        }
        TypeInner::Sampler { .. } => "sampler".to_string(),
        TypeInner::Struct { .. } => module.types[handle]
            .name
            .clone()
            .unwrap_or_else(|| "uint".to_string()),
    }
}

fn msl_param_type(module: &Module, handle: Handle<Type>) -> String {
    match &module.types[handle].inner {
        TypeInner::Pointer { base, space } => {
            let elem = msl_type_for_handle(module, *base, false);
            let qual = match space {
                AddressSpace::WorkGroup => "threadgroup",
                AddressSpace::Storage {
                    access: StorageAccess::Read,
                } => "const device",
                AddressSpace::Storage { .. } => "device",
                AddressSpace::Uniform | AddressSpace::Immediate | AddressSpace::PushConstant => {
                    "constant"
                }
                _ => "thread",
            };
            format!("{qual} {elem}*")
        }
        _ => msl_type_for_handle(module, handle, false),
    }
}

fn msl_scalar(s: Scalar) -> &'static str {
    match (s.kind, s.width) {
        (ScalarKind::Float, 2) => "half",
        (ScalarKind::Float, _) => "float",
        (ScalarKind::Sint, _) => "int",
        (ScalarKind::Uint, _) => "uint",
        (ScalarKind::Bool, _) => "bool",
    }
}

pub fn wgsl_type_str_to_msl(module: &Module, raw: &str) -> String {
    let s = raw.trim();
    if let Some(aliased) = module.aliases.get(s) {
        return wgsl_type_str_to_msl(module, aliased);
    }
    match s {
        "bool" => "bool".to_string(),
        "i32" => "int".to_string(),
        "u32" => "uint".to_string(),
        "f32" => "float".to_string(),
        "f16" => "half".to_string(),
        "vec2f" | "vec2<f32>" => "float2".to_string(),
        "vec3f" | "vec3<f32>" => "float3".to_string(),
        "vec4f" | "vec4<f32>" => "float4".to_string(),
        "vec2u" | "vec2<u32>" => "uint2".to_string(),
        "vec3u" | "vec3<u32>" => "uint3".to_string(),
        "vec4u" | "vec4<u32>" => "uint4".to_string(),
        "vec2i" | "vec2<i32>" => "int2".to_string(),
        "vec3i" | "vec3<i32>" => "int3".to_string(),
        "vec4i" | "vec4<i32>" => "int4".to_string(),
        "vec2<bool>" | "vec2b" => "bool2".to_string(),
        "vec3<bool>" | "vec3b" => "bool3".to_string(),
        "vec4<bool>" | "vec4b" => "bool4".to_string(),
        "mat2x2f" | "mat2x2<f32>" => "float2x2".to_string(),
        "mat3x3f" | "mat3x3<f32>" => "float3x3".to_string(),
        "mat4x4f" | "mat4x4<f32>" => "float4x4".to_string(),
        "atomic<u32>" => "atomic_uint".to_string(),
        "atomic<i32>" => "atomic_int".to_string(),
        _ if s.starts_with("array<") => {
            let inner = s
                .strip_prefix("array<")
                .and_then(|x| x.strip_suffix('>'))
                .unwrap_or("u32");
            let parts = split_comma_top_level(inner);
            let elem = wgsl_type_str_to_msl(module, parts.first().copied().unwrap_or("u32"));
            if let Some(cnt) = parts.get(1) {
                format!("_wgpu_arr<{elem}, {}>", cnt.trim())
            } else {
                elem
            }
        }
        _ => s.to_string(),
    }
}

fn translate_function_body(
    module: &Module,
    body: &str,
    ptr_params: &BTreeSet<String>,
    func_globals: &BTreeMap<String, Vec<Handle<GlobalVariable>>>,
    binding_map: &BindingMap,
    direct_out_struct: Option<&str>,
) -> String {
    let rewritten = rewrite_statements(
        module,
        body,
        ptr_params,
        func_globals,
        binding_map,
        direct_out_struct,
    );
    rewritten
}

fn rewrite_statements(
    module: &Module,
    src: &str,
    ptr_params: &BTreeSet<String>,
    func_globals: &BTreeMap<String, Vec<Handle<GlobalVariable>>>,
    binding_map: &BindingMap,
    direct_out_struct: Option<&str>,
) -> String {
    let mut out = String::with_capacity(src.len() + 128);
    let bytes = src.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        // Preserve whitespace
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            out.push(bytes[i] as char);
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }

        let rest = &src[i..];

        // Handle `{` or `}`
        if bytes[i] == b'{' || bytes[i] == b'}' {
            out.push(bytes[i] as char);
            i += 1;
            continue;
        }

        // Handle `if`, `else if`, `else`
        if let Some(after_if) = strip_kw_token(rest, "if") {
            let cond_len = find_block_open(after_if).unwrap_or(0);
            let cond = after_if[..cond_len].trim();
            let cond_msl = translate_expr_with_ctx(
                module,
                cond,
                ptr_params,
                func_globals,
                binding_map,
            );
            let cond_wrapped = if cond_msl.starts_with('(')
                && find_matching_delim(&cond_msl, '(', ')') == Some(cond_msl.len() - 1)
            {
                cond_msl
            } else {
                format!("({cond_msl})")
            };
            out.push_str(&format!("if {cond_wrapped} {{"));
            let consumed = (src.len() - after_if.len()) - i + cond_len + 1;
            i += consumed;
            continue;
        }
        if let Some(after_else) = strip_kw_token(rest, "else") {
            let trimmed_else = after_else.trim_start();
            if trimmed_else.starts_with("if")
                && trimmed_else[2..]
                    .chars()
                    .next()
                    .map(|c| c.is_ascii_whitespace() || c == '(')
                    .unwrap_or(false)
            {
                out.push_str("else ");
                i = src.len() - trimmed_else.len();
                continue;
            } else if let Some(brace_off) = find_block_open(after_else) {
                if after_else[..brace_off].trim().is_empty() {
                    out.push_str("else {");
                    i = (src.len() - after_else.len()) + brace_off + 1;
                    continue;
                }
            }
        }

        // Handle `while`
        if let Some(after_while) = strip_kw_token(rest, "while") {
            let cond_len = find_block_open(after_while).unwrap_or(0);
            let cond = after_while[..cond_len].trim();
            let cond_msl = translate_expr_with_ctx(
                module,
                cond,
                ptr_params,
                func_globals,
                binding_map,
            );
            let cond_wrapped = if cond_msl.starts_with('(')
                && find_matching_delim(&cond_msl, '(', ')') == Some(cond_msl.len() - 1)
            {
                cond_msl
            } else {
                format!("({cond_msl})")
            };
            out.push_str(&format!("while {cond_wrapped} {{"));
            let consumed = (src.len() - after_while.len()) - i + cond_len + 1;
            i += consumed;
            continue;
        }

        // Handle `loop { ... continuing { ... } }`
        if let Some(after_loop) = strip_kw_token(rest, "loop") {
            if let Some(open_off) = find_block_open(after_loop) {
                let block_str = &after_loop[open_off..];
                if let Some(close_off) = find_matching_delim(block_str, '{', '}') {
                    let loop_inner = &block_str[1..close_off];
                    out.push_str("while (true) {\n");
                    let inner_msl = rewrite_loop_body(
                        module,
                        loop_inner,
                        ptr_params,
                        func_globals,
                        binding_map,
                        direct_out_struct,
                    );
                    out.push_str(&inner_msl);
                    out.push('}');
                    i = (src.len() - after_loop.len()) + open_off + close_off + 1;
                    continue;
                }
            }
        }

        // Handle `for (...) {`
        if let Some(after_for) = strip_kw_token(rest, "for") {
            let after_for_trim = after_for.trim_start();
            if after_for_trim.starts_with('(') {
                if let Some(close_paren) = find_matching_delim(after_for_trim, '(', ')') {
                    let header = &after_for_trim[1..close_paren];
                    let parts: Vec<&str> = header.splitn(3, ';').collect();
                    let init_str = parts.first().copied().unwrap_or("").trim();
                    let cond_str = parts.get(1).copied().unwrap_or("").trim();
                    let update_str = parts.get(2).copied().unwrap_or("").trim();

                    let init_msl = if init_str.is_empty() {
                        String::new()
                    } else {
                        translate_simple_stmt(
                            module,
                            init_str,
                            ptr_params,
                            func_globals,
                            binding_map,
                            None,
                        )
                    };
                    let cond_msl = if cond_str.is_empty() {
                        String::new()
                    } else {
                        translate_expr_with_ctx(
                            module,
                            cond_str,
                            ptr_params,
                            func_globals,
                            binding_map,
                        )
                    };
                    let update_msl = if update_str.is_empty() {
                        String::new()
                    } else {
                        translate_simple_stmt(
                            module,
                            update_str,
                            ptr_params,
                            func_globals,
                            binding_map,
                            None,
                        )
                    };
                    out.push_str(&format!("for ({init_msl}; {cond_msl}; {update_msl})"));
                    i = (src.len() - after_for_trim.len()) + close_paren + 1;
                    continue;
                }
            }
        }

        // Handle `switch expr { case ...: { ... } default: { ... } }`
        if let Some(after_switch) = strip_kw_token(rest, "switch") {
            if let Some(open_off) = find_block_open(after_switch) {
                let expr_str = after_switch[..open_off].trim();
                let block_str = &after_switch[open_off..];
                if let Some(close_off) = find_matching_delim(block_str, '{', '}') {
                    let expr_msl = translate_expr_with_ctx(
                        module,
                        expr_str,
                        ptr_params,
                        func_globals,
                        binding_map,
                    );
                    let switch_body = &block_str[1..close_off];
                    let msl_cases = rewrite_switch_body(
                        module,
                        switch_body,
                        ptr_params,
                        func_globals,
                        binding_map,
                        direct_out_struct,
                    );
                    out.push_str(&format!("switch ({expr_msl}) {{\n{msl_cases}}}"));
                    i = (src.len() - after_switch.len()) + open_off + close_off + 1;
                    continue;
                }
            }
        }

        // Otherwise read until top-level `;`
        if let Some(semi_off) = find_stmt_semicolon(rest) {
            let stmt = rest[..semi_off].trim();
            if !stmt.is_empty() {
                let stmt_msl = translate_simple_stmt(
                    module,
                    stmt,
                    ptr_params,
                    func_globals,
                    binding_map,
                    direct_out_struct,
                );
                out.push_str(&stmt_msl);
            }
            out.push(';');
            i += semi_off + 1;
        } else {
            // Remaining fragment without semicolon
            let stmt = rest.trim();
            if !stmt.is_empty() {
                let stmt_msl = translate_simple_stmt(
                    module,
                    stmt,
                    ptr_params,
                    func_globals,
                    binding_map,
                    direct_out_struct,
                );
                out.push_str(&stmt_msl);
            }
            break;
        }
    }

    out
}

fn rewrite_loop_body(
    module: &Module,
    loop_inner: &str,
    ptr_params: &BTreeSet<String>,
    func_globals: &BTreeMap<String, Vec<Handle<GlobalVariable>>>,
    binding_map: &BindingMap,
    direct_out_struct: Option<&str>,
) -> String {
    // Check if there is a top-level `continuing { ... }` block
    if let Some((before_cont, cont_body)) = split_continuing_block(loop_inner) {
        let cont_msl = rewrite_statements(
            module,
            cont_body,
            ptr_params,
            func_globals,
            binding_map,
            direct_out_struct,
        );
        let main_msl = rewrite_statements(
            module,
            before_cont,
            ptr_params,
            func_globals,
            binding_map,
            direct_out_struct,
        );
        format!("{main_msl}\n{cont_msl}\n")
    } else {
        rewrite_statements(
            module,
            loop_inner,
            ptr_params,
            func_globals,
            binding_map,
            direct_out_struct,
        )
    }
}

fn split_continuing_block(src: &str) -> Option<(&str, &str)> {
    let mut brace = 0i32;
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => brace += 1,
            b'}' => brace = (brace - 1).max(0),
            _ if brace == 0 => {
                if let Some(after) = strip_kw_token(&src[i..], "continuing") {
                    if let Some(open) = find_block_open(after) {
                        let blk = &after[open..];
                        if let Some(close) = find_matching_delim(blk, '{', '}') {
                            return Some((&src[..i], &blk[1..close]));
                        }
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn rewrite_switch_body(
    module: &Module,
    switch_body: &str,
    ptr_params: &BTreeSet<String>,
    func_globals: &BTreeMap<String, Vec<Handle<GlobalVariable>>>,
    binding_map: &BindingMap,
    direct_out_struct: Option<&str>,
) -> String {
    let mut out = String::new();
    let mut rest = switch_body.trim();
    while !rest.is_empty() {
        if let Some(after_case) = strip_kw_token(rest, "case") {
            let open_brace = find_block_open(after_case).unwrap_or(0);
            let selectors_str = after_case[..open_brace].trim().trim_end_matches(':').trim();
            for sel in split_comma_top_level(selectors_str) {
                let sel = sel.trim();
                if sel == "default" {
                    out.push_str("default:\n");
                } else if !sel.is_empty() {
                    let sel_msl =
                        translate_expr_with_ctx(module, sel, ptr_params, func_globals, binding_map);
                    out.push_str(&format!("case {sel_msl}:\n"));
                }
            }
            let blk = &after_case[open_brace..];
            if let Some(close_brace) = find_matching_delim(blk, '{', '}') {
                let inner = &blk[1..close_brace];
                let inner_msl = rewrite_statements(
                    module,
                    inner,
                    ptr_params,
                    func_globals,
                    binding_map,
                    direct_out_struct,
                );
                out.push_str(&format!("{{\n{inner_msl}\nbreak;\n}}\n"));
                rest = blk[close_brace + 1..].trim_start();
            } else {
                break;
            }
        } else if let Some(after_def) = strip_kw_token(rest, "default") {
            let open_brace = find_block_open(after_def).unwrap_or(0);
            let blk = &after_def[open_brace..];
            if let Some(close_brace) = find_matching_delim(blk, '{', '}') {
                let inner = &blk[1..close_brace];
                let inner_msl = rewrite_statements(
                    module,
                    inner,
                    ptr_params,
                    func_globals,
                    binding_map,
                    direct_out_struct,
                );
                out.push_str(&format!("default: {{\n{inner_msl}\nbreak;\n}}\n"));
                rest = blk[close_brace + 1..].trim_start();
            } else {
                break;
            }
        } else {
            break;
        }
    }
    out
}

fn translate_simple_stmt(
    module: &Module,
    stmt: &str,
    ptr_params: &BTreeSet<String>,
    func_globals: &BTreeMap<String, Vec<Handle<GlobalVariable>>>,
    binding_map: &BindingMap,
    direct_out_struct: Option<&str>,
) -> String {
    let s = stmt.trim();
    if s == "discard" {
        return "discard_fragment()".to_string();
    }
    if s == "break" || s == "continue" || s == "return" {
        return s.to_string();
    }
    if let Some(after_ret) = strip_kw_token(s, "return") {
        let expr_msl = translate_expr_with_ctx(
            module,
            after_ret.trim(),
            ptr_params,
            func_globals,
            binding_map,
        );
        if let Some(out_struct) = direct_out_struct {
            return format!("return {out_struct}{{{expr_msl}}}");
        } else {
            return format!("return {expr_msl}");
        }
    }

    // `let name [: Type] = expr` or `const name [: Type] = expr`
    if let Some(after_let) = strip_kw_token(s, "let").or_else(|| strip_kw_token(s, "const")) {
        if let Some(eq_pos) = find_top_level_char(after_let, '=') {
            let lhs = after_let[..eq_pos].trim();
            let rhs = after_let[eq_pos + 1..].trim();
            let rhs_msl =
                translate_expr_with_ctx(module, rhs, ptr_params, func_globals, binding_map);
            if let Some(colon) = lhs.find(':') {
                let name = msl_escape_ident(lhs[..colon].trim());
                let ty_msl = wgsl_type_str_to_msl(module, lhs[colon + 1..].trim());
                return format!("const {ty_msl} {name} = {rhs_msl}");
            } else {
                let name = msl_escape_ident(lhs);
                return format!("const auto {name} = {rhs_msl}");
            }
        }
    }

    // `var name [: Type] [= expr]`
    if let Some(after_var) = strip_kw_token(s, "var") {
        let after_var = if after_var.trim_start().starts_with('<') {
            let trimmed = after_var.trim_start();
            if let Some(close) = find_matching_delim(trimmed, '<', '>') {
                trimmed[close + 1..].trim_start()
            } else {
                after_var
            }
        } else {
            after_var.trim_start()
        };

        if let Some(eq_pos) = find_top_level_char(after_var, '=') {
            let lhs = after_var[..eq_pos].trim();
            let rhs = after_var[eq_pos + 1..].trim();
            let rhs_msl =
                translate_expr_with_ctx(module, rhs, ptr_params, func_globals, binding_map);
            if let Some(colon) = lhs.find(':') {
                let name = msl_escape_ident(lhs[..colon].trim());
                let ty_msl = wgsl_type_str_to_msl(module, lhs[colon + 1..].trim());
                return format!("{ty_msl} {name} = {rhs_msl}");
            } else {
                let name = msl_escape_ident(lhs);
                return format!("auto {name} = {rhs_msl}");
            }
        } else if let Some(colon) = after_var.find(':') {
            let name = msl_escape_ident(after_var[..colon].trim());
            let ty_msl = wgsl_type_str_to_msl(module, after_var[colon + 1..].trim());
            return format!("{ty_msl} {name} = {{}}");
        }
    }

    translate_expr_with_ctx(module, s, ptr_params, func_globals, binding_map)
}

pub fn translate_expr(
    module: &Module,
    expr: &str,
    ptr_params: &BTreeSet<String>,
    func_globals: &BTreeMap<String, Vec<Handle<GlobalVariable>>>,
) -> String {
    let empty_map = BindingMap {
        buffers: BTreeMap::new(),
        textures: BTreeMap::new(),
        samplers: BTreeMap::new(),
        immediate_slot: None,
        array_length_slots: BTreeMap::new(),
    };
    translate_expr_with_ctx(module, expr, ptr_params, func_globals, &empty_map)
}

fn translate_expr_with_ctx(
    module: &Module,
    expr: &str,
    ptr_params: &BTreeSet<String>,
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
                            let arg_msl = translate_expr_with_ctx(
                                module,
                                arg,
                                ptr_params,
                                func_globals,
                                binding_map,
                            );
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
                        .map(|a| {
                            translate_expr_with_ctx(
                                module,
                                a.trim(),
                                ptr_params,
                                func_globals,
                                binding_map,
                            )
                        })
                        .collect();
                    if is_array_ctor {
                        out.push_str(&format!("{msl_ctor}{{{{ {} }}}}", translated_args.join(", ")));
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
            let is_member = start > 0
                && expr[..start]
                    .trim_end()
                    .as_bytes()
                    .last()
                    .copied()
                    == Some(b'.');
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
                            .map(|a| {
                                translate_expr_with_ctx(
                                    module,
                                    a.trim(),
                                    ptr_params,
                                    func_globals,
                                    binding_map,
                                )
                            })
                            .collect()
                    };
                    let call_msl = translate_call(
                        module,
                        ident,
                        &args,
                        raw_args,
                        func_globals,
                        binding_map,
                    );
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
        out.push(expr[i..].chars().next().unwrap());
        i += expr[i..].chars().next().unwrap().len_utf8();
    }

    out
}

fn msl_escape_ident(ident: &str) -> &str {
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

fn try_match_generic_type_ctor(
    module: &Module,
    s: &str,
    prev_byte: u8,
) -> Option<(String, bool, usize)> {
    if prev_byte.is_ascii_alphanumeric() || prev_byte == b'_' || prev_byte == b'.' {
        return None;
    }
    for prefix in ["vec2<", "vec3<", "vec4<", "mat2x2<", "mat3x3<", "mat4x4<", "array<"] {
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

fn translate_call(
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

    match resolved_ident {
        "u32" => format!("uint({})", args.join(", ")),
        "i32" => format!("int({})", args.join(", ")),
        "f32" => format!("float({})", args.join(", ")),
        "f16" => format!("half({})", args.join(", ")),
        "bool" => format!("bool({})", args.join(", ")),
        "vec2f" | "vec2<f32>" | "vec2" => {
            let t = if resolved_ident == "vec2" {
                infer_vec_ctor_type(2, args)
            } else {
                "float2"
            };
            format!("{t}({})", args.join(", "))
        }
        "vec3f" | "vec3<f32>" | "vec3" => {
            let t = if resolved_ident == "vec3" {
                infer_vec_ctor_type(3, args)
            } else {
                "float3"
            };
            format!("{t}({})", args.join(", "))
        }
        "vec4f" | "vec4<f32>" | "vec4" => {
            let t = if resolved_ident == "vec4" {
                infer_vec_ctor_type(4, args)
            } else {
                "float4"
            };
            format!("{t}({})", args.join(", "))
        }
        "vec2u" | "vec2<u32>" => format!("uint2({})", args.join(", ")),
        "vec3u" | "vec3<u32>" => format!("uint3({})", args.join(", ")),
        "vec4u" | "vec4<u32>" => format!("uint4({})", args.join(", ")),
        "vec2i" | "vec2<i32>" => format!("int2({})", args.join(", ")),
        "vec3i" | "vec3<i32>" => format!("int3({})", args.join(", ")),
        "vec4i" | "vec4<i32>" => format!("int4({})", args.join(", ")),
        "mat2x2f" | "mat2x2<f32>" | "mat2x2" => format!("float2x2({})", args.join(", ")),
        "mat3x3f" | "mat3x3<f32>" | "mat3x3" => format!("float3x3({})", args.join(", ")),
        "mat4x4f" | "mat4x4<f32>" | "mat4x4" => format!("float4x4({})", args.join(", ")),

        // Synchronization barriers
        "workgroupBarrier" => "threadgroup_barrier(mem_flags::mem_threadgroup)".to_string(),
        "storageBarrier" => "threadgroup_barrier(mem_flags::mem_device)".to_string(),
        "textureBarrier" => "threadgroup_barrier(mem_flags::mem_texture)".to_string(),

        // Atomics
        "atomicLoad" => format!(
            "atomic_load_explicit({}, memory_order_relaxed)",
            args.first().map(|s| s.as_str()).unwrap_or("")
        ),
        "atomicStore" => format!(
            "atomic_store_explicit({}, {}, memory_order_relaxed)",
            args.first().map(|s| s.as_str()).unwrap_or(""),
            args.get(1).map(|s| s.as_str()).unwrap_or("0")
        ),
        "atomicAdd" => format!(
            "atomic_fetch_add_explicit({}, {}, memory_order_relaxed)",
            args.first().map(|s| s.as_str()).unwrap_or(""),
            args.get(1).map(|s| s.as_str()).unwrap_or("0")
        ),
        "atomicSub" => format!(
            "atomic_fetch_sub_explicit({}, {}, memory_order_relaxed)",
            args.first().map(|s| s.as_str()).unwrap_or(""),
            args.get(1).map(|s| s.as_str()).unwrap_or("0")
        ),
        "atomicMax" => format!(
            "atomic_fetch_max_explicit({}, {}, memory_order_relaxed)",
            args.first().map(|s| s.as_str()).unwrap_or(""),
            args.get(1).map(|s| s.as_str()).unwrap_or("0")
        ),
        "atomicMin" => format!(
            "atomic_fetch_min_explicit({}, {}, memory_order_relaxed)",
            args.first().map(|s| s.as_str()).unwrap_or(""),
            args.get(1).map(|s| s.as_str()).unwrap_or("0")
        ),
        "atomicAnd" => format!(
            "atomic_fetch_and_explicit({}, {}, memory_order_relaxed)",
            args.first().map(|s| s.as_str()).unwrap_or(""),
            args.get(1).map(|s| s.as_str()).unwrap_or("0")
        ),
        "atomicOr" => format!(
            "atomic_fetch_or_explicit({}, {}, memory_order_relaxed)",
            args.first().map(|s| s.as_str()).unwrap_or(""),
            args.get(1).map(|s| s.as_str()).unwrap_or("0")
        ),
        "atomicXor" => format!(
            "atomic_fetch_xor_explicit({}, {}, memory_order_relaxed)",
            args.first().map(|s| s.as_str()).unwrap_or(""),
            args.get(1).map(|s| s.as_str()).unwrap_or("0")
        ),
        "atomicExchange" => format!(
            "atomic_exchange_explicit({}, {}, memory_order_relaxed)",
            args.first().map(|s| s.as_str()).unwrap_or(""),
            args.get(1).map(|s| s.as_str()).unwrap_or("0")
        ),
        "atomicCompareExchangeWeak" => format!(
            "_wgpu_atomic_compare_exchange_weak({}, {}, {})",
            args.first().map(|s| s.as_str()).unwrap_or(""),
            args.get(1).map(|s| s.as_str()).unwrap_or("0"),
            args.get(2).map(|s| s.as_str()).unwrap_or("0")
        ),

        // Math & Bitwise intrinsics
        "inverseSqrt" => format!("rsqrt({})", args.join(", ")),
        "round" => format!("rint({})", args.join(", ")),
        "countLeadingZeros" => format!("clz({})", args.join(", ")),
        "countTrailingZeros" => format!("ctz({})", args.join(", ")),
        "countOneBits" => format!("popcount({})", args.join(", ")),
        "reverseBits" => format!("reverse_bits({})", args.join(", ")),
        "degrees" => format!("_wgpu_degrees({})", args.join(", ")),
        "radians" => format!("_wgpu_radians({})", args.join(", ")),
        "dpdx" => format!("dfdx({})", args.join(", ")),
        "dpdy" => format!("dfdy({})", args.join(", ")),
        "unpack2x16float" => format!(
            "float2(as_type<half2>(uint({})))",
            args.first().map(|s| s.as_str()).unwrap_or("0u")
        ),
        "pack2x16float" => format!(
            "as_type<uint>(half2(float2({})))",
            args.first().map(|s| s.as_str()).unwrap_or("float2(0.0)")
        ),
        "unpack4x8unorm" => format!(
            "unpack_unorm4x8_to_float(uint({}))",
            args.first().map(|s| s.as_str()).unwrap_or("0u")
        ),
        "pack4x8unorm" => format!(
            "pack_float_to_unorm4x8(float4({}))",
            args.first().map(|s| s.as_str()).unwrap_or("float4(0.0)")
        ),

        // Texture intrinsics
        "textureDimensions" => {
            let tex = args.first().map(|s| s.as_str()).unwrap_or("tex");
            if let Some(lod) = args.get(1) {
                format!("uint2({tex}.get_width({lod}), {tex}.get_height({lod}))")
            } else {
                format!("uint2({tex}.get_width(), {tex}.get_height())")
            }
        }
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
        "textureLoad" => {
            let tex = args.first().map(|s| s.as_str()).unwrap_or("tex");
            let coords = args.get(1).map(|s| s.as_str()).unwrap_or("uint2(0)");
            let lod = args.get(2).map(|s| s.as_str()).unwrap_or("0");
            format!("{tex}.read(uint2({coords}), uint({lod}))")
        }

        // Runtime-sized array length
        "arrayLength" => {
            let arg = raw_args.trim().trim_start_matches('&').trim();
            let base_ident = arg.split('.').next().unwrap_or(arg).trim();
            if let Some(&(slot, base_off, stride, _)) = binding_map.array_length_slots.get(base_ident)
            {
                if base_off > 0 {
                    format!(
                        "((_wgpu_buffer_sizes[{slot}] > {base_off}u) ? ((_wgpu_buffer_sizes[{slot}] - {base_off}u) / {stride}u) : 0u)"
                    )
                } else {
                    format!("(_wgpu_buffer_sizes[{slot}] / {stride}u)")
                }
            } else {
                "(_wgpu_buffer_sizes[0] / 4u)".to_string()
            }
        }

        // Struct constructor or user helper function call
        _ => {
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
    }
}

fn infer_vec_ctor_type(dim: u32, args: &[String]) -> &'static str {
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

fn strip_kw_token<'a>(s: &'a str, kw: &str) -> Option<&'a str> {
    let rest = s.strip_prefix(kw)?;
    if rest.is_empty() {
        return Some(rest);
    }
    let next = rest.as_bytes()[0];
    if !next.is_ascii_alphanumeric() && next != b'_' {
        Some(rest)
    } else {
        None
    }
}

fn find_block_open(s: &str) -> Option<usize> {
    let mut paren = 0i32;
    let mut bracket = 0i32;
    for (idx, ch) in s.char_indices() {
        match ch {
            '(' => paren += 1,
            ')' => paren = (paren - 1).max(0),
            '[' => bracket += 1,
            ']' => bracket = (bracket - 1).max(0),
            '{' if paren == 0 && bracket == 0 => return Some(idx),
            _ => {}
        }
    }
    None
}

fn find_stmt_semicolon(s: &str) -> Option<usize> {
    let mut paren = 0i32;
    let mut bracket = 0i32;
    let mut brace = 0i32;
    for (idx, ch) in s.char_indices() {
        match ch {
            '(' => paren += 1,
            ')' => paren = (paren - 1).max(0),
            '[' => bracket += 1,
            ']' => bracket = (bracket - 1).max(0),
            '{' => brace += 1,
            '}' => brace = (brace - 1).max(0),
            ';' if paren == 0 && bracket == 0 && brace == 0 => return Some(idx),
            _ => {}
        }
    }
    None
}
