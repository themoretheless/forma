use super::*;

#[test]
fn evaluate_culling_stats_classifies_frustum_and_backface_triangles() {
    let vp_identity: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0, // col 0
        0.0, 1.0, 0.0, 0.0, // col 1
        0.0, 0.0, 1.0, 0.0, // col 2
        0.0, 0.0, 0.0, 1.0, // col 3
    ];
    // Tri 0: inside frustum, front-facing to eye at (0, 0, 5)
    // Tri 1: inside frustum, back-facing (reversed winding)
    // Tri 2: outside frustum (x > 2)
    let positions: [f32; 27] = [
        // Tri 0
        0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.5, 0.0, // Tri 1 (reversed)
        0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.5, 0.0, 0.0, // Tri 2 (outside)
        3.0, 0.0, 0.0, 3.5, 0.0, 0.0, 3.0, 0.5, 0.0,
    ];
    let indices: [u32; 9] = [0, 1, 2, 3, 4, 5, 6, 7, 8];
    let stats = evaluate_culling_stats(&positions, &indices, &vp_identity, [0.0, 0.0, 5.0]);
    assert_eq!(stats.total_triangles, 3);
    assert_eq!(stats.surviving_triangles, 1);
    assert_eq!(stats.backface_culled_triangles, 1);
    assert_eq!(stats.frustum_culled_triangles, 1);
    assert!((stats.cull_ratio() - 2.0 / 3.0).abs() < 1e-6);
}

#[test]
fn ssao_darkens_concave_depth_pockets_and_preserves_flat_planes() {
    let mut depth = vec![0.5_f32; 25];
    let flat_ao = compute_ssao_ao(&depth, 5, 5, 1, 0.001, 1.0);
    assert!(flat_ao.iter().all(|&v| (v - 1.0).abs() < 1e-6));

    // Center pixel is recessed (deeper = larger depth 0.52 surrounded by 0.50)
    depth[2 * 5 + 2] = 0.52;
    let cavity_ao = compute_ssao_ao(&depth, 5, 5, 1, 0.001, 1.0);
    assert!(cavity_ao[2 * 5 + 2] < 0.7);
}

#[test]
fn sobel_edge_outline_detects_step_boundaries() {
    let mut img = vec![0.0_f32; 25];
    for y in 0..5 {
        for x in 3..5 {
            img[y * 5 + x] = 1.0;
        }
    }
    let edges = sobel_edge_outline(&img, 5, 5, 0.5);
    // Column 2 straddles the 0->1 step
    assert_eq!(edges[2 * 5 + 2], 1.0);
    // Column 0 is inside uniform region
    assert_eq!(edges[2 * 5], 0.0);
}

#[test]
fn bilateral_depth_filter_smooths_noise_while_preserving_silhouette_edges() {
    let mut depth = vec![0.2_f32; 25];
    for y in 0..5 {
        for x in 3..5 {
            depth[y * 5 + x] = 0.9;
        }
    }
    // Add small noise on foreground pixel (1, 2)
    depth[2 * 5 + 1] = 0.21;
    let filtered = bilateral_depth_filter(&depth, 5, 5, 1.5, 0.03);
    // Foreground pixel next to step boundary (x=2) should stay near 0.2, not bleed to 0.55
    assert!((filtered[2 * 5 + 2] - 0.2).abs() < 0.02);
    // Noisy foreground pixel smoothed toward 0.2
    assert!((filtered[2 * 5 + 1] - 0.2).abs() < 0.008);
}

#[test]
fn compute_normal_map_from_depth_reconstructs_unit_normals() {
    let mut depth = vec![0.5_f32; 25];
    let flat_normals = compute_normal_map_from_depth(&depth, 5, 5, std::f32::consts::FRAC_PI_3);
    for n in &flat_normals {
        assert!(n[0].abs() < 1e-6);
        assert!(n[1].abs() < 1e-6);
        assert!((n[2] - 1.0).abs() < 1e-6);
    }
    // Sloped plane increasing in +X
    for y in 0..5 {
        for x in 0..5 {
            depth[y * 5 + x] = 0.3 + 0.05 * (x as f32);
        }
    }
    let sloped = compute_normal_map_from_depth(&depth, 5, 5, std::f32::consts::FRAC_PI_3);
    let center_n = sloped[2 * 5 + 2];
    let norm =
        (center_n[0] * center_n[0] + center_n[1] * center_n[1] + center_n[2] * center_n[2]).sqrt();
    assert!((norm - 1.0).abs() < 1e-5);
    assert!(center_n[0] < -0.1);
    assert!(center_n[2] > 0.0);
}

