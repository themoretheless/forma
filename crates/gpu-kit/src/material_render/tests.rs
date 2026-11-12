use super::*;
use crate::test_support::validate_wgsl;
#[cfg(target_os = "macos")]
use crate::test_support::{Readback, clear_pass, metal_device, texture_2d};
#[test]
fn material_shader_and_layout_validate() {
    assert_eq!(size_of::<MaterialVertex>(), 96);
    assert_eq!(size_of::<MaterialParameters>(), 176);
    validate_wgsl(
        include_str!("../../shaders/material_view.wgsl"),
        wgpu::naga::valid::Capabilities::empty(),
    );
}
#[test]
#[cfg(target_os = "macos")]
#[ignore = "actual Metal material draw, alpha, culling, lighting and root leases"]
fn material_draw_metal() {
    let (device, queue) = metal_device(wgpu::Features::FLOAT32_FILTERABLE);
    let budget = MemoryBudget::new(1024 * 1024);
    let uploader = MaterialTextureUploader::new(&device, &queue);
    let white = uploader
        .upload_rgba8(
            &budget,
            [1, 1],
            &[255; 4],
            false,
            Default::default(),
            || false,
        )
        .unwrap();
    let white_color = uploader
        .upload_rgba8(&budget, [1, 1], &[255; 4], true, Default::default(), || {
            false
        })
        .unwrap();
    let neutral = [&white_color, &white, &white, &white, &white_color];
    let renderer =
        MaterialRenderer::new(uploader.clone(), wgpu::TextureFormat::Rgba8UnormSrgb).unwrap();
    assert!(
        MaterialRenderer::new(uploader.clone(), wgpu::TextureFormat::Bc1RgbaUnormSrgb).is_err()
    );
    let target = texture_2d(
        &device,
        wgpu::Extent3d {
            width: 3,
            height: 3,
            depth_or_array_layers: 1,
        },
        wgpu::TextureFormat::Rgba8UnormSrgb,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let depth = texture_2d(
        &device,
        target.size(),
        crate::pipeline::DEPTH_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT,
    );
    let vertex = MaterialVertex {
        position: [0.; 3],
        normal: [0., 0., 1.],
        tangent: [1., 0., 0., 1.],
        color: [1.; 4],
        texcoords: [[0.5; 2]; 5],
    };
    let vertices = [[-1., -1., 0.5], [3., -1., 0.5], [-1., 3., 0.5]]
        .map(|position| MaterialVertex { position, ..vertex });
    let render = |draw: &MaterialDraw| {
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let view = target.create_view(&Default::default());
            let dv = depth.create_view(&Default::default());
            let mut pass = clear_pass(&mut encoder, &view, &dv);
            renderer.encode(draw, &mut pass).unwrap();
        }
        let readback = Readback::record(&device, &mut encoder, &target, 0, 4);
        queue.submit([encoder.finish()]);
        draw.resource_lease().retain_until_complete(&queue);
        // The centre texel of the 3x3 target.
        let pixel: [u8; 4] = readback.read(&device)[16..20].try_into().unwrap();
        pixel
    };
    let mut p = MaterialParameters::default();
    p.light_direction[3] = 0.;
    p.eye[3] = 0.;
    p.emissive = [0.25, 0.5, 1., 0.];
    p.base_color[3] = 0.25;
    for (mode, expected) in [
        (MaterialAlpha::Opaque, [137, 188, 255, 255]),
        (MaterialAlpha::Mask, [0, 0, 0, 0]),
        (MaterialAlpha::Blend, [71, 99, 137, 64]),
    ] {
        let draw = renderer
            .prepare(
                &budget,
                &vertices,
                p,
                neutral,
                MaterialRaster {
                    alpha: mode,
                    double_sided: false,
                    clockwise: false,
                },
                || false,
            )
            .unwrap();
        let actual = render(&draw);
        for c in 0..4 {
            assert!(
                actual[c].abs_diff(expected[c]) <= 1,
                "{mode:?}: {actual:?}/{expected:?}"
            );
        }
        eprintln!("material {mode:?}: {actual:?}");
    }
    let mut cutoff = p;
    cutoff.alpha[0] = 0.25;
    let equal = renderer
        .prepare(
            &budget,
            &vertices,
            cutoff,
            neutral,
            MaterialRaster {
                alpha: MaterialAlpha::Mask,
                double_sided: false,
                clockwise: false,
            },
            || false,
        )
        .unwrap();
    assert_eq!(render(&equal), [137, 188, 255, 255]);
    drop(equal);
    cutoff.alpha[0] = 0.25001;
    let above = renderer
        .prepare(
            &budget,
            &vertices,
            cutoff,
            neutral,
            MaterialRaster {
                alpha: MaterialAlpha::Mask,
                double_sided: false,
                clockwise: false,
            },
            || false,
        )
        .unwrap();
    assert_eq!(render(&above), [0; 4]);
    drop(above);
    p.emissive = [0.; 4];
    p.light_direction[3] = 1.;
    p.base_color = [0.5, 0.25, 0.125, 1.];
    p.factors[0] = 0.;
    let draw = renderer
        .prepare(
            &budget,
            &vertices,
            p,
            neutral,
            MaterialRaster {
                alpha: MaterialAlpha::Opaque,
                double_sided: false,
                clockwise: false,
            },
            || false,
        )
        .unwrap();
    let actual = render(&draw);
    for (c, numerator) in [0.49f64, 0.25, 0.13].into_iter().enumerate() {
        let v = numerator / std::f64::consts::PI;
        let expected = ((1.055 * v.powf(1. / 2.4) - 0.055) * 255.).round() as u8;
        assert!(
            actual[c].abs_diff(expected) <= 1,
            "direct GGX channel {c}: {actual:?}/{expected}"
        );
    }
    eprintln!("material dielectric direct GGX: {actual:?}");
    drop(draw);
    let mut normal_p = p;
    normal_p.alpha[2] = 1.;
    normal_p.light_direction = [1., 1., 1., 1.];
    for frame in 0..3 {
        let mapped = vertices.map(|mut v| {
            v.texcoords[2] = [(v.position[0] + 1.) / 2., (v.position[1] + 1.) / 2.];
            if frame == 1 {
                v.tangent = [0.; 4];
            }
            if frame == 2 {
                v.normal = [0.; 3];
                v.tangent = [0., 1., 0., 1.];
            }
            v
        });
        let draw = renderer
            .prepare(
                &budget,
                &mapped,
                normal_p,
                neutral,
                MaterialRaster {
                    alpha: MaterialAlpha::Opaque,
                    double_sided: false,
                    clockwise: false,
                },
                || false,
            )
            .unwrap();
        let actual = render(&draw);
        let expected = [110u8, 80, 58, 255];
        for c in 0..4 {
            assert!(
                actual[c].abs_diff(expected[c]) <= 1,
                "normal frame={frame}: {actual:?}"
            );
        }
        eprintln!("normal frame={frame}: {actual:?}");
    }
    let color16 = uploader
        .upload_rgba16(
            &budget,
            [1, 1],
            &[65535; 4],
            true,
            Default::default(),
            || false,
        )
        .unwrap();
    let data16 = uploader
        .upload_rgba16(
            &budget,
            [1, 1],
            &[65535; 4],
            false,
            Default::default(),
            || false,
        )
        .unwrap();
    let draw16 = renderer
        .prepare(
            &budget,
            &vertices,
            p,
            [&color16, &data16, &data16, &data16, &color16],
            MaterialRaster {
                alpha: MaterialAlpha::Opaque,
                double_sided: false,
                clockwise: false,
            },
            || false,
        )
        .unwrap();
    assert_eq!(render(&draw16), actual);
    eprintln!("16-bit source float32 texture bindings reproduce the RGBA8 white-map draw");
    drop(draw16);
    drop(color16);
    drop(data16);
    let backwards = [vertices[0], vertices[2], vertices[1]];
    for (double, clockwise, visible) in [
        (false, false, false),
        (true, false, true),
        (false, true, true),
    ] {
        let draw = renderer
            .prepare(
                &budget,
                &backwards,
                p,
                neutral,
                MaterialRaster {
                    alpha: MaterialAlpha::Opaque,
                    double_sided: double,
                    clockwise,
                },
                || false,
            )
            .unwrap();
        let pixel = render(&draw);
        assert_eq!(pixel[3] != 0, visible);
        if double {
            assert_eq!(pixel[..3], [0, 0, 0]);
        }
        if clockwise {
            assert_eq!(pixel, actual);
        }
    }
    // Linear HDR target preserves radiance above 1; no SDR transfer/tone map.
    let hdr_renderer =
        MaterialRenderer::new(uploader.clone(), wgpu::TextureFormat::Rgba16Float).unwrap();
    let hdr = texture_2d(
        &device,
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        wgpu::TextureFormat::Rgba16Float,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let hdr_depth = texture_2d(
        &device,
        hdr.size(),
        crate::pipeline::DEPTH_FORMAT,
        wgpu::TextureUsages::RENDER_ATTACHMENT,
    );
    let mut hdr_p = p;
    hdr_p.light_direction[3] = 16.;
    let hdr_draw = hdr_renderer
        .prepare(
            &budget,
            &vertices,
            hdr_p,
            neutral,
            MaterialRaster {
                alpha: MaterialAlpha::Opaque,
                double_sided: false,
                clockwise: false,
            },
            || false,
        )
        .unwrap();
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let view = hdr.create_view(&Default::default());
        let depth_view = hdr_depth.create_view(&Default::default());
        let mut pass = clear_pass(&mut encoder, &view, &depth_view);
        hdr_renderer.encode(&hdr_draw, &mut pass).unwrap();
    }
    let readback = Readback::record(&device, &mut encoder, &hdr, 0, 8);
    queue.submit([encoder.finish()]);
    let lease = hdr_draw.resource_lease();
    drop(hdr_draw);
    lease.retain_until_complete(&queue);
    let mapped = readback.read(&device);
    let mut radiance = [0f64; 4];
    for (c, value) in radiance.iter_mut().enumerate() {
        let bits = u16::from_ne_bytes(mapped[2 * c..2 * c + 2].try_into().unwrap());
        let e = (bits >> 10) & 31;
        let f = bits & 1023;
        assert_eq!(bits >> 15, 0);
        assert!(e < 31);
        *value = if e == 0 {
            f64::from(f) * 2f64.powi(-24)
        } else {
            (1. + f64::from(f) / 1024.) * 2f64.powi(i32::from(e) - 15)
        };
    }
    for (c, numerator) in [0.49, 0.25, 0.13].into_iter().enumerate() {
        assert!((radiance[c] - 16. * numerator / std::f64::consts::PI).abs() < 0.002);
    }
    assert_eq!(radiance[3], 1.);
    assert!(radiance[0] > 2.);
    eprintln!("linear HDR material radiance: {radiance:?}");
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let resident = budget.used();
    let tiny = budget.child(463); // 3*96 + 176 = 464 bytes required.
    assert!(matches!(
        renderer.prepare(
            &tiny,
            &vertices,
            p,
            neutral,
            MaterialRaster {
                alpha: MaterialAlpha::Opaque,
                double_sided: false,
                clockwise: false
            },
            || false
        ),
        Err(MaterialRenderError::Memory(_))
    ));
    assert_eq!(budget.used(), resident);
    let exact = budget.child(464);
    let exact_draw = renderer
        .prepare(
            &exact,
            &vertices,
            p,
            neutral,
            MaterialRaster {
                alpha: MaterialAlpha::Opaque,
                double_sided: false,
                clockwise: false,
            },
            || false,
        )
        .unwrap();
    assert_eq!(exact.used(), 464);
    let held = exact_draw.resource_lease();
    drop(exact_draw);
    assert_eq!(exact.used(), 464);
    drop(held);
    assert_eq!(exact.used(), 0);
    assert_eq!(budget.used(), resident);
    let mut checks = 0;
    assert!(matches!(
        renderer.prepare(
            &budget,
            &vertices,
            p,
            neutral,
            MaterialRaster {
                alpha: MaterialAlpha::Opaque,
                double_sided: false,
                clockwise: false
            },
            || {
                checks += 1;
                checks == 4
            }
        ),
        Err(MaterialRenderError::Cancelled)
    ));
    assert_eq!(budget.used(), resident);
    assert!(matches!(
        renderer.prepare(
            &budget,
            &vertices,
            p,
            [&white; 5],
            MaterialRaster {
                alpha: MaterialAlpha::Opaque,
                double_sided: false,
                clockwise: false
            },
            || false
        ),
        Err(MaterialRenderError::Invalid)
    ));
    let mut invalid = vertices;
    invalid[0].position[0] = f32::NAN;
    assert!(matches!(
        renderer.prepare(
            &budget,
            &invalid,
            p,
            neutral,
            MaterialRaster {
                alpha: MaterialAlpha::Opaque,
                double_sided: false,
                clockwise: false
            },
            || false
        ),
        Err(MaterialRenderError::Invalid)
    ));
    invalid[0].position[0] = 1e20;
    assert!(matches!(
        renderer.prepare(
            &budget,
            &invalid,
            p,
            neutral,
            MaterialRaster {
                alpha: MaterialAlpha::Opaque,
                double_sided: false,
                clockwise: false
            },
            || false
        ),
        Err(MaterialRenderError::Invalid)
    ));
    assert_eq!(budget.used(), resident);
    let foreign = MaterialRenderer::new(
        MaterialTextureUploader::new(&device, &queue),
        wgpu::TextureFormat::Rgba8UnormSrgb,
    )
    .unwrap();
    assert!(matches!(
        foreign.prepare(
            &budget,
            &vertices,
            p,
            neutral,
            MaterialRaster {
                alpha: MaterialAlpha::Opaque,
                double_sided: false,
                clockwise: false
            },
            || false
        ),
        Err(MaterialRenderError::Owner)
    ));
    drop(white);
    drop(white_color);
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    assert_eq!(budget.used(), 0);
}
