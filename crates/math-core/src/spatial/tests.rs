use super::*;

#[test]
fn quat_axis_angle_slerp_and_mat3_roundtrip() {
    let q = Quat::from_axis_angle([0.0, 0.0, 1.0], std::f64::consts::FRAC_PI_2).unwrap();
    let v = q.rotate_vec3([1.0, 0.0, 0.0]);
    assert!((v[0] - 0.0).abs() < 1e-12);
    assert!((v[1] - 1.0).abs() < 1e-12);
    assert!((v[2] - 0.0).abs() < 1e-12);

    let m = q.to_mat3();
    let recovered = Quat::from_mat3(m);
    assert!((q.dot(recovered).abs() - 1.0).abs() < 1e-12);

    let mid = Quat::IDENTITY.slerp(q, 0.5);
    let v_mid = mid.rotate_vec3([1.0, 0.0, 0.0]);
    let s45 = std::f64::consts::FRAC_1_SQRT_2;
    assert!((v_mid[0] - s45).abs() < 1e-12);
    assert!((v_mid[1] - s45).abs() < 1e-12);
}

#[test]
fn dual_quat_transforms_and_blends_rigid_motions() {
    let q = Quat::from_axis_angle([0.0, 1.0, 0.0], std::f64::consts::FRAC_PI_2).unwrap();
    let dq = DualQuat::from_rotation_translation(q, [10.0, -5.0, 2.0]);
    let p = dq.transform_point([1.0, 2.0, 3.0]);
    // Rotating [1, 2, 3] by +90 deg around Y gives [3, 2, -1], plus [10, -5, 2] = [13, -3, 1].
    assert!((p[0] - 13.0).abs() < 1e-12);
    assert!((p[1] - (-3.0)).abs() < 1e-12);
    assert!((p[2] - 1.0).abs() < 1e-12);

    let mid = DualQuat::IDENTITY.blend(dq, 0.5);
    let (mid_r, mid_t) = mid.to_rotation_translation();
    assert!((mid_r.norm() - 1.0).abs() < 1e-12);
    assert!(mid_t.iter().all(|x| x.is_finite()));
}

#[test]
fn ray_aabb_sphere_and_plane_intersections() {
    let ray = Ray3::new([-5.0, 0.5, 0.5], [1.0, 0.0, 0.0]).unwrap();
    let hit = ray
        .intersect_aabb([0.0, 0.0, 0.0], [2.0, 1.0, 1.0])
        .unwrap();
    assert!((hit.0 - 5.0).abs() < 1e-12);
    assert!((hit.1 - 7.0).abs() < 1e-12);

    let sphere_hit = ray.intersect_sphere([0.0, 0.5, 0.5], 1.5).unwrap();
    assert!((sphere_hit.0 - 3.5).abs() < 1e-12);
    assert!((sphere_hit.1 - 6.5).abs() < 1e-12);

    let plane = Plane3::from_point_normal([3.0, 0.0, 0.0], [-1.0, 0.0, 0.0]).unwrap();
    let t_plane = plane.intersect_ray(&ray).unwrap();
    assert!((t_plane - 8.0).abs() < 1e-12);
}

#[test]
fn frustum_culling_identifies_inside_and_outside_aabbs() {
    // Identity VP matrix defines the canonical [-1, 1]^3 clip cube.
    let frustum = Frustum::from_view_proj_row_major(IDENTITY).unwrap();
    assert!(frustum.contains_point([0.0, 0.0, 0.0]));
    assert!(!frustum.contains_point([2.0, 0.0, 0.0]));
    assert!(frustum.intersects_aabb([-0.5, -0.5, -0.5], [0.5, 0.5, 0.5]));
    assert!(frustum.intersects_aabb([0.8, -0.2, -0.2], [2.0, 0.2, 0.2]));
    assert!(!frustum.intersects_aabb([1.5, 1.5, 1.5], [2.5, 2.5, 2.5]));
    assert!(frustum.intersects_sphere([1.25, 0.0, 0.0], 0.5));
    assert!(!frustum.intersects_sphere([2.0, 0.0, 0.0], 0.5));

    let batch = [
        Aabb3::new([-0.5, -0.5, -0.5], [0.5, 0.5, 0.5]),
        Aabb3::new([1.5, 1.5, 1.5], [2.5, 2.5, 2.5]),
        Aabb3::empty(),
    ];
    assert_eq!(frustum.cull_aabbs_mask(&batch), vec![true, false, false]);
}

