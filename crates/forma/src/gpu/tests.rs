use super::*;
use crate::Button;

#[test]
fn invalid_timestamp_pairs_are_unavailable_not_wrapped_durations() {
    assert_eq!(timestamp_duration_ns(100, 150, 2.), Some(100.));
    assert_eq!(timestamp_duration_ns(150, 100, 1.), None);
    assert_eq!(timestamp_duration_ns(0, 0, 1.), None);
    assert_eq!(timestamp_duration_ns(100, u64::MAX, 1.), None);
    assert_eq!(timestamp_duration_ns(u64::MAX, 100, 1.), None);
    assert_eq!(timestamp_duration_ns(100, 150, f64::NAN), None);
    assert_eq!(timestamp_duration_ns(100, 150, 0.), None);
}

#[test]
fn memory_architecture_distinguishes_uma_and_duma_adapters() {
    let make = |device_type, backend| wgpu::AdapterInfo {
        name: "test".into(),
        vendor: 0,
        device: 0,
        device_type,
        device_pci_bus_id: String::new(),
        driver: String::new(),
        driver_info: String::new(),
        backend,
        subgroup_min_size: 4,
        subgroup_max_size: 64,
        transient_saves_memory: false,
    };
    let uma_igpu = MemoryArchitecture::from_adapter_info(&make(
        wgpu::DeviceType::IntegratedGpu,
        wgpu::Backend::Metal,
    ));
    assert_eq!(uma_igpu, MemoryArchitecture::Uma);
    assert_eq!(uma_igpu.as_str(), "UMA");
    assert!(uma_igpu.is_unified());

    let duma_dgpu = MemoryArchitecture::from_adapter_info(&make(
        wgpu::DeviceType::DiscreteGpu,
        wgpu::Backend::Vulkan,
    ));
    assert_eq!(duma_dgpu, MemoryArchitecture::Duma);
    assert_eq!(duma_dgpu.as_str(), "DUMA");
    assert!(!duma_dgpu.is_unified());

    assert_eq!(
        MemoryArchitecture::from_adapter_info(
            &make(wgpu::DeviceType::Other, wgpu::Backend::Metal,)
        ),
        MemoryArchitecture::Uma
    );
    assert_eq!(
        MemoryArchitecture::from_adapter_info(&make(wgpu::DeviceType::Other, wgpu::Backend::Dx12,)),
        MemoryArchitecture::Duma
    );
}

#[test]
fn storage_capacity_grows_amortized_without_exceeding_binding_budget() {
    assert_eq!(storage_capacity(80, 0, 1024).unwrap(), 128);
    assert_eq!(storage_capacity(100, 128, 1024).unwrap(), 128);
    assert_eq!(storage_capacity(32, 128, 1024).unwrap(), 128);
    assert_eq!(storage_capacity(129, 128, 1024).unwrap(), 256);
    assert_eq!(storage_capacity(513, 512, 768).unwrap(), 768);
    assert_eq!(storage_capacity(0, 0, 16).unwrap(), 4);
    assert!(storage_capacity(769, 512, 768).is_err());
    assert!(storage_capacity(767, 512, 767).is_err());
    assert!(storage_capacity(16, 1024, 512).is_err());
}

#[test]
fn missing_pipeline_validation_accepts_only_known_backend_messages() {
    assert!(missing_compute_pipeline_error(
        "dispatch_workgroups called without an active ComputePipeline"
    ));
    assert!(missing_compute_pipeline_error(
        "In a ComputePass, note: Compute pipeline must be set"
    ));
    assert!(!missing_compute_pipeline_error(
        "Compute pipeline is invalid"
    ));
    assert!(!missing_compute_pipeline_error("validation failed"));
}

#[test]
fn gpu_test_adapters_are_hardware_or_mesa_lavapipe() {
    let make = |device_type, backend, name: &str, driver: &str| wgpu::AdapterInfo {
        name: name.into(),
        vendor: 0,
        device: 0,
        device_type,
        device_pci_bus_id: String::new(),
        driver: driver.into(),
        driver_info: String::new(),
        backend,
        subgroup_min_size: 4,
        subgroup_max_size: 64,
        transient_saves_memory: false,
    };
    assert!(gpu_test_adapter_supported(&make(
        wgpu::DeviceType::DiscreteGpu,
        wgpu::Backend::Dx12,
        "",
        ""
    )));
    assert!(gpu_test_adapter_supported(&make(
        wgpu::DeviceType::Cpu,
        wgpu::Backend::Vulkan,
        "llvmpipe (LLVM 17.0.6, 256 bits)",
        ""
    )));
    assert!(gpu_test_adapter_supported(&make(
        wgpu::DeviceType::Cpu,
        wgpu::Backend::Vulkan,
        "Mesa CPU",
        "lavapipe"
    )));
    assert!(!gpu_test_adapter_supported(&make(
        wgpu::DeviceType::Cpu,
        wgpu::Backend::Vulkan,
        "SwiftShader",
        ""
    )));
    assert!(!gpu_test_adapter_supported(&make(
        wgpu::DeviceType::Cpu,
        wgpu::Backend::Metal,
        "llvmpipe",
        ""
    )));
}

