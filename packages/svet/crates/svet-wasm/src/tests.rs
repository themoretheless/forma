use super::*;
use crate::gpu_math::{grow_gpu_arena, perspective_matrix};
use crate::graph::edge_key;

#[test]
fn test_diamond_dependency_glitch_free() {
    let mut g = ReactiveGraph::new();
    let a = g.create_node(KIND_SIGNAL);
    let b = g.create_node(KIND_COMPUTED);
    let c = g.create_node(KIND_COMPUTED);
    let d = g.create_node(KIND_EFFECT);

    g.begin_tracking(b);
    g.track_read(a);
    g.end_tracking();

    g.begin_tracking(c);
    g.track_read(a);
    g.end_tracking();

    g.begin_tracking(d);
    g.track_read(b);
    g.track_read(c);
    g.end_tracking();

    g.trigger_write(a);

    assert_eq!(g.get_state(b), STATE_DIRTY);
    assert_eq!(g.get_state(c), STATE_DIRTY);

    let count = g.prepare_flush();
    assert_eq!(count, 1);
    assert_eq!(g.flush_buffer[0], d);
}

#[test]
fn test_free_list_slot_recycling() {
    let mut g = ReactiveGraph::new();
    let n0 = g.create_node(KIND_SIGNAL);
    let n1 = g.create_node(KIND_EFFECT);
    assert_eq!(g.active_count, 2);
    assert_eq!(g.nodes.len(), 2);

    g.dispose_node(n1);
    assert_eq!(g.active_count, 1);
    assert_eq!(g.nodes.len(), 2);

    // Next allocated node MUST reuse slot `n1` without growing `nodes` vector
    let n2 = g.create_node(KIND_EFFECT);
    assert_eq!(n2, n1);
    assert_eq!(g.active_count, 2);
    assert_eq!(g.nodes.len(), 2);
    let _ = n0;
}

#[test]
fn test_lis_reconciler() {
    let mut g = ReactiveGraph::new();
    let input = [2, 1, 5, 3, 6, 4, 8, 9, 7];
    let len = g.compute_lis(&input);
    assert_eq!(len, 5);
    let lis_values: Vec<u32> = g
        .lis_buffer
        .iter()
        .map(|&idx| input[idx as usize])
        .collect();
    assert_eq!(lis_values, vec![1, 3, 4, 8, 9]);
}

#[test]
fn test_gpu_uma_duma_dirty_page_tracking() {
    let mut g = ReactiveGraph::new();
    let off_a = g.gpu_alloc_f32(64);
    let off_b = g.gpu_alloc_f32(64);
    assert_eq!(off_a, 0);
    assert_eq!(off_b, 64);

    g.gpu_clear_dirty();
    assert_eq!(g.gpu_dirty_min, u32::MAX);
    assert_eq!(g.gpu_dirty_max, 0);

    // Mutate a slice inside tensor B [70..78]
    g.gpu_mark_dirty(70, 8);
    assert_eq!(g.gpu_dirty_min, 70);
    assert_eq!(g.gpu_dirty_max, 78);
}
#[test]
fn test_gpu_arena_growth_caps_reserved_storage() {
    let mut arena = Vec::new();
    assert!(grow_gpu_arena(&mut arena, 40, 64));
    arena[0] = 17.0;
    assert!(grow_gpu_arena(&mut arena, 44, 64));
    assert_eq!(arena.len(), 44);
    assert!(
        arena.capacity() <= 64,
        "reserved {} floats above limit 64",
        arena.capacity()
    );
    assert_eq!(arena[0], 17.0);
    assert!(arena[40..].iter().all(|&value| value == 0.0));
}

#[test]
fn test_gpu_arena_growth_is_amortized_and_preserves_zeroed_tail() {
    let mut arena = Vec::new();
    let mut changes = 0;
    for length in 1..=64 {
        let previous_capacity = arena.capacity();
        assert!(grow_gpu_arena(&mut arena, length, 64));
        changes += (previous_capacity != arena.capacity()) as usize;
        assert!(arena.capacity() <= 64);
        assert_eq!(arena[length - 1], 0.0);
        if length > 1 {
            assert_eq!(arena[0], 17.0);
        }
        arena[0] = 17.0;
    }
    assert!(
        changes < 16,
        "growth reserved on too many individual steps: {changes}"
    );
    let pointer = arena.as_ptr();
    assert!(!grow_gpu_arena(&mut arena, 65, 64));
    assert_eq!(arena.as_ptr(), pointer);
    assert_eq!(arena.len(), 64);
    assert_eq!(arena[0], 17.0);
}

#[test]
fn test_gpu_arena_fragmented_churn_reuses_storage_after_warmup() {
    let mut g = ReactiveGraph::new();
    // Keep a live prefix while differently sized neighbours are repeatedly
    // split and coalesced. No reset may hide retained arena growth.
    let anchor = g.gpu_alloc_f32(16);
    g.gpu_f32_arena[..16].fill(17.0);
    let sizes: Vec<u32> = (0..128).map(|i| 1 + (i * 37 % 251)).collect();
    let total: usize = sizes.iter().map(|n| ((n + 3) & !3) as usize).sum();
    let mut warmed_storage = None;
    let mut seed = 0x12345678u32;
    for cycle in 0..2000 {
        let mut blocks = Vec::new();
        for &size in &sizes {
            let offset = g.gpu_alloc_f32(size);
            assert_ne!(offset, u32::MAX);
            let range = offset as usize..offset as usize + size as usize;
            assert!(g.gpu_f32_arena[range.clone()].iter().all(|&v| v == 0.0));
            g.gpu_f32_arena[range].fill(cycle as f32 + 1.0);
            blocks.push((offset, size));
        }
        assert_eq!(g.gpu_f32_arena.len(), 16 + total);
        assert!(g.gpu_f32_arena[..16].iter().all(|&v| v == 17.0));
        // Deterministic Fisher-Yates order exercises both coalescing sides.
        for i in (1..blocks.len()).rev() {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            blocks.swap(i, seed as usize % (i + 1));
        }
        for (offset, size) in blocks {
            g.gpu_free_f32(offset, size);
        }
        assert_eq!(g.gpu_free_blocks, vec![(16, total as u32)]);
        g.gpu_clear_dirty();
        let storage = (
            g.gpu_f32_arena.as_ptr(),
            g.gpu_f32_arena.capacity(),
            g.gpu_dirty_blocks.capacity(),
        );
        if let Some(expected) = warmed_storage {
            assert_eq!(storage, expected);
        } else {
            warmed_storage = Some(storage);
        }
        assert!(g.gpu_free_blocks.capacity() <= 256);
    }
    g.gpu_free_f32(anchor, 16);
    assert_eq!(g.gpu_free_blocks, vec![(0, (16 + total) as u32)]);
}