#[test]
fn aabb3_and_obb3_operations_and_sat_intersection() {
    let a = Aabb3::from_points(&[[0.0, 0.0, 0.0], [2.0, 4.0, 6.0]]);
    assert_eq!(a.center(), [1.0, 2.0, 3.0]);
    assert_eq!(a.extents(), [2.0, 4.0, 6.0]);
    assert_eq!(a.volume(), 48.0);
    assert_eq!(a.surface_area(), 88.0);
    assert_eq!(a.longest_axis(), 2);
    assert!(a.contains_point([1.0, 2.0, 3.0]));
    assert!(!a.contains_point([3.0, 2.0, 3.0]));

    let b = Aabb3::new([1.0, 1.0, 1.0], [3.0, 3.0, 3.0]);
    assert!(a.intersects(&b));
    let inter = a.intersection(&b);
    assert_eq!(inter.min, [1.0, 1.0, 1.0]);
    assert_eq!(inter.max, [2.0, 3.0, 3.0]);
    let uni = a.union(&b);
    assert_eq!(uni.min, [0.0, 0.0, 0.0]);
    assert_eq!(uni.max, [3.0, 4.0, 6.0]);

    let rot = Quat::from_axis_angle([0.0, 0.0, 1.0], std::f64::consts::FRAC_PI_4).unwrap();
    let obb1 = Obb3::from_rotation([0.0, 0.0, 0.0], rot, [2.0, 0.5, 0.5]);
    assert!(obb1.contains_point([1.0, 1.0, 0.0]));
    assert!(!obb1.contains_point([2.0, -2.0, 0.0]));

    let obb2 = Obb3::from_aabb(&Aabb3::new([0.8, 0.8, -0.5], [1.5, 1.5, 0.5]));
    assert!(obb1.intersects_obb(&obb2));

    let obb3 = Obb3::from_aabb(&Aabb3::new([2.5, 2.5, -0.5], [3.5, 3.5, 0.5]));
    assert!(!obb1.intersects_obb(&obb3));
}

#[test]
fn barycentric_and_closest_point_on_triangle_3d_match_analytic_regions() {
    let a = [0.0, 0.0, 0.0];
    let b = [4.0, 0.0, 0.0];
    let c = [0.0, 4.0, 0.0];
    let bary = barycentric_coords_3d([1.0, 1.0, 5.0], a, b, c).unwrap();
    assert!((bary[0] - 0.5).abs() < 1e-12);
    assert!((bary[1] - 0.25).abs() < 1e-12);
    assert!((bary[2] - 0.25).abs() < 1e-12);

    // Inside face projection
    let cp_face = closest_point_on_triangle_3d([1.0, 1.0, 5.0], a, b, c);
    assert_eq!(cp_face, [1.0, 1.0, 0.0]);

    // Vertex Voronoi region A
    let cp_a = closest_point_on_triangle_3d([-2.0, -3.0, 1.0], a, b, c);
    assert_eq!(cp_a, a);

    // Hypotenuse edge Voronoi region BC
    let cp_bc = closest_point_on_triangle_3d([4.0, 4.0, 2.0], a, b, c);
    assert!((cp_bc[0] - 2.0).abs() < 1e-12);
    assert!((cp_bc[1] - 2.0).abs() < 1e-12);
    assert!((cp_bc[2] - 0.0).abs() < 1e-12);
}

