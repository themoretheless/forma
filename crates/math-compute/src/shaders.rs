//! WGSL sources for the device kernels (moved from `math-core`).

/// Cooperative target reduction with one workgroup per query. The production
/// Metal policy uses 64 lanes; the source retains the tunable WG=256 anchor.
pub const NEAREST_NEIGHBOR_COOPERATIVE_WGSL: &str =
    include_str!("nearest_neighbor_cooperative.wgsl");
/// WGSL source for the exact top-2 nearest-neighbor compute shader.
pub const NEAREST_TWO_WGSL: &str = include_str!("nearest_two.wgsl");
/// WGSL source for the exact top-4 nearest-neighbor compute shader.
pub const NEAREST_FOUR_WGSL: &str = include_str!("nearest_four.wgsl");
/// WGSL source for the fused transform-and-distance reduction shader.
pub const TRANSFORMED_DISTANCE_PAIR_SUM_WGSL: &str =
    include_str!("transformed_distance_pair_sum.wgsl");
/// WGSL source for the nearest-neighbor compute shader (feature `gpu`); the
/// `gpu` and `cuda` modules both target this exact formula.
pub const NEAREST_NEIGHBOR_WGSL: &str = include_str!("nearest_neighbor.wgsl");
/// WGSL source for the one-to-one squared-distance compute shader.
pub const DISTANCE_PAIRS_WGSL: &str = include_str!("distance_pairs.wgsl");
/// WGSL source for the one-to-one squared-distance reduction shader.
pub const DISTANCE_PAIR_SUM_WGSL: &str = include_str!("distance_pair_sum.wgsl");
/// WGSL template for directed Chamfer partial reduction.
pub const CHAMFER_WGSL: &str = include_str!("chamfer.wgsl");
/// WGSL template for fused point-cloud bounds + moments reduction.
pub const POINT_CLOUD_STATS_WGSL: &str = include_str!("point_cloud_stats.wgsl");
/// WGSL template for point-cloud axis-aligned bounds reduction.
pub const POINT_BOUNDS_WGSL: &str = include_str!("point_bounds.wgsl");
/// WGSL template for point-cloud moment reduction.
pub const POINT_MOMENTS_WGSL: &str = include_str!("point_moments.wgsl");
/// WGSL template for transformed point-cloud axis-aligned bounds reduction.
pub const TRANSFORMED_POINT_BOUNDS_WGSL: &str = include_str!("transformed_point_bounds.wgsl");
