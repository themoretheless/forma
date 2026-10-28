//! Stable fixed-capacity compaction shared by eager and compiled graphs.
use super::{indexing::*, scan, *};
use tensor_core::{Compacted, ScanOptions, compact_shape};

pub(in crate::native) fn compact<L: Lowering>(
    graph: &mut L,
    input: L::Value,
    mask: L::Value,
) -> Result<Compacted<L::Value, L::Value>, MlxError> {
    let input_spec = graph.spec(&input)?;
    let mask_spec = require(graph, &mask, MlxDtype::U32)?;
    let shape = compact_shape(&input_spec.shape, &mask_spec.shape)?;
    // Flattened native dimensions and arange lengths must fit i32.
    MlxBackend::dimensions(&shape)?;
    let empty = zeros(graph, shape.clone(), input_spec.dtype)?;
    if shape.is_empty() {
        return Ok(Compacted {
            values: empty,
            count: zeros(graph, Shape::new(vec![])?, MlxDtype::U32)?,
        });
    }
    let mask = broadcast(graph, mask, input_spec.shape)?;
    let mask = reshape(graph, mask, shape.clone())?;
    let zero = scalar_u32(graph, 0)?;
    let flags = compare(graph, mask, zero, CompareOp::NotEqual)?;
    let prefix = scan::scan(
        graph,
        flags.clone(),
        0,
        ScanOptions {
            inclusive: false,
            reverse: false,
        },
    )?;
    let count = sum_u32(graph, flags.clone(), &[0], false)?;
    let indices = graph.native(
        NativeOp::ArangeU32(shape.numel()),
        &[],
        TensorSpec {
            shape: shape.clone(),
            dtype: MlxDtype::U32,
        },
    )?;
    let rejected_before = binary_u32(graph, indices, prefix.clone(), BinaryOp::Subtract)?;
    let rejected_destination = binary_u32(graph, count.clone(), rejected_before, BinaryOp::Add)?;
    let destinations = select(graph, flags.clone(), prefix, rejected_destination)?;
    let input = reshape(graph, input, shape.clone())?;
    let updates = select(graph, flags, input, empty.clone())?;
    // Selected elements map bijectively to [0,count), rejected elements to
    // [count,n). Every destination is written once, including the zero tail.
    // Low selection and native movement preserve raw low payloads unchanged.
    let values = graph.native(
        NativeOp::PutAlongAxis(0),
        &[empty, destinations, updates],
        TensorSpec {
            shape,
            dtype: input_spec.dtype,
        },
    )?;
    Ok(Compacted { values, count })
}
