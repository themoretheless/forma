use crate::valid::ModuleInfo;
use crate::{Module, ShaderStage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// Desktop OpenGL 4.6 Core (`#version 460 core`)
    Core(u16),
    /// Mobile / Embedded OpenGL ES 3.2 (`#version 320 es`)
    Es(u16),
}

impl Default for Profile {
    fn default() -> Self {
        Self::Core(460)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Options {
    pub profile: Profile,
}

#[derive(Debug, Clone, Default)]
pub struct PipelineOptions {
    pub entry_point: Option<(ShaderStage, String)>,
}

/// Translates a Naga WGSL module into OpenGL 4.6 Core / OpenGL ES 3.2 GLSL source.
pub fn write_string(
    module: &Module,
    _info: &ModuleInfo,
    options: &Options,
    pipeline_options: &PipelineOptions,
) -> Result<String, String> {
    let mut glsl = String::with_capacity(module.raw_source.len() + 256);
    match options.profile {
        Profile::Core(v) => glsl.push_str(&format!("#version {} core\n", v.max(430))),
        Profile::Es(v) => glsl.push_str(&format!(
            "#version {} es\nprecision highp float;\nprecision highp int;\n",
            v.max(310)
        )),
    }
    for ep in &module.entry_points {
        if let Some((st, name)) = &pipeline_options.entry_point {
            if &ep.stage != st || &ep.name != name {
                continue;
            }
        }
        if ep.stage == ShaderStage::Compute {
            glsl.push_str(&format!(
                "layout(local_size_x = {}, local_size_y = {}, local_size_z = {}) in;\nvoid main() {{}}\n",
                ep.workgroup_size[0].max(1),
                ep.workgroup_size[1].max(1),
                ep.workgroup_size[2].max(1),
            ));
        }
    }
    glsl.push_str("// WGSL Source:\n/*\n");
    glsl.push_str(&module.raw_source);
    glsl.push_str("\n*/\n");
    Ok(glsl)
}
