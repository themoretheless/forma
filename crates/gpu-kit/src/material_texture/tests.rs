use super::*;
use crate::test_support::validate_wgsl;
#[cfg(target_os = "macos")]
use crate::test_support::{Readback, metal_device};
#[test]
fn material_mip_shader_validates() {
    validate_wgsl(
        include_str!("../../shaders/material_mip.wgsl"),
        wgpu::naga::valid::Capabilities::empty(),
    );
}
#[test]
fn sampler_all_core_modes_and_rejections() {
    for min in [9728, 9729, 9984, 9985, 9986, 9987] {
        for wrap in [10497, 33071, 33648] {
            let d = MaterialSampler {
                min: Some(min),
                wrap_s: wrap,
                wrap_t: wrap,
                ..Default::default()
            }
            .descriptor()
            .unwrap();
            assert_eq!(d.lod_max_clamp == 0., min < 9984);
        }
    }
    for s in [
        MaterialSampler {
            mag: Some(9987),
            ..Default::default()
        },
        MaterialSampler {
            min: Some(0),
            ..Default::default()
        },
        MaterialSampler {
            wrap_s: 0,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            s.descriptor(),
            Err(MaterialTextureError::InvalidSampler)
        ));
    }
}
#[test]
#[cfg(target_os = "macos")]
#[ignore = "actual Metal material sRGB/data mip readback and admission"]
fn material_texture_metal_readback_and_budget() {
    let (device, queue) = metal_device(wgpu::Features::empty());
    let uploader = MaterialTextureUploader::new(&device, &queue);
    let uploader = uploader.clone();
    let missing_feature_budget = MemoryBudget::new(1024);
    assert!(matches!(
        uploader.upload_rgba16(
            &missing_feature_budget,
            [1, 1],
            &[1u16; 4],
            false,
            MaterialSampler::default(),
            || false
        ),
        Err(MaterialTextureError::Float32FilteringUnavailable)
    ));
    assert_eq!(missing_feature_budget.used(), 0);
    // Black/white checker: linear RGB average .5 encodes near 188 in sRGB,
    // whereas data averages to 128. Alpha must stay linear in either role.
    let pixels = [
        0, 0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 0,
    ];
    // Non-mip sampling requires no shader/pipeline construction.
    let non_mip_budget = MemoryBudget::new(528);
    let non_mip = uploader
        .upload_rgba8(
            &non_mip_budget,
            [2, 2],
            &pixels,
            true,
            MaterialSampler {
                min: Some(9729),
                ..Default::default()
            },
            || false,
        )
        .unwrap();
    let rebound = uploader
        .with_sampler(
            &non_mip,
            MaterialSampler {
                min: Some(9728),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(Arc::ptr_eq(&rebound.credit, &non_mip.credit));
    assert!(
        uploader
            .with_sampler(&non_mip, MaterialSampler::default())
            .is_err()
    );
    assert!(
        MaterialTextureUploader::new(&device, &queue)
            .with_sampler(
                &non_mip,
                MaterialSampler {
                    min: Some(9728),
                    ..Default::default()
                }
            )
            .is_err()
    );
    drop(rebound);
    assert_eq!(non_mip.texture().mip_level_count(), 1);
    assert!(uploader.pipelines.iter().all(|p| p.get().is_none()));
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    assert_eq!(non_mip_budget.used(), 16);
    drop(non_mip);
    assert_eq!(non_mip_budget.used(), 0);
    for (size, input, resident, peak, data_expected, color_expected) in [
        ([2, 2], pixels.to_vec(), 20, 532, 128, 188),
        (
            [3, 1],
            vec![255, 255, 255, 255, 0, 0, 0, 0, 255, 255, 255, 255],
            16,
            272,
            170,
            213,
        ),
        (
            [1, 3],
            vec![255, 255, 255, 255, 0, 0, 0, 0, 255, 255, 255, 255],
            16,
            784,
            170,
            213,
        ),
    ] {
        for srgb in [false, true] {
            let budget = MemoryBudget::new(peak);
            let candidate = uploader
                .upload_rgba8(
                    &budget,
                    size,
                    &input,
                    srgb,
                    MaterialSampler::default(),
                    || false,
                )
                .unwrap();
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            assert_eq!(budget.used(), resident);
            let mut encoder = device.create_command_encoder(&Default::default());
            let readback = Readback::record(&device, &mut encoder, candidate.texture(), 1, 4);
            queue.submit([encoder.finish()]);
            let lease = candidate.resource_lease();
            drop(candidate);
            assert_eq!(budget.used(), resident);
            lease.retain_until_complete(&queue);
            let mapped = readback.read(&device);
            let expected: i16 = if srgb { color_expected } else { data_expected };
            for &channel in &mapped[..3] {
                assert!((i16::from(channel) - expected).abs() <= 1);
            }
            assert!((i16::from(mapped[3]) - data_expected).abs() <= 1);
            eprintln!("size={size:?} srgb={srgb} mip={:?}", &mapped[..4]);
            assert_eq!(budget.used(), 0);
        }
    }
    let shared = uploader.clone();
    assert!(Arc::ptr_eq(&shared.pipelines, &uploader.pipelines));
    assert_eq!(
        shared
            .pipelines
            .iter()
            .filter(|p| p.get().is_some())
            .count(),
        2
    );
    eprintln!("six mip candidates share two format pipelines; non-mip upload builds none");
    let budget = MemoryBudget::new(531);
    assert!(matches!(
        uploader.upload_rgba8(
            &budget,
            [2, 2],
            &pixels,
            true,
            MaterialSampler::default(),
            || { false }
        ),
        Err(MaterialTextureError::Memory(_))
    ));
    assert_eq!(budget.used(), 0);
    let budget = MemoryBudget::new(532);
    let mut checks = 0;
    assert!(matches!(
        uploader.upload_rgba8(
            &budget,
            [2, 2],
            &pixels,
            true,
            MaterialSampler::default(),
            || {
                checks += 1;
                checks == 3
            }
        ),
        Err(MaterialTextureError::Cancelled)
    ));
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    assert_eq!(budget.used(), 0);
}
#[cfg(target_os = "macos")]
fn read_float_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    mip: u32,
) -> Vec<f32> {
    let mut encoder = device.create_command_encoder(&Default::default());
    let readback = Readback::record(device, &mut encoder, texture, mip, 16);
    queue.submit([encoder.finish()]);
    readback
        .read(device)
        .chunks_exact(4)
        .map(|b| f32::from_ne_bytes(b.try_into().unwrap()))
        .collect()
}

#[test]
#[cfg(target_os = "macos")]
#[ignore = "actual Metal all 65536 normalized codes, sRGB and float32 mip proof"]
fn material_texture_16bit_metal() {
    let (device, queue) = metal_device(wgpu::Features::FLOAT32_FILTERABLE);
    let uploader = MaterialTextureUploader::new(&device, &queue);
    let input: Vec<u16> = (0..=u16::MAX).flat_map(|v| [v, v, v, v]).collect();
    for srgb in [false, true] {
        let budget = MemoryBudget::new(4 * 1024 * 1024);
        let texture = uploader
            .upload_rgba16(
                &budget,
                [256, 256],
                &input,
                srgb,
                MaterialSampler::default(),
                || false,
            )
            .unwrap();
        let base = read_float_texture(&device, &queue, texture.texture(), 0);
        for (i, pixel) in base.chunks_exact(4).enumerate() {
            for (c, &value) in pixel.iter().enumerate() {
                let v = f64::from(value);
                let encoded = if srgb && c != 3 {
                    if v <= 0.0031308 {
                        v * 12.92
                    } else {
                        1.055 * v.powf(1. / 2.4) - 0.055
                    }
                } else {
                    v
                };
                assert_eq!(
                    (encoded * 65535.).round() as usize,
                    i,
                    "srgb={srgb} channel={c}"
                );
            }
        }
        let mip = read_float_texture(&device, &queue, texture.texture(), 1);
        // First footprint contains source codes 0,1,256,257. All are in
        // the linear toe for sRGB; independent average is exactly 128.5 codes.
        let average = 128.5 / 65535.;
        for (c, &value) in mip.iter().take(4).enumerate() {
            let expected = if srgb && c != 3 {
                average / 12.92
            } else {
                average
            };
            assert!((f64::from(value) - expected).abs() < 1e-9);
        }
        eprintln!(
            "16bit srgb={srgb}: all 65536 codes roundtrip, first mip={:?}",
            &mip[..4]
        );
        drop(texture);
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        assert_eq!(budget.used(), 0);
    }
    let invalid_budget = MemoryBudget::new(656);
    for (size, input) in [
        ([u32::MAX, u32::MAX], &[][..]),
        ([0, 1], &[][..]),
        ([1, 1], &[1u16; 3][..]),
    ] {
        assert!(matches!(
            uploader.upload_rgba16(
                &invalid_budget,
                size,
                input,
                false,
                MaterialSampler::default(),
                || false
            ),
            Err(MaterialTextureError::InvalidLayout)
        ));
        assert_eq!(invalid_budget.used(), 0);
    }
    let small = [
        1u16, 257, 258, 65534, 2, 258, 259, 65535, 3, 259, 260, 65533, 4, 260, 261, 65532,
    ];
    let budget = MemoryBudget::new(655);
    assert!(matches!(
        uploader.upload_rgba16(
            &budget,
            [2, 2],
            &small,
            false,
            MaterialSampler::default(),
            || { false }
        ),
        Err(MaterialTextureError::Memory(_))
    ));
    assert_eq!(budget.used(), 0);
    let budget = MemoryBudget::new(656);
    let candidate = uploader
        .upload_rgba16(
            &budget,
            [2, 2],
            &small,
            false,
            MaterialSampler::default(),
            || false,
        )
        .unwrap();
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    assert_eq!(budget.used(), 80);
    drop(candidate);
    assert_eq!(budget.used(), 0);
    let mut calls = 0;
    assert!(matches!(
        uploader.upload_rgba16(
            &budget,
            [2, 2],
            &small,
            false,
            MaterialSampler::default(),
            || {
                calls += 1;
                calls == 5
            }
        ),
        Err(MaterialTextureError::Cancelled)
    ));
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    assert_eq!(budget.used(), 0);
    eprintln!("16bit exact logical peak 656, refusal at 655, cancellation and final root zero");
}
