use tensor_core::{
    Shape, TensorError, compact_shape, gather_shape, select_shape, validate_index_count,
};

fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}

#[test]
fn gather_inserts_index_axes_and_scalar_indices_remove_an_axis() {
    assert_eq!(
        gather_shape(&shape(&[2, 3, 4]), &shape(&[5, 6]), 1).unwrap(),
        shape(&[2, 5, 6, 4])
    );
    assert_eq!(
        gather_shape(&shape(&[2, 3, 4]), &shape(&[]), 1).unwrap(),
        shape(&[2, 4])
    );
    assert_eq!(
        gather_shape(&shape(&[2, 0, 4]), &shape(&[5]), 1).unwrap(),
        shape(&[2, 5, 4])
    );
    assert_eq!(
        gather_shape(&shape(&[2, 3, 4]), &shape(&[0, 6]), 1).unwrap(),
        shape(&[2, 0, 6, 4])
    );
    assert!(gather_shape(&shape(&[]), &shape(&[1]), 0).is_err());
    assert!(gather_shape(&shape(&[2]), &shape(&[1]), 1).is_err());
}

#[test]
fn selection_broadcasts_all_operands_but_compaction_preserves_input_capacity() {
    assert_eq!(
        select_shape(&shape(&[2, 1]), &shape(&[3]), &shape(&[])).unwrap(),
        shape(&[2, 3])
    );
    assert_eq!(
        select_shape(&shape(&[0, 1]), &shape(&[3]), &shape(&[1, 3])).unwrap(),
        shape(&[0, 3])
    );
    assert_eq!(
        compact_shape(&shape(&[2, 3]), &shape(&[3])).unwrap(),
        shape(&[6])
    );
    assert_eq!(
        compact_shape(&shape(&[]), &shape(&[])).unwrap(),
        shape(&[1])
    );
    assert_eq!(
        compact_shape(&shape(&[0, 3]), &shape(&[1, 3])).unwrap(),
        shape(&[0])
    );
    assert!(compact_shape(&shape(&[3]), &shape(&[2, 3])).is_err());
    assert!(select_shape(&shape(&[2]), &shape(&[3]), &shape(&[])).is_err());
}

#[test]
fn count_limits_are_checked_before_backend_allocation() {
    validate_index_count(&shape(&[u32::MAX as usize])).unwrap();
    if usize::BITS > 32 {
        let huge = shape(&[u32::MAX as usize + 1]);
        assert!(matches!(
            validate_index_count(&huge),
            Err(TensorError::IndexCountOverflow { .. })
        ));
        assert!(gather_shape(&shape(&[1]), &huge, 0).is_err());
        assert!(compact_shape(&huge, &shape(&[])).is_err());
    }
    validate_index_count(&shape(&[usize::MAX, 0])).unwrap();
}

#[test]
fn selection_checks_final_count_after_all_zero_axes_are_known() {
    let mask = shape(&[usize::MAX, 1, 1]);
    let yes = shape(&[1, 2, 1]);
    let no = shape(&[1, 1, 0]);
    assert_eq!(
        select_shape(&mask, &yes, &no).unwrap(),
        shape(&[usize::MAX, 2, 0])
    );
    assert_eq!(
        select_shape(&no, &yes, &mask).unwrap(),
        shape(&[usize::MAX, 2, 0])
    );
    assert!(select_shape(&mask, &yes, &shape(&[])).is_err());
}
