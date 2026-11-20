//! A pipeline that cannot be created is a validation error and an inert pipeline, not a panic.
use super::*;

const COMPUTE: &str = "
override gain: f32 = 1.0;
@compute @workgroup_size(1)
fn run() {
    let scaled = gain * 2.0;
}
";

const RENDER: &str = "
@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(index) - 1);
    let y = f32(i32(index & 1u) * 2 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}
@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return vec4<f32>(1.0, 0.0, 0.0, 1.0);
}
";

fn block_on_ready<T>(fut: impl Future<Output = T>) -> T {
    let mut pinned = std::pin::pin!(fut);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    match pinned.as_mut().poll(&mut cx) {
        std::task::Poll::Ready(val) => val,
        std::task::Poll::Pending => panic!("expected immediate future completion"),
    }
}

fn metal_device() -> Option<(Device, Queue)> {
    let adapter = Adapter::from_metal(MetalDevice::create_system_default()?);
    Some(
        block_on_ready(adapter.request_device(&DeviceDescriptor::default()))
            .expect("device creation must succeed"),
    )
}

fn module(device: &Device, source: &str) -> ShaderModule {
    device.create_shader_module(ShaderModuleDescriptor {
        label: None,
        source: ShaderSource::Wgsl(source.into()),
    })
}

