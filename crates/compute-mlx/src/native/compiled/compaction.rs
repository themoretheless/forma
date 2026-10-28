use super::*;
use crate::native::lowering::compaction;
use tensor_core::Compacted;

impl MlxProgramBuilder {
    /// Stable f32 selection with a resident u32 count and zero-filled tail.
    pub fn compact(
        &mut self,
        value: MlxValue,
        mask: MlxValue,
    ) -> Result<Compacted<MlxValue, MlxValue>, MlxError> {
        self.require(value, MlxDtype::F32)?;
        self.transaction(|graph| compaction::compact(graph, value, mask))
    }
    /// Stable unsigned selection; masks accept every nonzero bit pattern.
    pub fn compact_u32(
        &mut self,
        value: MlxValue,
        mask: MlxValue,
    ) -> Result<Compacted<MlxValue, MlxValue>, MlxError> {
        self.require(value, MlxDtype::U32)?;
        self.transaction(|graph| compaction::compact(graph, value, mask))
    }
    /// Raw low selection preserves NaN payloads, signed zero and subnormals.
    pub fn compact_low(
        &mut self,
        value: MlxValue,
        mask: MlxValue,
    ) -> Result<Compacted<MlxValue, MlxValue>, MlxError> {
        self.low(value)?;
        self.transaction(|graph| compaction::compact(graph, value, mask))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compact_values_and_count_roll_back_together_after_late_failure() {
        let backend = match MlxBackend::new_gpu() {
            Ok(b) => b,
            Err(error) => {
                assert!(std::env::var_os("COMPUTE_REQUIRE_MLX").is_none(), "{error}");
                return;
            }
        };
        let mut graph = backend.program();
        let input = graph
            .input_low(tensor_core::LowDtype::Bf16, Shape::new(vec![3]).unwrap())
            .unwrap();
        let mask = graph.input_u32(Shape::new(vec![3]).unwrap()).unwrap();
        let before = graph.nodes.len();
        let result: Result<Compacted<MlxValue, MlxValue>, MlxError> = graph.transaction(|graph| {
            let pair = graph.compact_low(input, mask)?;
            assert!(pair.values.index >= before && pair.count.index >= before);
            graph.reshape(pair.values, Shape::new(vec![4])?)?;
            Ok(pair)
        });
        assert!(result.is_err());
        assert_eq!(graph.nodes.len(), before);
        assert_eq!(graph.inputs.len(), 2);
        let next = graph.cast_to_f32(input).unwrap();
        assert_eq!(next.index, before);
    }
}
