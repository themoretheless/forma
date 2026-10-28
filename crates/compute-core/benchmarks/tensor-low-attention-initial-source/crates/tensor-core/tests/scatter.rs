use tensor_core::{Shape, TensorError, scatter_updates_shape};

fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}

#[test]
fn scatter_uses_gather_expansion_and_accepts_broadcast_updates() {
    let base = shape(&[2, 5, 3]);
    let indices = shape(&[4, 2]);
    let expected = shape(&[2, 4, 2, 3]);
    for updates in [
        expected.clone(),
        shape(&[1, 4, 1, 3]),
        shape(&[3]),
        shape(&[]),
    ] {
        assert_eq!(
            scatter_updates_shape(&base, &indices, &updates, 1).unwrap(),
            expected
        );
    }
    assert_eq!(
        scatter_updates_shape(&base, &shape(&[]), &shape(&[2, 3]), 1).unwrap(),
        shape(&[2, 3])
    );
    assert_eq!(
        scatter_updates_shape(&shape(&[5]), &shape(&[]), &shape(&[]), 0).unwrap(),
        shape(&[])
    );
}

#[test]
fn scatter_preserves_empty_shapes_and_rejects_expansion() {
    assert_eq!(
        scatter_updates_shape(&shape(&[0, 5]), &shape(&[3]), &shape(&[]), 1).unwrap(),
        shape(&[0, 3])
    );
    assert_eq!(
        scatter_updates_shape(&shape(&[2, 0, 3]), &shape(&[5]), &shape(&[3]), 1).unwrap(),
        shape(&[2, 5, 3])
    );
    assert_eq!(
        scatter_updates_shape(&shape(&[2, 5, 3]), &shape(&[0]), &shape(&[]), 1).unwrap(),
        shape(&[2, 0, 3])
    );
    assert!(
        scatter_updates_shape(&shape(&[2, 5, 3]), &shape(&[1]), &shape(&[2, 4, 3]), 1).is_err()
    );
    assert!(scatter_updates_shape(&shape(&[5]), &shape(&[2]), &shape(&[1, 2]), 0).is_err());
    assert!(matches!(
        scatter_updates_shape(&shape(&[]), &shape(&[1]), &shape(&[]), 0),
        Err(TensorError::AxisOutOfBounds { .. })
    ));
}

#[test]
fn scatter_checks_count_and_expanded_update_overflow() {
    if let Some(count) = (u32::MAX as usize).checked_add(1) {
        let too_many = shape(&[count]);
        assert!(matches!(
            scatter_updates_shape(&shape(&[0]), &too_many, &shape(&[]), 0),
            Err(TensorError::IndexCountOverflow { .. })
        ));
    }
    // The empty base is representable; replacing its empty axis would require
    // an unrepresentable number of update elements, even for a scalar update.
    assert_eq!(
        scatter_updates_shape(&shape(&[usize::MAX, 0]), &shape(&[2]), &shape(&[]), 1),
        Err(TensorError::ShapeOverflow)
    );
}
