//! Screen-space post-processing kernels (SSAO ambient occlusion, Sobel edge
//! silhouette detection) and frustum/backface culling telemetry for `raster-core`.

mod color;
mod culling;
mod depth;

pub use color::*;
pub use culling::*;
pub use depth::*;

#[cfg(test)]
mod tests;
