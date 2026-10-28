use crate::{
    ConvOptions, ConvPlan, HasLowDtype, TensorConvBackend, TensorError, TensorLowBackend,
    low_precision::check_low_dtypes,
};

/// Direct low-storage convolution with f32 accumulation. Both operands have
/// matching f16/BF16 storage, including empty operations. Implementations load
/// low operands directly; they do not expand whole inputs or weights to f32.
/// Bounded tiles/partials and output-sized accumulators are allowed.
///
/// Geometry and finite f32 arithmetic follow `TensorConvBackend`. Normal f32
/// products of BF16 subnormal inputs and large finite partners must survive;
/// other arithmetic underflow follows backend f32 limits. `conv_low_f32`
/// returns the f32 result before low rounding. `conv_low` rounds once to the
/// input dtype, nearest-even, with the same conversion contract as casts.
pub trait TensorLowConvBackend: TensorLowBackend + TensorConvBackend {
    fn conv_low_f32(
        &self,
        input: &Self::LowTensor,
        weight: &Self::LowTensor,
        options: &ConvOptions,
    ) -> Result<Self::Tensor, Self::Error>;

    fn conv_low(
        &self,
        input: &Self::LowTensor,
        weight: &Self::LowTensor,
        options: &ConvOptions,
    ) -> Result<Self::LowTensor, Self::Error> {
        let result = self.conv_low_f32(input, weight, options)?;
        self.cast_to_low(&result, input.low_dtype())
    }
}

/// Validate storage types before geometry or empty shortcuts. Runtime owner
/// checks remain the backend's responsibility.
pub fn low_convolution_plan(
    input: &impl HasLowDtype,
    weight: &impl HasLowDtype,
    options: &ConvOptions,
) -> Result<ConvPlan, TensorError> {
    check_low_dtypes(input, weight)?;
    ConvPlan::new(input.shape(), weight.shape(), options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HasShape, LowDtype, Shape};
    struct Low(Shape, LowDtype);
    impl HasShape for Low {
        fn shape(&self) -> &Shape {
            &self.0
        }
    }
    impl HasLowDtype for Low {
        fn low_dtype(&self) -> LowDtype {
            self.1
        }
    }
    #[test]
    fn dtype_mismatch_precedes_empty_and_invalid_geometry() {
        let a = Low(Shape::new(vec![0, 0, 3]).unwrap(), LowDtype::F16);
        let b = Low(Shape::new(vec![0, 0, 1]).unwrap(), LowDtype::Bf16);
        assert!(matches!(
            low_convolution_plan(&a, &b, &ConvOptions::new(1)),
            Err(TensorError::LowDtypeMismatch { .. })
        ));
        assert!(matches!(
            low_convolution_plan(&a, &b, &ConvOptions::new(3)),
            Err(TensorError::LowDtypeMismatch { .. })
        ));
        let b = Low(b.0, LowDtype::F16);
        assert_eq!(
            low_convolution_plan(&a, &b, &ConvOptions::new(1))
                .unwrap()
                .output
                .dims(),
            [0, 0, 3]
        );
    }
}
