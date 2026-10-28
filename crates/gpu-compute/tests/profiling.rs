use gpu_compute::{
    ByteReadback, GpuBuffer, GpuContext, GpuContextError, GpuTimer, GpuTimestamp, ReadbackError,
    TimestampError, wgpu,
};
use std::time::{Duration, Instant};

fn context() -> Option<GpuContext> {
    match GpuContext::with_timestamps() {
        Ok(context) => Some(context),
        Err(error @ GpuContextError::UnsupportedFeatures { .. }) => {
            eprintln!("timestamp profiling unsupported: {error}");
            assert!(std::env::var_os("COMPUTE_REQUIRE_TIMESTAMPS").is_none());
            None
        }
        Err(error) => {
            eprintln!("timestamp GPU unavailable: {error}");
            assert!(std::env::var_os("COMPUTE_REQUIRE_GPU").is_none());
            assert!(std::env::var_os("COMPUTE_REQUIRE_TIMESTAMPS").is_none());
            None
        }
    }
}

struct Workload {
    values: GpuBuffer,
    pipeline: wgpu::ComputePipeline,
    bindings: wgpu::BindGroup,
}
impl Workload {
    fn new(context: &GpuContext) -> Self {
        let values = GpuBuffer::new(
            context,
            4096 * 4,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        )
        .unwrap();
        let module = context
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("timestamp test workload"),
                source: wgpu::ShaderSource::Wgsl(
                    "@group(0) @binding(0) var<storage, read_write> values: array<u32>;
                     @compute @workgroup_size(64)
                     fn main(@builtin(global_invocation_id) id: vec3<u32>) {
                         if (id.x >= arrayLength(&values)) { return; }
                         var v = values[id.x] + id.x;
                         for (var i = 0u; i < 256u; i++) {
                             v = (v ^ (v >> 13u)) * 1664525u + 1013904223u;
                         }
                         values[id.x] = v;
                     }"
                    .into(),
                ),
            });
        let pipeline = context
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("timestamp test workload"),
                layout: None,
                module: &module,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });
        let bindings = context
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: values.raw().as_entire_binding(),
                }],
            });
        Self {
            values,
            pipeline,
            bindings,
        }
    }
    fn record(&self, pass: &mut wgpu::ComputePass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bindings, &[]);
        pass.dispatch_workgroups(64, 1, 1);
    }
    fn read(&self, context: &GpuContext, encoder: &mut wgpu::CommandEncoder) -> ByteReadback {
        ByteReadback::copy_buffer(&context.device, encoder, self.values.raw(), 0, 16 * 4).unwrap()
    }
}
fn expected(passes: usize) -> Vec<u32> {
    (0u32..16)
        .map(|index| {
            let mut value: u32 = 0;
            for _ in 0..passes {
                value = value.wrapping_add(index);
                for _ in 0..256 {
                    value = (value ^ (value >> 13))
                        .wrapping_mul(1664525)
                        .wrapping_add(1013904223);
                }
            }
            value
        })
        .collect()
}
fn validate_values(bytes: Vec<u8>, passes: usize) {
    let actual: Vec<_> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|bytes| u32::from_le_bytes(*bytes))
        .collect();
    assert_eq!(actual, expected(passes));
}
fn validate_sample(sample: GpuTimestamp, period: f64) {
    assert!(sample.elapsed_ns.is_finite());
    assert!(sample.elapsed_ns >= 0.0);
    assert_eq!(
        sample.elapsed_ns,
        (sample.end_ticks - sample.start_ticks) as f64 * period
    );
}

#[test]
fn timestamps_are_explicitly_enabled_and_context_identity_is_preserved() {
    let Some(default) = GpuContext::new() else {
        assert!(std::env::var_os("COMPUTE_REQUIRE_GPU").is_none());
        return;
    };
    assert!((default.enabled_features() - wgpu::Features::MAPPABLE_PRIMARY_BUFFERS).is_empty());
    assert!(matches!(
        GpuTimer::new(&default),
        Err(TimestampError::FeatureDisabled)
    ));
    let Some(profiled) = context() else { return };
    assert!(
        profiled
            .enabled_features()
            .contains(wgpu::Features::TIMESTAMP_QUERY)
    );
    assert!(profiled.same_device(&profiled.clone()));
    assert!(!profiled.same_device(&default));
    assert!(
        GpuTimer::new(&profiled.clone())
            .unwrap()
            .timestamp_period_ns()
            > 0.0
    );
}

