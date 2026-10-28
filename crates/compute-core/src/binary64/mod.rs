//! Integer implementation of binary64 arithmetic for GPU shaders without f64.
//!
//! Words are `(low, high)` u32 pairs, preserving the full 53-bit significand and
//! binary64 exponent range. Basic operations round to nearest, ties to even,
//! with gradual underflow. Arithmetic quiets NaNs; payload selection is not a
//! cross-platform promise. No dynamic rounding modes or exception flags exist.
//! Native-host tensor calls implement TensorF64Backend using these shaders.
pub const ARITHMETIC_WGSL: &str = include_str!("arithmetic.wgsl");

/// Binary64 polynomial functions and full-exponent-range trigonometric reduction.
/// Transcendentals are approximate; they do not promise correct rounding.
pub const TRANSCENDENTAL_WGSL: &str = concat!(
    include_str!("constants.wgsl"),
    include_str!("transcendental.wgsl")
);

#[cfg(not(target_arch = "wasm32"))]
mod tensor;
#[cfg(not(target_arch = "wasm32"))]
pub use tensor::GpuF64Tensor;
