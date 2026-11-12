//! Pipeline-overridable constants.

/// Preprocessor macro holding the value of WGSL `override NAME`. A runtime applies pipeline
/// constants by defining it ahead of the generated source; see [`with_pipeline_constants`].
pub fn override_macro(name: &str) -> String {
    format!("_WGPU_OVERRIDE_{name}")
}

/// Why a pipeline constant cannot be applied to a module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineConstantError {
    /// The module declares no `override` with this name.
    Unknown(String),
    /// The value is NaN or infinite.
    NonFinite(String),
}

impl std::fmt::Display for PipelineConstantError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown(name) => write!(f, "the shader declares no override named `{name}`"),
            Self::NonFinite(name) => write!(f, "the value of override `{name}` is not finite"),
        }
    }
}

impl std::error::Error for PipelineConstantError {}

/// Prefixes `msl` with definitions for the given pipeline-overridable constants.
///
/// As in WebGPU, a constant must name an `override` the module declares and carry a finite
/// value; anything else would otherwise surface as a Metal compile error or be ignored.
///
/// # Errors
/// Returns the first constant that is undeclared or not finite.
pub fn with_pipeline_constants(
    msl: &str,
    constants: &[(&str, f64)],
) -> Result<String, PipelineConstantError> {
    use std::fmt::Write;
    let mut out = String::with_capacity(msl.len() + constants.len() * 48);
    for (name, value) in constants {
        let mac = override_macro(name);
        // The declaration emitted for `override NAME`; the newline keeps a prefix from matching.
        if !msl.contains(&format!("#ifndef {mac}\n")) {
            return Err(PipelineConstantError::Unknown((*name).to_owned()));
        }
        if !value.is_finite() {
            return Err(PipelineConstantError::NonFinite((*name).to_owned()));
        }
        // `{:?}` keeps a decimal point so the literal is a float and casts to the declared type.
        let _ = writeln!(out, "#define {mac} {value:?}");
    }
    out.push_str(msl);
    Ok(out)
}
