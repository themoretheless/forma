//! Bounded, resident nearest-neighbor recipes built from shared tensor operations.
use super::{Result, TensorMath, TensorMathError, shape};
use tensor_core::{
    BinaryOp, CompareOp, ReduceOp, ScatterOp, TensorEvalBackend, TensorReduceBackend,
    TensorScatterBackend, UnaryOp,
};

/// Resource limits for the portable nearest-neighbor recipe.
///
/// The work estimate counts logical output-element evaluations of recipe
/// primitives, charging reductions by their input elements and view-only
/// operations as zero work. It includes
/// repeated rank searches, exclusion masks and full-output scatter copies. It
/// is a checked admission limit, not a measured device instruction count.
/// Workspace is a conservative admission estimate for live recipe-level logical
/// buffers, including old/new results. Caller inputs, primitive implementation
/// temporaries/materializations, metadata, caches and library workspaces are
/// excluded, as is evaluation of caller-built lazy input graphs. Neither limit
/// bounds native peak memory or backend internal work.
/// Evaluation fences release each tile's dependency graph before the next one.
#[derive(Clone, Copy, Debug)]
pub struct NeighborOptions {
    pub query_tile: usize,
    pub target_tile: usize,
    pub max_work_elements: usize,
    pub max_workspace_bytes: usize,
    pub max_output_bytes: usize,
}
impl Default for NeighborOptions {
    fn default() -> Self {
        Self {
            query_tile: 64,
            target_tile: 256,
            max_work_elements: 1_000_000_000,
            max_workspace_bytes: 64 * 1024 * 1024,
            max_output_bytes: 64 * 1024 * 1024,
        }
    }
}

/// Sorted resident results, both with shape `[query_count, k]`.
///
/// Missing slots contain index `u32::MAX` and squared distance `f32::MAX`.
/// An index distinguishes a missing slot from a valid distance equal to the
/// finite sentinel. Target indices always refer to the original logical input.
pub struct TensorNeighbors<T, U> {
    pub indices: U,
    pub squared_distances: T,
}

/// Resident directed Chamfer summary; all three tensors have scalar shape `[]`.
pub struct TensorDirectedChamfer<T> {
    pub samples: usize,
    pub mean_squared_distance: T,
    pub rms_distance: T,
    pub max_squared_distance: T,
}

struct Limits {
    ranks: usize,
}
impl Limits {
    fn check<E>(q: usize, t: usize, k: usize, options: NeighborOptions) -> Result<Self, E> {
        let invalid = TensorMathError::InvalidInput;
        if k == 0 || options.query_tile == 0 || options.target_tile == 0 {
            return Err(invalid("neighbor k and tile sizes must be positive"));
        }
        if q > u32::MAX as usize || t > u32::MAX as usize || k > u32::MAX as usize {
            return Err(invalid("neighbor dimensions exceed the u32 indexing range"));
        }
        let mul = |a: usize, b: usize| {
            a.checked_mul(b)
                .ok_or(invalid("neighbor resource estimate overflow"))
        };
        let add = |a: usize, b: usize| {
            a.checked_add(b)
                .ok_or(invalid("neighbor resource estimate overflow"))
        };
        let output = mul(q, k)?;
        if output > u32::MAX as usize {
            return Err(invalid("neighbor output exceeds the u32 indexing range"));
        }
        if mul(output, 8)? > options.max_output_bytes {
            return Err(invalid("neighbor output exceeds max_output_bytes"));
        }
        let ranks = k.min(t);
        let b = q.min(options.query_tile);
        let c = t.min(options.target_tile);
        let active = q != 0 && ranks != 0;
        let mut workspace = add(mul(output, if active { 16 } else { 8 })?, 256)?;
        let mut work = 0;
        if active {
            workspace = add(workspace, mul(mul(b, c)?, 128)?)?;
            workspace = add(workspace, mul(mul(b, k)?, 24)?)?;
            workspace = add(workspace, mul(add(b, c)?, 64)?)?;
            workspace = add(workspace, mul(q, 8)?)?;
            // r earlier winners are excluded during the search for rank r.
            let exclusions = mul(ranks, ranks - 1)? / 2;
            let per_pair = add(mul(ranks, 64)?, mul(exclusions, 4)?)?;
            work = mul(mul(q, t)?, per_pair)?;
            work = add(work, mul(mul(q.div_ceil(b), output)?, 4)?)?;
            // Each rank's local scatter copies a [B, k] tensor pair.
            work = add(work, mul(mul(mul(q, k)?, ranks)?, 4)?)?;
        }
        if workspace > options.max_workspace_bytes {
            return Err(invalid("neighbor workspace exceeds max_workspace_bytes"));
        }
        if work > options.max_work_elements {
            return Err(invalid("neighbor work exceeds max_work_elements"));
        }
        Ok(Self { ranks })
    }
}

