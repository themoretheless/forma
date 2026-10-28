use compute_core::{
    Binding, Kernel, KernelBindingError, KernelCache,
    gpu_compute::{BufferError, GpuBuffer, GpuContext},
    read_f32,
    shaders::SCALE_ADD_WGSL,
    wgpu,
};
use std::sync::Arc;
const LAYOUT: [Binding; 3] = [
    Binding::Uniform,
    Binding::StorageRead,
    Binding::StorageReadWrite,
];
fn context() -> Option<GpuContext> {
    let context = GpuContext::new();
    assert!(
        context.is_some() || std::env::var_os("COMPUTE_REQUIRE_GPU").is_none(),
        "GPU required"
    );
    context
}
fn storage(c: &GpuContext, bytes: u64) -> GpuBuffer {
    GpuBuffer::new(
        c,
        bytes,
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
    )
    .unwrap()
}

#[test]
fn actual_workgroup_size_covers_all_elements_without_a_tuning_anchor() {
    let Some(c) = context() else { return };
    let source = SCALE_ADD_WGSL.replace("const WG: u32 = 256;", "const WG: u32 = 64;");
    let kernel = Kernel::new(&c.device, "WG64", &source, "main", &LAYOUT).unwrap();
    assert_eq!(kernel.workgroup_size(), 64);
    assert_eq!(kernel.workgroup_count(257), 5);
    let params =
        compute_core::uniform_f32(&c.device, &c.queue, &[f32::from_bits(257), 2.0, 3.0, 0.0]);
    let input = compute_core::storage_f32(
        &c.device,
        &c.queue,
        &(0..257).map(|i| i as f32).collect::<Vec<_>>(),
    );
    let output = compute_core::storage_f32_zeroed(&c.device, &c.queue, 257);
    kernel.dispatch(&c.device, &c.queue, &[&params, &input, &output], 257);
    assert_eq!(
        read_f32(&c.device, &c.queue, &output, 257),
        (0..257).map(|i| i as f32 * 2.0 + 3.0).collect::<Vec<_>>()
    );
    let misleading = format!("// const WG: u32 = 256;\n{SCALE_ADD_WGSL}");
    assert!(
        Kernel::with_workgroup_size(
            &c.device,
            "comment anchor",
            &misleading,
            "main",
            &LAYOUT,
            64
        )
        .is_err()
    );
    assert!(
        Kernel::new(
            &c.device,
            "2D",
            "@compute @workgroup_size(8, 8) fn main() {}",
            "main",
            &[]
        )
        .is_err()
    );
}

#[test]
fn reflected_bindings_reject_errors_before_recording_and_remain_usable() {
    let Some(c) = context() else { return };
    let kernel = Kernel::from_context(&c, "checked", SCALE_ADD_WGSL, "main", &LAYOUT).unwrap();
    assert_eq!(
        kernel
            .binding_info()
            .iter()
            .map(|b| b.min_size)
            .collect::<Vec<_>>(),
        [16, 4, 4]
    );
    let params = GpuBuffer::new(
        &c,
        512,
        wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    )
    .unwrap();
    let input = storage(&c, 16);
    let output = storage(&c, 16);
    let p = params.view(0..16).unwrap();
    let i = input.view(0..16).unwrap();
    let o = output.view(0..16).unwrap();
    assert!(matches!(
        kernel.bind(&c, &[p, i]),
        Err(KernelBindingError::Count { .. })
    ));
    assert!(matches!(
        kernel.bind(&c, &[params.view(0..4).unwrap(), i, o]),
        Err(KernelBindingError::Size { binding: 0, .. })
    ));
    assert!(matches!(
        kernel.bind(&c, &[params.view(4..20).unwrap(), i, o]),
        Err(KernelBindingError::Unaligned { binding: 0 })
    ));
    assert!(matches!(
        kernel.bind(&c, &[i, i, o]),
        Err(KernelBindingError::Buffer(BufferError::MissingUsage))
    ));
    assert!(matches!(
        kernel.bind(&c, &[p, i, i]),
        Err(KernelBindingError::AliasedStorage { .. })
    ));
    let other = GpuContext::new().unwrap();
    assert!(matches!(
        kernel.bind(&other, &[p, i, o]),
        Err(KernelBindingError::Buffer(BufferError::ForeignDevice))
    ));
    let foreign = storage(&other, 16);
    assert!(matches!(
        kernel.bind(&c, &[p, foreign.view(0..16).unwrap(), o]),
        Err(KernelBindingError::Buffer(BufferError::ForeignDevice))
    ));
    c.queue.write_buffer(
        params.raw(),
        0,
        &compute_core::gpu_compute::pack_f32(&[f32::from_bits(4), 3.0, 2.0, 0.0]),
    );
    c.queue.write_buffer(
        input.raw(),
        0,
        &compute_core::gpu_compute::pack_f32(&[1.0, 2.0, 3.0, 4.0]),
    );
    let bindings = kernel.bind(&c.clone(), &[p, i, o]).unwrap();
    kernel.dispatch_bind_group(&c.device, &c.queue, &bindings, 1);
    assert_eq!(
        read_f32(&c.device, &c.queue, output.raw(), 4),
        [5.0, 8.0, 11.0, 14.0]
    );
}

