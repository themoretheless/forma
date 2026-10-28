//! Checked reduction parameters shared by eager and prepared execution.
use crate::{
    CudaError, CudaRuntime,
    runtime::{layout_metadata, rank},
};
use gpu_compute::cuda::{
    CudaFunction, CudaSlice, LaunchConfig, PushKernelArg,
    cudarc::driver::{DeviceRepr, ValidAsZeroBits},
};
use tensor_core::{Layout, ReduceOp, Shape, reduction_shape};

#[derive(Clone, Debug)]
enum Traversal {
    All {
        count: u64,
        rank: u32,
    },
    Axes {
        count: u64,
        output_rank: u32,
        reduce_rank: u32,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct ReductionPass {
    pub metadata: Vec<u64>,
    pub output_shape: Shape,
    traversal: Traversal,
    offset: u64,
    operation: u32,
    groups: u32,
    input_len: usize,
}

pub(crate) struct LoadedReduction<'a> {
    pub function: &'a CudaFunction,
    pub dtype: Option<u32>,
}

fn group_limit(multiprocessors: u32) -> Result<usize, CudaError> {
    multiprocessors
        .checked_mul(8)
        .filter(|&n| n != 0)
        .map(|n| n as usize)
        .ok_or(CudaError::InvalidInput(
            "invalid reduction multiprocessor count",
        ))
}

impl ReductionPass {
    pub fn axes(
        op: ReduceOp,
        input: &Layout,
        axes: &[usize],
        output_shape: Shape,
        multiprocessors: u32,
    ) -> Result<Self, CudaError> {
        let expected = reduction_shape(op, input.shape(), axes, false)?;
        if input.shape().is_empty()
            || output_shape.is_empty()
            || axes.is_empty()
            || expected.numel() != output_shape.numel()
        {
            return Err(CudaError::InvalidInput(
                "axis dispatch requires a nonempty contraction and matching output",
            ));
        }
        let rank = rank(input.shape())?;
        let reduce_rank = u32::try_from(axes.len())
            .map_err(|_| CudaError::InvalidInput("CUDA reduction rank exceeds u32::MAX"))?;
        let groups = output_shape
            .numel()
            .min(group_limit(multiprocessors)?)
            .max(1) as u32;
        let input_rank = input.shape().rank();
        let mut remaining = Vec::with_capacity(input_rank.saturating_sub(axes.len()));
        for axis in 0..input_rank {
            if !axes.contains(&axis) {
                remaining.push(axis);
            }
        }
        let mut metadata = Vec::with_capacity(input_rank * 2);
        let dims = input.shape().dims();
        let strides = input.strides();
        metadata.extend(remaining.iter().map(|&axis| dims[axis] as u64));
        metadata.extend(remaining.iter().map(|&axis| strides[axis] as u64));
        metadata.extend(axes.iter().map(|&axis| dims[axis] as u64));
        metadata.extend(axes.iter().map(|&axis| strides[axis] as u64));
        Ok(Self {
            metadata,
            traversal: Traversal::Axes {
                count: (input.shape().numel() / output_shape.numel()) as u64,
                output_rank: rank - reduce_rank,
                reduce_rank,
            },
            output_shape,
            offset: input.offset() as u64,
            operation: op as u32,
            groups,
            input_len: input.required_storage_len()?,
        })
    }

    pub fn all(op: ReduceOp, input: &Layout, multiprocessors: u32) -> Result<Self, CudaError> {
        if input.shape().is_empty() {
            return Err(CudaError::InvalidInput(
                "full reduction dispatch requires nonempty input",
            ));
        }
        let count = input.shape().numel();
        let groups = count
            .div_ceil(256 * 8)
            .min(group_limit(multiprocessors)?)
            .max(1);
        Ok(Self {
            metadata: layout_metadata(input),
            output_shape: Shape::new(vec![groups])?,
            traversal: Traversal::All {
                count: count as u64,
                rank: rank(input.shape())?,
            },
            offset: input.offset() as u64,
            operation: op as u32,
            groups: groups as u32,
            input_len: input.required_storage_len()?,
        })
    }

    pub fn is_all(&self) -> bool {
        matches!(self.traversal, Traversal::All { .. })
    }

