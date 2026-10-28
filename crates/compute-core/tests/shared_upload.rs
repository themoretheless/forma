#![cfg(not(target_arch = "wasm32"))]
use compute_core::{
    ComputeRuntime,
    gpu_compute::{self, GpuContext, wgpu},
};
use std::time::Duration;
use tensor_core::{Shape, TensorF64Backend};

fn context() -> Option<GpuContext> {
    let context = GpuContext::new();
    assert!(context.is_some() || std::env::var_os("COMPUTE_REQUIRE_GPU").is_none());
    context
}
const WAIT: Duration = Duration::from_secs(10);

#[test]
fn generated_updates_preserve_queue_order_aliases_and_pending_snapshots() {
    let Some(context) = context() else { return };
    let rt = ComputeRuntime::new(&context).unwrap();
    let input = rt.zeros::<u32>(5).unwrap();
    let alias = input.clone();
    rt.write_generated(&input, 0, 5, |i| i as u32 + 10).unwrap();
    let first = rt.read(&input).unwrap();
    rt.write_generated(&alias, 1, 3, |i| u32::MAX - i as u32)
        .unwrap();
    let second = rt.read(&input).unwrap();
    assert_eq!(
        second.wait(WAIT).unwrap(),
        [10, u32::MAX, u32::MAX - 1, u32::MAX - 2, 14]
    );
    assert_eq!(first.wait(WAIT).unwrap(), [10, 11, 12, 13, 14]);
    for (offset, len) in [(4, 2), (usize::MAX, 1), (0, usize::MAX)] {
        assert!(
            rt.write_generated(&input, offset, len, |_| panic!("invalid generator called"))
                .is_err()
        );
    }
    rt.write_generated(&input, 5, 0, |_| panic!("empty generator called"))
        .unwrap();
    let foreign = ComputeRuntime::new(&context)
        .unwrap()
        .zeros::<u32>(5)
        .unwrap();
    assert!(
        rt.write_generated(&foreign, 0, 1, |_| panic!("foreign generator called"))
            .is_err()
    );
    assert_eq!(
        rt.read(&input).unwrap().wait(WAIT).unwrap(),
        [10, u32::MAX, u32::MAX - 1, u32::MAX - 2, 14]
    );
}

#[test]
fn device_without_primary_mapping_keeps_the_portable_upload_path() {
    let instance = wgpu::Instance::default();
    let adapter = match gpu_compute::block_on(instance.request_adapter(&Default::default())) {
        Ok(adapter) => adapter,
        Err(_) => {
            assert!(std::env::var_os("COMPUTE_REQUIRE_GPU").is_none());
            return;
        }
    };
    let (device, queue) =
        gpu_compute::block_on(adapter.request_device(&Default::default())).unwrap();
    assert!(!gpu_compute::shared_memory_uploads(&device));
    let data = [0, u32::MAX, 0x8000_0000, 42];
    let buffer = compute_core::storage_u32(&device, &queue, &data);
    assert!(!buffer.usage().contains(wgpu::BufferUsages::MAP_WRITE));
    assert_eq!(
        compute_core::try_read_u32(&device, &queue, &buffer, data.len()).unwrap(),
        data
    );
}

#[test]
fn generated_upload_runs_shader_and_keeps_independent_readback_snapshots() {
    let Some(context) = context() else { return };
    let runtime = ComputeRuntime::new(&context).unwrap();
    let input = runtime.upload_generated(1027, |i| i as f32 * 0.25).unwrap();
    let mut program = runtime.program();
    let output = program.affine(&input, 2.0, -1.0).unwrap();
    let mut first = program.submit_read(&output).unwrap();
    runtime.write(&input, 0, &vec![7.0; 1027]).unwrap();
    let second = program.submit_read(&output).unwrap();
    assert_eq!(second.wait(WAIT).unwrap(), vec![13.0; 1027]);
    let actual = first.wait_mut(WAIT).unwrap();
    assert_eq!(
        actual,
        (0..1027).map(|i| i as f32 * 0.5 - 1.0).collect::<Vec<_>>()
    );
}

#[test]
fn initialized_buffer_uses_shared_allocation_only_with_native_support() {
    let Some(context) = context() else { return };
    let usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC;
    let buffer = gpu_compute::GpuBuffer::new_initialized(&context, 12, usage, |mut mapping| {
        mapping
            .slice(..8)
            .copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    })
    .unwrap();
    assert_eq!(
        buffer.raw().usage().contains(wgpu::BufferUsages::MAP_WRITE),
        gpu_compute::shared_memory_uploads(&context.device)
    );
    if cfg!(all(target_os = "macos", target_arch = "aarch64"))
        && context.backend == wgpu::Backend::Metal
    {
        assert!(gpu_compute::shared_memory_uploads(&context.device));
    }
    let runtime = ComputeRuntime::new(&context).unwrap();
    // Import needs COPY_DST too; read this low-level fixture through byte transport.
    let mut encoder = context.device.create_command_encoder(&Default::default());
    let mut ticket =
        gpu_compute::ByteReadback::copy_buffer(&context.device, &mut encoder, buffer.raw(), 0, 12)
            .unwrap();
    ticket.submitted(context.queue.submit([encoder.finish()]));
    assert_eq!(
        ticket.wait(WAIT).unwrap(),
        [1, 2, 3, 4, 5, 6, 7, 8, 0, 0, 0, 0]
    );
    assert!(
        runtime
            .upload_generated::<u32>(usize::MAX, |_| panic!("must validate before generation"))
            .is_err()
    );
    let empty = runtime
        .upload_generated::<u32>(0, |_| panic!("empty generator called"))
        .unwrap();
    assert!(runtime.read(&empty).unwrap().wait(WAIT).unwrap().is_empty());
}

#[test]
fn uploads_preserve_float_bit_patterns_including_binary64_word_order() {
    let Some(context) = context() else { return };
    let runtime = ComputeRuntime::new(&context).unwrap();
    let bits = [0, 0x8000_0000, 1, 0x7f80_0000, 0xff80_0000, 0x7fc0_1234];
    let input = runtime.upload(&bits.map(f32::from_bits)).unwrap();
    assert_eq!(
        runtime
            .read(&input)
            .unwrap()
            .wait(WAIT)
            .unwrap()
            .iter()
            .map(|v| v.to_bits())
            .collect::<Vec<_>>(),
        bits
    );
    let bits64 = [
        0,
        0x8000_0000_0000_0000,
        1,
        0x7ff0_0000_0000_0000,
        0xfff0_0000_0000_0000,
        0x7ff8_1234_abcd_5678,
    ];
    let input = runtime
        .upload_f64(
            Shape::new(vec![bits64.len()]).unwrap(),
            &bits64.map(f64::from_bits),
        )
        .unwrap();
    assert_eq!(
        runtime
            .read_f64(&input)
            .unwrap()
            .iter()
            .map(|v| v.to_bits())
            .collect::<Vec<_>>(),
        bits64
    );
    let empty = runtime
        .upload_f64(Shape::new(vec![0]).unwrap(), &[])
        .unwrap();
    assert!(runtime.read_f64(&empty).unwrap().is_empty());
}
