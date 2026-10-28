use super::*;
use tensor_core::{
    Compacted, Gathered, ScanOptions, TensorIndexBackend, compact_shape, gather_shape, select_shape,
};

pub(super) struct CheckedIndices {
    pub(super) valid: MlxTensor,
    pub(super) safe: MlxTensor,
    pub(super) invalid_count: MlxTensor,
}

impl MlxBackend {
    pub(super) fn zeros(&self, shape: Shape, dtype: MlxDtype) -> Result<MlxTensor, MlxError> {
        let dims = Self::dimensions(&shape)?;
        self.output("zeros", shape, dtype, |a, out| unsafe {
            (a.zeros)(
                out,
                dims.as_ptr(),
                dims.len(),
                dtype.raw(),
                self.context.stream,
            )
        })
    }

    pub(super) fn select_values(
        &self,
        mask: &MlxTensor,
        on_true: &MlxTensor,
        on_false: &MlxTensor,
    ) -> Result<MlxTensor, MlxError> {
        self.check(mask, Some(MlxDtype::U32))?;
        self.check(on_true, None)?;
        self.check(on_false, Some(on_true.dtype))?;
        let shape = select_shape(&mask.shape, &on_true.shape, &on_false.shape)?;
        if matches!(on_true.dtype, MlxDtype::F16 | MlxDtype::Bf16) {
            return self.select_low_values(mask, on_true, on_false, shape);
        }
        self.output("select", shape, on_true.dtype, |a, out| unsafe {
            // MLX converts the condition to bool, preserving nonzero-mask semantics.
            (a.r#where)(
                out,
                mask.array.raw,
                on_true.array.raw,
                on_false.array.raw,
                self.context.stream,
            )
        })
    }

    pub(super) fn binary_u32(
        &self,
        operation: ffi::Binary,
        left: &MlxTensor,
        right: &MlxTensor,
    ) -> Result<MlxTensor, MlxError> {
        self.check(left, Some(MlxDtype::U32))?;
        self.check(right, Some(MlxDtype::U32))?;
        let shape = left.shape.broadcast(&right.shape)?;
        self.output("index arithmetic", shape, MlxDtype::U32, |_, out| unsafe {
            operation(out, left.array.raw, right.array.raw, self.context.stream)
        })
    }

    pub(super) fn checked_indices(
        &self,
        indices: &MlxTensor,
        extent: usize,
    ) -> Result<CheckedIndices, MlxError> {
        self.check(indices, Some(MlxDtype::U32))?;
        tensor_core::validate_index_count(&indices.shape)?;
        let extent = u32::try_from(extent).map_err(|_| MlxError::TooLarge)?;
        let extent = self.upload_u32(Shape::new(vec![])?, &[extent])?;
        let zero = self.upload_u32(Shape::new(vec![])?, &[0])?;
        let valid = self.compare_values(CompareOp::Less, indices, &extent)?;
        let invalid = self.compare_values(CompareOp::Equal, &valid, &zero)?;
        let invalid_count = self.sum_axes(
            &invalid,
            &(0..indices.shape.rank()).collect::<Vec<_>>(),
            false,
        )?;
        let safe = self.select_values(&valid, indices, &zero)?;
        Ok(CheckedIndices {
            valid,
            safe,
            invalid_count,
        })
    }

    pub(super) fn gather_values(
        &self,
        input: &MlxTensor,
        indices: &MlxTensor,
        axis: usize,
    ) -> Result<Gathered<MlxTensor, MlxTensor>, MlxError> {
        self.check(input, None)?;
        self.check(indices, Some(MlxDtype::U32))?;
        let shape = gather_shape(&input.shape, &indices.shape, axis)?;
        Self::dimensions(&shape)?;
        let checked = self.checked_indices(indices, input.shape.dims()[axis])?;
        // Native take rejects a nonempty take from an empty axis. Other empty
        // output dimensions also need no gather; the index count remains valid.
        let values = if input.shape.dims()[axis] == 0 || shape.is_empty() {
            self.zeros(shape, input.dtype)?
        } else {
            let gathered = self.output("gather", shape.clone(), input.dtype, |a, out| unsafe {
                (a.take_axis)(
                    out,
                    input.array.raw,
                    checked.safe.array.raw,
                    axis as i32,
                    self.context.stream,
                )
            })?;
            let mut mask_dims = vec![1; axis];
            mask_dims.extend_from_slice(indices.shape.dims());
            mask_dims.resize(shape.rank(), 1);
            let valid = self.reshape(&checked.valid, Shape::new(mask_dims)?)?;
            let zero = self.zeros(Shape::new(vec![])?, input.dtype)?;
            self.select_values(&valid, &gathered, &zero)?
        };
        Ok(Gathered {
            values,
            invalid_count: checked.invalid_count,
        })
    }

