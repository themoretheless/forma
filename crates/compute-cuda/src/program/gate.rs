use crate::CudaError;

/// The same gate is used by native submission and CPU boundary tests. Validation
/// errors never grant a permit; execution errors permanently poison the state.
#[derive(Default)]
pub(super) struct ExecutionState {
    pub poisoned: bool,
}
pub(super) struct Permit<'a>(&'a mut ExecutionState);
impl ExecutionState {
    pub fn check(&self) -> Result<(), CudaError> {
        if self.poisoned {
            Err(CudaError::ProgramPoisoned)
        } else {
            Ok(())
        }
    }
    pub fn begin(
        &mut self,
        validate: impl FnOnce() -> Result<(), CudaError>,
    ) -> Result<Permit<'_>, CudaError> {
        self.check()?;
        validate()?;
        Ok(Permit(self))
    }
}
impl Permit<'_> {
    pub fn execute(self, enqueue: impl FnOnce() -> Result<(), CudaError>) -> Result<(), CudaError> {
        // Arm before calling into driver/library code. Only a normal successful
        // return makes the state reusable; errors and caught unwinds retain poison.
        self.0.poisoned = true;
        enqueue()?;
        self.0.poisoned = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::plan::{CudaDtype, TensorSpec};
    use super::super::validation::{self, Binding};
    use super::*;
    use std::cell::Cell;
    use tensor_core::{Layout, MatmulPrecision, Shape};
    #[test]
    fn late_invalid_output_cannot_enqueue_and_validation_errors_are_reusable() {
        let mut state = ExecutionState::default();
        let launches = Cell::new(0);
        let layout = Layout::contiguous(Shape::new(vec![3]).unwrap()).unwrap();
        let expected = vec![
            TensorSpec {
                layout: layout.clone(),
                dtype: CudaDtype::F32
            };
            2
        ];
        let binding = |id| Binding {
            layout: &layout,
            dtype: CudaDtype::F32,
            storage_len: 3,
            allocation: id,
            unique: true,
        };
        let result = state
            .begin(|| validation::outputs(&expected, &[binding(1)], &[binding(2), binding(1)]));
        assert!(result.is_err());
        assert_eq!(launches.get(), 0);
        assert!(!state.poisoned);
        state
            .begin(|| validation::outputs(&expected, &[binding(1)], &[binding(2), binding(3)]))
            .unwrap()
            .execute(|| {
                launches.set(launches.get() + 1);
                Ok(())
            })
            .unwrap();
        assert_eq!(launches.get(), 1);
    }
    #[test]
    fn late_precision_failure_blocks_enqueue_without_poisoning() {
        let mut state = ExecutionState::default();
        let launches = Cell::new(0);
        let precisions = [MatmulPrecision::F32, MatmulPrecision::AllowTf32];
        let result = state.begin(|| {
            validation::precisions(&precisions, |p| {
                if p == MatmulPrecision::AllowTf32 {
                    Err(CudaError::UnsupportedPrecision("TF32 disabled"))
                } else {
                    Ok(())
                }
            })
        });
        assert!(result.is_err());
        assert_eq!(launches.get(), 0);
        assert!(!state.poisoned);
        state
            .begin(|| validation::precisions(&precisions, |_| Ok(())))
            .unwrap()
            .execute(|| {
                launches.set(1);
                Ok(())
            })
            .unwrap();
        assert_eq!(launches.get(), 1);
    }
    #[test]
    fn an_enqueue_error_permanently_blocks_future_validation_and_launches() {
        let mut state = ExecutionState::default();
        let launches = Cell::new(0);
        let validations = Cell::new(0);
        state
            .begin(|| Ok(()))
            .unwrap()
            .execute(|| {
                launches.set(1);
                Err(CudaError::InvalidInput("mock driver enqueue error"))
            })
            .unwrap_err();
        assert!(state.poisoned);
        let result = state.begin(|| {
            validations.set(1);
            Ok(())
        });
        assert!(matches!(result, Err(CudaError::ProgramPoisoned)));
        assert_eq!((validations.get(), launches.get()), (0, 1));
    }
    #[test]
    fn late_dtype_failure_cannot_enqueue_or_poison_and_legacy_outputs_are_checked() {
        let mut state = ExecutionState::default();
        let layout = Layout::contiguous(Shape::new(vec![0]).unwrap()).unwrap();
        let expected = vec![
            TensorSpec {
                layout: layout.clone(),
                dtype: CudaDtype::F16
            };
            2
        ];
        let bind = |dtype, id| Binding {
            layout: &layout,
            dtype,
            storage_len: 1,
            allocation: id,
            unique: true,
        };
        assert!(
            state
                .begin(|| validation::outputs(
                    &expected,
                    &[],
                    &[bind(CudaDtype::F16, 1), bind(CudaDtype::Bf16, 2)]
                ))
                .is_err()
        );
        assert!(!state.poisoned);
        let launches = Cell::new(0);
        state
            .begin(|| {
                validation::outputs(
                    &expected,
                    &[],
                    &[bind(CudaDtype::F16, 1), bind(CudaDtype::F16, 2)],
                )
            })
            .unwrap()
            .execute(|| {
                launches.set(1);
                Ok(())
            })
            .unwrap();
        assert_eq!(launches.get(), 1);
        let f32spec = TensorSpec {
            layout: layout.clone(),
            dtype: CudaDtype::F32,
        };
        for dtype in [CudaDtype::U32, CudaDtype::F16, CudaDtype::Bf16] {
            let typed = TensorSpec {
                layout: layout.clone(),
                dtype,
            };
            assert!(
                validation::legacy_f32(
                    std::slice::from_ref(&f32spec),
                    std::slice::from_ref(&typed)
                )
                .is_err()
            );
            assert!(
                validation::legacy_f32(
                    std::slice::from_ref(&typed),
                    std::slice::from_ref(&f32spec)
                )
                .is_err()
            );
        }
        assert!(
            validation::legacy_f32(
                std::slice::from_ref(&f32spec),
                std::slice::from_ref(&f32spec)
            )
            .is_ok()
        );
    }
    #[test]
    fn enqueue_panic_poison_survives_catch_and_blocks_validation_and_replay() {
        let mut state = ExecutionState::default();
        let launches = Cell::new(0);
        let validations = Cell::new(0);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            state
                .begin(|| Ok(()))
                .unwrap()
                .execute(|| {
                    launches.set(launches.get() + 1);
                    panic!("injected panic after first enqueue");
                })
                .unwrap();
        }));
        assert!(result.is_err());
        assert!(state.poisoned);
        let later = state.begin(|| {
            validations.set(1);
            Ok(())
        });
        assert!(matches!(later, Err(CudaError::ProgramPoisoned)));
        assert_eq!((launches.get(), validations.get()), (1, 0));
    }
    #[test]
    fn validation_panic_does_not_poison_before_any_enqueue() {
        let mut state = ExecutionState::default();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = state.begin(|| panic!("validation failed before a permit existed"));
        }));
        assert!(result.is_err());
        assert!(!state.poisoned);
        state.begin(|| Ok(())).unwrap().execute(|| Ok(())).unwrap();
        assert!(!state.poisoned);
    }
}
