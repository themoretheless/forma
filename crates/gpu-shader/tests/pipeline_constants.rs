//! Pipeline-overridable constants are validated before they reach the Metal compiler.

use gpu_shader::back::msl::{PipelineConstantError, override_macro, with_pipeline_constants};

/// What the backend emits for `override gain: f32 = 1.0;`.
fn module() -> String {
    let mac = override_macro("gain");
    format!("#ifndef {mac}\n#define {mac} 1.0\n#endif\nconstant float gain = float({mac});\n")
}

#[test]
fn a_declared_override_is_defined_ahead_of_the_source() {
    let msl = module();
    let applied = with_pipeline_constants(&msl, &[("gain", 2.0)]).unwrap();
    assert_eq!(applied, format!("#define _WGPU_OVERRIDE_gain 2.0\n{msl}"));
    assert_eq!(with_pipeline_constants(&msl, &[]).unwrap(), msl);
}

#[test]
fn non_finite_values_are_rejected() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            with_pipeline_constants(&module(), &[("gain", value)]),
            Err(PipelineConstantError::NonFinite("gain".into()))
        );
    }
}

#[test]
fn names_the_module_does_not_declare_are_rejected() {
    assert_eq!(
        with_pipeline_constants(&module(), &[("gian", 2.0)]),
        Err(PipelineConstantError::Unknown("gian".into()))
    );
    // A prefix of a declared name is a different name.
    assert_eq!(
        with_pipeline_constants(&module(), &[("gai", 2.0)]),
        Err(PipelineConstantError::Unknown("gai".into()))
    );
    // A name that is not an identifier can never be declared, and must not reach `#define`.
    assert_eq!(
        with_pipeline_constants(&module(), &[("gain 3\n#include", 2.0)]),
        Err(PipelineConstantError::Unknown("gain 3\n#include".into()))
    );
}
