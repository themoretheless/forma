//! Map/reduce fusion and conditional sum, resident inputs and one scalar readback.
use compute_core::{
    BinaryOp, CompareOp, ComputeRuntime, FusionGraph, UnaryOp, gpu_compute::GpuContext,
};
use std::time::{Duration, Instant};
fn median(mut x: Vec<f64>) -> f64 {
    x.sort_by(f64::total_cmp);
    x[x.len() / 2] * 1000.0
}
fn scalar(x: f32, y: f32, case: &str) -> f32 {
    match case {
        "dot" => x * y,
        "squared_distance" => (x - y) * (x - y),
        "conditional" => {
            if x > y {
                x
            } else {
                0.0
            }
        }
        _ => {
            let a = (x * 0.5 + 0.25).sin();
            let b = (a * a * 0.75 + 0.1).cos();
            (b * b * 0.25 - 0.2).sin().abs()
        }
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let c = GpuContext::new().ok_or("GPU adapter required")?;
    let rt = ComputeRuntime::new(&c)?;
    println!(
        "backend={} 3 warmups,21 rotated samples; walltime includes submit,wait,4-byte readback; resident arrays; compile/prepare excluded",
        c.backend_label()
    );
    for case in ["dot", "squared_distance", "nonlinear", "conditional"] {
        let mut g = FusionGraph::new(2);
        let x = g.input(0)?;
        let y = g.input(1)?;
        let root = match case {
            "dot" => g.binary(BinaryOp::Multiply, &x, &y)?,
            "squared_distance" => {
                let d = g.binary(BinaryOp::Subtract, &x, &y)?;
                g.unary(UnaryOp::Square, &d)?
            }
            "conditional" => {
                let keep = g.compare(CompareOp::Greater, &x, &y)?;
                let zero = g.constant(0.0)?;
                g.select(&keep, &x, &zero)?
            }
            _ => {
                let a = g.affine(&x, 0.5, 0.25)?;
                let a = g.unary(UnaryOp::Sin, &a)?;
                let a = g.unary(UnaryOp::Square, &a)?;
                let b = g.affine(&a, 0.75, 0.1)?;
                let b = g.unary(UnaryOp::Cos, &b)?;
                let b = g.unary(UnaryOp::Square, &b)?;
                let d = g.affine(&b, 0.25, -0.2)?;
                let d = g.unary(UnaryOp::Sin, &d)?;
                g.unary(UnaryOp::Abs, &d)?
            }
        };
        let map_kernel = g.compile(&c, std::slice::from_ref(&root))?;
        let sum_kernel = g.compile_sum(&c, &root)?;
        for n in [4097, 1_000_003, 4_000_003] {
            let values: Vec<f32> = (0..n).map(|i| (i % 251) as f32 / 128.0 - 1.0).collect();
            let y = 0.25;
            let expected = values
                .iter()
                .map(|&x| f64::from(scalar(x, y, case)))
                .sum::<f64>();
            let magnitude = values
                .iter()
                .map(|&x| f64::from(scalar(x, y, case)).abs())
                .sum::<f64>();
            let input = rt.upload(&values)?;
            let other = rt.upload(&[y])?;
            let mut map = rt.program();
            let mapped = map.fused(&map_kernel, &[&input, &other], n)?;
            let map_sum = map.sum(&mapped[0])?;
            let mut fused = rt.program();
            let fused_sum = fused.fused_sum(&sum_kernel, &[&input, &other], n)?;
            let mut selection = rt.program();
            let selected_sum = if case == "conditional" {
                let mask = selection.compare(CompareOp::Greater, &input, &other)?;
                let selected = selection.compact(&input, &mask)?;
                Some(selection.sum(selected.values())?)
            } else {
                None
            };
            let paths = if selected_sum.is_some() { 5 } else { 4 };
            let execute = |path| -> Result<f64, Box<dyn std::error::Error>> {
                let result = match path {
                    0 => map.submit_read(&map_sum)?,
                    1 => fused.submit_read(&fused_sum)?,
                    2 => {
                        rt.write(&input, 0, &values)?;
                        fused.submit_read(&fused_sum)?
                    }
                    3 => {
                        return Ok(std::hint::black_box(&values)
                            .iter()
                            .map(|&x| f64::from(scalar(x, y, case)))
                            .sum());
                    }
                    _ => selection.submit_read(selected_sum.as_ref().unwrap())?,
                }
                .wait(Duration::from_secs(20))?;
                Ok(f64::from(result[0]))
            };
            let check = |got: f64| {
                assert!(
                    (got - expected).abs() < 3e-6 * magnitude.max(1.0),
                    "{case} n={n} {got}!={expected}"
                );
            };
            for i in 0..3 {
                for j in 0..paths {
                    check(execute((i + j) % paths)?);
                }
            }
            let mut samples: Vec<Vec<f64>> = (0..paths).map(|_| Vec::new()).collect();
            for i in 0..21 {
                for j in 0..paths {
                    let path = (i + j) % paths;
                    let t = Instant::now();
                    let got = execute(path)?;
                    samples[path].push(t.elapsed().as_secs_f64());
                    check(got);
                    std::hint::black_box(got);
                }
            }
            let raw = samples.clone();
            let times: Vec<f64> = samples.into_iter().map(median).collect();
            println!(
                "case={case} n={n} map_sum_ms={:.6} fused_sum_ms={:.6} upload_fused_ms={:.6} cpu_f64_ms={:.6} compact_sum_ms={:?} mapped_bytes={}->0 first_partials_bytes={}",
                times[0],
                times[1],
                times[2],
                times[3],
                times.get(4),
                n * 4,
                n.div_ceil(2048) * 4
            );
            println!("samples_seconds={raw:?}");
        }
    }
    Ok(())
}
