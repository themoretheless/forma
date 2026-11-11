use super::*;

#[test]
fn test_gpu_uma_duma_dirty_page_tracking() {
    let mut g = GpuArena::new();
    let off_a = g.alloc_f32(64);
    let off_b = g.alloc_f32(64);
    assert_eq!(off_a, 0);
    assert_eq!(off_b, 64);

    g.clear_dirty();
    assert_eq!(g.dirty_min, u32::MAX);
    assert_eq!(g.dirty_max, 0);

    // Mutate a slice inside tensor B [70..78]
    g.mark_dirty(70, 8);
    assert_eq!(g.dirty_min, 70);
    assert_eq!(g.dirty_max, 78);

    let mut spans = Vec::new();
    assert_eq!(g.collect_dirty_spans_into(&mut spans), 1);
    assert_eq!(spans, vec![64, 32]);
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
        changes += usize::from(previous_capacity != arena.capacity());
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
    let mut g = GpuArena::new();
    let anchor = g.alloc_f32(16);
    g.f32_arena[..16].fill(17.0);
    let sizes: Vec<u32> = (0..128).map(|i| 1 + (i * 37 % 251)).collect();
    let total: usize = sizes.iter().map(|n| ((n + 3) & !3) as usize).sum();
    let mut warmed_storage = None;
    let mut seed = 0x12345678u32;
    for cycle in 0..2000 {
        let mut blocks = Vec::new();
        for &size in &sizes {
            let offset = g.alloc_f32(size);
            assert_ne!(offset, u32::MAX);
            let range = offset as usize..offset as usize + size as usize;
            assert!(g.f32_arena[range.clone()].iter().all(|&v| v == 0.0));
            g.f32_arena[range].fill(cycle as f32 + 1.0);
            blocks.push((offset, size));
        }
        assert_eq!(g.f32_arena.len(), 16 + total);
        assert!(g.f32_arena[..16].iter().all(|&v| v == 17.0));
        for i in (1..blocks.len()).rev() {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            blocks.swap(i, seed as usize % (i + 1));
        }
        for (offset, size) in blocks {
            g.free_f32(offset, size);
        }
        assert_eq!(g.free_blocks, vec![(16, total as u32)]);
        g.clear_dirty();
        let storage = (
            g.f32_arena.as_ptr(),
            g.f32_arena.capacity(),
            g.dirty_blocks.capacity(),
        );
        if let Some(expected) = warmed_storage {
            assert_eq!(storage, expected);
        } else {
            warmed_storage = Some(storage);
        }
        assert!(g.free_blocks.capacity() <= 256);
    }
    g.free_f32(anchor, 16);
    assert_eq!(g.free_blocks, vec![(0, (16 + total) as u32)]);
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
    let mut g = GpuArena::new();
    assert_eq!(g.alloc_f32(4), 0);
    g.f32_arena[0] = 42.0;
    assert_eq!(g.alloc_f32(u32::MAX), u32::MAX);
    assert_eq!(MAX_GPU_ARENA_FLOATS, 67_108_864);
    assert_eq!(g.alloc_f32(MAX_GPU_ARENA_FLOATS + 1), u32::MAX);
    assert_eq!(g.f32_arena.len(), 4);
    assert_eq!(g.f32_arena[0], 42.0);
}

#[test]
fn test_gpu_picker_matches_webgpu_viewport_projection() {
    let mut g = GpuArena::new();
    let offset = g.alloc_f32(4);
    g.f32_arena[..4].copy_from_slice(&[0.8, 0.4, 0.2, 1.0]);
    assert_eq!(
        g.pick_nearest_vertex_3d(
            offset,
            1,
            ViewportPickQuery::new(200.0, 200.0, 180.0, 60.0, 1.0)
        ),
        0
    );
    assert_eq!(
        g.pick_nearest_vertex_3d(
            offset,
            1,
            ViewportPickQuery::new(200.0, 200.0, 152.0, 74.0, 1.0)
        ),
        -1
    );
}

