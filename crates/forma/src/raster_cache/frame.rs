//! The frame mask as runs of equal coverage.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FrameRun {
    // Exclusive global pixel index; raster size is bounded below u32::MAX.
    pub(super) end: u32,
    pub(super) value: u8,
}

#[cfg(test)]
thread_local! { pub(super) static FRAME_SAMPLES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

pub(super) fn frame_value(
    model: ControlView<'_>,
    key: GeometryKey,
    v: [f32; 4],
    x: u32,
    y: u32,
) -> u8 {
    #[cfg(test)]
    FRAME_SAMPLES.with(|count| count.set(count.get() + 1));
    let [vx, vy, vw, vh] = v;
    let b = &model.scene.button;
    let mut frame = (round_coverage(
        x,
        y,
        key.scale,
        [0., 0., model.width(), model.height()],
        model.scene.radius,
    ) * 16.) as u8;
    let mut viewport = 1;
    if model.scene.scroll {
        let px = (x as f32 + 0.5) / key.scale;
        let py = (y as f32 + 0.5) / key.scale;
        if px < vx || py < vy || px >= vx + vw || py >= vy + vh {
            viewport = 0;
        }
        if vw > 0. && vh > 0. {
            let ch = b.height;
            let cw = b.width;
            let vertical = ch > vh
                && px >= (vx + vw - 5.).max(vx)
                && px < vx + vw
                && py >= vy + model.scroll_y / ch * vh
                && py < vy + (model.scroll_y + vh) / ch * vh;
            let horizontal = cw > vw
                && py >= (vy + vh - 5.).max(vy)
                && py < vy + vh
                && px >= vx + model.scroll_x / cw * vw
                && px < vx + (model.scroll_x + vw) / cw * vw;
            if vertical || horizontal {
                viewport = 2;
            }
        }
    }
    frame |= viewport << 5;
    frame
}

pub(super) fn append_frame_run(runs: &mut Vec<FrameRun>, end: u32, value: u8) {
    match runs.last_mut() {
        Some(run) if run.value == value => run.end = end,
        _ => runs.push(FrameRun { end, value }),
    }
}

// All x comparisons use the original pixel-center arithmetic. This avoids
// floor/ceil conversions changing inclusive/exclusive viewport boundaries.
pub(super) fn frame_x_boundary(width: u32, scale: f32, threshold: f32) -> u32 {
    let (mut lo, mut hi) = (0, width);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if (mid as f32 + 0.5) / scale >= threshold {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    lo
}

pub(super) fn frame_runs(model: ControlView<'_>, key: GeometryKey) -> Vec<FrameRun> {
    let mut runs = Vec::new();
    if key.sparse {
        return runs;
    }
    let v = model.viewport_rect();
    let [vx, _, vw, vh] = v;
    let b = &model.scene.button;
    let mut thresholds = vec![model.width() / 2.];
    if model.scene.scroll {
        thresholds.extend([vx, vx + vw]);
        if vw > 0. && vh > 0. {
            thresholds.push((vx + vw - 5.).max(vx));
            if b.width > vw {
                thresholds.extend([
                    vx + model.scroll_x / b.width * vw,
                    vx + (model.scroll_x + vw) / b.width * vw,
                ]);
            }
        }
    }
    let monotone = key.width >= 64
        && model.width().is_finite()
        && model.height().is_finite()
        && model.width() > 0.
        && model.height() > 0.
        && model.scene.radius.is_finite()
        && model.scene.radius >= 0.
        && v.iter().chain(thresholds.iter()).all(|n| n.is_finite());
    let mut cuts = vec![0, key.width];
    if monotone {
        cuts.extend(
            thresholds
                .iter()
                .map(|&x| frame_x_boundary(key.width, key.scale, x)),
        );
        cuts.sort_unstable();
        cuts.dedup();
    }
    for y in 0..key.height {
        if !monotone {
            for x in 0..key.width {
                append_frame_run(
                    &mut runs,
                    y * key.width + x + 1,
                    frame_value(model, key, v, x, y),
                );
            }
            continue;
        }
        // Each interval has constant viewport/scrollbar classification and
        // lies on one side of the rounded frame center. Its coverage is
        // monotone, so an equal-value prefix cannot recur after it ends.
        for interval in cuts.windows(2) {
            let mut x = interval[0];
            while x < interval[1] {
                let value = frame_value(model, key, v, x, y);
                let (mut lo, mut hi) = (x + 1, interval[1]);
                while lo < hi {
                    let mid = lo + (hi - lo) / 2;
                    if frame_value(model, key, v, mid, y) == value {
                        lo = mid + 1;
                    } else {
                        hi = mid;
                    }
                }
                append_frame_run(&mut runs, y * key.width + lo, value);
                x = lo;
            }
        }
    }
    runs
}