#[test]
fn test_gpu_arena_reservation_overflow_preserves_existing_storage() {
    let mut arena = vec![17.0; 4];
    let capacity = arena.capacity();
    let pointer = arena.as_ptr();
    assert!(!grow_gpu_arena(&mut arena, usize::MAX, usize::MAX));
    assert_eq!(arena.as_ptr(), pointer);
    assert_eq!(arena.capacity(), capacity);
    assert_eq!(arena, vec![17.0; 4]);
}

#[test]
fn test_gpu_allocation_failure_preserves_existing_arena() {
    let mut g = ReactiveGraph::new();
    assert_eq!(g.gpu_alloc_f32(4), 0);
    g.gpu_f32_arena[0] = 42.0;
    assert_eq!(g.gpu_alloc_f32(u32::MAX), u32::MAX);
    let limit = svet_gpu_get_arena_limit_f32();
    assert_eq!(limit, 67_108_864);
    assert_eq!(g.gpu_alloc_f32(limit + 1), u32::MAX);
    assert_eq!(g.gpu_f32_arena.len(), 4);
    assert_eq!(g.gpu_f32_arena[0], 42.0);
}

#[test]
fn test_gpu_picker_matches_webgpu_viewport_projection() {
    let mut g = ReactiveGraph::new();
    let offset = g.gpu_alloc_f32(4);
    g.gpu_f32_arena[..4].copy_from_slice(&[0.8, 0.4, 0.2, 1.0]);
    assert_eq!(
        g.gpu_pick_nearest_vertex_3d(offset, 1, 200.0, 200.0, 180.0, 60.0, 1.0),
        0
    );
    assert_eq!(
        g.gpu_pick_nearest_vertex_3d(offset, 1, 200.0, 200.0, 152.0, 74.0, 1.0),
        -1
    );
}

#[test]
fn test_gpu_picker_rejects_clipped_vertices_and_invalid_queries() {
    let mut g = ReactiveGraph::new();
    let offset = g.gpu_alloc_f32(4);
    for vertex in [
        [0.0, 0.0, 0.2, -1.0],
        [0.0, 0.0, -0.1, 1.0],
        [0.0, 0.0, 1.1, 1.0],
        [1.1, 0.0, 0.2, 1.0],
        [0.0, 1.1, 0.2, 1.0],
        [0.0, 0.0, 0.0, 0.0],
        [f32::NAN, 0.0, 0.2, 1.0],
    ] {
        g.gpu_f32_arena[..4].copy_from_slice(&vertex);
        assert_eq!(
            g.gpu_pick_nearest_vertex_3d(offset, 1, 200.0, 200.0, 100.0, 100.0, 1000.0),
            -1
        );
    }
    for w in [0.000001, f32::MIN_POSITIVE, f32::from_bits(1)] {
        g.gpu_f32_arena[..4].copy_from_slice(&[0.0, 0.0, 0.0, w]);
        assert_eq!(
            g.gpu_pick_nearest_vertex_3d(offset, 1, 200.0, 200.0, 100.0, 100.0, 0.0),
            0
        );
    }
    g.gpu_f32_arena[..4].copy_from_slice(&[0.0, 0.0, 0.0000005, 0.000001]);
    assert_eq!(
        g.gpu_pick_nearest_vertex_3d(offset, 1, 200.0, 200.0, 100.0, 100.0, 0.0),
        0
    );
    assert_eq!(
        g.gpu_pick_nearest_vertex_3d(offset, 1, 0.0, 200.0, 100.0, 100.0, 1.0),
        -1
    );
    assert_eq!(
        g.gpu_pick_nearest_vertex_3d(offset, 1, 200.0, 200.0, 100.0, 100.0, -1.0),
        -1
    );
    assert_eq!(
        g.gpu_pick_nearest_vertex_3d(offset, 1, 200.0, 200.0, f32::NAN, 100.0, 1.0),
        -1
    );
}

#[test]
fn test_homogeneous_segment_clipping() {
    let mut g = ReactiveGraph::new();
    let offset = g.gpu_alloc_f32(8);
    g.gpu_f32_arena[..8].copy_from_slice(&[-0.5, 0.0, -0.5, 1.0, 0.5, 0.0, 0.5, 1.0]);
    assert_eq!(g.gpu_clip_primitive_3d(offset, 2, &[0, 1]), 2);
    assert_eq!(
        g.gpu_clip_output,
        vec![0.0, 0.0, 0.0, 1.0, 0.5, 0.0, 0.5, 1.0]
    );
    g.gpu_f32_arena[..8].copy_from_slice(&[-2.0, 0.0, 0.5, 1.0, 2.0, 0.0, 0.5, 1.0]);
    assert_eq!(g.gpu_clip_primitive_3d(offset, 2, &[0, 1]), 2);
    assert_eq!(
        g.gpu_clip_output,
        vec![-1.0, 0.0, 0.5, 1.0, 1.0, 0.0, 0.5, 1.0]
    );
    assert_eq!(g.gpu_clip_primitive_3d(offset, 2, &[0, 2]), 0);
    assert!(g.gpu_clip_output.is_empty());
    assert_eq!(g.gpu_clip_primitive_3d(u32::MAX, u32::MAX, &[0, 1]), 0);
}

#[test]
fn test_homogeneous_polygon_clipping() {
    let mut g = ReactiveGraph::new();
    let offset = g.gpu_alloc_f32(16);
    g.gpu_f32_arena[..16].copy_from_slice(&[
        -0.5, -0.5, -0.5, 1.0, 0.5, -0.5, 0.5, 1.0, 0.5, 0.5, 0.5, 1.0, -0.5, 0.5, -0.5, 1.0,
    ]);
    assert_eq!(g.gpu_clip_primitive_3d(offset, 4, &[0, 1, 2, 3]), 4);
    for v in g.gpu_clip_output.chunks_exact(4) {
        assert!(v[0] >= 0.0 && v[0] <= 0.5);
        assert!(v[2] >= 0.0 && v[2] <= v[3]);
    }
    // A completely clipped polygon must clear the previous scratch result.
    g.gpu_f32_arena[..16].fill(-1.0);
    assert_eq!(g.gpu_clip_primitive_3d(offset, 4, &[0, 1, 2, 3]), 0);
    assert!(g.gpu_clip_output.is_empty());
    g.gpu_f32_arena[0] = f32::NAN;
    assert_eq!(g.gpu_clip_primitive_3d(offset, 4, &[0, 1, 2, 3]), 0);
}

