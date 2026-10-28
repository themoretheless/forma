//! cargo run --release --offline -p compute-core --example chain
use compute_core::{
    Binding, ComputeBatch, Kernel, Reduction,
    gpu_compute::GpuContext,
    read_f32,
    shaders::{BLOCK_SUM_WGSL, SCALE_ADD_WGSL, ZIP_MUL_WGSL},
    storage_f32, storage_f32_zeroed, uniform_f32,
};

fn main() {
    let context = GpuContext::new().expect("a GPU adapter is required");
    let device = &context.device;
    let queue = &context.queue;
    let layout = [
        Binding::Uniform,
        Binding::StorageRead,
        Binding::StorageReadWrite,
    ];
    let scale = Kernel::new(device, "scale", SCALE_ADD_WGSL, "main", &layout).unwrap();
    let multiply = Kernel::new(
        device,
        "multiply",
        ZIP_MUL_WGSL,
        "main",
        &[
            Binding::Uniform,
            Binding::StorageRead,
            Binding::StorageRead,
            Binding::StorageReadWrite,
        ],
    )
    .unwrap();
    let sum = Kernel::new(device, "sum", BLOCK_SUM_WGSL, "main", &layout).unwrap();
    let count = 100_003u32;
    let values: Vec<f32> = (0..count).map(|i| (i % 7) as f32).collect();
    let input = storage_f32(device, queue, &values);
    let scaled = storage_f32_zeroed(device, queue, count as usize);
    let products = storage_f32_zeroed(device, queue, count as usize);
    let output = storage_f32_zeroed(device, queue, 1);
    let params = uniform_f32(device, queue, &[f32::from_bits(count), 2.0, 1.0, 0.0]);
    let final_params = uniform_f32(device, queue, &[f32::from_bits(1), 0.5, -3.0, 0.0]);
    let scale_bind = scale.create_bind_group(device, &[&params, &input, &scaled]);
    let mul_bind = multiply.create_bind_group(device, &[&params, &scaled, &input, &products]);
    let reduction = Reduction::new(device, queue, &sum, &products, count);
    let final_bind = scale.create_bind_group(device, &[&final_params, reduction.output(), &output]);
    let mut batch = ComputeBatch::new();
    batch.push(&scale, &scale_bind, scale.workgroup_count(count));
    batch.push(&multiply, &mul_bind, multiply.workgroup_count(count));
    batch.push_reduction(&reduction);
    batch.push(&scale, &final_bind, 1);
    for iteration in 0..3 {
        let values: Vec<f32> = values.iter().map(|v| v + iteration as f32).collect();
        queue.write_buffer(&input, 0, &compute_core::gpu_compute::pack_f32(&values));
        batch.submit(device, queue);
        let actual = read_f32(device, queue, &output, 1)[0];
        let expected = values.iter().map(|x| (x * 2.0 + 1.0) * x).sum::<f32>() * 0.5 - 3.0;
        assert_eq!(actual, expected);
        println!(
            "{} iteration {iteration}: {actual} (CPU reference {expected})",
            context.backend_label()
        );
    }
}
