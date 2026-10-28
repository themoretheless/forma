//! Shared attention metadata and checked launch over borrowed resident buffers.
use crate::{
    CudaError, CudaRuntime,
    runtime::{rank, validate_logical_size, validate_storage},
};
use gpu_compute::cuda::{CudaSlice, LaunchConfig, PushKernelArg, cudarc::driver::DeviceRepr};
use tensor_core::{AttentionPlan, Layout, Shape};

fn canonical_layout(layout: &Layout, promoted: &Shape, batch: &Shape) -> Result<Layout, CudaError> {
    let mut strides = layout.strides().to_vec();
    if layout.shape().rank() == 2 {
        strides.insert(0, 0);
    }
    let promoted_layout = Layout::new(promoted.clone(), strides, layout.offset())?;
    let mut dims = batch.dims().to_vec();
    dims.extend_from_slice(&promoted.dims()[promoted.rank() - 3..]);
    Ok(promoted_layout.broadcast_to(Shape::new(dims)?)?)
}

pub(crate) fn attention_metadata(
    plan: &AttentionPlan,
    query: &Layout,
    key: &Layout,
    value: &Layout,
    mask: Option<&Layout>,
) -> Result<Vec<u64>, CudaError> {
    let query = canonical_layout(query, &plan.query, &plan.batch)?;
    let key = canonical_layout(key, &plan.key, &plan.batch)?;
    let value = canonical_layout(value, &plan.value, &plan.batch)?;
    let mask = mask
        .map(|m| m.broadcast_to(plan.scores.clone()))
        .transpose()?;
    let r = query.shape().rank();
    let row_rank = r - 1;
    let mut metadata: Vec<u64> = Vec::with_capacity(row_rank * 5 + 11);
    metadata.extend(plan.canonical_output.dims()[..row_rank].iter().map(|&v| v as u64));
    for (layout, kv) in [(&query, false), (&key, true), (&value, true)] {
        metadata.extend(
            layout.strides()[..row_rank]
                .iter()
                .enumerate()
                .map(|(axis, &s)| {
                    if kv && axis == row_rank - 1 {
                        0
                    } else {
                        s as u64
                    }
                }),
        );
    }
    metadata
        .extend((0..row_rank).map(|axis| mask.as_ref().map_or(0, |m| m.strides()[axis] as u64)));
    metadata.extend([
        query.offset() as u64,
        key.offset() as u64,
        value.offset() as u64,
        mask.as_ref().map_or(0, |m| m.offset() as u64),
        query.strides()[r - 1] as u64,
        key.strides()[r - 2] as u64,
        key.strides()[r - 1] as u64,
        value.strides()[r - 2] as u64,
        value.strides()[r - 1] as u64,
        mask.as_ref().map_or(0, |m| m.strides()[r - 1] as u64),
        plan.group_size as u64,
    ]);
    Ok(metadata)
}