#[test]
fn test_segment_clipping_all_planes_and_seeded_reference_samples() {
    let mut g = ReactiveGraph::new();
    let offset = g.gpu_alloc_f32(8);
    for outside in [
        [-2.0, 0.0, 0.5, 1.0],
        [2.0, 0.0, 0.5, 1.0],
        [0.0, -2.0, 0.5, 1.0],
        [0.0, 2.0, 0.5, 1.0],
        [0.0, 0.0, -1.0, 1.0],
        [0.0, 0.0, 2.0, 1.0],
    ] {
        g.gpu_f32_arena[..4].copy_from_slice(&outside);
        g.gpu_f32_arena[4..8].copy_from_slice(&[0.0, 0.0, 0.5, 1.0]);
        assert_eq!(g.gpu_clip_primitive_3d(offset, 2, &[0, 1]), 2);
        let forward = g.gpu_clip_output.clone();
        assert_eq!(g.gpu_clip_primitive_3d(offset, 2, &[1, 0]), 2);
        assert_eq!(&forward[..4], &g.gpu_clip_output[4..]);
    }
    let mut seed: u32 = 42;
    let mut next = || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 8) as f32 / 16777216.0 * 4.0 - 2.0
    };
    for _ in 0..500 {
        let a = [next(), next(), next(), next() + 1.0];
        let b = [next(), next(), next(), next() + 1.0];
        g.gpu_f32_arena[..4].copy_from_slice(&a);
        g.gpu_f32_arena[4..8].copy_from_slice(&b);
        let size = g.gpu_clip_primitive_3d(offset, 2, &[0, 1]);
        let mut sampled_inside = false;
        for step in 0..=32 {
            let t = step as f32 / 32.0;
            let v: [f32; 4] = std::array::from_fn(|axis| a[axis] + (b[axis] - a[axis]) * t);
            sampled_inside |= v[3] > 0.0
                && v[0].abs() <= v[3]
                && v[1].abs() <= v[3]
                && v[2] >= 0.0
                && v[2] <= v[3];
        }
        if sampled_inside {
            assert_eq!(size, 2);
        }
        for v in g.gpu_clip_output.chunks_exact(4) {
            assert!(v.iter().all(|value| value.is_finite()));
            assert!(v[3] > 0.0 && v[0].abs() <= v[3] + 1e-5 && v[1].abs() <= v[3] + 1e-5);
            assert!(v[2] >= -1e-5 && v[2] <= v[3] + 1e-5);
        }
    }
}

#[test]
fn test_visible_vertex_picker_skips_hidden_candidate_and_finds_visible_neighbor() {
    let mut g = ReactiveGraph::new();
    let offset = g.gpu_alloc_f32(20);
    g.gpu_f32_arena[..20].copy_from_slice(&[
        -0.8, -0.8, 0.2, 1.0, 0.8, -0.8, 0.2, 1.0, 0.0, 0.8, 0.2, 1.0, 0.0, 0.0, 0.8, 1.0, 0.15,
        0.0, 0.1, 1.0,
    ]);
    g.gpu_pick_indices = vec![0, 1, 2];
    assert_eq!(
        g.gpu_pick_nearest_vertex_3d(offset, 4, 200.0, 200.0, 100.0, 100.0, 20.0),
        3
    );
    assert_eq!(
        g.gpu_pick_visible_vertex_3d(offset, 4, 3, 200.0, 200.0, 100.0, 100.0, 20.0),
        -1
    );
    assert_eq!(
        g.gpu_pick_visible_vertex_3d(offset, 5, 3, 200.0, 200.0, 100.0, 100.0, 20.0),
        4
    );
    assert_eq!(
        g.gpu_pick_visible_vertex_3d(offset, 5, 3, 200.0, 200.0, 20.0, 180.0, 1.0),
        0
    );
    g.gpu_f32_arena[14] = 0.2;
    assert_eq!(
        g.gpu_pick_visible_vertex_3d(offset, 5, 3, 200.0, 200.0, 100.0, 100.0, 1.0),
        3
    );
}

#[test]
fn test_visible_picker_clips_triangles_and_handles_perspective_winding() {
    let mut g = ReactiveGraph::new();
    let offset = g.gpu_alloc_f32(16);
    g.gpu_f32_arena[..16].copy_from_slice(&[
        -0.8, -0.8, -0.2, 1.0, 0.8, -0.8, 0.2, 1.0, 0.0, 0.8, 0.2, 1.0, 0.0, 0.0, 0.8, 1.0,
    ]);
    g.gpu_pick_indices = vec![0, 1, 2];
    assert_eq!(
        g.gpu_pick_visible_vertex_3d(offset, 4, 3, 200.0, 200.0, 100.0, 100.0, 1.0),
        -1
    );
    g.gpu_f32_arena[..12].copy_from_slice(&[
        -0.8, -0.8, 0.2, 1.0, 1.6, -1.6, 0.4, 2.0, 0.0, 0.4, 0.1, 0.5,
    ]);
    g.gpu_pick_indices = vec![2, 1, 0];
    assert_eq!(
        g.gpu_pick_visible_vertex_3d(offset, 4, 3, 200.0, 200.0, 100.0, 100.0, 1.0),
        -1
    );
    g.gpu_pick_indices.clear();
    assert_eq!(
        g.gpu_pick_visible_vertex_3d(offset, 4, 0, 200.0, 200.0, 100.0, 100.0, 1.0),
        3
    );
}

#[test]
fn test_snapshot_picker_preserves_live_geometry_and_owns_only_scratch() {
    let mut g = ReactiveGraph::new();
    g.create_node(KIND_SIGNAL);
    g.gpu_f32_arena = vec![111.0, 222.0, 333.0, 444.0];
    g.gpu_pick_vertices = vec![
        -0.8, -0.8, 0.2, 1.0, 0.8, -0.8, 0.2, 1.0, 0.0, 0.8, 0.2, 1.0, 0.0, 0.0, 0.8, 1.0,
    ];
    g.gpu_pick_indices = vec![0, 1, 2];
    let pointer = g.gpu_pick_vertices.as_ptr();
    assert_eq!(
        g.gpu_pick_snapshot_3d(4, 3, 1, 200.0, 200.0, 100.0, 100.0, 1.0),
        -1
    );
    assert_eq!(
        g.gpu_pick_snapshot_3d(4, 3, 0, 200.0, 200.0, 100.0, 100.0, 1.0),
        3
    );
    assert_eq!(g.gpu_f32_arena, vec![111.0, 222.0, 333.0, 444.0]);
    assert_eq!(g.active_count, 1);
    assert_eq!(g.gpu_pick_vertices.as_ptr(), pointer);
    assert_eq!(g.gpu_pick_indices, vec![0, 1, 2]);
}