#[test]
fn reused_timer_preserves_tickets_within_encoder_and_across_submissions() {
    let Some(context) = context() else { return };
    let timer = GpuTimer::new(&context).unwrap();
    let workload = Workload::new(&context);
    let mut first_encoder = context.device.create_command_encoder(&Default::default());
    let mut first = timer
        .record_compute(&mut first_encoder, "first", |pass| workload.record(pass))
        .unwrap();
    assert_eq!(
        first.try_read(),
        Err(TimestampError::Readback(ReadbackError::NotSubmitted))
    );
    let mut second = timer
        .record_compute(&mut first_encoder, "second", |pass| workload.record(pass))
        .unwrap();
    let mut first_values = workload.read(&context, &mut first_encoder);
    let first_submission = context.queue.submit([timer.finish(first_encoder).unwrap()]);
    first.submitted(first_submission.clone());
    second.submitted(first_submission.clone());
    first_values.submitted(first_submission);

    // Reuse the timer before reading either earlier result; each ticket retains
    // independent query storage until it is consumed.
    let mut second_encoder = context.device.create_command_encoder(&Default::default());
    let mut third = timer
        .record_compute(&mut second_encoder, "third", |pass| workload.record(pass))
        .unwrap();
    let mut second_values = workload.read(&context, &mut second_encoder);
    let second_submission = context
        .queue
        .submit([timer.finish(second_encoder).unwrap()]);
    third.submitted(second_submission.clone());
    second_values.submitted(second_submission);
    for sample in [
        third.wait(Duration::from_secs(10)).unwrap(),
        second.wait(Duration::from_secs(10)).unwrap(),
        first.wait_mut(Duration::from_secs(10)).unwrap(),
    ] {
        validate_sample(sample, timer.timestamp_period_ns());
    }
    assert_eq!(
        first.try_read(),
        Err(TimestampError::Readback(ReadbackError::Consumed))
    );
    validate_values(first_values.wait(Duration::from_secs(10)).unwrap(), 2);
    validate_values(second_values.wait(Duration::from_secs(10)).unwrap(), 3);
}

#[test]
fn cancelled_ticket_and_invalid_pass_do_not_poison_timer() {
    let Some(context) = context() else { return };
    let timer = GpuTimer::new(&context).unwrap();
    let workload = Workload::new(&context);
    let mut cancelled_encoder = context.device.create_command_encoder(&Default::default());
    let mut cancelled = timer
        .record_compute(&mut cancelled_encoder, "cancelled", |pass| {
            workload.record(pass)
        })
        .unwrap();
    cancelled.cancel();
    cancelled.submitted(
        context
            .queue
            .submit([timer.finish(cancelled_encoder).unwrap()]),
    );
    assert_eq!(
        cancelled.try_read(),
        Err(TimestampError::Readback(ReadbackError::Cancelled))
    );

    let outer = context
        .device
        .push_error_scope(wgpu::ErrorFilter::Validation);
    let mut invalid_encoder = context.device.create_command_encoder(&Default::default());
    // Dispatch without a pipeline is a deterministic backend validation error.
    let invalid = timer.record_compute(&mut invalid_encoder, "invalid", |pass| {
        pass.dispatch_workgroups(1, 1, 1);
    });
    // Native wgpu defers this validation until finish. Backends that report it
    // immediately still return a recoverable error from record_compute.
    match invalid {
        Ok(ticket) => {
            assert!(
                matches!(timer.finish(invalid_encoder), Err(TimestampError::Device(message)) if !message.is_empty())
            );
            drop(ticket);
        }
        Err(TimestampError::Device(message)) => {
            assert!(!message.is_empty());
            drop(invalid_encoder);
        }
        Err(error) => panic!("unexpected validation error: {error}"),
    }
    assert!(gpu_compute::block_on(outer.pop()).is_none());

    let mut encoder = context.device.create_command_encoder(&Default::default());
    let mut valid = timer
        .record_compute(&mut encoder, "valid", |pass| workload.record(pass))
        .unwrap();
    let mut values = workload.read(&context, &mut encoder);
    let submission = context.queue.submit([timer.finish(encoder).unwrap()]);
    valid.submitted(submission.clone());
    values.submitted(submission);
    validate_sample(
        valid.wait(Duration::from_secs(10)).unwrap(),
        timer.timestamp_period_ns(),
    );
    validate_values(values.wait(Duration::from_secs(10)).unwrap(), 2);
}

