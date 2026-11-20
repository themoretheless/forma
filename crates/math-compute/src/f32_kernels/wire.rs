//! Shared host/device wire conversions and partial-reduction folds for f32 point kernels.

#[cfg(any(target_arch = "wasm32", feature = "cuda"))]
use crate::{M3, V3};

#[cfg(feature = "gpu")]
pub(crate) fn zip_nearest_one_bytes(raw_index: &[u8], raw_dist: &[u8]) -> Vec<(u32, f64)> {
    raw_index
        .as_chunks::<4>()
        .0
        .iter()
        .zip(raw_dist.as_chunks::<4>().0.iter())
        .map(|(index, dist)| (u32::from_le_bytes(*index), f32::from_le_bytes(*dist) as f64))
        .collect()
}

#[cfg(feature = "cuda")]
pub(crate) fn zip_nearest_one(indices: Vec<u32>, distances: Vec<f32>) -> Vec<(u32, f64)> {
    indices
        .into_iter()
        .zip(distances)
        .map(|(i, d)| (i, d as f64))
        .collect()
}

pub(crate) fn zip_nearest_two(
    i0: &[u32],
    d0: &[f32],
    i1: &[u32],
    d1: &[f32],
) -> Vec<crate::TwoNearest> {
    (0..i0.len())
        .map(|idx| [(i0[idx], d0[idx] as f64), (i1[idx], d1[idx] as f64)])
        .collect()
}

pub(crate) fn zip_nearest_four(
    indices: &[u32],
    distances: &[f32],
    query_count: usize,
) -> Vec<crate::FourNearest> {
    (0..query_count)
        .map(|idx| {
            std::array::from_fn(|k| {
                let off = idx * 4 + k;
                (indices[off], distances[off] as f64)
            })
        })
        .collect()
}

pub(crate) fn fold_chamfer(
    sums: &[f32],
    maxes: &[f32],
    query_count: usize,
) -> crate::DirectedChamfer {
    let sum: f64 = sums.iter().map(|&v| v as f64).sum();
    let max_squared_distance = maxes.iter().map(|&v| v as f64).fold(0., f64::max);
    let mean_squared_distance = sum / query_count as f64;
    crate::DirectedChamfer {
        samples: query_count,
        mean_squared_distance,
        rms_distance: mean_squared_distance.sqrt(),
        max_squared_distance,
    }
}

#[cfg(any(target_arch = "wasm32", feature = "cuda"))]
pub(crate) fn pack_points_f32(points: &[V3]) -> Vec<f32> {
    points.iter().flatten().map(|&v| v as f32).collect()
}

#[cfg(any(target_arch = "wasm32", feature = "cuda"))]
pub(crate) fn pack_transform_f32(m: M3, t: V3) -> [f32; 12] {
    let mut out = [0.0_f32; 12];
    for row in 0..3 {
        let base = row * 4;
        out[base] = m[row][0] as f32;
        out[base + 1] = m[row][1] as f32;
        out[base + 2] = m[row][2] as f32;
        out[base + 3] = t[row] as f32;
    }
    out
}

#[cfg(any(target_arch = "wasm32", feature = "cuda"))]
pub(crate) fn fold_point_bounds(
    point_count: usize,
    raw_min: &[f32],
    raw_max: &[f32],
    partial_count: usize,
) -> crate::PointBounds {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for chunk in raw_min.as_chunks::<3>().0.iter().take(partial_count) {
        for axis in 0..3 {
            min[axis] = min[axis].min(chunk[axis] as f64);
        }
    }
    for chunk in raw_max.as_chunks::<3>().0.iter().take(partial_count) {
        for axis in 0..3 {
            max[axis] = max[axis].max(chunk[axis] as f64);
        }
    }
    crate::PointBounds::new(point_count, min, max)
}

#[cfg(any(target_arch = "wasm32", feature = "cuda"))]
pub(crate) fn fold_f64_vec(values: &[f32]) -> Vec<f64> {
    values.iter().map(|&v| v as f64).collect()
}

#[cfg(any(target_arch = "wasm32", feature = "cuda"))]
pub(crate) fn fold_f64_sum(partials: &[f32]) -> f64 {
    partials.iter().map(|&v| v as f64).sum()
}

#[cfg(any(target_arch = "wasm32", feature = "cuda"))]
pub(crate) fn fold_point_moments(
    point_count: usize,
    raw: &[f32],
    partial_count: usize,
) -> crate::PointMoments {
    let mut accum = [0.; 9];
    for chunk in raw.as_chunks::<9>().0.iter().take(partial_count) {
        for item in 0..9 {
            accum[item] += chunk[item] as f64;
        }
    }
    crate::PointMoments::from_sums(
        point_count,
        [accum[0], accum[1], accum[2]],
        [accum[3], accum[4], accum[5], accum[6], accum[7], accum[8]],
    )
}

#[cfg(any(target_arch = "wasm32", feature = "cuda"))]
pub(crate) fn fold_point_cloud_stats(
    point_count: usize,
    raw: &[f32],
    partial_count: usize,
) -> crate::PointCloudStats {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    let mut accum = [0.; 9];
    for chunk in raw.as_chunks::<15>().0.iter().take(partial_count) {
        for axis in 0..3 {
            min[axis] = min[axis].min(chunk[axis] as f64);
            max[axis] = max[axis].max(chunk[axis + 3] as f64);
        }
        for item in 0..9 {
            accum[item] += chunk[item + 6] as f64;
        }
    }
    let bounds = crate::PointBounds::new(point_count, min, max);
    let moments = crate::PointMoments::from_sums(
        point_count,
        [accum[0], accum[1], accum[2]],
        [accum[3], accum[4], accum[5], accum[6], accum[7], accum[8]],
    );
    crate::PointCloudStats::from_parts(bounds, moments)
}
