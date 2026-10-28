use gpu_compute::{BufferError, GpuBuffer, GpuContext, wgpu};

#[test]
fn unsupported_buffer_usages_return_errors_before_allocation() {
    let Some(context) = GpuContext::new() else {
        assert!(std::env::var_os("COMPUTE_REQUIRE_GPU").is_none());
        return;
    };
    for usage in [
        wgpu::BufferUsages::empty(),
        wgpu::BufferUsages::BLAS_INPUT,
        wgpu::BufferUsages::TLAS_INPUT,
        wgpu::BufferUsages::BLAS_INPUT | wgpu::BufferUsages::STORAGE,
    ] {
        assert!(
            matches!(
                GpuBuffer::new(&context, 16, usage),
                Err(BufferError::InvalidUsage)
            ),
            "usage {usage:?} should be rejected"
        );
    }
    if !context
        .enabled_features()
        .contains(wgpu::Features::MAPPABLE_PRIMARY_BUFFERS)
    {
        for usage in [
            wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::STORAGE,
            wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::COPY_DST,
        ] {
            assert!(matches!(
                GpuBuffer::new(&context, 16, usage),
                Err(BufferError::InvalidUsage)
            ));
        }
    }
    for usage in [
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::COPY_SRC,
    ] {
        let buffer = GpuBuffer::new(&context, 16, usage).unwrap();
        assert_eq!(buffer.size(), 16);
        assert_eq!(buffer.raw().usage(), usage);
    }
}