#[test]
fn test_gpu_picker_rejects_clipped_vertices_and_invalid_queries() {
    let mut g = GpuArena::new();
    let offset = g.alloc_f32(4);
    for vertex in [
        [0.0, 0.0, 0.2, -1.0],
        [0.0, 0.0, -0.1, 1.0],
        [0.0, 0.0, 1.1, 1.0],
        [1.1, 0.0, 0.2, 1.0],
        [0.0, 1.1, 0.2, 1.0],
        [0.0, 0.0, 0.0, 0.0],
        [f32::NAN, 0.0, 0.2, 1.0],
    ] {
        g.f32_arena[..4].copy_from_slice(&vertex);
        assert_eq!(
            g.pick_nearest_vertex_3d(
                offset,
                1,
                ViewportPickQuery::new(200.0, 200.0, 100.0, 100.0, 1000.0)
            ),
            -1
        );
    }
    for w in [0.000001, f32::MIN_POSITIVE, f32::from_bits(1)] {
        g.f32_arena[..4].copy_from_slice(&[0.0, 0.0, 0.0, w]);
        assert_eq!(
            g.pick_nearest_vertex_3d(
                offset,
                1,
                ViewportPickQuery::new(200.0, 200.0, 100.0, 100.0, 0.0)
            ),
            0
        );
    }
    g.f32_arena[..4].copy_from_slice(&[0.0, 0.0, 0.0000005, 0.000001]);
    assert_eq!(
        g.pick_nearest_vertex_3d(
            offset,
            1,
            ViewportPickQuery::new(200.0, 200.0, 100.0, 100.0, 0.0)
        ),
        0
    );
    assert_eq!(
        g.pick_nearest_vertex_3d(
            offset,
            1,
            ViewportPickQuery::new(0.0, 200.0, 100.0, 100.0, 1.0)
        ),
        -1
    );
    assert_eq!(
        g.pick_nearest_vertex_3d(
            offset,
            1,
            ViewportPickQuery::new(200.0, 200.0, 100.0, 100.0, -1.0)
        ),
        -1
    );
    assert_eq!(
        g.pick_nearest_vertex_3d(
            offset,
            1,
            ViewportPickQuery::new(200.0, 200.0, f32::NAN, 100.0, 1.0)
        ),
        -1
    );
}

#[test]
fn test_homogeneous_segment_clipping() {
    let mut g = GpuArena::new();
    let offset = g.alloc_f32(8);
    g.f32_arena[..8].copy_from_slice(&[-0.5, 0.0, -0.5, 1.0, 0.5, 0.0, 0.5, 1.0]);
    assert_eq!(g.clip_primitive_3d(offset, 2, &[0, 1]), 2);
    assert_eq!(g.clip_output, vec![0.0, 0.0, 0.0, 1.0, 0.5, 0.0, 0.5, 1.0]);
    g.f32_arena[..8].copy_from_slice(&[-2.0, 0.0, 0.5, 1.0, 2.0, 0.0, 0.5, 1.0]);
    assert_eq!(g.clip_primitive_3d(offset, 2, &[0, 1]), 2);
    assert_eq!(g.clip_output, vec![-1.0, 0.0, 0.5, 1.0, 1.0, 0.0, 0.5, 1.0]);
    assert_eq!(g.clip_primitive_3d(offset, 2, &[0, 2]), 0);
    assert!(g.clip_output.is_empty());
    assert_eq!(g.clip_primitive_3d(u32::MAX, u32::MAX, &[0, 1]), 0);
}

#[test]
fn test_homogeneous_polygon_clipping() {
    let mut g = GpuArena::new();
    let offset = g.alloc_f32(16);
    g.f32_arena[..16].copy_from_slice(&[
        -0.5, -0.5, -0.5, 1.0, 0.5, -0.5, 0.5, 1.0, 0.5, 0.5, 0.5, 1.0, -0.5, 0.5, -0.5, 1.0,
    ]);
    assert_eq!(g.clip_primitive_3d(offset, 4, &[0, 1, 2, 3]), 4);
    for v in g.clip_output.chunks_exact(4) {
        assert!(v[0] >= 0.0 && v[0] <= 0.5);
        assert!(v[2] >= 0.0 && v[2] <= v[3]);
    }
    g.f32_arena[..16].fill(-1.0);
    assert_eq!(g.clip_primitive_3d(offset, 4, &[0, 1, 2, 3]), 0);
    assert!(g.clip_output.is_empty());
    g.f32_arena[0] = f32::NAN;
    assert_eq!(g.clip_primitive_3d(offset, 4, &[0, 1, 2, 3]), 0);
}

#[test]
fn test_segment_clipping_all_planes_and_seeded_reference_samples() {
    let mut g = GpuArena::new();
    let offset = g.alloc_f32(8);
    for outside in [
        [-2.0, 0.0, 0.5, 1.0],
        [2.0, 0.0, 0.5, 1.0],
        [0.0, -2.0, 0.5, 1.0],
        [0.0, 2.0, 0.5, 1.0],
        [0.0, 0.0, -1.0, 1.0],
        [0.0, 0.0, 2.0, 1.0],
    ] {
        g.f32_arena[..4].copy_from_slice(&outside);
        g.f32_arena[4..8].copy_from_slice(&[0.0, 0.0, 0.5, 1.0]);
        assert_eq!(g.clip_primitive_3d(offset, 2, &[0, 1]), 2);
        let forward = g.clip_output.clone();
        assert_eq!(g.clip_primitive_3d(offset, 2, &[1, 0]), 2);
        assert_eq!(&forward[..4], &g.clip_output[4..]);
    }
    let mut seed: u32 = 42;
    let mut next = || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 8) as f32 / 16777216.0 * 4.0 - 2.0
    };
    for _ in 0..500 {
        let a = [next(), next(), next(), next() + 1.0];
        let b = [next(), next(), next(), next() + 1.0];
        g.f32_arena[..4].copy_from_slice(&a);
        g.f32_arena[4..8].copy_from_slice(&b);
        let size = g.clip_primitive_3d(offset, 2, &[0, 1]);
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
        for v in g.clip_output.chunks_exact(4) {
            assert!(v.iter().all(|value| value.is_finite()));
            assert!(v[3] > 0.0 && v[0].abs() <= v[3] + 1e-5 && v[1].abs() <= v[3] + 1e-5);
            assert!(v[2] >= -1e-5 && v[2] <= v[3] + 1e-5);
        }
    }
}

