use super::*;
use tensor_core::LowDtype;

/// Typed graph transport keeps low arrays behind MlxLowTensor's checked API.
#[derive(Clone, Copy)]
pub enum MlxProgramInput<'a> {
    Tensor(&'a MlxTensor),
    Low(&'a MlxLowTensor),
}
impl<'a> From<&'a MlxTensor> for MlxProgramInput<'a> {
    fn from(t: &'a MlxTensor) -> Self {
        Self::Tensor(t)
    }
}
impl<'a> From<&'a MlxLowTensor> for MlxProgramInput<'a> {
    fn from(t: &'a MlxLowTensor) -> Self {
        Self::Low(t)
    }
}

#[derive(Clone)]
pub enum MlxProgramOutput {
    Tensor(MlxTensor),
    Low(MlxLowTensor),
}
impl MlxProgramOutput {
    pub fn dtype(&self) -> MlxDtype {
        match self {
            Self::Tensor(t) => t.dtype,
            Self::Low(t) => t.dtype.into(),
        }
    }
    pub fn as_tensor(&self) -> Result<&MlxTensor, MlxError> {
        match self {
            Self::Tensor(t) => Ok(t),
            _ => Err(MlxError::Dtype),
        }
    }
    pub fn as_low(&self) -> Result<&MlxLowTensor, MlxError> {
        match self {
            Self::Low(t) => Ok(t),
            _ => Err(MlxError::Dtype),
        }
    }
    pub fn into_tensor(self) -> Result<MlxTensor, MlxError> {
        match self {
            Self::Tensor(t) => Ok(t),
            _ => Err(MlxError::Dtype),
        }
    }
    pub fn into_low(self) -> Result<MlxLowTensor, MlxError> {
        match self {
            Self::Low(t) => Ok(t),
            _ => Err(MlxError::Dtype),
        }
    }
}
impl HasShape for MlxProgramOutput {
    fn shape(&self) -> &Shape {
        match self {
            Self::Tensor(t) => &t.shape,
            Self::Low(t) => t.shape(),
        }
    }
}
impl MlxCompiledProgram {
    pub fn run_typed(
        &self,
        inputs: &[MlxProgramInput<'_>],
    ) -> Result<Vec<MlxProgramOutput>, MlxError> {
        let tensors: Vec<_> = inputs
            .iter()
            .map(|input| match input {
                MlxProgramInput::Tensor(t) => *t,
                MlxProgramInput::Low(t) => &t.tensor,
            })
            .collect();
        Ok(self
            .run_raw(&tensors)?
            .into_iter()
            .map(|tensor| match tensor.dtype {
                MlxDtype::F32 | MlxDtype::U32 => MlxProgramOutput::Tensor(tensor),
                MlxDtype::F16 => MlxProgramOutput::Low(MlxLowTensor {
                    tensor,
                    dtype: LowDtype::F16,
                }),
                MlxDtype::Bf16 => MlxProgramOutput::Low(MlxLowTensor {
                    tensor,
                    dtype: LowDtype::Bf16,
                }),
            })
            .collect())
    }
}