#[test]
fn alternating_workloads_measure_the_current_pass_instead_of_previous_samples() {
    let Some(context) = context() else { return };
    let timer = GpuTimer::new(&context).unwrap();
    let workload = Workload::new(&context);
    let run = |dispatches: usize| {
        let started = Instant::now();
        let mut encoder = context.device.create_command_encoder(&Default::default());
        let mut ticket = timer
            .record_compute(&mut encoder, "different duration", |pass| {
                for _ in 0..dispatches {
                    workload.record(pass);
                }
            })
            .unwrap();
        ticket.submitted(context.queue.submit([timer.finish(encoder).unwrap()]));
        let sample = ticket.wait(Duration::from_secs(10)).unwrap();
        let wall_ns = started.elapsed().as_secs_f64() * 1e9;
        eprintln!(
            "dispatches={dispatches} gpu_ns={} wall_ns={wall_ns}",
            sample.elapsed_ns
        );
        // The complete host interval encloses this submission and its readback.
        // A much longer GPU interval belongs to a different pass or is invalid.
        assert!(
            sample.elapsed_ns <= wall_ns + 10_000.0,
            "GPU sample exceeds enclosing wall interval"
        );
        sample
    };
    let mut short = Vec::new();
    let mut long = Vec::new();
    let mut completed_end = 0;
    for pair in 0..8 {
        for variant in [pair % 2, 1 - pair % 2] {
            let sample = run(if variant == 0 { 1 } else { 128 });
            // The prior ticket was completely read before this submission was
            // encoded. Reusing its query contents must not look like a new pass.
            assert!(
                sample.start_ticks >= completed_end,
                "pass returned a sample older than the completed submission"
            );
            completed_end = sample.end_ticks;
            if variant == 0 {
                short.push(sample.elapsed_ns);
            } else {
                long.push(sample.elapsed_ns);
            }
        }
    }
    short.sort_by(f64::total_cmp);
    long.sort_by(f64::total_cmp);
    assert!(
        long[4] > short[4] * 8.0,
        "distinct workloads must retain their own durations: short={short:?} long={long:?}"
    );

    // Read tickets in reverse order after both passes complete. A single pass
    // can be delayed by unrelated GPU work; retain the same 8x separation gate
    // over seven samples per order, as with the alternating submissions above.
    // Freshness and wall bounds still apply to every individual sample.
    let mut mixed: [[Vec<f64>; 2]; 2] =
        std::array::from_fn(|_| std::array::from_fn(|_| Vec::new()));
    for repetition in 0..7 {
        for order in [repetition % 2, 1 - repetition % 2] {
            let dispatch_counts = [[1, 128], [128, 1]][order];
            let started = Instant::now();
            let mut encoder = context.device.create_command_encoder(&Default::default());
            let mut tickets = Vec::new();
            for dispatches in dispatch_counts {
                let ticket = timer
                    .record_compute(&mut encoder, "mixed encoder", |pass| {
                        for _ in 0..dispatches {
                            workload.record(pass);
                        }
                    })
                    .unwrap();
                tickets.push((dispatches, ticket));
            }
            let submitted = context.queue.submit([timer.finish(encoder).unwrap()]);
            for (_, ticket) in &mut tickets {
                ticket.submitted(submitted.clone());
            }
            let mut elapsed = [0.0; 2];
            let prior_end = completed_end;
            for (dispatches, ticket) in tickets.into_iter().rev() {
                let sample = ticket.wait(Duration::from_secs(10)).unwrap();
                let wall_ns = started.elapsed().as_secs_f64() * 1e9;
                validate_sample(sample, timer.timestamp_period_ns());
                assert!(
                    sample.start_ticks >= prior_end,
                    "mixed-encoder ticket returned an older submission's sample"
                );
                assert!(
                    sample.elapsed_ns <= wall_ns + 10_000.0,
                    "mixed-encoder GPU sample exceeds enclosing wall interval"
                );
                completed_end = completed_end.max(sample.end_ticks);
                elapsed[usize::from(dispatches > 1)] = sample.elapsed_ns;
            }
            eprintln!(
                "same_encoder_order={dispatch_counts:?} repetition={repetition} short_ns={} long_ns={}",
                elapsed[0], elapsed[1]
            );
            mixed[order][0].push(elapsed[0]);
            mixed[order][1].push(elapsed[1]);
        }
    }
    for (order, [mut short, mut long]) in mixed.into_iter().enumerate() {
        short.sort_by(f64::total_cmp);
        long.sort_by(f64::total_cmp);
        assert!(
            long[3] > short[3] * 8.0,
            "same-encoder order {order} retained the wrong pass: short={short:?} long={long:?}"
        );
    }
}
