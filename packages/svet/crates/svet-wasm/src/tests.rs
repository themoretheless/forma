use super::*;

#[test]
fn c_abi_lis_and_dirty_spans_share_output_view_safely() {
    svet_reset();

    // 1. Buffered LIS reconciliation via shared `scratch_in` + `scratch_out`
    let input = [2u32, 1, 5, 3, 6, 4, 8, 9, 7];
    let in_ptr = svet_prepare_lis_input(input.len());
    unsafe {
        std::slice::from_raw_parts_mut(in_ptr, input.len()).copy_from_slice(&input);
    }
    let lis_len = svet_compute_lis_buffered(input.len());
    assert_eq!(lis_len, 5);
    let lis_indices = unsafe { std::slice::from_raw_parts(svet_get_lis_ptr(), lis_len) };
    let lis_values: Vec<u32> = lis_indices.iter().map(|&idx| input[idx as usize]).collect();
    assert_eq!(lis_values, vec![1, 3, 4, 8, 9]);

    // 2. DUMA dirty spans read from the same `svet_get_lis_ptr()` contract
    let off_a = svet_gpu_alloc_f32(64);
    let off_b = svet_gpu_alloc_f32(64);
    assert_eq!(off_a, 0);
    assert_eq!(off_b, 64);
    svet_gpu_clear_dirty();
    svet_gpu_mark_dirty(70, 8);
    let span_count = svet_gpu_collect_dirty_spans();
    assert_eq!(span_count, 1);
    let spans = unsafe { std::slice::from_raw_parts(svet_get_lis_ptr(), span_count * 2) };
    assert_eq!(spans, &[64, 32]);

    svet_reset();
}

#[test]
fn c_abi_alloc_and_free_u32_handle_zero_and_exact_sizes() {
    assert!(svet_alloc_u32(0).is_null());
    unsafe {
        svet_free_u32(std::ptr::null_mut(), 0);
    }

    for len in [1usize, 3, 17, 1024] {
        let ptr = svet_alloc_u32(len);
        assert!(!ptr.is_null());
        unsafe {
            let slice = std::slice::from_raw_parts_mut(ptr, len);
            assert!(slice.iter().all(|&v| v == 0));
            slice.fill(0xA5A5_5A5A);
            let lis_len = svet_compute_lis(ptr, len);
            assert_eq!(lis_len, 1);
            svet_free_u32(ptr, len);
        }
    }
    svet_reset();
}

#[test]
fn c_abi_buffered_source_sync_and_tracking_roundtrip() {
    svet_reset();
    let s0 = svet_create_node(KIND_SIGNAL);
    let s1 = svet_create_node(KIND_SIGNAL);
    let eff = svet_create_node(KIND_EFFECT);

    let in_ptr = svet_prepare_lis_input(2);
    unsafe {
        std::slice::from_raw_parts_mut(in_ptr, 2).copy_from_slice(&[s0, s1]);
    }
    svet_sync_sources_buffered(eff, 2);
    assert_eq!(svet_get_edge_count(), 2);
    assert_eq!(svet_depends_on(eff, s0), 1);
    assert_eq!(svet_depends_on(eff, s1), 1);

    assert_eq!(svet_is_tracking(), 0);
    svet_begin_tracking(eff);
    assert_eq!(svet_is_tracking(), 1);
    svet_track_read(s0);
    svet_end_tracking();
    assert_eq!(svet_is_tracking(), 0);
    assert_eq!(svet_get_edge_count(), 1);
    assert_eq!(svet_depends_on(eff, s1), 0);

    svet_reset();
}

#[test]
fn c_abi_gpu_math_and_picking_roundtrip() {
    svet_reset();
    assert_eq!(svet_gpu_get_arena_limit_f32(), 67_108_864);
    assert_eq!(svet_gpu_valid_perspective(1.0, 1.0, 0.1, 100.0), 1);
    assert_eq!(svet_gpu_valid_perspective(0.0, 1.0, 0.1, 100.0), 0);

    let mat = svet_gpu_alloc_f32(16);
    let verts = svet_gpu_alloc_f32(4);
    let out = svet_gpu_alloc_f32(4);
    svet_gpu_mat4_compose_trs(mat, 0.5, -0.25, 0.2, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
    unsafe {
        let base = svet_gpu_get_arena_ptr();
        std::slice::from_raw_parts_mut(base.add(verts as usize), 4)
            .copy_from_slice(&[0.0, 0.0, 0.0, 1.0]);
    }
    svet_gpu_transform_positions_vec4(mat, verts, out, 1);
    assert_eq!(
        svet_gpu_pick_nearest_vertex_3d(out, 1, 200.0, 200.0, 150.0, 125.0, 2.0),
        0
    );
    svet_reset();
}

#[cfg(not(target_arch = "wasm32"))]
mod native_abi_thread_tests {
    use super::*;

    #[test]
    fn native_abi_threads_are_isolated() {
        svet_reset();
        let source = svet_create_node(KIND_SIGNAL);
        let child = std::thread::spawn(|| {
            assert_eq!(
                svet_get_node_count(),
                0,
                "native ABI inherited another thread's graph"
            );
            let own = svet_create_node(KIND_SIGNAL);
            assert_eq!(svet_get_node_count(), 1);
            svet_dispose_node(own);
            assert_eq!(svet_get_node_count(), 0);
        });
        let result = child.join();
        let count = svet_get_node_count();
        svet_dispose_node(source);
        svet_reset();
        result.unwrap();
        assert_eq!(count, 1, "another thread changed the caller's graph");
    }

    #[test]
    fn native_abi_concurrent_flush_buffers_are_thread_local() {
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    svet_reset();
                    let source = svet_create_node(KIND_SIGNAL);
                    let observer = svet_create_node(KIND_EFFECT);
                    svet_link_1(observer, source);
                    svet_set_state(observer, STATE_CLEAN);
                    barrier.wait();
                    for _ in 0..256 {
                        svet_trigger_write(source);
                        assert_eq!(svet_prepare_flush(), 1);
                        assert_eq!(unsafe { *svet_get_flush_ptr() }, observer);
                        svet_set_state(observer, STATE_CLEAN);
                        std::thread::yield_now();
                    }
                    assert_eq!(svet_get_node_count(), 2);
                    svet_dispose_node(observer);
                    svet_dispose_node(source);
                    assert_eq!(svet_get_node_count(), 0);
                    svet_reset();
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
    }
}
