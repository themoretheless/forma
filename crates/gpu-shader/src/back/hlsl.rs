//! The HLSL back end: a [`Module`] as Microsoft DirectX 12 HLSL source.

use crate::back::lower::{lower_checked_module, to_naga_stage};
use crate::valid::ModuleInfo;
use crate::{Module, ShaderStage};

/// HLSL target Shader Model supported by `gpu-shader`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShaderModel {
    V5_1,
    #[default]
    V6_0,
}

impl ShaderModel {
    const fn to_naga(self) -> naga::back::hlsl::ShaderModel {
        match self {
            Self::V5_1 => naga::back::hlsl::ShaderModel::V5_1,
            Self::V6_0 => naga::back::hlsl::ShaderModel::V6_0,
        }
    }
}

/// Configuration options for HLSL translation.
#[derive(Debug, Clone)]
pub struct Options {
    pub shader_model: ShaderModel,
    pub zero_initialize_workgroup_memory: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            shader_model: ShaderModel::default(),
            zero_initialize_workgroup_memory: true,
        }
    }
}

/// Per-pipeline selection options for HLSL emission.
#[derive(Debug, Clone, Default)]
pub struct PipelineOptions {
    pub entry_point: Option<(ShaderStage, String)>,
}

/// Emits HLSL source for a checked shader [`Module`].
pub fn write_string(
    module: &Module,
    info: &ModuleInfo,
    options: &Options,
    pipeline_options: &PipelineOptions,
) -> Result<String, String> {
    let ep_filter = pipeline_options
        .entry_point
        .as_ref()
        .map(|(stage, name)| (*stage, name.as_str()));
    let (ir, ir_info) = lower_checked_module(module, info, ep_filter)?;

    let naga_options = naga::back::hlsl::Options {
        shader_model: options.shader_model.to_naga(),
        zero_initialize_workgroup_memory: options.zero_initialize_workgroup_memory,
        fake_missing_bindings: true,
        ..Default::default()
    };
    let naga_pipeline = naga::back::hlsl::PipelineOptions {
        entry_point: pipeline_options
            .entry_point
            .as_ref()
            .map(|(stage, name)| (to_naga_stage(*stage), name.clone())),
    };

    let mut out = String::new();
    let mut writer = naga::back::hlsl::Writer::new(&mut out, &naga_options, &naga_pipeline);
    writer
        .write(&ir, &ir_info, None)
        .map_err(|err| format!("HLSL emission failed: {err}"))?;
    Ok(out)
}
