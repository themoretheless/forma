//! The SPIR-V back end: a [`Module`] as validated Vulkan SPIR-V words and assembly.

use crate::back::lower::{lower_checked_module, to_naga_stage};
use crate::valid::ModuleInfo;
use crate::{Module, ShaderStage};
use std::fmt::Write as _;

/// Configuration options for SPIR-V translation.
#[derive(Debug, Clone)]
pub struct Options {
    pub lang_version: (u8, u8),
    pub debug: bool,
    pub zero_initialize_workgroup_memory: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            lang_version: (1, 3),
            debug: false,
            zero_initialize_workgroup_memory: true,
        }
    }
}

/// Per-pipeline selection options for SPIR-V emission.
#[derive(Debug, Clone, Default)]
pub struct PipelineOptions {
    pub entry_point: Option<(ShaderStage, String)>,
}

/// Summary extracted by structurally validating a binary SPIR-V word stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpirvHeaderInfo {
    pub version_major: u8,
    pub version_minor: u8,
    pub generator: u32,
    pub bound: u32,
    pub instruction_count: usize,
    pub entry_points: Vec<String>,
}

/// Emits validated SPIR-V words for a checked shader [`Module`].
pub fn write_vec(
    module: &Module,
    info: &ModuleInfo,
    options: &Options,
    pipeline_options: &PipelineOptions,
) -> Result<Vec<u32>, String> {
    let ep_filter = pipeline_options
        .entry_point
        .as_ref()
        .map(|(stage, name)| (*stage, name.as_str()));
    let (ir, ir_info) = lower_checked_module(module, info, ep_filter)?;

    let mut flags = naga::back::spv::WriterFlags::ADJUST_COORDINATE_SPACE
        | naga::back::spv::WriterFlags::LABEL_VARYINGS
        | naga::back::spv::WriterFlags::CLAMP_FRAG_DEPTH;
    if options.debug {
        flags |= naga::back::spv::WriterFlags::DEBUG;
    }

    let naga_options = naga::back::spv::Options {
        lang_version: options.lang_version,
        flags,
        fake_missing_bindings: true,
        zero_initialize_workgroup_memory: if options.zero_initialize_workgroup_memory {
            naga::back::spv::ZeroInitializeWorkgroupMemoryMode::Polyfill
        } else {
            naga::back::spv::ZeroInitializeWorkgroupMemoryMode::None
        },
        ..Default::default()
    };

    let naga_pipeline = pipeline_options.entry_point.as_ref().map(|(stage, name)| {
        naga::back::spv::PipelineOptions {
            shader_stage: to_naga_stage(*stage),
            entry_point: name.clone(),
        }
    });

    let words = naga::back::spv::write_vec(&ir, &ir_info, &naga_options, naga_pipeline.as_ref())
        .map_err(|err| format!("SPIR-V emission failed: {err}"))?;
    let _ = validate_spirv_words(&words)?;
    Ok(words)
}

/// Decodes a SPIR-V little-endian NUL-terminated UTF-8 literal string starting at `operands`.
fn decode_literal_string(operands: &[u32]) -> Result<String, String> {
    let mut bytes = Vec::with_capacity(operands.len() * 4);
    for &word in operands {
        for byte in word.to_le_bytes() {
            if byte == 0 {
                return String::from_utf8(bytes)
                    .map_err(|err| format!("invalid UTF-8 literal string in SPIR-V: {err}"));
            }
            bytes.push(byte);
        }
    }
    Err("unterminated literal string in SPIR-V instruction".to_string())
}

