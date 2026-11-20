//! WGSL module validator: type checking, mutability, built-ins, and stage contracts.

use crate::valid::builtins::err;
use crate::valid::expr::check_value_expr;
use crate::valid::stmt::check_block;
use crate::valid::types::{ElemScalar, ValType};
use crate::{
    AddressSpace, Binding, BuiltIn, GlobalVariable, Handle, Module, ShaderStage, StorageAccess,
    Type, TypeInner, VectorSize,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::ops::Index;

mod builtins;
mod expr;
mod stmt;
mod types;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValidationFlags(u32);

impl ValidationFlags {
    pub const fn all() -> Self {
        Self(0xFFFF_FFFF)
    }

    pub const fn empty() -> Self {
        Self(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities(u32);

impl Capabilities {
    pub const fn all() -> Self {
        Self(0xFFFF_FFFF)
    }

    pub const fn empty() -> Self {
        Self(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlobalUse(u8);

impl GlobalUse {
    pub const EMPTY: Self = Self(0);
    pub const READ: Self = Self(1);
    pub const WRITE: Self = Self(2);
    pub const READ_WRITE: Self = Self(3);

    #[inline]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

#[derive(Debug, Clone)]
pub struct FunctionInfo {
    globals: Vec<GlobalUse>,
}

impl FunctionInfo {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.globals.iter().all(|g| g.is_empty())
    }
}

impl Index<Handle<GlobalVariable>> for FunctionInfo {
    type Output = GlobalUse;

    #[inline]
    fn index(&self, handle: Handle<GlobalVariable>) -> &Self::Output {
        self.globals
            .get(handle.index())
            .unwrap_or(&GlobalUse::EMPTY)
    }
}

#[derive(Debug, Clone)]
pub struct ModuleInfo {
    functions: Vec<FunctionInfo>,
    entry_points: Vec<FunctionInfo>,
}

impl ModuleInfo {
    #[inline]
    pub fn get_entry_point(&self, index: usize) -> &FunctionInfo {
        &self.entry_points[index]
    }

    #[inline]
    pub fn get_function(&self, index: usize) -> &FunctionInfo {
        &self.functions[index]
    }
}

#[derive(Debug, Clone)]
pub struct ValidationError {
    pub message: String,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ValidationError {}

pub struct Validator {
    flags: ValidationFlags,
    _capabilities: Capabilities,
}

impl Validator {
    pub fn new(flags: ValidationFlags, capabilities: Capabilities) -> Self {
        Self {
            flags,
            _capabilities: capabilities,
        }
    }

    pub fn validate(&mut self, module: &Module) -> Result<ModuleInfo, ValidationError> {
        if self.flags != ValidationFlags::empty() {
            validate_module_semantics(module)?;
        }

        let num_globals = module.global_variables.len();
        let functions = module
            .functions
            .iter()
            .map(|(_, func)| build_function_info(module, func, num_globals))
            .collect();
        let entry_points = module
            .entry_points
            .iter()
            .map(|ep| build_function_info(module, &ep.function, num_globals))
            .collect();

        Ok(ModuleInfo {
            functions,
            entry_points,
        })
    }
}

fn build_function_info(
    module: &Module,
    func: &crate::Function,
    num_globals: usize,
) -> FunctionInfo {
    let mut visited = BTreeSet::new();
    let used = collect_transitive_globals(module, func, &mut visited);
    let mut globals = vec![GlobalUse::EMPTY; num_globals];
    for g in used {
        if g.index() < globals.len() {
            globals[g.index()] = GlobalUse::READ_WRITE;
        }
    }
    FunctionInfo { globals }
}

fn collect_transitive_globals(
    module: &Module,
    func: &crate::Function,
    visited_funcs: &mut BTreeSet<String>,
) -> BTreeSet<Handle<GlobalVariable>> {
    let mut used = func.used_globals.clone();
    for callee_name in &func.called_functions {
        if visited_funcs.insert(callee_name.clone()) {
            for (_, f) in module.functions.iter() {
                if f.name.as_deref() == Some(callee_name.as_str()) {
                    let sub = collect_transitive_globals(module, f, visited_funcs);
                    used.extend(sub);
                }
            }
        }
    }
    used
}

struct GlobalSig {
    ty: ValType,
    space: AddressSpace,
    access: StorageAccess,
    mutable: bool,
}

struct FnSig {
    params: Vec<ValType>,
    ret: Option<ValType>,
}

struct ModuleEnv<'a> {
    module: &'a Module,
    structs: BTreeMap<String, Handle<Type>>,
    const_values: BTreeMap<String, u32>,
    constants: BTreeMap<String, ValType>,
    globals: BTreeMap<String, GlobalSig>,
    functions: BTreeMap<String, FnSig>,
}

struct LocalVar {
    ty: ValType,
    mutable: bool,
    space: AddressSpace,
}

#[derive(Default)]
struct ScopeStack {
    frames: Vec<BTreeMap<String, LocalVar>>,
}

impl ScopeStack {
    fn push_scope(&mut self) {
        self.frames.push(BTreeMap::new());
    }

    fn pop_scope(&mut self) {
        self.frames.pop();
    }

    fn insert(&mut self, name: String, var: LocalVar) {
        if let Some(frame) = self.frames.last_mut() {
            frame.insert(name, var);
        }
    }

    fn lookup(&self, name: &str) -> Option<&LocalVar> {
        self.frames.iter().rev().find_map(|f| f.get(name))
    }
}

fn validate_module_semantics(module: &Module) -> Result<(), ValidationError> {
    let mut env = ModuleEnv {
        module,
        structs: build_struct_map(module),
        const_values: BTreeMap::new(),
        constants: BTreeMap::new(),
        globals: BTreeMap::new(),
        functions: BTreeMap::new(),
    };

    register_and_check_constants(&mut env)?;
    register_and_check_globals(&mut env)?;
    register_functions(&mut env)?;

    for (_, func) in module.functions.iter() {
        validate_function_body(&env, func)?;
    }
    for ep in &module.entry_points {
        validate_entry_point(&env, ep)?;
    }
    Ok(())
}

fn build_struct_map(module: &Module) -> BTreeMap<String, Handle<Type>> {
    let mut map = BTreeMap::new();
    for (handle, ty) in module.types.iter() {
        if let Some(name) = &ty.name {
            if matches!(ty.inner, TypeInner::Struct { .. }) {
                map.insert(name.clone(), handle);
            }
        }
    }
    map
}

fn register_and_check_constants(env: &mut ModuleEnv<'_>) -> Result<(), ValidationError> {
    let scopes = ScopeStack::default();
    for (_, c) in env.module.constants.iter() {
        let Some(name) = c.name.as_deref() else {
            continue;
        };
        if let Some(val) = c.u32_value {
            env.const_values.insert(name.to_string(), val);
        }
        let init = check_value_expr(env, &scopes, &c.expr_src)
            .map_err(|e| err(format!("in constant `{name}`: {}", e.message)))?;
        let final_ty = if let Some(h) = c.ty {
            let declared = ValType::from_ir(env.module, h);
            if !init.ty.coerces_to(&declared) {
                return Err(err(format!(
                    "constant `{name}` expected `{}`, got `{}`",
                    declared.describe(env.module),
                    init.ty.describe(env.module)
                )));
            }
            declared
        } else if c.is_override {
            init.ty.concretize()
        } else {
            init.ty
        };
        env.constants.insert(name.to_string(), final_ty);
    }
    Ok(())
}

fn register_and_check_globals(env: &mut ModuleEnv<'_>) -> Result<(), ValidationError> {
    let scopes = ScopeStack::default();
    for (_, gv) in env.module.global_variables.iter() {
        let ty = ValType::from_ir(env.module, gv.ty);
        let gname = gv.name.as_deref().unwrap_or("<global>");

        if matches!(gv.space, AddressSpace::Uniform)
            && !ty.is_host_shareable_constructible(env.module)
        {
            return Err(err(format!(
                "uniform variable `{gname}` has non-constructible type `{}`",
                ty.describe(env.module)
            )));
        }
        if ty.contains_dynamic_array(env.module)
            && !matches!(gv.space, AddressSpace::Storage { .. })
        {
            return Err(err(format!(
                "variable `{gname}` with runtime-sized array must use `storage` address space"
            )));
        }
        if let Some(init_src) = &gv.init {
            if !matches!(gv.space, AddressSpace::Private | AddressSpace::Function) {
                return Err(err(format!(
                    "global `{gname}` in `{:?}` address space cannot have an initializer",
                    gv.space
                )));
            }
            let init = check_value_expr(env, &scopes, init_src)
                .map_err(|e| err(format!("global `{gname}` initializer: {}", e.message)))?;
            if !init.ty.coerces_to(&ty) {
                return Err(err(format!(
                    "global `{gname}` initializer `{}` does not match `{}`",
                    init.ty.describe(env.module),
                    ty.describe(env.module)
                )));
            }
        }

        let (access, mutable) = match gv.space {
            AddressSpace::Storage { access } => (access, access != StorageAccess::Read),
            AddressSpace::Private | AddressSpace::Function | AddressSpace::WorkGroup => {
                (StorageAccess::ReadWrite, true)
            }
            AddressSpace::Uniform
            | AddressSpace::Handle
            | AddressSpace::Immediate
            | AddressSpace::PushConstant => (StorageAccess::Read, false),
        };
        if let Some(name) = &gv.name {
            env.globals.insert(
                name.clone(),
                GlobalSig {
                    ty,
                    space: gv.space,
                    access,
                    mutable,
                },
            );
        }
    }
    Ok(())
}

fn register_functions(env: &mut ModuleEnv<'_>) -> Result<(), ValidationError> {
    for (_, func) in env.module.functions.iter() {
        let Some(name) = &func.name else { continue };
        let params = func
            .arguments
            .iter()
            .map(|a| ValType::from_ir(env.module, a.ty))
            .collect();
        let ret = func
            .result
            .as_ref()
            .map(|r| ValType::from_ir(env.module, r.ty));
        env.functions.insert(name.clone(), FnSig { params, ret });
    }
    Ok(())
}

fn validate_entry_point(
    env: &ModuleEnv<'_>,
    ep: &crate::EntryPoint,
) -> Result<(), ValidationError> {
    if ep.stage == ShaderStage::Compute {
        if ep.function.result.is_some() {
            return Err(err(format!(
                "compute entry point `{}` cannot return a value",
                ep.name
            )));
        }
        if ep.workgroup_size.contains(&0) {
            return Err(err(format!(
                "compute entry point `{}` workgroup_size dimensions must be >= 1",
                ep.name
            )));
        }
    }

    for arg in &ep.function.arguments {
        validate_binding_for_stage(env.module, arg.ty, arg.binding, ep.stage, false)?;
    }

    if ep.stage == ShaderStage::Vertex {
        let has_pos = entry_point_outputs_position(env.module, &ep.function)?;
        if !has_pos {
            return Err(err(format!(
                "vertex entry point `{}` must return `@builtin(position) vec4<f32>`",
                ep.name
            )));
        }
    }
    if let Some(res) = &ep.function.result {
        validate_binding_for_stage(env.module, res.ty, res.binding, ep.stage, true)?;
    }

    validate_function_body(env, &ep.function)
}

fn validate_binding_for_stage(
    module: &Module,
    ty_handle: Handle<Type>,
    binding: Option<Binding>,
    stage: ShaderStage,
    is_output: bool,
) -> Result<(), ValidationError> {
    if let Some(Binding::BuiltIn(b)) = binding {
        validate_builtin_io(
            module,
            b,
            &ValType::from_ir(module, ty_handle),
            stage,
            is_output,
        )?;
    }
    if let TypeInner::Struct { members, .. } = &module.types[ty_handle].inner {
        for m in members {
            if let Some(Binding::BuiltIn(b)) = m.binding {
                validate_builtin_io(module, b, &ValType::from_ir(module, m.ty), stage, is_output)?;
            }
        }
    }
    Ok(())
}

fn entry_point_outputs_position(
    module: &Module,
    func: &crate::Function,
) -> Result<bool, ValidationError> {
    let Some(res) = &func.result else {
        return Ok(false);
    };
    if let Some(Binding::BuiltIn(b)) = res.binding {
        return Ok(b == BuiltIn::Position);
    }
    if let TypeInner::Struct { members, .. } = &module.types[res.ty].inner {
        return Ok(members
            .iter()
            .any(|m| matches!(m.binding, Some(Binding::BuiltIn(BuiltIn::Position)))));
    }
    Ok(false)
}

fn validate_builtin_io(
    module: &Module,
    builtin: BuiltIn,
    ty: &ValType,
    stage: ShaderStage,
    is_output: bool,
) -> Result<(), ValidationError> {
    let stage_ok = matches!(
        (builtin, stage, is_output),
        (
            BuiltIn::VertexIndex | BuiltIn::InstanceIndex,
            ShaderStage::Vertex,
            false
        ) | (BuiltIn::Position, ShaderStage::Vertex, true)
            | (
                BuiltIn::Position | BuiltIn::FrontFacing,
                ShaderStage::Fragment,
                false
            )
            | (BuiltIn::FragDepth, ShaderStage::Fragment, true)
            | (
                BuiltIn::GlobalInvocationId
                    | BuiltIn::LocalInvocationId
                    | BuiltIn::LocalInvocationIndex
                    | BuiltIn::WorkGroupId
                    | BuiltIn::NumWorkGroups,
                ShaderStage::Compute,
                false,
            )
    );
    if !stage_ok {
        let dir = if is_output { "output" } else { "input" };
        return Err(err(format!(
            "@builtin({builtin:?}) is not valid on {stage} {dir}"
        )));
    }
    let expected = match builtin {
        BuiltIn::Position => ValType::Vector(VectorSize::Quad, ElemScalar::F32),
        BuiltIn::VertexIndex | BuiltIn::InstanceIndex | BuiltIn::LocalInvocationIndex => {
            ValType::Scalar(ElemScalar::U32)
        }
        BuiltIn::FrontFacing => ValType::Scalar(ElemScalar::Bool),
        BuiltIn::FragDepth => ValType::Scalar(ElemScalar::F32),
        BuiltIn::GlobalInvocationId
        | BuiltIn::LocalInvocationId
        | BuiltIn::WorkGroupId
        | BuiltIn::NumWorkGroups => ValType::Vector(VectorSize::Tri, ElemScalar::U32),
    };
    if *ty != expected {
        return Err(err(format!(
            "@builtin({builtin:?}) must have type `{}`, got `{}`",
            expected.describe(module),
            ty.describe(module)
        )));
    }
    Ok(())
}

fn validate_function_body(
    env: &ModuleEnv<'_>,
    func: &crate::Function,
) -> Result<(), ValidationError> {
    let fn_name = func.name.as_deref().unwrap_or("<anonymous>");
    let mut scopes = ScopeStack::default();
    scopes.push_scope();
    for arg in &func.arguments {
        if let Some(name) = &arg.name {
            scopes.insert(
                name.clone(),
                LocalVar {
                    ty: ValType::from_ir(env.module, arg.ty),
                    mutable: false,
                    space: AddressSpace::Function,
                },
            );
        }
    }
    let expected_ret = func
        .result
        .as_ref()
        .map(|r| ValType::from_ir(env.module, r.ty));
    let returns = check_block(env, &mut scopes, &func.body_src, expected_ret.as_ref())
        .map_err(|e| err(format!("in function `{fn_name}`: {}", e.message)))?;
    scopes.pop_scope();

    if expected_ret.is_some() && !returns {
        return Err(err(format!(
            "function `{fn_name}` does not return a value on all control-flow paths"
        )));
    }
    Ok(())
}
