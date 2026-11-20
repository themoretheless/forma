//! The GLSL back end: a [`Module`] as OpenGL / OpenGL ES GLSL source.

use crate::back::lower::{lower_checked_module, sequential_binding_slots, to_naga_stage};
use crate::valid::ModuleInfo;
use crate::{Module, ShaderStage};

/// GLSL language profile and version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    Desktop(u16),
    Embedded { version: u16, is_webgl: bool },
}

impl Default for Version {
    fn default() -> Self {
        Self::Embedded {
            version: 310,
            is_webgl: false,
        }
    }
}

impl Version {
    const fn to_naga(self) -> naga::back::glsl::Version {
        match self {
            Self::Desktop(v) => naga::back::glsl::Version::Desktop(v),
            Self::Embedded { version, is_webgl } => {
                naga::back::glsl::Version::Embedded { version, is_webgl }
            }
        }
    }
}

/// Configuration options for GLSL translation.
#[derive(Debug, Clone)]
pub struct Options {
    pub version: Version,
    pub zero_initialize_workgroup_memory: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            version: Version::default(),
            zero_initialize_workgroup_memory: true,
        }
    }
}

/// Per-stage selection options for GLSL emission.
#[derive(Debug, Clone)]
pub struct PipelineOptions {
    pub shader_stage: ShaderStage,
    pub entry_point: String,
}

/// Emits GLSL source for a single `(stage, entry_point)` of a checked shader [`Module`].
pub fn write_string(
    module: &Module,
    info: &ModuleInfo,
    options: &Options,
    pipeline_options: &PipelineOptions,
) -> Result<String, String> {
    let ep = (
        pipeline_options.shader_stage,
        pipeline_options.entry_point.as_str(),
    );
    let (ir, ir_info) = lower_checked_module(module, info, Some(ep))?;
    emit_naga_entry_point(&ir, &ir_info, options, ep.0, ep.1)
}

/// Emits GLSL source for every entry point in a checked shader [`Module`].
pub fn write_all_entry_points(
    module: &Module,
    info: &ModuleInfo,
    options: &Options,
) -> Result<String, String> {
    if module.entry_points.is_empty() {
        return Err("GLSL emission requires at least one entry point".to_string());
    }
    let mut parts = Vec::with_capacity(module.entry_points.len());
    for ep in &module.entry_points {
        let (ir, ir_info) = lower_checked_module(module, info, Some((ep.stage, &ep.name)))?;
        let glsl = emit_naga_entry_point(&ir, &ir_info, options, ep.stage, &ep.name)?;
        if module.entry_points.len() > 1 {
            parts.push(format!(
                "// entry point: {} ({})\n{glsl}",
                ep.name, ep.stage
            ));
        } else {
            parts.push(glsl);
        }
    }
    Ok(parts.join("\n"))
}

fn emit_naga_entry_point(
    ir: &naga::Module,
    ir_info: &naga::valid::ModuleInfo,
    options: &Options,
    stage: ShaderStage,
    entry_point: &str,
) -> Result<String, String> {
    let slots = sequential_binding_slots(ir)?;
    let mut binding_map = naga::back::glsl::BindingMap::default();
    for ((group, binding), slot) in slots {
        binding_map.insert(naga::ResourceBinding { group, binding }, slot);
    }

    let naga_options = naga::back::glsl::Options {
        version: options.version.to_naga(),
        writer_flags: naga::back::glsl::WriterFlags::ADJUST_COORDINATE_SPACE,
        binding_map,
        zero_initialize_workgroup_memory: options.zero_initialize_workgroup_memory,
    };
    let naga_pipeline = naga::back::glsl::PipelineOptions {
        shader_stage: to_naga_stage(stage),
        entry_point: entry_point.to_string(),
        multiview: None,
    };
    let mut out = String::new();
    let mut writer = naga::back::glsl::Writer::new(
        &mut out,
        ir,
        ir_info,
        &naga_options,
        &naga_pipeline,
        naga::proc::BoundsCheckPolicies::default(),
    )
    .map_err(|err| format!("GLSL writer init failed: {err}"))?;
    writer
        .write()
        .map_err(|err| format!("GLSL emission failed: {err}"))?;
    Ok(out)
}