/// Validates the header, instruction stream framing, opcodes, and function balance of SPIR-V words.
pub fn validate_spirv_words(words: &[u32]) -> Result<SpirvHeaderInfo, String> {
    if words.len() < 5 {
        return Err(format!(
            "SPIR-V module truncated: expected at least 5 header words, got {}",
            words.len()
        ));
    }
    if words[0] != spirv::MAGIC_NUMBER {
        return Err(format!(
            "invalid SPIR-V magic number: expected 0x{:08x}, got 0x{:08x}",
            spirv::MAGIC_NUMBER,
            words[0]
        ));
    }
    let version_major = ((words[1] >> 16) & 0xff) as u8;
    let version_minor = ((words[1] >> 8) & 0xff) as u8;
    let generator = words[2];
    let bound = words[3];
    if bound == 0 {
        return Err("invalid SPIR-V header: id bound must be non-zero".to_string());
    }
    if words[4] != 0 {
        return Err(format!(
            "invalid SPIR-V header: reserved schema word must be 0, got {}",
            words[4]
        ));
    }

    let mut cursor = 5usize;
    let mut instruction_count = 0usize;
    let mut capability_count = 0usize;
    let mut memory_model_count = 0usize;
    let mut entry_points = Vec::new();
    let mut open_functions = 0usize;

    while cursor < words.len() {
        let head = words[cursor];
        let word_count = (head >> 16) as usize;
        let opcode_raw = u32::from(head as u16);
        if word_count == 0 {
            return Err(format!(
                "invalid SPIR-V instruction at word {cursor}: word count cannot be 0"
            ));
        }
        let next_cursor = cursor
            .checked_add(word_count)
            .ok_or_else(|| "SPIR-V instruction offset overflow".to_string())?;
        if next_cursor > words.len() {
            return Err(format!(
                "truncated SPIR-V instruction at word {cursor}: needs {word_count} words, {} remaining",
                words.len() - cursor
            ));
        }
        let op = spirv::Op::from_u32(opcode_raw)
            .ok_or_else(|| format!("unknown SPIR-V opcode {opcode_raw} at word {cursor}"))?;
        let operands = &words[cursor + 1..next_cursor];
        match op {
            spirv::Op::Capability => capability_count += 1,
            spirv::Op::MemoryModel => memory_model_count += 1,
            spirv::Op::EntryPoint => {
                if operands.len() < 3 {
                    return Err("malformed SPIR-V OpEntryPoint instruction".to_string());
                }
                entry_points.push(decode_literal_string(&operands[2..])?);
            }
            spirv::Op::Function => {
                if open_functions != 0 {
                    return Err("nested SPIR-V OpFunction is forbidden".to_string());
                }
                open_functions = 1;
            }
            spirv::Op::FunctionEnd => {
                if open_functions == 0 {
                    return Err("unmatched SPIR-V OpFunctionEnd".to_string());
                }
                open_functions = 0;
            }
            _ => {}
        }
        instruction_count += 1;
        cursor = next_cursor;
    }

    if capability_count == 0 {
        return Err("invalid SPIR-V module: missing OpCapability".to_string());
    }
    if memory_model_count == 0 {
        return Err("invalid SPIR-V module: missing OpMemoryModel".to_string());
    }
    if entry_points.is_empty() {
        return Err("invalid SPIR-V module: missing OpEntryPoint".to_string());
    }
    if open_functions != 0 {
        return Err("invalid SPIR-V module: unclosed OpFunction body".to_string());
    }

    Ok(SpirvHeaderInfo {
        version_major,
        version_minor,
        generator,
        bound,
        instruction_count,
        entry_points,
    })
}

/// Validates SPIR-V words and formats human-readable SPIR-V disassembly decoded from the binary.
pub fn disassemble_words(words: &[u32]) -> Result<String, String> {
    let header = validate_spirv_words(words)?;
    let mut out = String::new();
    let _ = writeln!(out, "; SPIR-V");
    let _ = writeln!(
        out,
        "; Version: {}.{}",
        header.version_major, header.version_minor
    );
    let _ = writeln!(out, "; Generator: 0x{:08x}", header.generator);
    let _ = writeln!(out, "; Bound: {}", header.bound);
    let _ = writeln!(out, "; Schema: 0");

    let mut cursor = 5usize;
    while cursor < words.len() {
        let head = words[cursor];
        let word_count = (head >> 16) as usize;
        let op = spirv::Op::from_u32(u32::from(head as u16))
            .ok_or_else(|| "unknown SPIR-V opcode during disassembly".to_string())?;
        let operands = &words[cursor + 1..cursor + word_count];

        let _ = write!(out, "Op{op:?}");
        if op == spirv::Op::EntryPoint && operands.len() >= 3 {
            let model = spirv::ExecutionModel::from_u32(operands[0]);
            let name = decode_literal_string(&operands[2..])?;
            let _ = write!(out, " {model:?} %{} {:?}", operands[1], name);
            // Count words consumed by the string literal so trailing interface ids stay visible.
            let string_words = (name.len() + 1).div_ceil(4);
            for &iface_id in operands.get(2 + string_words..).unwrap_or(&[]) {
                let _ = write!(out, " %{iface_id}");
            }
        } else {
            for &operand in operands {
                let _ = write!(out, " 0x{operand:08x}");
            }
        }
        out.push('\n');
        cursor += word_count;
    }
    Ok(out)
}
