use super::{GpuArray, GpuMathError, MathGpuProgram, PointCloudView};

/// Device-resident nearest target for every query. Ties select the first target.
/// An empty target cloud produces `u32::MAX` indices and `f32::MAX` distances,
/// the same sentinel as the synchronous WGSL adapter.
#[derive(Clone)]
pub struct GpuNearestNeighbors {
    pub indices: GpuArray<u32>,
    pub squared_distances: GpuArray<f32>,
}

impl MathGpuProgram<'_> {
    /// Records brute-force nearest neighbors, O(queries * targets), using f32
    /// arithmetic. Coordinates are expected to be finite; the plan cannot
    /// inspect device data. Squared distances that overflow leave the sentinel.
    pub fn nearest_neighbors(
        &mut self,
        queries: PointCloudView<'_>,
        targets: PointCloudView<'_>,
    ) -> Result<GpuNearestNeighbors, GpuMathError> {
        self.check(queries)?;
        self.check(targets)?;
        let output = GpuNearestNeighbors {
            indices: self.runtime.zeros(queries.len())?,
            squared_distances: self.runtime.zeros(queries.len())?,
        };
        self.nearest_neighbors_into(queries, targets, &output.indices, &output.squared_distances)?;
        Ok(output)
    }

    /// Records into existing arrays of exactly `queries.len()` elements.
    /// Inputs and outputs must use this context and distinct allocations.
    /// All validation finishes before any dispatch is added to the plan.
    pub fn nearest_neighbors_into(
        &mut self,
        queries: PointCloudView<'_>,
        targets: PointCloudView<'_>,
        indices: &GpuArray<u32>,
        squared_distances: &GpuArray<f32>,
    ) -> Result<(), GpuMathError> {
        self.check(queries)?;
        self.check(targets)?;
        self.check_output(indices, queries.len(), &[queries.view, targets.view])?;
        self.check_output(
            squared_distances,
            queries.len(),
            &[queries.view, targets.view, indices.view()],
        )?;
        if queries.is_empty() {
            return Ok(());
        }
        let params = self.params(queries.count, targets.count);
        // Zero-length buffers cannot be bound. The loop does not read targets
        // when target_count is zero, so the query binding is a valid placeholder.
        let target_view = if targets.is_empty() {
            queries.view
        } else {
            targets.view
        };
        let (kernel, _, groups) = self.kernels.nearest.select(queries.len(), targets.len());
        let bindings = kernel.create_bind_group_resources(
            self.runtime.device(),
            &[
                params.as_entire_binding(),
                queries.view.storage_binding(self.runtime.context())?,
                target_view.storage_binding(self.runtime.context())?,
                indices.view().storage_binding(self.runtime.context())?,
                squared_distances
                    .view()
                    .storage_binding(self.runtime.context())?,
            ],
        );
        self.batch.push(kernel, &bindings, groups);
        Ok(())
    }
}