#[test]
fn perspective_validation_rejects_invalid_and_degenerate_float32_parameters() {
    for p in [
        [0.0, 1.0, 0.1, 100.0],
        [std::f32::consts::PI, 1.0, 0.1, 100.0],
        [1.0, 0.0, 0.1, 100.0],
        [1.0, 1.0, 0.0, 100.0],
        [1.0, 1.0, 10.0, 10.0],
        [f32::NAN, 1.0, 0.1, 100.0],
        [1.0, f32::INFINITY, 0.1, 100.0],
        [f32::from_bits(1), 1.0, 0.1, 100.0],
        [1.0, f32::from_bits(1), 0.1, 100.0],
    ] {
        assert!(perspective_matrix(p[0], p[1], p[2], p[3]).is_none());
    }
}

#[test]
fn perspective_rejection_preserves_destination_and_dirty_state() {
    let mut g = ReactiveGraph::new();
    let offset = g.gpu_alloc_f32(16);
    g.gpu_f32_arena[..16].fill(7.0);
    g.gpu_clear_dirty();
    g.gpu_mat4_perspective(offset, 1.0, 0.0, 0.1, 100.0);
    assert_eq!(g.gpu_f32_arena, vec![7.0; 16]);
    assert_eq!(g.gpu_dirty_min, u32::MAX);
}

#[test]
fn perspective_large_depth_range_avoids_intermediate_overflow() {
    let m = perspective_matrix(1.0, 1.0, 1.0e20, 1.0e30).unwrap();
    assert!(m.iter().all(|v| v.is_finite()));
    assert!(m[14] < -9.0e19);
    for (z, expected) in [(-1.0e20, 0.0), (-1.0e30, 1.0)] {
        let depth = (m[10] * z + m[14]) / -z;
        assert!((depth - expected).abs() < 1.0e-6);
    }
}

#[test]
fn dependency_reachability_handles_diamonds_cycles_and_disposal() {
    let mut g = ReactiveGraph::new();
    let s = g.create_node(KIND_SIGNAL);
    let a = g.create_node(KIND_COMPUTED);
    let b = g.create_node(KIND_COMPUTED);
    let c = g.create_node(KIND_COMPUTED);
    g.link(s, a);
    g.link(s, b);
    g.link(a, c);
    g.link(b, c);
    assert!(g.depends_on(c, s));
    assert!(!g.depends_on(s, c));
    g.link(c, a);
    let independent = g.create_node(KIND_SIGNAL);
    assert!(!g.depends_on(c, independent));
    g.dispose_node(s);
    assert!(!g.depends_on(c, s));
}

#[test]
fn dependency_queries_reuse_scratch_and_leave_reactive_state_unchanged() {
    let mut g = ReactiveGraph::new();
    let source = g.create_node(KIND_SIGNAL);
    let derived = g.create_node(KIND_COMPUTED);
    g.link(source, derived);
    assert!(g.depends_on(derived, source));
    let stack = g.dependency_stack.as_ptr();
    let seen = g.dependency_seen.as_ptr();
    let states = g.node_states.clone();
    let topology = g.topology_epoch;
    let clock = g.global_epoch;
    for _ in 0..100 {
        assert!(g.depends_on(derived, source));
    }
    assert_eq!(g.dependency_stack.as_ptr(), stack);
    assert_eq!(g.dependency_seen.as_ptr(), seen);
    assert_eq!(g.node_states, states);
    assert_eq!(g.topology_epoch, topology);
    assert_eq!(g.global_epoch, clock);
    g.dependency_epoch = u32::MAX;
    assert!(g.depends_on(derived, source));
    assert_eq!(g.node_states, states);
}

#[test]
fn pending_queue_does_not_run_a_replacement_that_was_never_scheduled() {
    let mut g = ReactiveGraph::new();
    let source = g.create_node(KIND_SIGNAL);
    let old = g.create_node(KIND_EFFECT);
    g.sync_sources(old, &[source]);
    g.trigger_write(source);
    g.dispose_node(old);
    let fresh = g.create_node(KIND_EFFECT);
    assert_eq!(fresh, old);
    assert_eq!(g.prepare_flush(), 0);
}

#[test]
fn pending_queue_preserves_real_schedule_order_when_a_slot_is_reused() {
    let mut g = ReactiveGraph::new();
    let a = g.create_node(KIND_SIGNAL);
    let b = g.create_node(KIND_SIGNAL);
    let c = g.create_node(KIND_SIGNAL);
    let old = g.create_node(KIND_EFFECT);
    let peer = g.create_node(KIND_EFFECT);
    g.sync_sources(old, &[a]);
    g.sync_sources(peer, &[b]);
    g.trigger_write(a);
    g.trigger_write(b);
    g.dispose_node(old);
    let fresh = g.create_node(KIND_EFFECT);
    assert_eq!(fresh, old);
    g.sync_sources(fresh, &[c]);
    g.trigger_write(c);
    assert_eq!(g.prepare_flush(), 2);
    assert_eq!(g.flush_buffer, vec![peer, fresh]);
    assert_eq!(g.prepare_flush(), 0);
}

#[test]
fn pending_queue_rejects_replacement_signal_and_computed_slots() {
    for kind in [KIND_SIGNAL, KIND_COMPUTED] {
        let mut g = ReactiveGraph::new();
        let source = g.create_node(KIND_SIGNAL);
        let old = g.create_node(KIND_EFFECT);
        g.sync_sources(old, &[source]);
        g.trigger_write(source);
        g.dispose_node(old);
        assert_eq!(g.create_node(kind), old);
        assert_eq!(g.prepare_flush(), 0);
    }
}