    pub(super) fn compact_values(
        &self,
        input: &MlxTensor,
        mask: &MlxTensor,
    ) -> Result<Compacted<MlxTensor, MlxTensor>, MlxError> {
        self.check(input, None)?;
        self.check(mask, Some(MlxDtype::U32))?;
        let shape = compact_shape(&input.shape, &mask.shape)?;
        // Flattened MLX dimensions and arange lengths must fit i32.
        Self::dimensions(&shape)?;
        let zeros = self.zeros(shape.clone(), input.dtype)?;
        if shape.is_empty() {
            return Ok(Compacted {
                values: zeros,
                count: self.zeros(Shape::new(vec![])?, MlxDtype::U32)?,
            });
        }
        let mask = self.broadcast_to(mask, input.shape.clone())?;
        let mask = self.reshape(&mask, shape.clone())?;
        let zero = self.upload_u32(Shape::new(vec![])?, &[0])?;
        let flags = self.compare_values(CompareOp::NotEqual, &mask, &zero)?;
        let prefix = self.scan(&flags, 0, false, false)?;
        let count = self.sum_axes(&flags, &[0], false)?;
        let indices = self.output("indices", shape.clone(), MlxDtype::U32, |a, out| unsafe {
            (a.arange)(
                out,
                0.0,
                shape.numel() as f64,
                1.0,
                ffi::U32,
                self.context.stream,
            )
        })?;
        let rejected_before = self.binary_u32(self.context.api.subtract, &indices, &prefix)?;
        let rejected_destination =
            self.binary_u32(self.context.api.add, &count, &rejected_before)?;
        let destinations = self.select_values(&flags, &prefix, &rejected_destination)?;
        let input = self.reshape(input, shape.clone())?;
        let updates = self.select_values(&flags, &input, &zeros)?;
        // Selected elements map bijectively to [0,count); rejected elements
        // map bijectively to [count,n). These disjoint intervals cover [0,n),
        // including all-true/all-false masks, so scatter has no duplicate writes.
        let values = self.output("compact", shape, input.dtype, |a, out| unsafe {
            (a.put_along_axis)(
                out,
                zeros.array.raw,
                destinations.array.raw,
                updates.array.raw,
                0,
                self.context.stream,
            )
        })?;
        Ok(Compacted { values, count })
    }
}

impl TensorIndexBackend for MlxBackend {
    type UIntTensor = MlxTensor;

    fn upload_u32(&self, shape: Shape, values: &[u32]) -> Result<MlxTensor, MlxError> {
        self.upload_u32(shape, values)
    }
    fn read_u32(&self, input: &MlxTensor) -> Result<Vec<u32>, MlxError> {
        self.read_u32(input)
    }
    fn materialize_u32(&self, input: &MlxTensor) -> Result<MlxTensor, MlxError> {
        self.check(input, Some(MlxDtype::U32))?;
        self.materialize(input)
    }
    fn reshape_u32(&self, input: &MlxTensor, shape: Shape) -> Result<MlxTensor, MlxError> {
        self.check(input, Some(MlxDtype::U32))?;
        self.reshape(input, shape)
    }
    fn permute_u32(&self, input: &MlxTensor, axes: &[usize]) -> Result<MlxTensor, MlxError> {
        self.check(input, Some(MlxDtype::U32))?;
        self.permute(input, axes)
    }
    fn broadcast_u32(&self, input: &MlxTensor, shape: Shape) -> Result<MlxTensor, MlxError> {
        self.check(input, Some(MlxDtype::U32))?;
        self.broadcast_to(input, shape)
    }
    fn compare(&self, op: CompareOp, a: &MlxTensor, b: &MlxTensor) -> Result<MlxTensor, MlxError> {
        self.compare(op, a, b)
    }
    fn compare_u32(
        &self,
        op: CompareOp,
        a: &MlxTensor,
        b: &MlxTensor,
    ) -> Result<MlxTensor, MlxError> {
        self.check(a, Some(MlxDtype::U32))?;
        self.compare_values(op, a, b)
    }
    fn select_f32(
        &self,
        mask: &MlxTensor,
        a: &MlxTensor,
        b: &MlxTensor,
    ) -> Result<MlxTensor, MlxError> {
        self.check(a, Some(MlxDtype::F32))?;
        self.select_values(mask, a, b)
    }
    fn select_u32(
        &self,
        mask: &MlxTensor,
        a: &MlxTensor,
        b: &MlxTensor,
    ) -> Result<MlxTensor, MlxError> {
        self.check(a, Some(MlxDtype::U32))?;
        self.select_values(mask, a, b)
    }
    fn scan_f32(
        &self,
        input: &MlxTensor,
        axis: usize,
        options: ScanOptions,
    ) -> Result<MlxTensor, MlxError> {
        self.check(input, Some(MlxDtype::F32))?;
        self.scan(input, axis, options.inclusive, options.reverse)
    }
    fn scan_u32(
        &self,
        input: &MlxTensor,
        axis: usize,
        options: ScanOptions,
    ) -> Result<MlxTensor, MlxError> {
        self.check(input, Some(MlxDtype::U32))?;
        self.scan(input, axis, options.inclusive, options.reverse)
    }
    fn gather_f32(
        &self,
        input: &MlxTensor,
        indices: &MlxTensor,
        axis: usize,
    ) -> Result<Gathered<MlxTensor, MlxTensor>, MlxError> {
        self.check(input, Some(MlxDtype::F32))?;
        self.gather_values(input, indices, axis)
    }
    fn gather_u32(
        &self,
        input: &MlxTensor,
        indices: &MlxTensor,
        axis: usize,
    ) -> Result<Gathered<MlxTensor, MlxTensor>, MlxError> {
        self.check(input, Some(MlxDtype::U32))?;
        self.gather_values(input, indices, axis)
    }
    fn compact_f32(
        &self,
        input: &MlxTensor,
        mask: &MlxTensor,
    ) -> Result<Compacted<MlxTensor, MlxTensor>, MlxError> {
        self.check(input, Some(MlxDtype::F32))?;
        self.compact_values(input, mask)
    }
    fn compact_u32(
        &self,
        input: &MlxTensor,
        mask: &MlxTensor,
    ) -> Result<Compacted<MlxTensor, MlxTensor>, MlxError> {
        self.check(input, Some(MlxDtype::U32))?;
        self.compact_values(input, mask)
    }
}
