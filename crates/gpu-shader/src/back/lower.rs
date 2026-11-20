//! Validation-first lowering from `gpu-shader` WGSL sources to SSA backend IR.

use crate::valid::ModuleInfo;
use crate::{Module, ShaderStage};
use std::collections::BTreeMap;

/// Maps `gpu-shader` pipeline stages onto `naga` stages for backend emitters.
pub(crate) const fn to_naga_stage(stage: ShaderStage) -> naga::ShaderStage {
    match stage {
        ShaderStage::Vertex => naga::ShaderStage::Vertex,
        ShaderStage::Fragment => naga::ShaderStage::Fragment,
        ShaderStage::Compute => naga::ShaderStage::Compute,
    }
}

/// Validates and lowers a checked `Module` into `naga` IR with overrides evaluated.
pub(crate) fn lower_checked_module(
    module: &Module,
    _info: &ModuleInfo,
    entry_point: Option<(ShaderStage, &str)>,
) -> Result<(naga::Module, naga::valid::ModuleInfo), String> {
    if module.raw_source.trim().is_empty() {
        return Err("WGSL module has no source text for backend lowering".to_string());
    }
    let ir = naga::front::wgsl::parse_str(&module.raw_source).map_err(|err| err.to_string())?;
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    let ir_info = validator
        .validate(&ir)
        .map_err(|err| format!("shader validation failed: {err}"))?;

    let ep_filter = entry_point.map(|(stage, name)| (to_naga_stage(stage), name));
    let (resolved, resolved_info) = naga::back::pipeline_constants::process_overrides(
        &ir,
        &ir_info,
        ep_filter,
        &Default::default(),
    )
    .map_err(|err| format!("pipeline override evaluation failed: {err}"))?;
    Ok((resolved.into_owned(), resolved_info.into_owned()))
}

/// Parses and validates WGSL via `gpu-shader` first, keeping local diagnostics authoritative.
pub(crate) fn parse_and_validate(wgsl_source: &str) -> Result<(Module, ModuleInfo), String> {
    let module = crate::front::wgsl::parse_str(wgsl_source).map_err(|err| err.to_string())?;
    let mut validator = crate::valid::Validator::new(
        crate::valid::ValidationFlags::all(),
        crate::valid::Capabilities::all(),
    );
    let info = validator.validate(&module).map_err(|err| err.to_string())?;
    Ok((module, info))
}

/// Assigns deterministic `u8` binding indices to distinct resource bindings for GLSL targets.
pub(crate) fn sequential_binding_slots(
    ir: &naga::Module,
) -> Result<BTreeMap<(u32, u32), u8>, String> {
    let mut bindings = Vec::new();
    for (_, global) in ir.global_variables.iter() {
        if let Some(binding) = global.binding {
            bindings.push((binding.group, binding.binding));
        }
    }
    bindings.sort_unstable();
    bindings.dedup();
    if bindings.len() > usize::from(u8::MAX) {
        return Err(format!(
            "GLSL target supports at most {} bindings, shader uses {}",
            u8::MAX,
            bindings.len()
        ));
    }
    Ok(bindings
        .into_iter()
        .enumerate()
        .map(|(slot, key)| (key, slot as u8))
        .collect())
}