#[test]
#[ignore = "requires a real GPU; run explicitly with --features gpu -- --ignored"]
fn profiled_renderer_reports_owned_buffers_and_reuses_them() {
    pollster::block_on(async {
        let instance = wgpu::Instance::default();
        let adapter = instance.request_adapter(&Default::default()).await.unwrap();
        let adapter_info = adapter.get_info();
        assert_gpu_test_adapter(&adapter_info);
        eprintln!(
            "GPU_TEST_ADAPTER {}",
            serde_json::json!({"test":"profiled-renderer","name":adapter_info.name,"vendor":adapter_info.vendor,"device":adapter_info.device,
                "device_type":format!("{:?}",adapter_info.device_type),"backend":format!("{:?}",adapter_info.backend),"driver":adapter_info.driver,"driver_info":adapter_info.driver_info})
        );
        let mut renderer = Renderer::new_profiled(&adapter, wgpu::TextureFormat::Rgba8Unorm)
            .await
            .unwrap();
        let mut model = Button::from_sources(crate::EXAMPLE, crate::BUTTON_COMPONENT).unwrap();
        model.pixels(96, 64, 1.);
        assert!(model.raster_cache.borrow().is_some());
        let texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Forma profiling smoke test target"),
            size: wgpu::Extent3d {
                width: 96,
                height: 64,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let target = texture.create_view(&Default::default());
        assert_eq!(renderer.read_gpu_duration_ns().unwrap(), None);
        let initial = renderer.resource_stats();
        assert_eq!(
            initial.buffer_bytes,
            (PARAMS_LEN * 4) as u64 + initial.timing_buffer_bytes
        );
        assert_eq!(
            initial.buffer_allocations_total,
            initial.buffer_count as u64
        );
        assert_eq!(initial.bind_group_count, 0);
        let vp96 = Viewport::new(96, 64, 1.);
        let mut uploaded_once = None;
        for _ in 0..2 {
            renderer.draw(&model, &target, vp96, true, true).unwrap();
            assert!(
                model.raster_cache.borrow().is_none(),
                "GPU submission must release CPU rasters"
            );
            let duration = renderer.read_gpu_duration_ns().unwrap();
            if !renderer.gpu_timestamps_enabled() {
                assert_eq!(duration, None);
            }
            if let Some(ns) = duration {
                assert!(ns.is_finite() && ns >= 0., "invalid GPU duration: {ns}");
            }
            assert_eq!(renderer.read_gpu_duration_ns().unwrap(), None);
            let uploaded = renderer.resource_stats().uploaded_bytes_total;
            if let Some(first) = uploaded_once {
                assert_eq!(
                    uploaded, first,
                    "an unchanged frame must not upload any payload"
                );
            }
            uploaded_once = Some(uploaded);
        }
        let stats = renderer.resource_stats();
        assert_eq!(stats.buffer_count, initial.buffer_count + 4);
        assert_eq!(
            stats.buffer_allocations_total,
            initial.buffer_allocations_total + 4
        );
        assert_eq!(stats.geometry_uploads_total, 1);
        assert_eq!(stats.tile_uploads_total, 1);
        assert_eq!(stats.bind_group_count, 1);
        assert_eq!(
            stats.buffer_bytes,
            stats.uniform_bytes
                + stats.command_bytes
                + stats.edge_bytes
                + stats.tile_bytes
                + stats.paint_bytes
                + stats.timing_buffer_bytes
        );
        assert_eq!(stats.buffer_allocation_bytes_total, stats.buffer_bytes);
        assert!(stats.uploaded_bytes_total >= stats.uniform_bytes);
        assert_eq!(stats.cpu_geometry_cache_bytes, 0);
        assert!(stats.cpu_shared_geometry_bytes > 0);
        assert!(stats.cpu_tile_scratch_bytes > 0);
        assert_eq!(stats.bind_group_creations_total, 1);
        let expected_host = (stats.cpu_shared_geometry_bytes
            + stats.cpu_tile_scratch_bytes
            + stats.cpu_paint_scratch_bytes) as u64;
        match stats.memory_architecture {
            MemoryArchitecture::Uma => {
                assert_eq!(
                    stats.uma_shared_pool_bytes,
                    stats.buffer_bytes + expected_host
                );
                assert_eq!(stats.duma_device_local_bytes, 0);
                assert_eq!(stats.duma_host_ram_bytes, 0);
            }
            MemoryArchitecture::Duma => {
                assert_eq!(stats.uma_shared_pool_bytes, 0);
                assert_eq!(stats.duma_device_local_bytes, stats.buffer_bytes);
                assert_eq!(stats.duma_host_ram_bytes, expected_host);
            }
        }

        // Same tile grid: viewport pixels change, no tile upload or allocation.
        renderer
            .draw(&model, &target, Viewport::new(95, 63, 1.), true, true)
            .unwrap();
        let sub_tile = renderer.resource_stats();
        assert_eq!(sub_tile.tile_uploads_total, stats.tile_uploads_total);
        assert_eq!(
            sub_tile.buffer_allocations_total,
            stats.buffer_allocations_total
        );
        assert_eq!(
            sub_tile.bind_group_creations_total,
            stats.bind_group_creations_total
        );

        // Replacing geometry with a smaller scene must keep all three capacities.
        let smaller = Button::from_sources(
                "component Demo { Frame { width:64; height:64; padding:0; Button { width:64; height:64; } } }",
                "component Button { Rectangle { radius:8; background:#ffffff; } }",
            ).unwrap();
        renderer.draw(&smaller, &target, vp96, true, true).unwrap();
        let shrunk = renderer.resource_stats();
        assert_eq!(shrunk.buffer_bytes, stats.buffer_bytes);
        assert_eq!(
            shrunk.buffer_allocations_total,
            stats.buffer_allocations_total
        );
        assert_eq!(
            shrunk.bind_group_creations_total,
            stats.bind_group_creations_total
        );
        renderer.draw(&model, &target, vp96, true, true).unwrap();
        let restored = renderer.resource_stats();
        assert_eq!(restored.buffer_bytes, stats.buffer_bytes);
        assert_eq!(
            restored.buffer_allocations_total,
            stats.buffer_allocations_total
        );
        assert_eq!(
            restored.bind_group_creations_total,
            stats.bind_group_creations_total
        );

        // Growing the viewport enough needs a larger tile buffer exactly once;
        // returning to a smaller viewport retains that allocation and bind group.
        let large_texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Forma profiling resize test target"),
            size: wgpu::Extent3d {
                width: 1024,
                height: 1024,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        renderer
            .draw(
                &model,
                &large_texture.create_view(&Default::default()),
                Viewport::new(1024, 1024, 1.),
                true,
                true,
            )
            .unwrap();
        let grown = renderer.resource_stats();
        assert_eq!(
            grown.buffer_allocations_total,
            stats.buffer_allocations_total + 1
        );
        assert_eq!(
            grown.bind_group_creations_total,
            stats.bind_group_creations_total + 1
        );
        assert!(grown.tile_bytes > stats.tile_bytes);
        renderer.draw(&model, &target, vp96, true, true).unwrap();
        let back = renderer.resource_stats();
        assert_eq!(back.buffer_bytes, grown.buffer_bytes);
        assert_eq!(
            back.buffer_allocations_total,
            grown.buffer_allocations_total
        );
        assert_eq!(
            back.bind_group_creations_total,
            grown.bind_group_creations_total
        );
        let stable_uploads = renderer.uploads;
        model.pointer(100., 90., 0);
        model.focus(true);
        model.tick(16.);
        renderer.draw(&model, &target, vp96, true, true).unwrap();
        assert_eq!(renderer.uploads, stable_uploads);
        assert!(Arc::ptr_eq(
            renderer.geometry.as_ref().unwrap(),
            &model.vector_snapshot(1., true)
        ));

        let mut expected_uploads = stable_uploads;
        for (scale, background) in [(2., true), (2., false), (1., true)] {
            renderer
                .draw(
                    &model,
                    &target,
                    Viewport::new(96, 64, scale),
                    background,
                    true,
                )
                .unwrap();
            expected_uploads += 1;
            assert_eq!(renderer.uploads, expected_uploads);
        }
        model.load_source(crate::EXAMPLE).unwrap();
        renderer.draw(&model, &target, vp96, true, true).unwrap();
        expected_uploads += 1;
        assert_eq!(renderer.uploads, expected_uploads);
        model
            .load_component(crate::EXAMPLE, crate::BUTTON_COMPONENT)
            .unwrap();
        renderer.draw(&model, &target, vp96, true, true).unwrap();
        expected_uploads += 1;
        assert_eq!(renderer.uploads, expected_uploads);
        let held = Arc::clone(renderer.geometry.as_ref().unwrap());
        model = Button::new();
        renderer.draw(&model, &target, vp96, true, true).unwrap();
        expected_uploads += 1;
        assert_eq!(renderer.uploads, expected_uploads);
        assert!(!Arc::ptr_eq(&held, renderer.geometry.as_ref().unwrap()));

        let mut scrolling = Button::from_source(
                "component Demo { Frame { width:40; height:30; Scroll { Button { width:80; height:60; text:'О'; } } } }",
            ).unwrap();
        renderer
            .draw(&scrolling, &target, vp96, true, true)
            .unwrap();
        let before_scroll = renderer.uploads;
        let tiles_before_scroll = renderer.resource_stats().tile_uploads_total;
        let params_before_scroll = renderer.resource_stats().uploaded_bytes_total;
        scrolling.scroll(5., 5.);
        renderer
            .draw(&scrolling, &target, vp96, true, true)
            .unwrap();
        // Scrolling re-bins tiles and updates the uniform; geometry stays resident.
        assert_eq!(renderer.uploads, before_scroll);
        assert_eq!(
            renderer.resource_stats().tile_uploads_total,
            tiles_before_scroll + 1
        );
        assert!(renderer.resource_stats().uploaded_bytes_total > params_before_scroll);
        scrolling.scroll(0., 0.);
        let bytes = renderer.resource_stats().uploaded_bytes_total;
        renderer
            .draw(&scrolling, &target, vp96, true, true)
            .unwrap();
        assert_eq!(renderer.uploads, before_scroll);
        assert_eq!(renderer.resource_stats().uploaded_bytes_total, bytes);
        renderer
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        // Completion service must not require another draw/present.
        let frames_before_idle = renderer.frames;
        renderer
            .device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .unwrap();
        assert!(device_queue_idle(&renderer.device));
        for _ in 0..3 {
            renderer.check_health().unwrap();
        }
        assert_eq!(renderer.frames, frames_before_idle);
        eprintln!("GPU_IDLE_COMPLETION_RESULT queue_drained=true health_without_draw=true");
        // Finishing an invalid compute dispatch reaches the Device handler;
        // no invalid commands are submitted to hardware. Upstream wgpu defers
        // this validation until finish, while the in-tree backend reports eagerly.
        let frames_before_fault = renderer.frames;
        let uploads_before_fault = renderer.resource_stats().uploaded_bytes_total;
        let mut invalid_encoder = renderer.device.create_command_encoder(&Default::default());
        {
            let mut pass = invalid_encoder.begin_compute_pass(&Default::default());
            pass.dispatch_workgroups(1, 1, 1);
        }
        drop(invalid_encoder.finish());
        let first_error = renderer.check_health().unwrap_err();
        assert!(
            missing_compute_pipeline_error(&first_error),
            "expected a missing compute pipeline diagnostic, got {first_error}"
        );
        // Device-level descriptor validation follows the same handler and
        // cannot replace the first actionable cause in the renderer latch.
        let invalid_buffer = renderer.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("validation delivery probe"),
            size: 4,
            usage: wgpu::BufferUsages::empty(),
            mapped_at_creation: false,
        });
        drop(invalid_buffer);
        assert_eq!(renderer.check_health().unwrap_err(), first_error);
        assert_eq!(
            renderer
                .draw(&model, &target, vp96, true, true)
                .unwrap_err(),
            first_error
        );
        assert_eq!(renderer.frames, frames_before_fault);
        assert_eq!(
            renderer.resource_stats().uploaded_bytes_total,
            uploads_before_fault
        );
        eprintln!(
            "GPU_FAULT_LATCH_RESULT encoder_validation_delivered=true first_error_preserved=true submission_blocked=true"
        );
        println!(
            "GPU profiling smoke: {stats:?}; allocator={:?}",
            renderer.backend_allocator_stats()
        );
    });
}
