//! Spatial primitives: quaternions (`Quat`), dual quaternions (`DualQuat`),
//! rays (`Ray3`), planes (`Plane3`), view frustums (`Frustum`), 3D bounding
//! boxes (`Aabb3`, `Obb3`), and triangle barycentric / closest-point queries.

use crate::affine::{AffineMatrix, IDENTITY};
use crate::{Error, M3, Result, V3, add, cross, dot, norm, scale, sub, unit};

mod boxes;
mod morton;
mod primitives;
mod rotation;
mod solve;
mod spline;
mod triangle;

pub use boxes::*;
pub use morton::*;
pub use primitives::*;
pub use rotation::*;
pub use solve::*;
pub use spline::*;
pub use triangle::*;

#[cfg(test)]
mod tests;
