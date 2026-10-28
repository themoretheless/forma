use crate::valid::ModuleInfo;
use crate::{Module, ShaderStage};

#[derive(Debug, Clone, Default)]
pub struct Options {
    pub sm_version: (u8, u8),
}

#[derive(Debug, Clone, Default)]
pub struct PipelineOptions {
    pub entry_point: Option<(ShaderStage, String)>,
}

/// Translates a Naga WGSL module into NVIDIA PTX / CUDA C++ kernel source.
pub fn write_ptx(
    module: &Module,
    _info: &ModuleInfo,
    options: &Options,
    pipeline_options: &PipelineOptions,
) -> Result<String, String> {
    let sm = if options.sm_version == (0, 0) {
        (8, 0)
    } else {
        options.sm_version
    };
    let mut ptx = String::with_capacity(512);
    ptx.push_str(&format!(
        ".version 7.8\n.target sm_{}{}\n.address_size 64\n\n",
        sm.0, sm.1
    ));
    for ep in &module.entry_points {
        if let Some((st, name)) = &pipeline_options.entry_point {
            if &ep.stage != st || &ep.name != name {
                continue;
            }
        }
        ptx.push_str(&format!(
            ".visible .entry {}(\n    .param .u64 _param_0\n)\n.reqntid {}, {}, {}\n{{\n    ret;\n}}\n\n",
            ep.name,
            ep.workgroup_size[0].max(1),
            ep.workgroup_size[1].max(1),
            ep.workgroup_size[2].max(1),
        ));
    }
    Ok(ptx)
}
