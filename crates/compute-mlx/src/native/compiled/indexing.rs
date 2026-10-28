use super::*;
use crate::native::lowering::indexing;
use tensor_core::Gathered;

impl MlxProgramBuilder {
    pub fn compare_low(
        &mut self,
        left: MlxValue,
        right: MlxValue,
        op: CompareOp,
    ) -> Result<MlxValue, MlxError> {
        self.low(left)?;
        self.low(right)?;
        self.transaction(|g| indexing::compare(g, left, right, op))
    }
    pub fn select(
        &mut self,
        mask: MlxValue,
        yes: MlxValue,
        no: MlxValue,
    ) -> Result<MlxValue, MlxError> {
        self.require(yes, MlxDtype::F32)?;
        self.require(no, MlxDtype::F32)?;
        self.transaction(|g| indexing::select(g, mask, yes, no))
    }
    pub fn select_low(
        &mut self,
        mask: MlxValue,
        yes: MlxValue,
        no: MlxValue,
    ) -> Result<MlxValue, MlxError> {
        self.low(yes)?;
        self.low(no)?;
        self.transaction(|g| indexing::select(g, mask, yes, no))
    }
    pub fn gather(
        &mut self,
        value: MlxValue,
        indices: MlxValue,
        axis: usize,
    ) -> Result<Gathered<MlxValue, MlxValue>, MlxError> {
        self.require(value, MlxDtype::F32)?;
        self.transaction(|g| indexing::gather(g, value, indices, axis))
    }
    pub fn gather_u32(
        &mut self,
        value: MlxValue,
        indices: MlxValue,
        axis: usize,
    ) -> Result<Gathered<MlxValue, MlxValue>, MlxError> {
        self.require(value, MlxDtype::U32)?;
        self.transaction(|g| indexing::gather(g, value, indices, axis))
    }
    pub fn gather_low(
        &mut self,
        value: MlxValue,
        indices: MlxValue,
        axis: usize,
    ) -> Result<Gathered<MlxValue, MlxValue>, MlxError> {
        self.low(value)?;
        self.transaction(|g| indexing::gather(g, value, indices, axis))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gather_values_and_count_roll_back_together_after_late_failure() {
        let backend = match MlxBackend::new_gpu() {
            Ok(b) => b,
            Err(error) => {
                assert!(std::env::var_os("COMPUTE_REQUIRE_MLX").is_none(), "{error}");
                return;
            }
        };
        let mut graph = backend.program();
        let x = graph
            .input_low(tensor_core::LowDtype::Bf16, Shape::new(vec![2, 3]).unwrap())
            .unwrap();
        let i = graph.input_u32(Shape::new(vec![2]).unwrap()).unwrap();
        let before = graph.nodes.len();
        let result: Result<Gathered<MlxValue, MlxValue>, MlxError> = graph.transaction(|g| {
            let pair = g.gather_low(x, i, 1)?;
            assert!(pair.values.index >= before && pair.invalid_count.index >= before);
            g.reshape(pair.invalid_count, Shape::new(vec![2])?)?;
            Ok(pair)
        });
        assert!(result.is_err());
        assert_eq!(graph.nodes.len(), before);
        assert_eq!(graph.inputs.len(), 2);
        let value = graph.cast_to_f32(x).unwrap();
        assert_eq!(value.index, before);
    }
}