#[test]
fn cache_keys_use_compiled_contract_and_eviction_preserves_live_kernels() {
    let Some(c) = context() else { return };
    let mut cache = KernelCache::new(&c, 2);
    let first = cache.get("first", SCALE_ADD_WGSL, "main", &LAYOUT).unwrap();
    assert!(Arc::ptr_eq(
        &first,
        &cache
            .get("another label", SCALE_ADD_WGSL, "main", &LAYOUT)
            .unwrap()
    ));
    let small = cache
        .get_with_workgroup_size("64", SCALE_ADD_WGSL, "main", &LAYOUT, 64)
        .unwrap();
    assert!(Arc::ptr_eq(
        &small,
        &cache
            .get("normalized", small.source(), "main", &LAYOUT)
            .unwrap()
    ));
    assert!(cache.get("bad source", "broken", "main", &[]).is_err());
    assert!(
        cache
            .get("bad entry", SCALE_ADD_WGSL, "missing", &LAYOUT)
            .is_err()
    );
    assert!(
        cache
            .get(
                "bad layout",
                SCALE_ADD_WGSL,
                "main",
                &[Binding::StorageRead; 3]
            )
            .is_err()
    );
    assert_eq!(cache.stats().evictions, 0);
    cache
        .get_with_workgroup_size("128", SCALE_ADD_WGSL, "main", &LAYOUT, 128)
        .unwrap();
    assert_eq!(cache.len(), 2);
    assert_eq!(cache.stats().evictions, 1);
    assert!(!Arc::ptr_eq(
        &first,
        &cache
            .get("recompiled", SCALE_ADD_WGSL, "main", &LAYOUT)
            .unwrap()
    ));
    cache.clear();
    assert!(cache.is_empty());
    // A retained pipeline remains valid after both eviction and clearing.
    let params =
        compute_core::uniform_f32(&c.device, &c.queue, &[f32::from_bits(1), 2.0, 1.0, 0.0]);
    let input = compute_core::storage_f32(&c.device, &c.queue, &[4.0]);
    let output = compute_core::storage_f32_zeroed(&c.device, &c.queue, 1);
    first.dispatch(&c.device, &c.queue, &[&params, &input, &output], 1);
    assert_eq!(read_f32(&c.device, &c.queue, &output, 1), [9.0]);
    let mut no_cache = KernelCache::new(&c, 0);
    no_cache
        .get("zero capacity", SCALE_ADD_WGSL, "main", &LAYOUT)
        .unwrap();
    assert!(no_cache.is_empty());
}

#[test]
fn one_bind_group_remains_compatible_across_entry_points_using_different_slots() {
    let Some(c) = context() else { return };
    let source = r#"
        @group(0) @binding(0) var<storage, read_write> a: array<f32>;
        @group(0) @binding(1) var<storage, read_write> b: array<vec2<f32>>;
        @compute @workgroup_size(1) fn first() { a[0] = 7.0; }
        @compute @workgroup_size(1) fn second() { b[0] = vec2<f32>(9.0, 11.0); }
    "#;
    let layout = [Binding::StorageReadWrite; 2];
    let first = Kernel::from_context(&c, "first", source, "first", &layout).unwrap();
    let second = Kernel::from_context(&c, "second", source, "second", &layout).unwrap();
    assert_eq!(first.binding_info()[0].min_size, 4);
    assert_eq!(second.binding_info()[1].min_size, 8);
    let a = storage(&c, 4);
    let b = storage(&c, 8);
    let group = first
        .bind(&c, &[a.view(0..4).unwrap(), b.view(0..8).unwrap()])
        .unwrap();
    let mut encoder = c.device.create_command_encoder(&Default::default());
    first.record_dispatch(&mut encoder, &group, 1);
    second.record_dispatch(&mut encoder, &group, 1);
    c.queue.submit([encoder.finish()]);
    assert_eq!(read_f32(&c.device, &c.queue, a.raw(), 1), [7.0]);
    assert_eq!(read_f32(&c.device, &c.queue, b.raw(), 2), [9.0, 11.0]);
}