type Constants<'a> = &'a [(&'a str, f64)];

fn compute_pipeline(device: &Device, entry: &str, constants: Constants) -> ComputePipeline {
    device.create_compute_pipeline(&ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &module(device, COMPUTE),
        entry_point: Some(entry),
        compilation_options: PipelineCompilationOptions {
            constants,
            zero_initialize_workgroup_memory: false,
        },
        cache: None,
    })
}

fn render_pipeline(device: &Device, fragment_entry: &str) -> RenderPipeline {
    let module = module(device, RENDER);
    device.create_render_pipeline(&RenderPipelineDescriptor {
        label: None,
        layout: None,
        vertex: VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(FragmentState {
            module: &module,
            entry_point: Some(fragment_entry),
            targets: &[Some(ColorTargetState {
                format: TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// The validation error a pipeline creation left in a fresh scope.
fn rejection(device: &Device, create: impl FnOnce()) -> Option<String> {
    let scope = device.push_error_scope(ErrorFilter::Validation);
    create();
    block_on_ready(scope.pop()).map(|error| {
        assert!(matches!(error, Error::Validation { .. }));
        error.to_string()
    })
}

#[test]
fn a_rejected_compute_pipeline_is_a_validation_error_and_its_dispatches_are_skipped() {
    let Some((device, queue)) = metal_device() else {
        return;
    };
    let cases: [(&str, Constants, &str); 3] = [
        ("run", &[("gian", 2.0)], "no override named `gian`"),
        ("run", &[("gain", f64::NAN)], "not finite"),
        ("missing", &[], "compute pipeline compilation failed"),
    ];
    for (entry, constants, expected) in cases {
        let mut pipeline = None;
        let message = rejection(&device, || {
            pipeline = Some(compute_pipeline(&device, entry, constants));
        })
        .unwrap_or_else(|| panic!("`{expected}` was not reported"));
        assert!(message.contains(expected), "{message}");

        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(pipeline.as_ref().unwrap());
            pass.dispatch_workgroups(1, 1, 1);
        }
        queue.submit([encoder.finish()]);
        device.poll(PollType::wait_indefinitely()).unwrap();
    }

    let accepted = rejection(&device, || {
        compute_pipeline(&device, "run", &[("gain", 2.0)]);
    });
    assert_eq!(accepted, None);
}

#[test]
fn a_rejected_render_pipeline_is_a_validation_error_and_its_draws_are_skipped() {
    let Some((device, queue)) = metal_device() else {
        return;
    };
    let mut invalid = None;
    let message = rejection(&device, || {
        invalid = Some(render_pipeline(&device, "missing"))
    })
    .expect("the missing fragment entry point was not reported");
    assert!(
        message.contains("Fragment function `missing` not found"),
        "{message}"
    );
    let invalid = invalid.unwrap();

    let mut valid = None;
    assert_eq!(
        rejection(&device, || valid =
            Some(render_pipeline(&device, "fs_main"))),
        None
    );
    let valid = valid.unwrap();

    let target = device.create_texture(&TextureDescriptor {
        label: None,
        size: Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    // An invalid pipeline alone, then an invalid one replaced by a valid one.
    for pipelines in [&[&invalid][..], &[&invalid, &valid]] {
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(Color::TRANSPARENT),
                        store: StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                ..Default::default()
            });
            for pipeline in pipelines {
                pass.set_pipeline(pipeline);
                pass.draw(0..3, 0..1);
            }
        }
        queue.submit([encoder.finish()]);
        device.poll(PollType::wait_indefinitely()).unwrap();
    }
}

#[test]
#[should_panic(expected = "compute pipeline compilation failed")]
fn a_rejected_pipeline_with_nobody_listening_still_panics() {
    let Some((device, _queue)) = metal_device() else {
        panic!("compute pipeline compilation failed: no Metal device to prove it on");
    };
    compute_pipeline(&device, "missing", &[]);
}

#[test]
fn a_shader_that_does_not_parse_is_reported_when_the_module_is_created() {
    let Some((device, _queue)) = metal_device() else {
        return;
    };
    let message = rejection(&device, || {
        module(&device, "@compute @workgroup_size(1) fn run( {");
    });
    assert!(
        message.is_some(),
        "a malformed shader was accepted silently"
    );
}

#[test]
fn ill_typed_wgsl_fails_in_create_shader_module_without_reaching_metal() {
    let Some((device, _queue)) = metal_device() else {
        return;
    };
    let before = device
        .metal
        .metal_shader_compilations
        .load(std::sync::atomic::Ordering::Relaxed);

    let mut bad_compute = None;
    let shader_err = rejection(&device, || {
        bad_compute = Some(module(
            &device,
            "@compute @workgroup_size(1) fn run() { let x: u32 = 1.0; }",
        ));
    })
    .expect("ill-typed compute WGSL must fail in create_shader_module");
    assert!(
        shader_err.contains("shader validation failed"),
        "{shader_err}"
    );
    let bad_compute = bad_compute.unwrap();
    let pipeline_err = rejection(&device, || {
        device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: None,
            layout: None,
            module: &bad_compute,
            entry_point: Some("run"),
            compilation_options: Default::default(),
            cache: None,
        });
    })
    .expect("compute pipeline using invalid shader module must fail");
    assert!(
        pipeline_err.contains("invalid shader module"),
        "{pipeline_err}"
    );

    let mut bad_render = None;
    let render_shader_err = rejection(&device, || {
        bad_render = Some(module(
            &device,
            "@vertex fn vs_main() -> @builtin(position) vec4<f32> { return vec4<u32>(0u); }
             @fragment fn fs_main() -> @location(0) vec4<f32> { return vec4<f32>(1.0); }",
        ));
    })
    .expect("ill-typed render WGSL must fail in create_shader_module");
    assert!(
        render_shader_err.contains("shader validation failed"),
        "{render_shader_err}"
    );
    let bad_render = bad_render.unwrap();
    let render_pipeline_err = rejection(&device, || {
        device.create_render_pipeline(&RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: VertexState {
                module: &bad_render,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(FragmentState {
                module: &bad_render,
                entry_point: Some("fs_main"),
                targets: &[Some(ColorTargetState {
                    format: TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
    })
    .expect("render pipeline using invalid shader module must fail");
    assert!(
        render_pipeline_err.contains("invalid vertex shader module"),
        "{render_pipeline_err}"
    );

    let after = device
        .metal
        .metal_shader_compilations
        .load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(after, before, "ill-typed WGSL reached Metal compiler");
}
