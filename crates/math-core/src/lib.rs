//! Small dense linear algebra and accelerated geometry math kernels.

mod acceleration;
pub mod affine;
pub mod projective;
mod bounds;
mod chamfer;
mod device;
mod distance_pairs;
mod error;
mod linalg;
mod local_plane;
mod moments;
mod nearest_four;
mod nearest_neighbor;
mod nearest_two;
mod registration;
mod rounding;
pub use rounding::{next_down, next_up};
pub mod spatial;
mod stats;
mod transform_error;
mod types;

pub use acceleration::Acceleration;
pub use bounds::{
    PointBounds, point_bounds, point_bounds_accelerated, transformed_point_bounds,
    transformed_point_bounds_accelerated,
};
pub use chamfer::{
    ChamferDistance, DirectedChamfer, chamfer_distance, directed_chamfer_distance,
    directed_hausdorff_distance, hausdorff_distance,
};
pub use device::{DeviceKernels, device_kernels, install_device_kernels};
pub use distance_pairs::{
    squared_distance_pair_sum, squared_distance_pair_sum_accelerated, squared_distance_pairs,
    squared_distance_pairs_accelerated,
};
pub use error::{Error, Result, ensure};
pub use linalg::{
    add, add2, cholesky_banded, cross, cross2, det, dot, dot2, eigen, finite, mm, mv, norm, norm2,
    rotation, scale, scale2, smallest, solve, sub, sub2, svd, tr, transform_points, unit, unit2,
};
pub use local_plane::{LocalPlane, local_point_planes};
pub use moments::{
    PointMoments, PointPlane, PointPrincipalAxes, point_centroid, point_fit_plane, point_moments,
    point_moments_accelerated, point_principal_axes, weighted_point_centroid,
};
pub use nearest_four::{
    FourNearest, nearest_four, nearest_four_accelerated, nearest_four_first_two,
};
pub use nearest_neighbor::{nearest_neighbor, nearest_neighbor_accelerated};
pub use nearest_two::{
    Neighbor, TwoNearest, nearest_two, nearest_two_accelerated, nearest_two_first_only,
    nearest_two_ratios_accelerated,
};
pub use registration::{IcpOptions, IcpReport, RigidTransform, icp_register, rigid_transform};
pub use spatial::{
    Aabb3, CatmullRomSpline3, DualQuat, Frustum, Obb3, Plane3, Quat, Ray3, barycentric_coords_3d,
    closest_point_on_triangle_3d, evaluate_catmull_rom_3d, evaluate_catmull_rom_tangent_3d,
    expand_bits_10_to_30, morton3d_decode, morton3d_encode, morton3d_from_unit_point,
    plane_from_points_pca, ray_triangle_intersect_moller_trumbore, solve_linear_3x3,
    symmetric_eigen3,
};
pub use stats::{
    PointCloudStats, point_cloud_stats, point_cloud_stats_accelerated,
    transformed_point_cloud_stats, transformed_point_cloud_stats_accelerated,
};
pub use transform_error::{
    transformed_squared_distance_pair_rmse, transformed_squared_distance_pair_sum,
    transformed_squared_distance_pair_sum_accelerated,
};
pub use types::{ID, M3, V2, V3};

pub mod camera_gestures;
pub mod orbit_camera;
pub mod viewport;

pub mod witness_bounds;
pub mod profile_plane;

pub mod parallel;
pub use parallel::{available_threads,par_map,par_map_index,par_try_map};