#[test]
fn pending_lifetime_wrap_purges_old_tokens_without_dropping_other_jobs() {
    let mut g = ReactiveGraph::new();
    let a = g.create_node(KIND_SIGNAL);
    let b = g.create_node(KIND_SIGNAL);
    let old = g.create_node(KIND_EFFECT);
    let peer = g.create_node(KIND_EFFECT);
    g.pending_effects.push(ScheduledEffect {
        id: old,
        lifetime: 1,
    });
    g.nodes[old as usize].lifetime = u32::MAX;
    g.sync_sources(old, &[a]);
    g.sync_sources(peer, &[b]);
    g.trigger_write(a);
    g.trigger_write(b);
    g.dispose_node(old);
    let fresh = g.create_node(KIND_EFFECT);
    assert_eq!(fresh, old);
    assert_eq!(g.nodes[fresh as usize].lifetime, 1);
    assert!(g.pending_effects.iter().all(|pending| pending.id != old));
    assert_eq!(g.pending_effect_count(), 1);
    g.sync_sources(fresh, &[a]);
    g.trigger_write(a);
    assert_eq!(g.pending_effect_count(), 2);
    assert_eq!(g.prepare_flush(), 2);
    assert_eq!(g.flush_buffer, vec![peer, fresh]);
}

#[test]
fn pending_count_observes_dirty_work_without_consuming_it() {
    let mut g = ReactiveGraph::new();
    let source = g.create_node(KIND_SIGNAL);
    let effect = g.create_node(KIND_EFFECT);
    g.sync_sources(effect, &[source]);
    g.trigger_write(source);
    assert_eq!(g.pending_effect_count(), 1);
    assert_eq!(g.pending_effect_count(), 1);
    assert!(g.nodes[effect as usize].in_queue);
    assert_eq!(g.pending_effects.len(), 1);
    g.set_state(effect, STATE_CLEAN);
    assert_eq!(g.pending_effect_count(), 0);
    g.trigger_write(source);
    assert_eq!(g.pending_effect_count(), 1);
    assert_eq!(g.pending_effects.len(), 1);
    assert_eq!(g.prepare_flush(), 1);
    assert_eq!(g.pending_effect_count(), 0);
}

#[test]
fn pending_churn_keeps_entry_count_and_capacity_bounded_without_flushing() {
    let mut g = ReactiveGraph::new();
    let source = g.create_node(KIND_SIGNAL);
    let mut warm_capacity = 0;
    for i in 0..10_000 {
        let effect = g.create_node(KIND_EFFECT);
        g.sync_sources(effect, &[source]);
        g.trigger_write(source);
        g.dispose_node(effect);
        assert!(
            g.pending_effects.len() <= 66,
            "expired pending jobs grew with churn"
        );
        if i == 200 {
            warm_capacity = g.pending_effects.capacity();
        }
    }
    assert_eq!(g.active_count, 1);
    assert_eq!(g.nodes.len(), 2);
    assert_eq!(g.pending_effects.capacity(), warm_capacity);
    assert_eq!(g.pending_effect_count(), 0);
    assert_eq!(g.prepare_flush(), 0);
}

#[test]
fn pending_compaction_preserves_live_job_order_and_flags() {
    let mut g = ReactiveGraph::new();
    let churn = g.create_node(KIND_SIGNAL);
    let a = g.create_node(KIND_SIGNAL);
    let b = g.create_node(KIND_SIGNAL);
    let left = g.create_node(KIND_EFFECT);
    let right = g.create_node(KIND_EFFECT);
    g.sync_sources(left, &[a]);
    g.sync_sources(right, &[b]);
    g.trigger_write(a);
    g.trigger_write(b);
    for _ in 0..1000 {
        let effect = g.create_node(KIND_EFFECT);
        g.sync_sources(effect, &[churn]);
        g.trigger_write(churn);
        g.dispose_node(effect);
    }
    assert!(g.pending_effects.len() <= 74);
    assert!(g.nodes[left as usize].in_queue && g.nodes[right as usize].in_queue);
    assert_eq!(g.pending_effect_count(), 2);
    assert_eq!(g.prepare_flush(), 2);
    assert_eq!(g.flush_buffer, vec![left, right]);
}

#[test]
fn pending_compaction_retains_clean_scheduled_jobs_for_later_writes() {
    let mut g = ReactiveGraph::new();
    let churn = g.create_node(KIND_SIGNAL);
    let source = g.create_node(KIND_SIGNAL);
    let live = g.create_node(KIND_EFFECT);
    g.sync_sources(live, &[source]);
    g.trigger_write(source);
    g.set_state(live, STATE_CLEAN);
    for _ in 0..1000 {
        let effect = g.create_node(KIND_EFFECT);
        g.sync_sources(effect, &[churn]);
        g.trigger_write(churn);
        g.dispose_node(effect);
    }
    assert!(g.pending_effects.len() <= 70);
    assert_eq!(g.pending_effect_count(), 0);
    g.trigger_write(source);
    assert_eq!(g.pending_effect_count(), 1);
    assert_eq!(g.prepare_flush(), 1);
    assert_eq!(g.flush_buffer, vec![live]);
}

#[test]
fn pending_population_drop_compacts_expired_entries_without_new_writes() {
    let mut g = ReactiveGraph::new();
    let source = g.create_node(KIND_SIGNAL);
    let effects: Vec<_> = (0..1000)
        .map(|_| {
            let id = g.create_node(KIND_EFFECT);
            g.sync_sources(id, &[source]);
            id
        })
        .collect();
    g.trigger_write(source);
    for &id in &effects[1..] {
        g.dispose_node(id);
    }
    assert!(g.pending_effects.len() <= 68);
    assert_eq!(g.pending_effect_count(), 1);
    assert_eq!(g.prepare_flush(), 1);
    assert_eq!(g.flush_buffer, vec![effects[0]]);
}

#[test]
fn cached_no_effect_source_still_invalidates_lazy_computeds() {
    let mut g = ReactiveGraph::new();
    let source = g.create_node(KIND_SIGNAL);
    let derived = g.create_node(KIND_COMPUTED);
    g.sync_sources(derived, &[source]);
    assert_eq!(g.trigger_and_flush_fast(source), 0);
    g.set_state(derived, STATE_CLEAN);
    assert_eq!(g.trigger_and_flush_fast(source), 0);
    assert_eq!(g.get_state(derived), STATE_DIRTY);
}

#[test]
fn cached_terminal_source_does_not_dirty_unrelated_computeds() {
    let mut g = ReactiveGraph::new();
    let source = g.create_node(KIND_SIGNAL);
    let other = g.create_node(KIND_SIGNAL);
    let own = g.create_node(KIND_COMPUTED);
    let unrelated = g.create_node(KIND_COMPUTED);
    let effect = g.create_node(KIND_EFFECT);
    g.sync_sources(own, &[source]);
    g.sync_sources(unrelated, &[other]);
    g.sync_sources(effect, &[own]);
    g.trigger_write(other);
    assert_eq!(g.trigger_and_flush_fast(source), (effect as i32) + 1);
    g.set_state(own, STATE_CLEAN);
    g.set_state(unrelated, STATE_CLEAN);
    g.set_state(effect, STATE_CLEAN);
    assert_eq!(g.trigger_and_flush_fast(source), (effect as i32) + 1);
    assert_eq!(g.get_state(unrelated), STATE_CLEAN);
}

