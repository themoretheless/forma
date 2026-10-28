use tensor_core::{
    AttentionOptions, HasLowDtype, HasShape, LowDtype, Shape, TensorError, low_attention_plan,
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
fn low_attention_preserves_grouped_broadcast_geometry_and_mask_precision() {
    let dtype = LowDtype::Bf16;
    let q = input(&[2, 1, 6, 3, 5], dtype);
    let k = input(&[1, 4, 2, 7, 5], dtype);
    let v = input(&[4, 2, 7, 9], dtype);
    let mask = Shape::new(vec![3, 7]).unwrap();
    let plan = low_attention_plan(
        &q,
        &k,
        &v,
        Some(&mask),
        AttentionOptions {
            scale: Some(-0.25),
            causal: Some(-2),
        },
    )
    .unwrap();
    assert_eq!(plan.output.dims(), &[2, 4, 6, 3, 9]);
    assert_eq!(plan.group_size, 3);
    assert_eq!(plan.scale, -0.25);
    assert_eq!(plan.causal, Some(-2));
}

#[test]
fn low_attention_checks_both_other_dtypes_before_empty_execution() {
    let options = AttentionOptions::default();
    for count in [0, 3] {
        let q = input(&[count, 5], LowDtype::F16);
        let k = input(&[7, 5], LowDtype::F16);
        let v = input(&[7, 9], LowDtype::F16);
        assert_eq!(
            low_attention_plan(&q, &k, &v, None, options)
                .unwrap()
                .output
                .dims(),
            &[count, 9]
        );
        for (key, value) in [
            (
                input(&[7, 5], LowDtype::Bf16),
                input(&[7, 9], LowDtype::F16),
            ),
            (
                input(&[7, 5], LowDtype::F16),
                input(&[7, 9], LowDtype::Bf16),
            ),
        ] {
            assert!(matches!(
                low_attention_plan(&q, &key, &value, None, options),
                Err(TensorError::LowDtypeMismatch { .. })
            ));
        }
        assert!(low_attention_plan(&q, &input(&[7, 4], LowDtype::F16), &v, None, options).is_err());
        assert!(
            low_attention_plan(
                &q,
                &k,
                &v,
                None,
                AttentionOptions {
                    scale: Some(f32::NAN),
                    causal: None
                }
            )
            .is_err()
        );
    }
}
