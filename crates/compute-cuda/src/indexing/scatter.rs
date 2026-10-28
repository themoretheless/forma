use super::{
    CudaScalar, output,
    scatter_dispatch::{LoadedScatter, ScatterDispatch, ScatterInputs, ScatterOwnersDispatch},
};
use crate::{CudaError, CudaRuntime, CudaTensor};
use gpu_compute::cuda::{CudaFunction, cudarc::driver::DeviceRepr};
use tensor_core::{ScatterOp, Scattered, TensorScatterBackend, scatter_updates_shape};

impl CudaRuntime {
    pub(crate) fn scatter_typed<T: CudaScalar>(
        &self,
        op: ScatterOp,
        input: &CudaTensor<T>,
        indices: &CudaTensor<u32>,
        updates: &CudaTensor<T>,
        axis: usize,
    ) -> Result<Scattered<CudaTensor<T>, CudaTensor<u32>>, CudaError> {
        self.check(input)?;
        self.check(indices)?;
        self.check(updates)?;
        let expected =
            scatter_updates_shape(input.shape(), indices.shape(), updates.shape(), axis)?;
        // Validate the logical byte count after broadcasting as well as the
        // underlying storage before any operation is enqueued.
        let updates = self.view(updates, updates.layout.broadcast_to(expected)?)?;
        let mut values = self.copy_typed(input)?;
        let invalid_count = self.scatter_loaded(
            op,
            indices,
            &updates,
            &mut values,
            axis,
            &self.indexing.scatter[T::KIND],
            None,
        )?;
        Ok(Scattered {
            values,
            invalid_count,
        })
    }

    /// Shared dispatch after callers validate the broadcast update shape and
    /// allocate an independent contiguous base copy. Source and destination
    /// scalar widths may differ; the selected kernel controls their ABI.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn scatter_loaded<I: DeviceRepr, O: DeviceRepr>(
        &self,
        op: ScatterOp,
        indices: &CudaTensor<u32>,
        updates: &CudaTensor<I>,
        values: &mut CudaTensor<O>,
        axis: usize,
        kernel: &CudaFunction,
        dtype: Option<u32>,
    ) -> Result<CudaTensor<u32>, CudaError> {
        let pass = ScatterDispatch::new::<I, O>(
            op,
            values.shape(),
            &indices.layout,
            &updates.layout,
            axis,
        )?;
        let length = values.shape().dims()[axis];
        let owner_pass = if pass.needs_owners() {
            Some(ScatterOwnersDispatch::new(&indices.layout, length)?)
        } else {
            None
        };
        let invalid_count = self.invalid_index_count(indices, length)?;
        if !pass.has_work() {
            return Ok(invalid_count);
        }
        let owners = if let Some(owner_pass) = owner_pass {
            // Fresh eager storage supplies each owner election's zero state.
            let mut owners = self.zeros_typed::<u32>(owner_pass.shape.clone())?;
            let metadata = self.metadata(&owner_pass.geometry.metadata)?;
            owner_pass.launch(
                self,
                indices.storage.as_ref(),
                output(&mut owners),
                &metadata,
            )?;
            Some(owners)
        } else {
            None
        };
        // Fold kernels do not dereference owners; reuse a live scalar pointer.
        let owners = owners.as_ref().unwrap_or(&invalid_count);
        let metadata = self.metadata(&pass.metadata)?;
        pass.launch(
            self,
            LoadedScatter {
                function: kernel,
                dtype,
            },
            ScatterInputs {
                indices: indices.storage.as_ref(),
                updates: updates.storage.as_ref(),
                owners: owners.storage.as_ref(),
            },
            output(values),
            &metadata,
        )?;
        Ok(invalid_count)
    }
}

impl TensorScatterBackend for CudaRuntime {
    fn scatter_f32(
        &self,
        op: ScatterOp,
        input: &Self::Tensor,
        indices: &Self::UIntTensor,
        updates: &Self::Tensor,
        axis: usize,
    ) -> Result<Scattered<Self::Tensor, Self::UIntTensor>, CudaError> {
        self.scatter_typed(op, input, indices, updates, axis)
    }

    fn scatter_u32(
        &self,
        op: ScatterOp,
        input: &Self::UIntTensor,
        indices: &Self::UIntTensor,
        updates: &Self::UIntTensor,
        axis: usize,
    ) -> Result<Scattered<Self::UIntTensor, Self::UIntTensor>, CudaError> {
        self.scatter_typed(op, input, indices, updates, axis)
    }
}