#[test]
fn cached_empty_source_drains_preexisting_foreign_pending_effects() {
    let mut g = ReactiveGraph::new();
    let source = g.create_node(KIND_SIGNAL);
    let other = g.create_node(KIND_SIGNAL);
    let effect = g.create_node(KIND_EFFECT);
    g.sync_sources(effect, &[other]);
    assert_eq!(g.trigger_and_flush_fast(source), 0);
    g.trigger_write(other);
    assert_eq!(g.trigger_and_flush_fast(source), (effect as i32) + 1);
    assert_eq!(g.pending_effect_count(), 0);
}

#[test]
fn terminal_cache_is_source_specific_when_a_foreign_effect_is_drained() {
    let mut g = ReactiveGraph::new();
    let source = g.create_node(KIND_SIGNAL);
    let other = g.create_node(KIND_SIGNAL);
    let effect = g.create_node(KIND_EFFECT);
    let derived = g.create_node(KIND_COMPUTED);
    g.sync_sources(effect, &[other]);
    g.sync_sources(derived, &[source]);
    g.trigger_write(other);
    assert_eq!(g.trigger_and_flush_fast(source), (effect as i32) + 1);
    g.set_state(effect, STATE_CLEAN);
    g.set_state(derived, STATE_CLEAN);
    assert_eq!(g.trigger_and_flush_fast(source), 0);
    assert_eq!(g.get_state(effect), STATE_CLEAN);
    assert_eq!(g.get_state(derived), STATE_DIRTY);
}

#[test]
fn topology_wrap_does_not_restore_a_stale_no_subscriber_cache() {
    let mut g = ReactiveGraph::new();
    let source = g.create_node(KIND_SIGNAL);
    assert_eq!(g.trigger_and_flush_fast(source), 0);
    let effect = g.create_node(KIND_EFFECT);
    let other = g.create_node(KIND_SIGNAL);
    let derived = g.create_node(KIND_COMPUTED);
    g.topology_epoch = u32::MAX;
    g.sync_sources(effect, &[source]);
    g.link(other, derived);
    assert_eq!(g.trigger_and_flush_fast(source), (effect as i32) + 1);
}

#[test]
fn topology_wrap_does_not_restore_a_detached_effect_cache() {
    let mut g = ReactiveGraph::new();
    let source = g.create_node(KIND_SIGNAL);
    let effect = g.create_node(KIND_EFFECT);
    g.sync_sources(effect, &[source]);
    assert_eq!(g.trigger_and_flush_fast(source), (effect as i32) + 1);
    g.set_state(effect, STATE_CLEAN);
    let other = g.create_node(KIND_SIGNAL);
    let a = g.create_node(KIND_COMPUTED);
    let b = g.create_node(KIND_COMPUTED);
    g.topology_epoch = u32::MAX;
    g.clear_sources(effect);
    g.link(other, a);
    g.link(other, b);
    assert_eq!(g.trigger_and_flush_fast(source), 0);
}

#[test]
fn topology_wrap_cannot_dirty_a_signal_reusing_a_cached_effect_slot() {
    let mut g = ReactiveGraph::new();
    let source = g.create_node(KIND_SIGNAL);
    let effect = g.create_node(KIND_EFFECT);
    g.sync_sources(effect, &[source]);
    assert_eq!(g.trigger_and_flush_fast(source), (effect as i32) + 1);
    let other = g.create_node(KIND_SIGNAL);
    let derived = g.create_node(KIND_COMPUTED);
    g.topology_epoch = u32::MAX;
    g.dispose_node(effect);
    let replacement = g.create_node(KIND_SIGNAL);
    assert_eq!(replacement, effect);
    g.link(other, derived);
    assert_eq!(g.trigger_and_flush_fast(source), 0);
    assert_eq!(g.get_state(replacement), STATE_CLEAN);
}

#[test]
fn topology_wrap_invalidates_cached_downstream_computed_slots() {
    let mut g = ReactiveGraph::new();
    let source = g.create_node(KIND_SIGNAL);
    let derived = g.create_node(KIND_COMPUTED);
    let effect = g.create_node(KIND_EFFECT);
    g.sync_sources(derived, &[source]);
    g.sync_sources(effect, &[derived]);
    assert_eq!(g.trigger_and_flush_fast(source), (effect as i32) + 1);
    assert_eq!(g.downstream_computeds[source as usize], vec![derived]);
    g.topology_epoch = u32::MAX;
    g.clear_sources(derived);
    g.clear_sources(effect);
    g.dispose_node(derived);
    g.dispose_node(effect);
    let a = g.create_node(KIND_SIGNAL);
    let b = g.create_node(KIND_SIGNAL);
    assert_eq!(a, effect);
    assert_eq!(b, derived);
    assert_eq!(g.trigger_and_flush_fast(source), 0);
    assert_eq!(g.get_state(a), STATE_CLEAN);
    assert_eq!(g.get_state(b), STATE_CLEAN);
}

#[test]
fn topology_wrap_rebuilds_cache_and_noop_links_preserve_it() {
    let mut g = ReactiveGraph::new();
    let source = g.create_node(KIND_SIGNAL);
    let effect = g.create_node(KIND_EFFECT);
    g.sync_sources(effect, &[source]);
    assert_eq!(g.trigger_and_flush_fast(source), (effect as i32) + 1);
    g.set_state(effect, STATE_CLEAN);
    let other = g.create_node(KIND_SIGNAL);
    let derived = g.create_node(KIND_COMPUTED);
    g.topology_epoch = u32::MAX;
    g.link(other, derived);
    assert_eq!(g.topology_epoch, 1);
    assert_eq!(g.terminal_cache_epoch[source as usize], 0);
    assert_eq!(g.single_terminal_effect[source as usize], u32::MAX);
    assert_eq!(g.trigger_and_flush_fast(source), (effect as i32) + 1);
    assert_eq!(g.terminal_cache_epoch[source as usize], g.topology_epoch);
    let epoch = g.topology_epoch;
    g.link(source, effect);
    g.clear_sources(other);
    assert_eq!(g.topology_epoch, epoch);
    assert_eq!(g.terminal_cache_epoch[source as usize], epoch);
    g.set_state(effect, STATE_CLEAN);
    assert_eq!(g.trigger_and_flush_fast(source), (effect as i32) + 1);
    assert_eq!(g.get_state(effect), STATE_DIRTY);
}