    /// Raw storage arguments use cudarc's normal read/write dependency tracking.
    /// Immutable metadata must have been uploaded from this pass during preparation.
    pub fn launch<I: DeviceRepr, O: DeviceRepr + ValidAsZeroBits>(
        &self,
        runtime: &CudaRuntime,
        kernel: LoadedReduction<'_>,
        input: &CudaSlice<I>,
        output: &mut CudaSlice<O>,
        metadata: &CudaSlice<u64>,
    ) -> Result<(), CudaError> {
        if input.len() < self.input_len
            || output.len() < self.output_shape.numel()
            || metadata.len() < self.metadata.len()
        {
            return Err(CudaError::InvalidInput(
                "reduction dispatch storage is too small",
            ));
        }
        let mut builder = runtime.device.stream.launch_builder(kernel.function);
        builder.arg(input).arg(output).arg(metadata);
        let output_count = self.output_shape.numel() as u64;
        match &self.traversal {
            Traversal::All { count, rank } => {
                builder.arg(count).arg(rank);
            }
            Traversal::Axes {
                count,
                output_rank,
                reduce_rank,
            } => {
                builder
                    .arg(&output_count)
                    .arg(count)
                    .arg(output_rank)
                    .arg(reduce_rank);
            }
        }
        builder.arg(&self.offset).arg(&self.operation);
        if let Some(dtype) = kernel.dtype.as_ref() {
            builder.arg(dtype);
        }
        // The descriptor checks source addressing and output count. The kernel
        // specialization is internal and shares the eager path's type policy.
        unsafe {
            builder.launch(LaunchConfig {
                grid_dim: (self.groups, 1, 1),
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
    fn shape(d: &[usize]) -> Shape {
        Shape::new(d.to_vec()).unwrap()
    }

    #[test]
    fn axes_preserve_broadcast_order_offset_and_kept_shape() {
        let input = Layout::new(shape(&[3, 5, 2]), vec![1, 0, 3], 7).unwrap();
        let pass =
            ReductionPass::axes(ReduceOp::Sum, &input, &[2, 0], shape(&[1, 5, 1]), 4).unwrap();
        assert_eq!(pass.metadata, [5, 0, 2, 3, 3, 1]);
        assert_eq!(pass.offset, 7);
        assert_eq!(pass.input_len, 13);
        assert_eq!(pass.groups, 5);
        assert!(!pass.is_all());
        assert!(matches!(
            pass.traversal,
            Traversal::Axes {
                count: 6,
                output_rank: 1,
                reduce_rank: 2
            }
        ));
    }

    #[test]
    fn full_pass_hierarchy_bounds_and_strided_first_load() {
        for (n, expected) in [(1, 1), (2048, 1), (2049, 2), (131077, 65)] {
            let input = Layout::new(shape(&[n]), vec![2], 3).unwrap();
            let first = ReductionPass::all(ReduceOp::Max, &input, 40).unwrap();
            assert!(first.is_all());
            assert_eq!(first.metadata, [n as u64, 2]);
            assert_eq!(first.output_shape, shape(&[expected]));
            assert_eq!(first.input_len, 3 + 2 * (n - 1) + 1);
            let next = Layout::contiguous(first.output_shape).unwrap();
            let final_pass = ReductionPass::all(ReduceOp::Max, &next, 40).unwrap();
            assert_eq!(final_pass.output_shape, shape(&[1]));
        }
        let input = Layout::new(shape(&[usize::MAX]), vec![0], 0).unwrap();
        assert_eq!(
            ReductionPass::all(ReduceOp::Sum, &input, 2).unwrap().groups,
            16
        );
    }

    #[test]
    fn descriptors_reject_invalid_axes_empty_contractions_and_device_limits() {
        let input = Layout::contiguous(shape(&[2, 3])).unwrap();
        for axes in [vec![], vec![2], vec![0, 0]] {
            assert!(ReductionPass::axes(ReduceOp::Sum, &input, &axes, shape(&[2]), 1).is_err());
        }
        assert!(ReductionPass::axes(ReduceOp::Sum, &input, &[1], shape(&[3]), 1).is_err());
        let empty = Layout::contiguous(shape(&[2, 0])).unwrap();
        assert!(ReductionPass::all(ReduceOp::Sum, &empty, 1).is_err());
        assert!(ReductionPass::axes(ReduceOp::Sum, &empty, &[1], shape(&[2]), 1).is_err());
        for mp in [0, u32::MAX] {
            assert!(ReductionPass::all(ReduceOp::Sum, &input, mp).is_err());
        }
    }
}
