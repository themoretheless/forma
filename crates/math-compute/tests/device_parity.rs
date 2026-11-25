//! Consolidated device parity tests moved from `math-core`.
#![cfg(feature = "gpu")]

#[path = "device/bounds.rs"]
mod bounds;
#[path = "device/chamfer.rs"]
mod chamfer;
#[path = "device/distance_pairs.rs"]
mod distance_pairs;
#[path = "device/local_plane.rs"]
mod local_plane;
#[path = "device/moments.rs"]
mod moments;
#[path = "device/nearest_four.rs"]
mod nearest_four;
#[path = "device/nearest_neighbor.rs"]
mod nearest_neighbor;
#[path = "device/nearest_two.rs"]
mod nearest_two;
#[path = "device/stats.rs"]
mod stats;
#[path = "device/transform_error.rs"]
mod transform_error;