#[derive(Clone, Debug)]
pub(crate) struct AttentionDispatch {
    pub metadata: Vec<u64>,
    pub shape: Shape,
    required: [usize; 4],
    rows: u64,
    keys: u64,
    depth: u64,
    value_depth: u64,
    row_rank: u32,
    scale: f32,
    flags: u32,
    causal_offset: i32,
    groups: u32,
}
pub(crate) struct AttentionBuffers<'a, T> {
    pub query: &'a CudaSlice<T>,
    pub key: &'a CudaSlice<T>,
    pub value: &'a CudaSlice<T>,
}
pub(crate) struct AttentionInput<'a, T> {
    pub buffers: AttentionBuffers<'a, T>,
    pub dtype: Option<u32>,
}
pub(crate) enum AttentionMaskStorage<'a> {
    None,
    Keep(&'a CudaSlice<u32>),
    Additive(&'a CudaSlice<f32>),
}
pub(crate) struct AttentionOutputs<'a> {
    pub values: &'a mut CudaSlice<f32>,
    pub accumulator: &'a mut CudaSlice<f64>,
}
impl AttentionDispatch {
    pub fn new<T: DeviceRepr>(
        plan: &AttentionPlan,
        query: &Layout,
        key: &Layout,
        value: &Layout,
        mask: Option<(&Layout, bool)>,
        multiprocessors: u32,
    ) -> Result<Self, CudaError> {
        for layout in [query, key, value] {
            validate_logical_size::<T>(layout.shape())?;
        }
        validate_logical_size::<f64>(&plan.output)?;
        let groups =
            multiprocessors
                .checked_mul(8)
                .filter(|&n| n != 0)
                .ok_or(CudaError::InvalidInput(
                    "invalid CUDA attention launch groups",
                ))?;
        let rows = if plan.output.is_empty() {
            0
        } else {
            (plan.output.numel() / plan.value_depth) as u64
        };
        Ok(Self {
            metadata: attention_metadata(plan, query, key, value, mask.map(|m| m.0))?,
            shape: plan.output.clone(),
            required: [
                query.required_storage_len()?,
                key.required_storage_len()?,
                value.required_storage_len()?,
                mask.map(|m| m.0.required_storage_len())
                    .transpose()?
                    .unwrap_or(0),
            ],
            rows,
            keys: plan.keys as u64,
            depth: plan.depth as u64,
            value_depth: plan.value_depth as u64,
            row_rank: rank(&plan.canonical_output)? - 1,
            scale: plan.scale,
            flags: mask.map_or(0, |(_, keep)| if keep { 1 } else { 2 })
                | if plan.causal.is_some() { 4 } else { 0 },
            causal_offset: plan.causal.unwrap_or(0),
            groups,
        })
    }
    pub fn launch<T: DeviceRepr>(
        &self,
        rt: &CudaRuntime,
        input: AttentionInput<'_, T>,
        mask: AttentionMaskStorage<'_>,
        output: AttentionOutputs<'_>,
        metadata: &CudaSlice<u64>,
    ) -> Result<(), CudaError> {
        let b = input.buffers;
        if std::mem::size_of::<T>() != if input.dtype.is_some() { 2 } else { 4 }
            || input.dtype.is_some_and(|d| d > 1)
        {
            return Err(CudaError::Dtype);
        }
        let (flag, len) = match &mask {
            AttentionMaskStorage::None => (0, 0),
            AttentionMaskStorage::Keep(s) => (1, s.len()),
            AttentionMaskStorage::Additive(s) => (2, s.len()),
        };
        if flag != self.flags & 3 {
            return Err(CudaError::Dtype);
        }
        validate_storage(&[
            (b.query.len(), self.required[0]),
            (b.key.len(), self.required[1]),
            (b.value.len(), self.required[2]),
            (len, self.required[3]),
            (output.values.len(), self.shape.numel()),
            (output.accumulator.len(), self.shape.numel()),
            (metadata.len(), self.metadata.len()),
        ])?;
        if self.rows == 0 || self.keys == 0 {
            return Ok(());
        }
        let mut launch = rt
            .device
            .stream
            .launch_builder(&rt.attention.0[usize::from(input.dtype.is_some())]);
        launch.arg(b.query).arg(b.key).arg(b.value);
        match &mask {
            AttentionMaskStorage::Keep(s) => {
                launch.arg(*s);
            }
            _ => {
                launch.arg(b.query);
            }
        }
        match &mask {
            AttentionMaskStorage::Additive(s) => {
                launch.arg(*s);
            }
            _ => {
                launch.arg(b.query);
            }
        }
        unsafe {
            launch
                .arg(output.values)
                .arg(output.accumulator)
                .arg(metadata)
                .arg(&self.rows)
                .arg(&self.keys)
                .arg(&self.depth)
                .arg(&self.value_depth)
                .arg(&self.row_rank)
                .arg(&self.scale)
                .arg(&self.flags)
                .arg(&self.causal_offset);
            if let Some(dtype) = input.dtype.as_ref() {
                launch.arg(dtype);
            }
            launch.launch(LaunchConfig {
                grid_dim: (self.rows.min(self.groups as u64) as u32, 1, 1),
                block_dim: (256, 1, 1),
                shared_mem_bytes: 0,
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tensor_core::AttentionOptions;
    fn dense(d: &[usize]) -> Layout {
        Layout::contiguous(Shape::new(d.to_vec()).unwrap()).unwrap()
    }
    #[test]
    fn attention_reserves_real_f64_width_and_valid_launch_groups() {
        let q = dense(&[1, 1]);
        let k = dense(&[1, 1]);
        let v = dense(&[1, usize::MAX / 8 + 1]);
        let plan = AttentionPlan::new(
            q.shape(),
            k.shape(),
            v.shape(),
            None,
            AttentionOptions::default(),
        )
        .unwrap();
        assert!(AttentionDispatch::new::<u16>(&plan, &q, &k, &v, None, 1).is_err());
        let v = dense(&[1, 3]);
        let plan = AttentionPlan::new(
            q.shape(),
            k.shape(),
            v.shape(),
            None,
            AttentionOptions::default(),
        )
        .unwrap();
        for groups in [0, u32::MAX] {
            assert!(AttentionDispatch::new::<f32>(&plan, &q, &k, &v, None, groups).is_err());
        }
        let p = AttentionDispatch::new::<f32>(&plan, &q, &k, &v, None, 1).unwrap();
        assert_eq!((p.rows, p.value_depth, p.required), (1, 3, [1, 1, 3, 0]));
    }
}
