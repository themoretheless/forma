use tensor_core::{HasLowDtype, HasShape, LowDtype, Shape, TensorError, low_scatter_updates_shape};

struct Input(Shape, LowDtype);
impl HasShape for Input {
    fn shape(&self) -> &Shape {
        &self.0
    }
}
impl HasLowDtype for Input {
    fn low_dtype(&self) -> LowDtype {
        self.1
    }
}
fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}
fn input(dims: &[usize], dtype: LowDtype) -> Input {
    Input(shape(dims), dtype)
}

#[test]
fn low_scatter_preserves_axis_expansion_and_checks_storage_even_when_empty() {
    let base = input(&[2, 3, 4], LowDtype::Bf16);
    let updates = input(&[1, 5, 1, 4], LowDtype::Bf16);
    assert_eq!(
        low_scatter_updates_shape(&base, &shape(&[5, 6]), &updates, 1).unwrap(),
        shape(&[2, 5, 6, 4])
    );
    assert_eq!(
        low_scatter_updates_shape(&base, &shape(&[]), &input(&[4], LowDtype::Bf16), 1).unwrap(),
        shape(&[2, 4])
    );
    let base = input(&[0, 3], LowDtype::F16);
    assert_eq!(
        low_scatter_updates_shape(&base, &shape(&[4]), &input(&[], LowDtype::F16), 1).unwrap(),
        shape(&[0, 4])
    );
    assert!(matches!(
        low_scatter_updates_shape(&base, &shape(&[4]), &input(&[], LowDtype::Bf16), 1),
        Err(TensorError::LowDtypeMismatch { .. })
    ));
    assert!(
        low_scatter_updates_shape(&base, &shape(&[4]), &input(&[2, 4], LowDtype::F16), 1).is_err()
    );
    assert!(
        low_scatter_updates_shape(
            &input(&[], LowDtype::F16),
            &shape(&[]),
            &input(&[], LowDtype::F16),
            0
        )
        .is_err()
    );
}

#[test]
fn low_scatter_rejects_unrepresentable_index_counts_before_execution() {
    if usize::BITS > 32 {
        assert!(matches!(
            low_scatter_updates_shape(
                &input(&[1], LowDtype::F16),
                &shape(&[u32::MAX as usize + 1]),
                &input(&[], LowDtype::F16),
                0
            ),
            Err(TensorError::IndexCountOverflow { .. })
        ));
    }
    assert!(matches!(
        low_scatter_updates_shape(
            &input(&[usize::MAX, 2, 0], LowDtype::Bf16),
            &shape(&[3]),
            &input(&[], LowDtype::Bf16),
            2
        ),
        Err(TensorError::ShapeOverflow)
    ));
}