#[test]
fn cursor_pruning_invalidates_cached_terminal_effects() {
    let mut g = ReactiveGraph::new();
    let a = g.create_node(KIND_SIGNAL);
    let b = g.create_node(KIND_SIGNAL);
    let e = g.create_node(KIND_EFFECT);
    g.sync_sources(e, &[a, b]);
    assert_eq!(g.trigger_and_flush_fast(b), (e as i32) + 1);
    g.set_state(e, STATE_CLEAN);
    g.begin_tracking(e);
    g.track_read(a);
    g.end_tracking();
    assert_eq!(g.trigger_and_flush_fast(b), 0);
}

#[test]
fn cursor_replacement_invalidates_both_old_and_new_source_caches() {
    let mut g = ReactiveGraph::new();
    let a = g.create_node(KIND_SIGNAL);
    let b = g.create_node(KIND_SIGNAL);
    let e = g.create_node(KIND_EFFECT);
    g.sync_sources(e, &[a]);
    assert_eq!(g.trigger_and_flush_fast(a), (e as i32) + 1);
    g.set_state(e, STATE_CLEAN);
    assert_eq!(g.trigger_and_flush_fast(b), 0);
    g.begin_tracking(e);
    g.track_read(b);
    g.end_tracking();
    assert_eq!(g.trigger_and_flush_fast(a), 0);
    assert_eq!(g.trigger_and_flush_fast(b), (e as i32) + 1);
}

#[test]
fn cursor_dependency_reordering_preserves_both_forward_edges() {
    let mut g = ReactiveGraph::new();
    let a = g.create_node(KIND_SIGNAL);
    let b = g.create_node(KIND_SIGNAL);
    let e = g.create_node(KIND_EFFECT);
    g.sync_sources(e, &[a, b]);
    g.begin_tracking(e);
    g.track_read(b);
    g.track_read(a);
    g.end_tracking();
    assert_eq!(g.sources[e as usize], vec![b, a]);
    assert!(g.subscribers[a as usize].contains(&e));
    assert!(g.subscribers[b as usize].contains(&e));
    assert_eq!(g.trigger_and_flush_fast(b), (e as i32) + 1);
}

#[test]
fn cursor_ignores_self_and_invalid_reads_without_advancing() {
    let mut g = ReactiveGraph::new();
    let a = g.create_node(KIND_SIGNAL);
    let e = g.create_node(KIND_EFFECT);
    g.begin_tracking(e);
    g.track_read(e);
    g.track_read(u32::MAX);
    g.track_read(a);
    g.end_tracking();
    assert_eq!(g.sources[e as usize], vec![a]);
}

#[test]
fn cursor_cannot_mutate_a_replacement_observer_lifetime() {
    let mut g = ReactiveGraph::new();
    let a = g.create_node(KIND_SIGNAL);
    let b = g.create_node(KIND_SIGNAL);
    let old = g.create_node(KIND_EFFECT);
    g.begin_tracking(old);
    g.track_read(a);
    g.dispose_node(old);
    let fresh = g.create_node(KIND_EFFECT);
    assert_eq!(fresh, old);
    g.track_read(b);
    g.end_tracking();
    assert!(g.sources[fresh as usize].is_empty());
    assert_eq!(g.get_state(fresh), STATE_DIRTY);
}

#[test]
fn cursor_stale_inner_frame_preserves_the_outer_tracker() {
    let mut g = ReactiveGraph::new();
    let a = g.create_node(KIND_SIGNAL);
    let b = g.create_node(KIND_SIGNAL);
    let outer = g.create_node(KIND_EFFECT);
    let inner = g.create_node(KIND_COMPUTED);
    g.begin_tracking(outer);
    g.track_read(a);
    g.begin_tracking(inner);
    g.track_read(a);
    g.dispose_node(inner);
    let fresh = g.create_node(KIND_COMPUTED);
    assert_eq!(fresh, inner);
    g.track_read(b);
    g.end_tracking();
    g.track_read(b);
    g.end_tracking();
    assert_eq!(g.sources[outer as usize], vec![a, b]);
    assert!(g.sources[fresh as usize].is_empty());
    assert!(g.tracking_stack.is_empty());
    assert_eq!(g.get_state(fresh), STATE_DIRTY);
}

#[test]
fn cursor_lifetime_wrap_invalidates_ancient_tracking_frames() {
    let mut g = ReactiveGraph::new();
    let a = g.create_node(KIND_SIGNAL);
    let old = g.create_node(KIND_EFFECT);
    g.begin_tracking(old);
    g.nodes[old as usize].lifetime = u32::MAX;
    g.dispose_node(old);
    let fresh = g.create_node(KIND_EFFECT);
    assert_eq!(fresh, old);
    assert_eq!(g.nodes[fresh as usize].lifetime, 1);
    g.track_read(a);
    g.end_tracking();
    assert!(g.sources[fresh as usize].is_empty());
    assert_eq!(g.get_state(fresh), STATE_DIRTY);
}

#[test]
fn cursor_source_disposal_does_not_turn_an_unread_tail_into_a_read_prefix() {
    let mut g = ReactiveGraph::new();
    let a = g.create_node(KIND_SIGNAL);
    let b = g.create_node(KIND_SIGNAL);
    let c = g.create_node(KIND_SIGNAL);
    let e = g.create_node(KIND_EFFECT);
    g.sync_sources(e, &[a, b]);
    g.begin_tracking(e);
    g.track_read(a);
    g.dispose_node(a);
    g.track_read(c);
    g.end_tracking();
    assert_eq!(g.sources[e as usize], vec![c]);
    assert!(!g.subscribers[b as usize].contains(&e));
}

#[test]
fn cursor_explicit_source_replacement_restarts_the_discovery_cursor() {
    let mut g = ReactiveGraph::new();
    let a = g.create_node(KIND_SIGNAL);
    let b = g.create_node(KIND_SIGNAL);
    let c = g.create_node(KIND_SIGNAL);
    let d = g.create_node(KIND_SIGNAL);
    let e = g.create_node(KIND_EFFECT);
    g.begin_tracking(e);
    g.track_read(a);
    g.sync_sources(e, &[c, d]);
    g.track_read(b);
    g.end_tracking();
    assert_eq!(g.sources[e as usize], vec![b]);
}

