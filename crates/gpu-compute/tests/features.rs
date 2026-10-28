use gpu_compute::{GpuContext, GpuContextError, wgpu};

#[test]
fn native_subgroups_and_timestamps_require_explicit_opt_in() {
    let required = wgpu::Features::SUBGROUP | wgpu::Features::TIMESTAMP_QUERY;
    let context = match GpuContext::with_features(required) {
        Ok(context) => context,
        Err(GpuContextError::UnsupportedFeatures {
            required: wanted,
            available,
        }) => {
            assert_eq!(wanted, required);
            assert!(!available.contains(wanted));
            assert!(std::env::var_os("COMPUTE_REQUIRE_SUBGROUPS").is_none());
            return;
        }
        Err(error) => {
            assert!(std::env::var_os("COMPUTE_REQUIRE_GPU").is_none(), "{error}");
            assert!(
                std::env::var_os("COMPUTE_REQUIRE_SUBGROUPS").is_none(),
                "{error}"
            );
            return;
        }
    };
    assert!(context.enabled_features().contains(required));
    assert!(context.subgroup_report().supported);
    assert!(context.same_device(&context.clone()));
    let default = GpuContext::new().unwrap();
    let implicit = if gpu_compute::shared_memory_uploads(&default.device) {
        wgpu::Features::MAPPABLE_PRIMARY_BUFFERS
    } else {
        wgpu::Features::empty()
    };
    assert_eq!(default.enabled_features(), implicit);
    assert!(!default.subgroup_report().supported);
    assert!(!context.same_device(&default));
}
