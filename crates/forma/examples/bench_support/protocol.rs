//! Hand a finished series to `cargo airbug-bench` in its worker protocol.
//!
//! The benchmark executables keep their own JSON line; this adds a second, machine-readable
//! view of the *raw* per-frame series, which the summary line cannot give back: order,
//! autocorrelation and the individual values behind each percentile. It is built after the
//! measured phase from arrays the benchmark already filled, so it adds no work, allocation
//! or clock read to a measured frame.
//!
//! The line is printed only under the process runner (`AIRBUG_BENCH_RUNNER=1`), so scripts
//! that read the existing output are unaffected:
//!
//! ```sh
//! cargo airbug-bench run --protocol --program target/release/examples/perf_bench \
//!     -o .airbug-bench/forma -- Button.ui Button.component.ui gpu animation 1280 720 240
//! ```
use airbug_bench::{Availability, Case, Metric, Recorder, frames};
use std::collections::BTreeMap;

/// What became of the GPU timestamp series.
pub enum GpuPass<'a> {
    /// One duration per profiled frame, milliseconds; `expected` frames were drawn.
    Series { ms: &'a [f64], expected: usize },
    /// The adapter has no timestamp queries, or the scenario does not use them.
    Unsupported(&'a str),
}

pub struct Series<'a> {
    pub case: &'a str,
    pub contract: BTreeMap<String, String>,
    /// `(metric id, scope, phase, values in milliseconds)`, in frame order.
    pub durations: Vec<(&'static str, &'a str, &'a str, &'a [f64])>,
    pub gpu: GpuPass<'a>,
    pub notes: Vec<String>,
}

fn nanoseconds(ms: f64) -> u128 {
    (ms * 1e6).round().max(0.0) as u128
}

/// Print `BENCH_RESULT=…` when the process runner is listening; otherwise do nothing.
pub fn emit(series: Series<'_>) {
    if std::env::var_os("AIRBUG_BENCH_RUNNER").is_none_or(|value| value != "1") {
        return;
    }
    let run = (|| -> airbug_bench::Result<airbug_bench::Run> {
        let mut metrics: Vec<Metric> = series
            .durations
            .iter()
            .map(|(id, scope, phase, _)| {
                let mut metric = Metric::duration(id, scope, "individual frame");
                metric.phase = (*phase).into();
                metric
            })
            .collect();
        // A separately profiled renderer: never a part of the frames above.
        let mut gpu = Metric::duration(
            frames::GPU_PASS,
            "GPU pass; timestamp queries on a separate profiled renderer",
            "individual pass",
        );
        gpu.phase = "gpu".into();
        metrics.push(gpu);
        let mut recorder = Recorder::new();
        recorder.case(Case {
            id: series.case.into(),
            contract: series.contract,
            metrics,
        })?;
        for (id, _, _, values) in &series.durations {
            for ms in values.iter() {
                recorder.observe(series.case, id, nanoseconds(*ms))?;
            }
        }
        match series.gpu {
            GpuPass::Series { ms, expected } => {
                for value in ms {
                    recorder.observe(series.case, frames::GPU_PASS, nanoseconds(*value))?;
                }
                // A query that did not resolve is a missing result, not a fast pass.
                for _ in ms.len()..expected {
                    let why =
                        Availability::Incomplete("GPU timestamp query did not resolve".into());
                    recorder.unavailable(series.case, frames::GPU_PASS, why)?;
                }
            }
            GpuPass::Unsupported(why) => {
                recorder.unavailable(
                    series.case,
                    frames::GPU_PASS,
                    Availability::Unsupported(why.into()),
                )?;
            }
        }
        for note in series.notes {
            recorder.note(note);
        }
        recorder.finish()
    })();
    match run.and_then(|run| Ok(serde_json::to_string(&run)?)) {
        Ok(json) => println!("BENCH_RESULT={json}"),
        // The benchmark itself succeeded; say why the protocol view is missing.
        Err(error) => eprintln!("airbug-bench protocol output skipped: {error}"),
    }
}