struct Constants<T, U> {
    far: T,
    missing: U,
    zero: U,
    one: U,
}

impl<B> TensorMath<'_, B>
where
    B: TensorReduceBackend + TensorScatterBackend + TensorEvalBackend,
{
    /// Find the first `k` targets in `(squared_distance, target_index)` order.
    ///
    /// Coordinates and each f32 subtraction, square and sum must be finite.
    /// Distances use direct coordinate differences; ties use exact u32 indices.
    /// Ordering follows distances computed by the selected backend, so nearby
    /// unequal f64 distances can become ties after f32 rounding. Backend f32
    /// underflow behavior applies. No coordinate values are read on the host.
    ///
    /// Any positive `k` within the checked limits is accepted. Only
    /// `min(k, target_count)` ranks are searched. This portable implementation
    /// recomputes distances for each rank and excludes earlier winners; its
    /// work grows as O(Q T k²), and output assembly copies `[Q, k]` once per
    /// query tile. Existing specialized kernels can be faster.
    pub fn nearest(
        &self,
        queries: &B::Tensor,
        targets: &B::Tensor,
        k: usize,
        options: NeighborOptions,
    ) -> Result<TensorNeighbors<B::Tensor, B::UIntTensor>, B::Error> {
        let q = self.check_points(queries)?;
        let t = self.check_points(targets)?;
        let limits = Limits::check(q, t, k, options)?;
        let b = self.backend;
        // Validate both owners even when no tensor arithmetic is necessary.
        b.evaluate(&[queries, targets], &[])
            .map_err(TensorMathError::Backend)?;
        let constants = Constants {
            far: b
                .upload_f32(shape(&[])?, &[f32::MAX])
                .map_err(TensorMathError::Backend)?,
            missing: b
                .upload_u32(shape(&[])?, &[u32::MAX])
                .map_err(TensorMathError::Backend)?,
            zero: b
                .upload_u32(shape(&[])?, &[0])
                .map_err(TensorMathError::Backend)?,
            one: b
                .upload_u32(shape(&[])?, &[1])
                .map_err(TensorMathError::Backend)?,
        };
        let mut distances = b
            .broadcast_to(&constants.far, shape(&[q, k])?)
            .map_err(TensorMathError::Backend)?;
        let mut indices = b
            .broadcast_u32(&constants.missing, shape(&[q, k])?)
            .map_err(TensorMathError::Backend)?;
        if q != 0 && limits.ranks != 0 {
            for start in (0..q).step_by(options.query_tile) {
                let rows = options.query_tile.min(q - start);
                let row_ids = b
                    .upload_u32(
                        shape(&[rows])?,
                        &(start..start + rows).map(|x| x as u32).collect::<Vec<_>>(),
                    )
                    .map_err(TensorMathError::Backend)?;
                let query_points = b
                    .gather_f32(queries, &row_ids, 0)
                    .map_err(TensorMathError::Backend)?
                    .values;
                let query_view = b
                    .reshape(&query_points, shape(&[rows, 1, 3])?)
                    .map_err(TensorMathError::Backend)?;
                let mut local_d = b
                    .broadcast_to(&constants.far, shape(&[rows, k])?)
                    .map_err(TensorMathError::Backend)?;
                let mut local_i = b
                    .broadcast_u32(&constants.missing, shape(&[rows, k])?)
                    .map_err(TensorMathError::Backend)?;
                let mut selected = Vec::with_capacity(limits.ranks);
                for rank in 0..limits.ranks {
                    let mut best_d = b
                        .broadcast_to(&constants.far, shape(&[rows, 1])?)
                        .map_err(TensorMathError::Backend)?;
                    let mut best_i = b
                        .broadcast_u32(&constants.missing, shape(&[rows, 1])?)
                        .map_err(TensorMathError::Backend)?;
                    for first in (0..t).step_by(options.target_tile) {
                        let cols = options.target_tile.min(t - first);
                        let target_ids = b
                            .upload_u32(
                                shape(&[cols])?,
                                &(first..first + cols).map(|x| x as u32).collect::<Vec<_>>(),
                            )
                            .map_err(TensorMathError::Backend)?;
                        let (tile_d, tile_i) = self.neighbor_tile(
                            &query_view,
                            targets,
                            &target_ids,
                            &selected,
                            &constants,
                        )?;
                        let less = b
                            .compare(CompareOp::Less, &tile_d, &best_d)
                            .map_err(TensorMathError::Backend)?;
                        let equal = b
                            .compare(CompareOp::Equal, &tile_d, &best_d)
                            .map_err(TensorMathError::Backend)?;
                        let index_less = b
                            .compare_u32(CompareOp::Less, &tile_i, &best_i)
                            .map_err(TensorMathError::Backend)?;
                        let tied = b
                            .select_u32(&equal, &index_less, &constants.zero)
                            .map_err(TensorMathError::Backend)?;
                        let choose = b
                            .select_u32(&less, &constants.one, &tied)
                            .map_err(TensorMathError::Backend)?;
                        let next_d = b
                            .select_f32(&choose, &tile_d, &best_d)
                            .map_err(TensorMathError::Backend)?;
                        let next_i = b
                            .select_u32(&choose, &tile_i, &best_i)
                            .map_err(TensorMathError::Backend)?;
                        b.evaluate(&[&next_d], &[&next_i])
                            .map_err(TensorMathError::Backend)?;
                        best_d = next_d;
                        best_i = next_i;
                    }
                    let column = b
                        .upload_u32(shape(&[])?, &[rank as u32])
                        .map_err(TensorMathError::Backend)?;
                    let d = b
                        .reshape(&best_d, shape(&[rows])?)
                        .map_err(TensorMathError::Backend)?;
                    let i = b
                        .reshape_u32(&best_i, shape(&[rows])?)
                        .map_err(TensorMathError::Backend)?;
                    local_d = b
                        .scatter_f32(ScatterOp::Replace, &local_d, &column, &d, 1)
                        .map_err(TensorMathError::Backend)?
                        .values;
                    local_i = b
                        .scatter_u32(ScatterOp::Replace, &local_i, &column, &i, 1)
                        .map_err(TensorMathError::Backend)?
                        .values;
                    b.evaluate(&[&local_d], &[&local_i])
                        .map_err(TensorMathError::Backend)?;
                    selected.push(best_i);
                }
                distances = b
                    .scatter_f32(ScatterOp::Replace, &distances, &row_ids, &local_d, 0)
                    .map_err(TensorMathError::Backend)?
                    .values;
                indices = b
                    .scatter_u32(ScatterOp::Replace, &indices, &row_ids, &local_i, 0)
                    .map_err(TensorMathError::Backend)?
                    .values;
                b.evaluate(&[&distances], &[&indices])
                    .map_err(TensorMathError::Backend)?;
            }
        }
        b.evaluate(&[&distances], &[&indices])
            .map_err(TensorMathError::Backend)?;
        Ok(TensorNeighbors {
            indices,
            squared_distances: distances,
        })
    }

    fn neighbor_tile(
        &self,
        queries: &B::Tensor,
        targets: &B::Tensor,
        target_ids: &B::UIntTensor,
        selected: &[B::UIntTensor],
        constants: &Constants<B::Tensor, B::UIntTensor>,
    ) -> Result<(B::Tensor, B::UIntTensor), B::Error> {
        use tensor_core::HasShape;
        let b = self.backend;
        let cols = target_ids.shape().numel();
        let target_points = b
            .gather_f32(targets, target_ids, 0)
            .map_err(TensorMathError::Backend)?
            .values;
        let target_view = b
            .reshape(&target_points, shape(&[1, cols, 3])?)
            .map_err(TensorMathError::Backend)?;
        let delta = b
            .binary(BinaryOp::Subtract, queries, &target_view)
            .map_err(TensorMathError::Backend)?;
        let square = b
            .unary(UnaryOp::Square, &delta)
            .map_err(TensorMathError::Backend)?;
        let distance = b
            .sum_axes(&square, &[2], false)
            .map_err(TensorMathError::Backend)?;
        let ids = b
            .reshape_u32(target_ids, shape(&[1, cols])?)
            .map_err(TensorMathError::Backend)?;
        let mut eligible = b
            .broadcast_u32(&constants.one, distance.shape().clone())
            .map_err(TensorMathError::Backend)?;
        for prior in selected {
            let different = b
                .compare_u32(CompareOp::NotEqual, &ids, prior)
                .map_err(TensorMathError::Backend)?;
            eligible = b
                .select_u32(&different, &eligible, &constants.zero)
                .map_err(TensorMathError::Backend)?;
            // Do not retain one full tile mask for every earlier rank.
            b.evaluate(&[], &[&eligible])
                .map_err(TensorMathError::Backend)?;
        }
        let masked = b
            .select_f32(&eligible, &distance, &constants.far)
            .map_err(TensorMathError::Backend)?;
        let minimum = b
            .reduce_f32(ReduceOp::Min, &masked, &[1], true)
            .map_err(TensorMathError::Backend)?;
        let equal = b
            .compare(CompareOp::Equal, &distance, &minimum)
            .map_err(TensorMathError::Backend)?;
        let allowed = b
            .select_u32(&eligible, &equal, &constants.zero)
            .map_err(TensorMathError::Backend)?;
        let candidates = b
            .select_u32(&allowed, &ids, &constants.missing)
            .map_err(TensorMathError::Backend)?;
        let index = b
            .reduce_u32(ReduceOp::Min, &candidates, &[1], true)
            .map_err(TensorMathError::Backend)?;
        Ok((minimum, index))
    }

    /// Summarize the nearest squared distance from each query to the targets.
    /// Both clouds must be nonempty. The f32 sum of nearest squared distances
    /// must be finite, as required by the backend's mean reduction contract.
    pub fn directed_chamfer(
        &self,
        queries: &B::Tensor,
        targets: &B::Tensor,
        options: NeighborOptions,
    ) -> Result<TensorDirectedChamfer<B::Tensor>, B::Error> {
        let samples = self.check_points(queries)?;
        if samples == 0 || self.check_points(targets)? == 0 {
            return Err(TensorMathError::InvalidInput(
                "directed Chamfer requires nonempty point clouds",
            ));
        }
        let nearest = self.nearest(queries, targets, 1, options)?;
        let mean_squared_distance = self
            .backend
            .mean_axes(&nearest.squared_distances, &[0, 1], false)
            .map_err(TensorMathError::Backend)?;
        let max_squared_distance = self
            .backend
            .reduce_f32(ReduceOp::Max, &nearest.squared_distances, &[0, 1], false)
            .map_err(TensorMathError::Backend)?;
        let rms_distance = self
            .backend
            .unary(UnaryOp::Sqrt, &mean_squared_distance)
            .map_err(TensorMathError::Backend)?;
        self.backend
            .evaluate(
                &[&mean_squared_distance, &max_squared_distance, &rms_distance],
                &[],
            )
            .map_err(TensorMathError::Backend)?;
        Ok(TensorDirectedChamfer {
            samples,
            mean_squared_distance,
            rms_distance,
            max_squared_distance,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checked_limits_accept_missing_ranks_and_reject_each_budget() {
        let options = NeighborOptions::default();
        assert_eq!(Limits::check::<()>(5, 3, 37, options).unwrap().ranks, 3);
        assert_eq!(Limits::check::<()>(0, 3, 37, options).unwrap().ranks, 3);
        assert!(Limits::check::<()>(2, 3, 0, options).is_err());
        assert!(
            Limits::check::<()>(
                0,
                0,
                1,
                NeighborOptions {
                    max_workspace_bytes: 0,
                    ..options
                }
            )
            .is_err()
        );
        for changed in [
            NeighborOptions {
                query_tile: 0,
                ..options
            },
            NeighborOptions {
                target_tile: 0,
                ..options
            },
            NeighborOptions {
                max_output_bytes: 1,
                ..options
            },
            NeighborOptions {
                max_workspace_bytes: 1,
                ..options
            },
            NeighborOptions {
                max_work_elements: 1,
                ..options
            },
        ] {
            assert!(Limits::check::<()>(5, 3, 4, changed).is_err());
        }
    }
    #[test]
    fn checked_limits_reject_overflow_and_excessive_rank_work() {
        let options = NeighborOptions {
            max_output_bytes: usize::MAX,
            max_workspace_bytes: usize::MAX,
            max_work_elements: usize::MAX,
            ..NeighborOptions::default()
        };
        assert!(Limits::check::<()>(usize::MAX, 1, 1, options).is_err());
        assert!(
            Limits::check::<()>(
                u32::MAX as usize,
                u32::MAX as usize,
                u32::MAX as usize,
                options
            )
            .is_err()
        );
        assert!(
            Limits::check::<()>(
                100,
                100,
                100,
                NeighborOptions {
                    max_work_elements: 100_000,
                    ..NeighborOptions::default()
                }
            )
            .is_err()
        );
    }
}
