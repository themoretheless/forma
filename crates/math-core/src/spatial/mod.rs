//! Spatial primitives: quaternions (`Quat`), dual quaternions (`DualQuat`),
//! rays (`Ray3`), planes (`Plane3`), view frustums (`Frustum`), 3D bounding
//! boxes (`Aabb3`, `Obb3`), and triangle barycentric / closest-point queries.

use crate::affine::{AffineMatrix, IDENTITY};
use crate::{Error, M3, Result, V3, add, cross, dot, norm, scale, sub, unit};

mod aabb;
mod dual_quat;
mod frustum;
mod morton;
mod obb;
mod plane;
mod quat;
mod ray;
mod solve;
mod spline;
mod triangle;

pub use aabb::*;
pub use dual_quat::*;
pub use frustum::*;
pub use morton::*;
pub use obb::*;
pub use plane::*;
pub use quat::*;
pub use ray::*;
pub use solve::*;
pub use spline::*;
pub use triangle::*;

#[cfg(test)]
mod tests;
