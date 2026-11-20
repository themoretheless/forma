//! Entry points, helper functions and the globals they receive.

use super::*;

pub(super) fn is_vertex_input_struct(module: &Module, handle: Handle<Type>) -> bool {
    module.entry_points.iter().any(|ep| {
        ep.stage == ShaderStage::Vertex && ep.function.arguments.iter().any(|a| a.ty == handle)
    })
}

pub(super) fn is_fragment_return_struct(module: &Module, handle: Handle<Type>) -> bool {
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

pub(super) fn runtime_array_info(module: &Module, ty: Handle<Type>) -> Option<(u32, u32)> {
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

pub(super) fn compute_all_function_globals(
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

pub(super) fn collect_callee_globals(
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

pub(super) fn msl_global_param_decl(module: &Module, gh: Handle<GlobalVariable>) -> String {
    let gv = &module.global_variables[gh];
    let gname = gv.name.as_deref().unwrap_or("g");
    match gv.space {
        AddressSpace::Uniform | AddressSpace::Immediate | AddressSpace::PushConstant => {
            let ty = msl_type_for_handle(module, gv.ty, false);
            format!("constant {ty}& {gname}")
        }
        AddressSpace::Storage { access } => msl_storage_param_decl(module, gv.ty, gname, access),
        AddressSpace::WorkGroup => msl_workgroup_param_decl(module, gv.ty, gname),
        AddressSpace::Private | AddressSpace::Function => {
            let ty = msl_type_for_handle(module, gv.ty, false);
            format!("thread {ty}& {gname}")
        }
        AddressSpace::Handle => {
            let ty = msl_type_for_handle(module, gv.ty, false);
            format!("{ty} {gname}")
        }
    }
}

fn msl_storage_param_decl(
    module: &Module,
    ty: Handle<Type>,
    gname: &str,
    access: StorageAccess,
) -> String {
    let qual = if access == StorageAccess::Read {
        "const device"
    } else {
        "device"
    };
    match &module.types[ty].inner {
        TypeInner::Array {
            base,
            size: ArraySize::Dynamic,
            ..
        } => {
            let elem_ty = msl_type_for_handle(module, *base, false);
            format!("{qual} {elem_ty}* {gname}")
        }
        _ => {
            let ty_msl = msl_type_for_handle(module, ty, false);
            format!("{qual} {ty_msl}& {gname}")
        }
    }
}

fn msl_workgroup_param_decl(module: &Module, ty: Handle<Type>, gname: &str) -> String {
    match &module.types[ty].inner {
        TypeInner::Array {
            base,
            size: ArraySize::Constant(_),
            ..
        } => {
            let elem_ty = msl_type_for_handle(module, *base, false);
            format!("threadgroup {elem_ty}* {gname}")
        }
        _ => {
            let ty_msl = msl_type_for_handle(module, ty, false);
            format!("threadgroup {ty_msl}& {gname}")
        }
    }
}

struct HelperSignature {
    ret_ty: String,
    fname: String,
    params: Vec<String>,
    ptr_params: BTreeSet<String>,
}

fn build_helper_signature(
    module: &Module,
    func: &Function,
    func_globals: &BTreeMap<String, Vec<Handle<GlobalVariable>>>,
) -> HelperSignature {
    let raw_fname = func.name.as_deref().unwrap_or("helper");
    let fname = if raw_fname == "main" {
        "_wgpu_helper_main".to_string()
    } else {
        msl_escape_ident(raw_fname).to_string()
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
    HelperSignature {
        ret_ty,
        fname,
        params,
        ptr_params,
    }
}

pub(super) fn emit_helper_function_prototype(
    out: &mut String,
    module: &Module,
    func: &Function,
    func_globals: &BTreeMap<String, Vec<Handle<GlobalVariable>>>,
    _fh: Handle<Function>,
    _binding_map: &BindingMap,
) {
    let sig = build_helper_signature(module, func, func_globals);
    out.push_str(&format!(
        "{} {}({});\n",
        sig.ret_ty,
        sig.fname,
        sig.params.join(", ")
    ));
}

pub(super) fn emit_helper_function(
    out: &mut String,
    module: &Module,
    func: &Function,
    func_globals: &BTreeMap<String, Vec<Handle<GlobalVariable>>>,
    _fh: Handle<Function>,
    binding_map: &BindingMap,
) {
    let sig = build_helper_signature(module, func, func_globals);
    out.push_str(&format!(
        "{} {}({}) {{\n",
        sig.ret_ty,
        sig.fname,
        sig.params.join(", ")
    ));
    let body = translate_function_body(
        module,
        &func.body_src,
        &sig.ptr_params,
        func_globals,
        binding_map,
        None,
    );
    out.push_str(&body);
    out.push_str("}\n\n");
}

pub(super) fn emit_entry_point(
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
        emit_stage_in_struct(out, module, func, &stage_in_struct_name);
    }
    let (ret_ty_str, direct_out_struct_name) = emit_stage_out(out, module, ep);

    let stage_kw = match ep.stage {
        ShaderStage::Compute => "kernel",
        ShaderStage::Vertex => "vertex",
        ShaderStage::Fragment => "fragment",
    };

    let mut signature = Signature::default();
    if has_vertex_input_attrs {
        signature
            .params
            .push(format!("{stage_in_struct_name} _stage_in [[stage_in]]"));
    }
    bind_arguments(&mut signature, module, func, has_vertex_input_attrs);
    // Bind used globals in entry point signature (or declare workgroup/private globals in preamble)
    for &gh in &all_used_globals {
        bind_global(
            &mut signature,
            module,
            &module.global_variables[gh],
            binding_map,
        );
    }
    if uses_array_length(module, func) {
        signature.params.push(format!(
            "constant uint* _wgpu_buffer_sizes [[buffer({MSL_ARRAY_LENGTHS_BUFFER_SLOT})]]"
        ));
    }
    let Signature { params, preamble } = signature;

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

/// What an entry point is declared with: its parameter list, and the statements that open its
/// body to declare the globals Metal has no binding for.
#[derive(Default)]
struct Signature {
    params: Vec<String>,
    preamble: String,
}

/// The struct that carries `@location` vertex attributes into a vertex entry point.
fn emit_stage_in_struct(
    out: &mut String,
    module: &Module,
    func: &Function,
    stage_in_struct_name: &str,
) {
    out.push_str(&format!("struct {stage_in_struct_name} {{\n"));
    for (idx, arg) in func.arguments.iter().enumerate() {
        if let Some(Binding::Location { location, .. }) = arg.binding {
            let aname = msl_arg_name(arg.name.as_deref(), idx);
            let aty = msl_type_for_handle(module, arg.ty, false);
            out.push_str(&format!("    {aty} {aname} [[attribute({location})]];\n"));
        }
    }
    out.push_str("};\n\n");
}

/// The return type of an entry point. A bare value with a binding is wrapped in a struct, whose
/// name is returned as the second element.
fn emit_stage_out(out: &mut String, module: &Module, ep: &EntryPoint) -> (String, Option<String>) {
    let func = &ep.function;
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
    (ret_ty_str, direct_out_struct_name)
}

fn bind_arguments(
    signature: &mut Signature,
    module: &Module,
    func: &Function,
    has_vertex_input_attrs: bool,
) {
    let Signature { params, preamble } = signature;
    for (idx, arg) in func.arguments.iter().enumerate() {
        let aname = msl_arg_name(arg.name.as_deref(), idx);
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
}

fn bind_global(
    signature: &mut Signature,
    module: &Module,
    gv: &GlobalVariable,
    binding_map: &BindingMap,
) {
    let gname = gv.name.as_deref().unwrap_or("g");
    match gv.space {
        AddressSpace::Uniform | AddressSpace::Immediate | AddressSpace::PushConstant => {
            bind_uniform_or_immediate_global(signature, module, gv, gname);
        }
        AddressSpace::Storage { access } => {
            bind_storage_global(signature, module, gv, gname, access);
        }
        AddressSpace::Handle => bind_handle_global(signature, module, gv, binding_map, gname),
        AddressSpace::WorkGroup | AddressSpace::Private | AddressSpace::Function => {
            declare_local_global(signature, module, gv, gname);
        }
    }
}

fn bind_uniform_or_immediate_global(
    signature: &mut Signature,
    module: &Module,
    gv: &GlobalVariable,
    gname: &str,
) {
    let slot = match gv.space {
        AddressSpace::Uniform => {
            let Some(rb) = gv.binding else { return };
            msl_buffer_slot(rb.group, rb.binding)
        }
        _ => MSL_IMMEDIATE_BUFFER_SLOT,
    };
    let ty = msl_type_for_handle(module, gv.ty, false);
    signature
        .params
        .push(format!("constant {ty}& {gname} [[buffer({slot})]]"));
}

fn bind_storage_global(
    signature: &mut Signature,
    module: &Module,
    gv: &GlobalVariable,
    gname: &str,
    access: StorageAccess,
) {
    let Some(rb) = gv.binding else { return };
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
            signature
                .params
                .push(format!("{qual} {elem_ty}* {gname} [[buffer({slot})]]"));
        }
        _ => {
            let ty = msl_type_for_handle(module, gv.ty, false);
            signature
                .params
                .push(format!("{qual} {ty}& {gname} [[buffer({slot})]]"));
        }
    }
}

fn bind_handle_global(
    signature: &mut Signature,
    module: &Module,
    gv: &GlobalVariable,
    binding_map: &BindingMap,
    gname: &str,
) {
    let Some(rb) = gv.binding else { return };
    match module.types[gv.ty].inner {
        TypeInner::Image { .. } => {
            let slot = binding_map.textures[&(rb.group, rb.binding)];
            let ty = msl_type_for_handle(module, gv.ty, false);
            signature
                .params
                .push(format!("{ty} {gname} [[texture({slot})]]"));
        }
        TypeInner::Sampler { .. } => {
            let slot = binding_map.samplers[&(rb.group, rb.binding)];
            let ty = msl_type_for_handle(module, gv.ty, false);
            signature
                .params
                .push(format!("{ty} {gname} [[sampler({slot})]]"));
        }
        _ => {}
    }
}

fn declare_local_global(
    signature: &mut Signature,
    module: &Module,
    gv: &GlobalVariable,
    gname: &str,
) {
    match gv.space {
        AddressSpace::WorkGroup => match &module.types[gv.ty].inner {
            TypeInner::Array {
                base,
                size: ArraySize::Constant(n),
                ..
            } => {
                let elem_ty = msl_type_for_handle(module, *base, false);
                let count = n.get();
                signature
                    .preamble
                    .push_str(&format!("    threadgroup {elem_ty} {gname}[{count}];\n"));
            }
            _ => {
                let ty = msl_type_for_handle(module, gv.ty, false);
                signature
                    .preamble
                    .push_str(&format!("    threadgroup {ty} {gname};\n"));
            }
        },
        _ => {
            let ty = msl_type_for_handle(module, gv.ty, false);
            if let Some(init) = &gv.init {
                let init_msl = translate_expr(module, init, &BTreeSet::new(), &BTreeMap::new());
                signature
                    .preamble
                    .push_str(&format!("    {ty} {gname} = {init_msl};\n"));
            } else {
                signature
                    .preamble
                    .push_str(&format!("    {ty} {gname} = {{}};\n"));
            }
        }
    }
}

/// Whether the entry point or a function it calls asks for the length of a runtime-sized array.
fn uses_array_length(module: &Module, func: &Function) -> bool {
    func.body_src.contains("arrayLength")
        || func.called_functions.iter().any(|cf| {
            module.functions.iter().any(|(_, f)| {
                f.name.as_deref() == Some(cf.as_str()) && f.body_src.contains("arrayLength")
            })
        })
}

/// The MSL name of an entry-point argument: its escaped WGSL name, or `arg_N` when unnamed.
pub(super) fn msl_arg_name(name: Option<&str>, idx: usize) -> String {
    name.map_or_else(|| format!("arg_{idx}"), |n| msl_escape_ident(n).to_string())
}
