//! Headless resize: old full-viewport RGBA path vs retained scene/XRGB path.
//! Includes painting into a host buffer, excludes OS resize/present and allocation
//! of that host buffer. Both paths use identical sizes, DPI and source.
use std::{hint::black_box, time::Instant};

fn legacy(b: &forma_vector::Button, dst: &mut [u32], w: u32, h: u32, s: f32) {
    let pixels = b.pixels(w, h, s);
    assert_eq!(pixels.len(), dst.len() * 4);
    for (dst, p) in dst.iter_mut().zip(pixels.chunks_exact(4)) {
        let a = p[3] as u32;
        let c = |k: usize, bg: u32| (p[k] as u32 * a + bg * (255 - a) + 127) / 255;
        *dst = (c(0, 17) << 16) | (c(1, 19) << 8) | c(2, 25);
    }
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let source = args
        .get(1)
        .map(|p| std::fs::read_to_string(p).unwrap())
        .unwrap_or(forma_vector::EXAMPLE.into());
    let template = args
        .get(2)
        .map(|p| std::fs::read_to_string(p).unwrap())
        .unwrap_or(forma_vector::BUTTON_COMPONENT.into());
    for (w, h) in [(1600, 1000), (3200, 2000), (3840, 2160)] {
        for retained in [false, true] {
            let b = forma_vector::Button::from_sources(&source, &template).unwrap();
            let mut buffer = vec![0u32; (w * h) as usize];
            let mut times = Vec::new();
            for i in 0..20 {
                // Repeated grow/shrink steps; no change to logical layout.
                let cw = w - i % 10 * 7;
                let ch = h - i % 10 * 3;
                let dst = &mut buffer[..(cw * ch) as usize];
                let t = Instant::now();
                if retained {
                    b.paint_native(dst, cw, ch, 2.).unwrap();
                } else {
                    legacy(&b, dst, cw, ch, 2.);
                }
                black_box(&dst);
                times.push(t.elapsed().as_secs_f64() * 1000.);
            }
            let cold = times[0];
            let mean = times[1..].iter().sum::<f64>() / (times.len() - 1) as f64;
            times[1..].sort_by(f64::total_cmp);
            println!("{w}x{h} {}: first={cold:.3}ms resize_mean={mean:.3}ms resize_max={:.3}ms raster={:?}",if retained{"retained"}else{"legacy"},times[times.len()-1],b.raster_stats());
        }
    }
}