#[test]
fn moller_trumbore_ray_triangle_and_symmetric_eigen3_and_solve_linear_3x3() {
    let v0 = [0.0, 0.0, 0.0];
    let v1 = [2.0, 0.0, 0.0];
    let v2 = [0.0, 2.0, 0.0];
    let ray = Ray3::new([0.5, 0.5, 3.0], [0.0, 0.0, -1.0]).unwrap();
    let (t, u, v) = ray_triangle_intersect_moller_trumbore(&ray, v0, v1, v2).unwrap();
    assert!((t - 3.0).abs() < 1e-12);
    assert!((u - 0.25).abs() < 1e-12);
    assert!((v - 0.25).abs() < 1e-12);

    // Miss outside triangle
    let miss_ray = Ray3::new([2.5, 2.5, 3.0], [0.0, 0.0, -1.0]).unwrap();
    assert!(ray_triangle_intersect_moller_trumbore(&miss_ray, v0, v1, v2).is_none());

    // Batch closest-triangle hit
    let verts = [
        [0.0, 0.0, 0.0],
        [2.0, 0.0, 0.0],
        [0.0, 2.0, 0.0],
        [0.0, 0.0, 1.5],
        [2.0, 0.0, 1.5],
        [0.0, 2.0, 1.5],
    ];
    let tris = [[0, 1, 2], [3, 4, 5]];
    let closest = ray.intersect_triangles_closest(&verts, &tris).unwrap();
    assert_eq!(closest.0, 1);
    assert!((closest.1 - 1.5).abs() < 1e-12);
    assert!(
        miss_ray
            .intersect_triangles_closest(&verts, &tris)
            .is_none()
    );

    // Symmetric 3x3 eigendecomposition sorted descending
    let cov = [[5.0, 2.0, 0.0], [2.0, 5.0, 0.0], [0.0, 0.0, 1.0]];
    let (evals, evecs) = symmetric_eigen3(cov);
    assert!((evals[0] - 7.0).abs() < 1e-12);
    assert!((evals[1] - 3.0).abs() < 1e-12);
    assert!((evals[2] - 1.0).abs() < 1e-12);
    for k in 0..3 {
        let col = [evecs[0][k], evecs[1][k], evecs[2][k]];
        let av = crate::linalg::mv(cov, col);
        for r in 0..3 {
            assert!((av[r] - evals[k] * col[r]).abs() < 1e-11);
        }
    }

    // 3x3 linear solver with partial pivoting
    let a = [[0.0, 2.0, 1.0], [1.0, -2.0, -3.0], [-1.0, 1.0, 2.0]];
    let b = [-8.0, 0.0, 3.0];
    let x = solve_linear_3x3(a, b).unwrap();
    let ax = crate::linalg::mv(a, x);
    for i in 0..3 {
        assert!((ax[i] - b[i]).abs() < 1e-12);
    }
    // Singular matrix returns None
    let singular = [[1.0, 2.0, 3.0], [2.0, 4.0, 6.0], [0.0, 1.0, 1.0]];
    assert!(solve_linear_3x3(singular, [1.0, 2.0, 3.0]).is_none());
}

#[test]
fn catmull_rom_spline_and_plane_from_points_pca_work() {
    let p0 = [0.0, 0.0, 0.0];
    let p1 = [1.0, 0.0, 0.0];
    let p2 = [2.0, 1.0, 0.0];
    let p3 = [3.0, 1.0, 0.0];
    let spline = CatmullRomSpline3::centripetal(p0, p1, p2, p3);
    let start = spline.evaluate(0.0);
    let end = spline.evaluate(1.0);
    assert!((norm(sub(start, p1))) < 1e-12);
    assert!((norm(sub(end, p2))) < 1e-12);

    // Verify analytical tangent against central finite difference at t = 0.5
    let eps = 1e-6;
    let c_plus = evaluate_catmull_rom_3d(p0, p1, p2, p3, 0.5 + eps, 0.5);
    let c_minus = evaluate_catmull_rom_3d(p0, p1, p2, p3, 0.5 - eps, 0.5);
    let fd = scale(sub(c_plus, c_minus), 0.5 / eps);
    let tan = spline.tangent(0.5);
    assert!(norm(sub(tan, fd)) < 1e-7);

    // PCA plane fitting on tilted plane z = 2x - y + 3
    let pts = [
        [0.0, 0.0, 3.0],
        [1.0, 0.0, 5.0],
        [0.0, 1.0, 2.0],
        [-1.0, -1.0, 2.0],
        [2.0, 1.0, 6.0],
    ];
    let (centroid, normal) = plane_from_points_pca(&pts).unwrap();
    // Check that all points lie on the fitted plane dot(p - centroid, normal) == 0
    for &p in &pts {
        assert!(dot(sub(p, centroid), normal).abs() < 1e-11);
    }
    // Collinear points return None
    assert!(plane_from_points_pca(&[[0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [2.0, 2.0, 2.0]]).is_none());

    // 3D Morton code (Z-order curve) encode / decode roundtrip and unit-point quantization
    for &(x, y, z) in &[
        (0, 0, 0),
        (1, 2, 3),
        (511, 256, 1023),
        (1023, 1023, 1023),
        (341, 682, 170),
    ] {
        let code = morton3d_encode(x, y, z);
        assert_eq!(morton3d_decode(code), (x, y, z));
    }
    assert_eq!(morton3d_from_unit_point([0.0, 0.0, 0.0]), 0);
    assert_eq!(morton3d_from_unit_point([1.0, 1.0, 1.0]), 0x3FFF_FFFF);
    assert!(morton3d_from_unit_point([0.2, 0.2, 0.2]) < morton3d_from_unit_point([0.8, 0.8, 0.8]));
}
