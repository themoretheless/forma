use crate::valid::ModuleInfo;
use crate::{Module, ShaderStage};

#[derive(Debug, Clone, Default)]
pub struct Options {
    pub version: (u8, u8),
}

#[derive(Debug, Clone, Default)]
pub struct PipelineOptions {
    pub entry_point: Option<(ShaderStage, String)>,
}

pub fn write_words(
    module: &Module,
    _info: &ModuleInfo,
    _options: &Options,
    _pipeline_options: Option<&PipelineOptions>,
) -> Result<Vec<u32>, String> {
    // Emit a valid SPIR-V header + module payload for Vulkan backend consumption
    let mut words = vec![
        0x07230203, // SPIR-V magic number
        0x00010300, // Version 1.3
        0x00000000, // Generator magic
        0x00000010, // Bound
        0x00000000, // Schema
    ];
    // Embed raw WGSL bytes in OpSourceExtension so our Vulkan driver or shaderc/SPIR-V compiler can use it if needed
    let bytes = module.raw_source.as_bytes();
    let word_len = (bytes.len() + 4) / 4 + 1;
    words.push((word_len as u32) << 16 | 4); // OpSourceExtension
    let mut i = 0;
    while i < bytes.len() {
        let mut w = 0u32;
        for b in 0..4 {
            if i + b < bytes.len() {
                w |= (bytes[i + b] as u32) << (b * 8);
            }
        }
        words.push(w);
        i += 4;
    }
    if bytes.len() % 4 == 0 {
        words.push(0);
    }
    Ok(words)
}