#[test]
fn fxaa_luma_antialias_and_tone_map_aces_filmic_work() {
    let mut img = vec![[0.0_f32, 0.0, 0.0, 1.0]; 25];
    for y in 0..5 {
        for x in 2..5 {
            img[y * 5 + x] = [1.0, 1.0, 1.0, 1.0];
        }
    }
    let aa = fxaa_luma_antialias(&img, 5, 5);
    assert_eq!(aa.len(), 25);
    // Step edge pixel at (2, 2) should be blended between 0.0 and 1.0
    let edge_r = aa[2 * 5 + 2][0];
    assert!(edge_r > 0.1 && edge_r < 0.9);
    // Uniform pixel at (4, 2) stays 1.0
    assert!((aa[2 * 5 + 4][0] - 1.0).abs() < 1e-6);

    let hdr = [[0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [10.0, 10.0, 10.0]];
    let mapped = tone_map_aces_filmic(&hdr, 1.0, 2.2);
    assert_eq!(mapped[0], [0.0, 0.0, 0.0]);
    assert!(mapped[1][0] > 0.5 && mapped[1][0] < 1.0);
    assert!(mapped[2][0] > mapped[1][0] && mapped[2][0] <= 1.0);

    // Separable HDR bloom bright-pass and blur
    let mut hdr_grid = vec![[0.2_f32, 0.2, 0.2]; 25];
    hdr_grid[2 * 5 + 2] = [5.0, 5.0, 5.0]; // Emissive highlight at center (2, 2)
    let bloomed = bloom_bright_pass_and_blur(&hdr_grid, 5, 5, 1.0, 0.5);
    assert_eq!(bloomed.len(), 25);
    // Center pixel increases above 5.0
    assert!(bloomed[2 * 5 + 2][0] > 5.0);
    // Immediate neighbor (2, 1) receives bloom halo above its 0.2 base
    assert!(bloomed[5 + 2][0] > 0.3);
    // Far corner (0, 0) receives much less than immediate neighbor
    assert!(bloomed[0][0] < bloomed[5 + 2][0]);

    // Hi-Z conservative max-depth mip pyramid level
    let depth_4x4 = [
        0.1_f32, 0.2, 0.3, 0.4, // row 0
        0.5, 0.6, 0.7, 0.8, // row 1
        0.15, 0.25, 0.95, 0.35, // row 2
        0.45, 0.55, 0.65, 0.75, // row 3
    ];
    let (mip1, mw, mh) = build_hiz_mip_level_max(&depth_4x4, 4, 4);
    assert_eq!((mw, mh), (2, 2));
    assert_eq!(mip1, vec![0.6, 0.8, 0.55, 0.95]);
    let (mip2, mw2, mh2) = build_hiz_mip_level_max(&mip1, mw, mh);
    assert_eq!((mw2, mh2), (1, 1));
    assert_eq!(mip2, vec![0.95]);

    // Vignette darkens corners while preserving center and alpha
    let mut flat_rgba = vec![1.0_f32; 3 * 3 * 4];
    flat_rgba[4 * 4 + 3] = 0.75; // center alpha
    let vignet = apply_chromatic_aberration_and_vignette(&flat_rgba, 3, 3, 0.5);
    // Center pixel (1, 1) has r^2 = 0 -> unchanged RGB = 1.0
    assert!((vignet[4 * 4] - 1.0).abs() < 1e-6);
    assert!((vignet[4 * 4 + 3] - 0.75).abs() < 1e-6);
    // Corner pixel (0, 0) has r^2 = 1.0 -> vig = 0.5
    assert!((vignet[0] - 0.5).abs() < 1e-6);
    assert!((vignet[3] - 1.0).abs() < 1e-6);
}
