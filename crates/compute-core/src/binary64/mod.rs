//! Software IEEE-754 binary64 arithmetic on integer-only WGSL devices.
//!
//! `f64` does not exist in baseline WGSL, so `GpuF64Tensor` represents values
//! as `vec2<u32>` word pairs in `(low, high)` little-endian order and performs
//! all rounding, reduction and polynomial evaluation through 32-bit unsigned
//! operations and explicit rounding to nearest, ties to even.

/// Pure-integer IEEE-754 binary64 base arithmetic (`add`, `sub`, `mul`, `eq`, `lt`).
pub const ARITHMETIC_BASE_WGSL: &str = include_str!("arithmetic_base.wgsl");

/// Full integer-only IEEE-754 binary64 arithmetic including `div` and `sqrt`.
pub const ARITHMETIC_WGSL: &str = concat!(
    include_str!("arithmetic_base.wgsl"),
    "\n",
    include_str!("arithmetic.wgsl"),
);

const CONSTANTS_WGSL: &str = include_str!("constants.wgsl");
const TRANSCENDENTAL_BODY_WGSL: &str = include_str!("transcendental.wgsl");
const ARITHMETIC_EXT_WGSL: &str = include_str!("arithmetic.wgsl");

/// Binary64 polynomial functions and full-exponent-range trigonometric reduction.
/// Transcendentals are approximate; they do not promise correct rounding.
pub const TRANSCENDENTAL_WGSL: &str = concat!(
    include_str!("constants.wgsl"),
    "\n",
    include_str!("transcendental.wgsl"),
);

fn wgsl_section<'a>(source: &'a str, marker: &str) -> &'a str {
    let tag = format!("// --- {marker} ---");
    let start = source
        .find(&tag)
        .unwrap_or_else(|| panic!("missing WGSL section {marker}"));
    let tail = &source[start + tag.len()..];
    let end = tail.find("\n// --- ").unwrap_or(tail.len());
    tail[..end].trim()
}

/// Binary64 base arithmetic plus `d64_div`.
pub fn div_wgsl() -> String {
    format!(
        "{ARITHMETIC_BASE_WGSL}\n{}",
        wgsl_section(ARITHMETIC_EXT_WGSL, "DIV"),
    )
}

/// Binary64 base arithmetic plus `d64_sqrt`.
pub fn sqrt_wgsl() -> String {
    format!(
        "{ARITHMETIC_BASE_WGSL}\n{}",
        wgsl_section(ARITHMETIC_EXT_WGSL, "SQRT"),
    )
}

/// Self-contained binary64 `d64_exp` preamble (no unused transcendental or `d64_sqrt` loops).
pub fn exp_wgsl() -> String {
    format!(
        "{}\n{}\n{}\n{}\n{}",
        div_wgsl(),
        wgsl_section(CONSTANTS_WGSL, "CONSTS"),
        wgsl_section(CONSTANTS_WGSL, "EXP"),
        wgsl_section(TRANSCENDENTAL_BODY_WGSL, "COMMON"),
        wgsl_section(TRANSCENDENTAL_BODY_WGSL, "EXP"),
    )
}

/// Self-contained binary64 `d64_log` preamble (no unused transcendental or `d64_sqrt` loops).
pub fn log_wgsl() -> String {
    format!(
        "{}\n{}\n{}\n{}\n{}",
        div_wgsl(),
        wgsl_section(CONSTANTS_WGSL, "CONSTS"),
        wgsl_section(CONSTANTS_WGSL, "LOG"),
        wgsl_section(TRANSCENDENTAL_BODY_WGSL, "COMMON"),
        wgsl_section(TRANSCENDENTAL_BODY_WGSL, "LOG"),
    )
}

/// Self-contained binary64 `d64_sincos` preamble (no unused `d64_div`, `d64_sqrt`, `exp`, or `log`).
pub fn sincos_wgsl() -> String {
    format!(
        "{ARITHMETIC_BASE_WGSL}\n{}\n{}\n{}",
        wgsl_section(CONSTANTS_WGSL, "CONSTS"),
        wgsl_section(CONSTANTS_WGSL, "SINCOS"),
        wgsl_section(TRANSCENDENTAL_BODY_WGSL, "SINCOS"),
    )
}

#[cfg(not(target_arch = "wasm32"))]
mod tensor;
#[cfg(not(target_arch = "wasm32"))]
pub use tensor::GpuF64Tensor;