#[test]
fn test_visible_vertex_picker_skips_hidden_candidate_and_finds_visible_neighbor() {
    let mut g = GpuArena::new();
    let offset = g.alloc_f32(20);
    g.f32_arena[..20].copy_from_slice(&[
        -0.8, -0.8, 0.2, 1.0, 0.8, -0.8, 0.2, 1.0, 0.0, 0.8, 0.2, 1.0, 0.0, 0.0, 0.8, 1.0, 0.15,
        0.0, 0.1, 1.0,
    ]);
    g.pick_indices = vec![0, 1, 2];
    let center_r20 = ViewportPickQuery::new(200.0, 200.0, 100.0, 100.0, 20.0);
    assert_eq!(g.pick_nearest_vertex_3d(offset, 4, center_r20), 3);
    assert_eq!(g.pick_visible_vertex_3d(offset, 4, 3, center_r20), -1);
    assert_eq!(g.pick_visible_vertex_3d(offset, 5, 3, center_r20), 4);
    assert_eq!(
        g.pick_visible_vertex_3d(
            offset,
            5,
            3,
            ViewportPickQuery::new(200.0, 200.0, 20.0, 180.0, 1.0)
        ),
        0
    );
    g.f32_arena[14] = 0.2;
    assert_eq!(
        g.pick_visible_vertex_3d(
            offset,
            5,
            3,
            ViewportPickQuery::new(200.0, 200.0, 100.0, 100.0, 1.0)
        ),
        3
    );
}

#[test]
fn test_visible_picker_clips_triangles_and_handles_perspective_winding() {
    let mut g = GpuArena::new();
    let offset = g.alloc_f32(16);
    g.f32_arena[..16].copy_from_slice(&[
        -0.8, -0.8, -0.2, 1.0, 0.8, -0.8, 0.2, 1.0, 0.0, 0.8, 0.2, 1.0, 0.0, 0.0, 0.8, 1.0,
    ]);
    g.pick_indices = vec![0, 1, 2];
    let center = ViewportPickQuery::new(200.0, 200.0, 100.0, 100.0, 1.0);
    assert_eq!(g.pick_visible_vertex_3d(offset, 4, 3, center), -1);
    g.f32_arena[..12].copy_from_slice(&[
        -0.8, -0.8, 0.2, 1.0, 1.6, -1.6, 0.4, 2.0, 0.0, 0.4, 0.1, 0.5,
    ]);
    g.pick_indices = vec![2, 1, 0];
    assert_eq!(g.pick_visible_vertex_3d(offset, 4, 3, center), -1);
    g.pick_indices.clear();
    assert_eq!(g.pick_visible_vertex_3d(offset, 4, 0, center), 3);
}

#[test]
fn test_snapshot_picker_preserves_live_geometry_and_owns_only_scratch() {
    let mut g = GpuArena::new();
    g.f32_arena = vec![111.0, 222.0, 333.0, 444.0];
    g.pick_vertices = vec![
        -0.8, -0.8, 0.2, 1.0, 0.8, -0.8, 0.2, 1.0, 0.0, 0.8, 0.2, 1.0, 0.0, 0.0, 0.8, 1.0,
    ];
    g.pick_indices = vec![0, 1, 2];
    let pointer = g.pick_vertices.as_ptr();
    let center = ViewportPickQuery::new(200.0, 200.0, 100.0, 100.0, 1.0);
    assert_eq!(g.pick_snapshot_3d(4, 3, 1, center), -1);
    assert_eq!(g.pick_snapshot_3d(4, 3, 0, center), 3);
    assert_eq!(g.f32_arena, vec![111.0, 222.0, 333.0, 444.0]);
    assert_eq!(g.pick_vertices.as_ptr(), pointer);
    assert_eq!(g.pick_indices, vec![0, 1, 2]);
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
    let mut g = GpuArena::new();
    let offset = g.alloc_f32(16);
    g.f32_arena[..16].fill(7.0);
    g.clear_dirty();
    g.mat4_perspective(offset, 1.0, 0.0, 0.1, 100.0);
    assert_eq!(g.f32_arena, vec![7.0; 16]);
    assert_eq!(g.dirty_min, u32::MAX);
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
