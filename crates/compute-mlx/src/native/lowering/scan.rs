//! Shared resident scan recipes. Low inputs are decoded by the existing Metal
//! kernels; only block totals and final prefixes use f32 storage.
use super::{indexing, *};
use crate::native::low_kernels::{self, WIDTH, reduction_launch};
use tensor_core::{ReduceOp, ScanOptions};

/// Native scan keeps the input dtype, including the existing generic eager API.
/// Typed compiled entry points validate f32/u32 before recording this recipe.
pub(in crate::native) fn scan<L: Lowering>(
    graph: &mut L,
    input: L::Value,
    axis: usize,
    options: ScanOptions,
) -> Result<L::Value, MlxError> {
    let spec = graph.spec(&input)?;
    spec.shape.validate_axes(&[axis])?;
    let axis = i32::try_from(axis).map_err(|_| MlxError::TooLarge)?;
    graph.native(
        NativeOp::Scan {
            axis,
            inclusive: options.inclusive,
            reverse: options.reverse,
        },
        &[input],
        spec,
    )
}

pub(in crate::native) fn scan_low_f32<L: Lowering>(
    graph: &mut L,
    input: L::Value,
    axis: usize,
    options: ScanOptions,
) -> Result<L::Value, MlxError> {
    let spec = graph.spec(&input)?;
    if !matches!(spec.dtype, MlxDtype::F16 | MlxDtype::Bf16) {
        return Err(MlxError::Dtype);
    }
    spec.shape.validate_axes(&[axis])?;
    if spec.shape.is_empty() {
        return indexing::zeros(graph, spec.shape, MlxDtype::F32);
    }
    let rank = spec.shape.rank();
    let order: Vec<_> = (0..rank).filter(|&a| a != axis).chain([axis]).collect();
    let mut inverse = vec![0; rank];
    for (position, &original) in order.iter().enumerate() {
        inverse[original] = position;
    }
    let permuted = indexing::permute(graph, input, &order)?;
    let permuted_shape = spec.shape.permute(&order)?;
    let count = spec.shape.dims()[axis];
    let rows = spec.shape.numel() / count;
    let parts = count.div_ceil(WIDTH);
    let params = graph.constant_u32(
        Shape::new(vec![4])?,
        &[
            u32::try_from(rank - 1).map_err(|_| MlxError::TooLarge)?,
            count as u32,
            (count as u64 >> 32) as u32,
            u32::try_from(parts).map_err(|_| MlxError::TooLarge)?,
        ],
    )?;
    let carry = if parts == 1 {
        // A vector dummy is required: custom scalar arguments are generated as
        // values, whereas the kernel type-checks carry[group] in every variant.
        indexing::zeros(graph, Shape::new(vec![1])?, MlxDtype::F32)?
    } else {
        let mut dims = permuted_shape.dims().to_vec();
        dims[rank - 1] = parts;
        let totals = graph.metal(
            low_kernels::key(
                low_kernels::reduction_source(ReduceOp::Sum, true, false, options.reverse),
                &["inp", "params"],
            ),
            &[permuted.clone(), params.clone()],
            TensorSpec {
                shape: Shape::new(dims)?,
                dtype: MlxDtype::F32,
            },
            spec.dtype == MlxDtype::Bf16,
            reduction_launch(rows, parts),
        )?;
        // Totals already follow traversal order, also for reversed input.
        scan(
            graph,
            totals,
            rank - 1,
            ScanOptions {
                inclusive: false,
                reverse: false,
            },
        )?
    };
    let source = include_str!("../../metal/low_scan.metal")
        .replace("REVERSE", if options.reverse { "true" } else { "false" })
        .replace(
            "INCLUSIVE",
            if options.inclusive { "true" } else { "false" },
        )
        .replace("CARRY", if parts > 1 { "true" } else { "false" });
    let prefix = graph.metal(
        low_kernels::key(source, &["inp", "params", "carry"]),
        &[permuted, params, carry],
        TensorSpec {
            shape: permuted_shape,
            dtype: MlxDtype::F32,
        },
        spec.dtype == MlxDtype::Bf16,
        reduction_launch(rows, parts),
    )?;
    indexing::permute(graph, prefix, &inverse)
}
