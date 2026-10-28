use super::*;
use tensor_core::{ScatterOp, Scattered, TensorScatterBackend, scatter_updates_shape};

impl MlxBackend {
    pub(super) fn scatter_values(
        &self,
        op: ScatterOp,
        input: &MlxTensor,
        indices: &MlxTensor,
        updates: &MlxTensor,
        axis: usize,
    ) -> Result<Scattered<MlxTensor, MlxTensor>, MlxError> {
        self.check(input, None)?;
        self.check(indices, Some(MlxDtype::U32))?;
        self.check(updates, Some(input.dtype))?;
        let update_shape =
            scatter_updates_shape(&input.shape, &indices.shape, &updates.shape, axis)?;
        Self::dimensions(&update_shape)?;
        let checked = self.checked_indices(indices, input.shape.dims()[axis])?;
        // No valid writes exist, but invalid indices still have to be counted
        // independently of empty output slices. Never call native take/scatter
        // when the indexed dimension or the index tensor has no safe element.
        if input.shape.is_empty() || indices.shape.is_empty() {
            return Ok(Scattered {
                values: input.clone(),
                invalid_count: checked.invalid_count,
            });
        }
        let updates = self.broadcast_to(updates, update_shape.clone())?;
        let values = if op == ScatterOp::Replace {
            self.scatter_replace(input, &checked, &updates, axis)?
        } else {
            let mut mask_dims = vec![1; axis];
            mask_dims.extend_from_slice(indices.shape.dims());
            mask_dims.resize(update_shape.rank(), 1);
            let valid = self.reshape(&checked.valid, Shape::new(mask_dims)?)?;
            let identity = self.scatter_identity(op, input.dtype)?;
            let updates = self.select_values(&valid, &updates, &identity)?;
            // MLX scatter slices put index axes first and retain a size-one
            // indexed axis in each update slice. The common contract instead
            // inserts index axes at the selected input axis.
            let order: Vec<usize> = (axis..axis + indices.shape.rank())
                .chain(0..axis)
                .chain(axis + indices.shape.rank()..update_shape.rank())
                .collect();
            let updates = self.permute(&updates, &order)?;
            let mut native_dims = updates.shape.dims().to_vec();
            native_dims.insert(indices.shape.rank() + axis, 1);
            let updates = self.reshape(&updates, Shape::new(native_dims)?)?;
            let operation = match op {
                ScatterOp::Add => self.context.api.scatter_add_single,
                ScatterOp::Multiply => self.context.api.scatter_prod_single,
                ScatterOp::Min => self.context.api.scatter_min_single,
                ScatterOp::Max => self.context.api.scatter_max_single,
                ScatterOp::Replace => unreachable!(),
            };
            self.output(
                "scatter reduction",
                input.shape.clone(),
                input.dtype,
                |_, out| unsafe {
                    operation(
                        out,
                        input.array.raw,
                        checked.safe.array.raw,
                        updates.array.raw,
                        axis as i32,
                        self.context.stream,
                    )
                },
            )?
        };
        Ok(Scattered {
            values,
            invalid_count: checked.invalid_count,
        })
    }

    fn scatter_identity(&self, op: ScatterOp, dtype: MlxDtype) -> Result<MlxTensor, MlxError> {
        let shape = Shape::new(vec![])?;
        match dtype {
            MlxDtype::F16 | MlxDtype::Bf16 => Err(MlxError::Dtype),
            MlxDtype::F32 => self.upload_f32(
                shape,
                &[match op {
                    // Negative zero also preserves a negative-zero base when an
                    // invalid update is redirected to element zero.
                    ScatterOp::Add => -0.0,
                    ScatterOp::Multiply => 1.0,
                    ScatterOp::Min => f32::MAX,
                    ScatterOp::Max => -f32::MAX,
                    ScatterOp::Replace => unreachable!(),
                }],
            ),
            MlxDtype::U32 => self.upload_u32(
                shape,
                &[match op {
                    ScatterOp::Add | ScatterOp::Max => 0,
                    ScatterOp::Multiply => 1,
                    ScatterOp::Min => u32::MAX,
                    ScatterOp::Replace => unreachable!(),
                }],
            ),
        }
    }

    fn scatter_replace(
        &self,
        input: &MlxTensor,
        checked: &index::CheckedIndices,
        updates: &MlxTensor,
        axis: usize,
    ) -> Result<MlxTensor, MlxError> {
        let n = checked.safe.shape.numel();
        let flat = Shape::new(vec![n])?;
        Self::dimensions(&flat)?;
        let ordinals = self.output(
            "scatter ordinals",
            flat.clone(),
            MlxDtype::U32,
            |a, out| unsafe {
                (a.arange)(out, 1.0, n as f64 + 1.0, 1.0, ffi::U32, self.context.stream)
            },
        )?;
        let zero = self.upload_u32(Shape::new(vec![])?, &[0])?;
        let one = self.upload_u32(Shape::new(vec![])?, &[1])?;
        let valid = self.reshape(&checked.valid, flat.clone())?;
        let ordinals = self.select_values(&valid, &ordinals, &zero)?;
        let ordinals = self.reshape(&ordinals, Shape::new(vec![n, 1])?)?;
        let indices = self.reshape(&checked.safe, flat)?;
        let extent = input.shape.dims()[axis];
        let owners = self.zeros(Shape::new(vec![extent])?, MlxDtype::U32)?;
        let owners = self.output(
            "scatter owners",
            owners.shape.clone(),
            MlxDtype::U32,
            |a, out| unsafe {
                (a.scatter_max_single)(
                    out,
                    owners.array.raw,
                    indices.array.raw,
                    ordinals.array.raw,
                    0,
                    self.context.stream,
                )
            },
        )?;
        // Max over one-based positions selects the final logical index for
        // every duplicate, independently of native execution order. Owner zero
        // means untouched; sanitize it before subtracting one or taking values.
        let touched = self.compare_values(CompareOp::Greater, &owners, &zero)?;
        let winner = self.binary_u32(self.context.api.maximum, &owners, &one)?;
        let winner = self.binary_u32(self.context.api.subtract, &winner, &one)?;
        let mut dims = input.shape.dims().to_vec();
        dims[axis] = n;
        let updates = self.reshape(updates, Shape::new(dims)?)?;
        let selected = self.output(
            "scatter winning updates",
            input.shape.clone(),
            input.dtype,
            |a, out| unsafe {
                (a.take_axis)(
                    out,
                    updates.array.raw,
                    winner.array.raw,
                    axis as i32,
                    self.context.stream,
                )
            },
        )?;
        let mut dims = vec![1; input.shape.rank()];
        dims[axis] = extent;
        let touched = self.reshape(&touched, Shape::new(dims)?)?;
        self.select_values(&touched, &selected, input)
    }
}

impl TensorScatterBackend for MlxBackend {
    fn scatter_f32(
        &self,
        op: ScatterOp,
        input: &MlxTensor,
        indices: &MlxTensor,
        updates: &MlxTensor,
        axis: usize,
    ) -> Result<Scattered<MlxTensor, MlxTensor>, MlxError> {
        self.check(input, Some(MlxDtype::F32))?;
        self.scatter_values(op, input, indices, updates, axis)
    }

    fn scatter_u32(
        &self,
        op: ScatterOp,
        input: &MlxTensor,
        indices: &MlxTensor,
        updates: &MlxTensor,
        axis: usize,
    ) -> Result<Scattered<MlxTensor, MlxTensor>, MlxError> {
        self.check(input, Some(MlxDtype::U32))?;
        self.scatter_values(op, input, indices, updates, axis)
    }
}