#[test]
fn cursor_pause_and_invalid_begin_preserve_outer_tracking() {
    let mut g = ReactiveGraph::new();
    let a = g.create_node(KIND_SIGNAL);
    let b = g.create_node(KIND_SIGNAL);
    let c = g.create_node(KIND_SIGNAL);
    let e = g.create_node(KIND_EFFECT);
    g.begin_tracking(e);
    g.track_read(a);
    g.pause_tracking();
    g.track_read(b);
    g.resume_tracking();
    g.begin_tracking(u32::MAX);
    g.track_read(b);
    g.end_tracking();
    g.begin_tracking(a);
    g.track_read(b);
    g.end_tracking();
    g.track_read(c);
    g.end_tracking();
    assert_eq!(g.sources[e as usize], vec![a, c]);
    assert!(!g.subscribers[b as usize].contains(&e));
    assert!(g.tracking_stack.is_empty());
}

#[test]
fn cursor_stable_and_reordered_reads_reuse_storage_and_topology_cache() {
    let mut g = ReactiveGraph::new();
    let a = g.create_node(KIND_SIGNAL);
    let b = g.create_node(KIND_SIGNAL);
    let e = g.create_node(KIND_EFFECT);
    g.begin_tracking(e);
    g.track_read(a);
    g.track_read(b);
    g.end_tracking();
    assert_eq!(g.trigger_and_flush_fast(a), (e as i32) + 1);
    g.set_state(e, STATE_CLEAN);
    let sources = g.sources[e as usize].as_ptr();
    let subs_a = g.subscribers[a as usize].as_ptr();
    let subs_b = g.subscribers[b as usize].as_ptr();
    let stack = g.tracking_stack.as_ptr();
    let epoch = g.topology_epoch;
    for i in 0..100 {
        g.begin_tracking(e);
        if i % 2 == 0 {
            g.track_read(b);
            g.track_read(a);
        } else {
            g.track_read(a);
            g.track_read(b);
        }
        g.end_tracking();
    }
    assert_eq!(g.sources[e as usize].as_ptr(), sources);
    assert_eq!(g.subscribers[a as usize].as_ptr(), subs_a);
    assert_eq!(g.subscribers[b as usize].as_ptr(), subs_b);
    assert_eq!(g.tracking_stack.as_ptr(), stack);
    assert_eq!(g.topology_epoch, epoch);
    assert_eq!(g.terminal_cache_epoch[a as usize], epoch);
    assert_eq!(g.trigger_and_flush_fast(a), (e as i32) + 1);
}

#[test]
fn cursor_recursive_begin_does_not_replace_the_outer_dependency_pass() {
    let mut g = ReactiveGraph::new();
    let a = g.create_node(KIND_SIGNAL);
    let b = g.create_node(KIND_SIGNAL);
    let c = g.create_node(KIND_SIGNAL);
    let e = g.create_node(KIND_EFFECT);
    g.begin_tracking(e);
    g.track_read(a);
    g.begin_tracking(e);
    g.track_read(b);
    g.end_tracking();
    g.track_read(c);
    g.end_tracking();
    assert_eq!(g.sources[e as usize], vec![a, c]);
    assert!(!g.subscribers[b as usize].contains(&e));
    assert!(g.tracking_stack.is_empty());
}

#[test]
fn cursor_fresh_lifetime_can_track_before_the_stale_frame_is_closed() {
    let mut g = ReactiveGraph::new();
    let a = g.create_node(KIND_SIGNAL);
    let b = g.create_node(KIND_SIGNAL);
    let old = g.create_node(KIND_EFFECT);
    g.begin_tracking(old);
    g.track_read(a);
    g.dispose_node(old);
    let fresh = g.create_node(KIND_EFFECT);
    assert_eq!(fresh, old);
    g.begin_tracking(fresh);
    g.track_read(b);
    g.end_tracking();
    g.track_read(a);
    g.end_tracking();
    assert_eq!(g.sources[fresh as usize], vec![b]);
    assert_eq!(g.get_state(fresh), STATE_CLEAN);
    assert_eq!(g.trigger_and_flush_fast(a), 0);
    assert_eq!(g.trigger_and_flush_fast(b), (fresh as i32) + 1);
}

#[cfg(not(target_arch = "wasm32"))]
mod native_abi_thread_tests {
    use super::*;

    #[test]
    fn native_abi_threads_are_isolated() {
        // Calls are deliberately sequential, so the old implementation fails
        // deterministically without exercising its unsynchronized data race.
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
                        // No mutation on this thread occurs between pointer fetch
                        // and read; other threads mutate only their own buffers.
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
    #[test]
    fn long_subscriber_lists_stay_consistent_through_link_unlink_and_reuse() {
        let mut g = ReactiveGraph::new();
        let source = g.create_node(KIND_SIGNAL);
        let other = g.create_node(KIND_SIGNAL);
        let effects: Vec<u32> = (0..200).map(|_| g.create_node(KIND_EFFECT)).collect();
        for &effect in &effects {
            g.sync_sources(effect, &[source, other, source]);
        }
        assert!(g.sub_indexed[source as usize]);
        assert_eq!(g.subscribers[source as usize].len(), 200);
        assert_eq!(g.sub_index.len(), 400);
        let check = |g: &ReactiveGraph| {
            for node in [source, other] {
                for (position, &sub) in g.subscribers[node as usize].iter().enumerate() {
                    assert_eq!(g.sub_index[&edge_key(node as usize, sub)], position as u32);
                    assert!(g.sources[sub as usize].contains(&node));
                }
            }
        };
        check(&g);
        // Unlinking from the middle moves the last subscriber into the hole.
        for &effect in effects.iter().step_by(3) {
            g.sync_sources(effect, &[other]);
        }
        check(&g);
        assert_eq!(g.subscribers[source as usize].len(), 133);
        for &effect in effects.iter().skip(1).step_by(3) {
            g.dispose_node(effect);
        }
        check(&g);
        assert_eq!(g.subscribers[source as usize].len(), 66);
        assert_eq!(g.subscribers[other as usize].len(), 133);
        // Disposing the source drops its index entries; a reused slot starts clean.
        g.dispose_node(source);
        assert_eq!(g.sub_index.len(), 133);
        let reused = g.create_node(KIND_SIGNAL);
        assert_eq!(reused, source);
        assert!(!g.sub_indexed[reused as usize] && g.subscribers[reused as usize].is_empty());
        for &effect in effects.iter().skip(2).step_by(3) {
            assert_eq!(g.sources[effect as usize], vec![other]);
        }
        g.reset();
        assert!(g.sub_index.is_empty() && g.sub_indexed.is_empty());
    }
}
