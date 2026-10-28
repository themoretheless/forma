use tensor_core::{
    HasLowDtype, HasShape, LowDtype, Shape, TensorError, low_binary_shape, low_select_shape,
};
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
fn input(dims: &[usize], dtype: LowDtype) -> Input {
    Input(Shape::new(dims.to_vec()).unwrap(), dtype)
}

#[test]
fn low_binary_checks_dtype_and_broadcast_even_for_empty_outputs() {
    let a = input(&[2, 1, 3], LowDtype::F16);
    let b = input(&[4, 1], LowDtype::F16);
    assert_eq!(low_binary_shape(&a, &b).unwrap().dims(), &[2, 4, 3]);
    let empty = input(&[2, 0, 3], LowDtype::F16);
    let scalar = input(&[], LowDtype::F16);
    assert_eq!(
        low_binary_shape(&empty, &scalar).unwrap().dims(),
        &[2, 0, 3]
    );
    assert!(matches!(
        low_binary_shape(&empty, &input(&[], LowDtype::Bf16)),
        Err(TensorError::LowDtypeMismatch { .. })
    ));
    assert!(matches!(
        low_binary_shape(&a, &input(&[2], LowDtype::F16)),
        Err(TensorError::IncompatibleBroadcast { .. })
    ));
}

#[test]
fn low_selection_checks_types_and_broadcasts_all_three_shapes_together() {
    let yes = input(&[usize::MAX, 1, 1], LowDtype::Bf16);
    let no = input(&[1, 2, 1], LowDtype::Bf16);
    let mask = Shape::new(vec![1, 1, 0]).unwrap();
    assert_eq!(
        low_select_shape(&mask, &yes, &no).unwrap().dims(),
        &[usize::MAX, 2, 0]
    );
    assert!(matches!(
        low_binary_shape(&yes, &no),
        Err(TensorError::ShapeOverflow)
    ));
    assert!(matches!(
        low_select_shape(&mask, &yes, &input(&[1, 2, 1], LowDtype::F16)),
        Err(TensorError::LowDtypeMismatch { .. })
    ));
    assert!(
        low_select_shape(
            &Shape::new(vec![2]).unwrap(),
            &input(&[3], LowDtype::F16),
            &input(&[], LowDtype::F16)
        )
        .is_err()
    );
}
