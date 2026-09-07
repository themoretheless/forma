//! Cadence of successful presentation calls, independent of the renderer.
//!
//! The sample retains the complete intervals ending in the latest second,
//! including the preceding boundary frame. This measures `(frames - 1) / time`
//! between actual presentation timestamps, without inventing a first interval.

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

const SAMPLE_WINDOW: Duration = Duration::from_secs(1);
const IDLE_AFTER: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FrameRate {
    Waiting,
    Sampling,
    Active { fps: f64, frame_ms: f64 },
    Idle,
}

#[derive(Debug, Default)]
pub struct FrameStats {
    presented: VecDeque<Instant>,
}

impl FrameStats {
    /// Record only completed calls to present, never skipped redraw attempts.
    pub fn presented(&mut self, now: Instant) {
        if let Some(&last) = self.presented.back() {
            if now < last {
                return;
            }
            if now.duration_since(last) >= IDLE_AFTER {
                self.presented.clear();
            }
        }
        self.presented.push_back(now);

        // Keep one frame at or before the window boundary so that every counted
        // interval has both endpoints, even when no frame lands on the boundary.
        while self.presented.len() > 2 && now.duration_since(self.presented[1]) >= SAMPLE_WINDOW {
            self.presented.pop_front();
        }
    }

    /// Keep the latest cadence estimate until one second without a new frame.
    pub fn snapshot(&self, now: Instant) -> FrameRate {
        let Some(&last) = self.presented.back() else {
            return FrameRate::Waiting;
        };
        if now.saturating_duration_since(last) >= IDLE_AFTER {
            return FrameRate::Idle;
        }
        let elapsed = last.duration_since(self.presented[0]).as_secs_f64();
        if self.presented.len() < 2 || elapsed == 0. {
            return FrameRate::Sampling;
        }
        let intervals = (self.presented.len() - 1) as f64;
        FrameRate::Active {
            fps: intervals / elapsed,
            frame_ms: elapsed * 1000. / intervals,
        }
    }

    /// The next transition to idle; callers need not schedule an expired deadline.
    pub fn idle_deadline(&self) -> Option<Instant> {
        self.presented.back().map(|&last| last + IDLE_AFTER)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_rate(actual: FrameRate, expected_fps: f64) {
        let FrameRate::Active { fps, frame_ms } = actual else {
            panic!("Expected an active cadence, got {actual:?}");
        };
        assert!((fps - expected_fps).abs() < 0.00001, "FPS: {fps}");
        assert!(
            (frame_ms - 1000. / expected_fps).abs() < 0.00001,
            "Frame interval: {frame_ms} ms"
        );
    }

    #[test]
    fn waits_for_a_frame_and_then_for_an_interval() {
        let start = Instant::now();
        let mut stats = FrameStats::default();
        assert_eq!(stats.snapshot(start), FrameRate::Waiting);
        assert_eq!(stats.idle_deadline(), None);

        stats.presented(start);
        assert_eq!(stats.snapshot(start), FrameRate::Sampling);
        assert_eq!(
            stats.snapshot(start + Duration::from_millis(999)),
            FrameRate::Sampling
        );
        assert_eq!(stats.idle_deadline(), Some(start + IDLE_AFTER));
        assert_eq!(stats.snapshot(start + IDLE_AFTER), FrameRate::Idle);
    }

    #[test]
    fn measures_60_and_120_fps_without_counting_a_phantom_first_interval() {
        let start = Instant::now();
        for fps in [60., 120.] {
            let mut stats = FrameStats::default();
            for frame in 0..=(fps as u32 * 3) {
                let now = start + Duration::from_secs_f64(frame as f64 / fps);
                stats.presented(now);
                if frame > 0 {
                    assert_rate(stats.snapshot(now), fps);
                }
            }
            assert!(stats.presented.len() <= fps as usize + 2);
        }
    }

    #[test]
    fn retains_the_frame_before_the_sliding_window_boundary() {
        let start = Instant::now();
        let mut stats = FrameStats::default();
        for millis in [0, 400, 800, 1300] {
            stats.presented(start + Duration::from_millis(millis));
        }
        // The interval 0..400 ms crosses the 300 ms window boundary.
        assert_rate(
            stats.snapshot(start + Duration::from_millis(1300)),
            3. / 1.3,
        );

        stats.presented(start + Duration::from_millis(1800));
        // A frame exactly at the new boundary is the new first endpoint.
        assert_eq!(
            stats.presented.front(),
            Some(&(start + Duration::from_millis(800)))
        );
        assert_rate(stats.snapshot(start + Duration::from_millis(1800)), 2.);
    }

    #[test]
    fn drops_old_cadence_when_the_window_moves_on() {
        let start = Instant::now();
        let mut stats = FrameStats::default();
        for frame in 0..=60 {
            stats.presented(start + Duration::from_secs_f64(frame as f64 / 60.));
        }
        for frame in 1..=240 {
            stats.presented(start + Duration::from_secs_f64(1. + frame as f64 / 120.));
        }
        assert_rate(stats.snapshot(start + Duration::from_secs(3)), 120.);
    }

    #[test]
    fn preserves_the_last_estimate_until_idle_then_starts_a_new_sample() {
        let start = Instant::now();
        let mut stats = FrameStats::default();
        stats.presented(start);
        let last = start + Duration::from_millis(10);
        stats.presented(last);
        assert_rate(stats.snapshot(last), 100.);
        assert_rate(stats.snapshot(last + Duration::from_millis(999)), 100.);
        assert_eq!(stats.snapshot(last + IDLE_AFTER), FrameRate::Idle);

        let resumed = last + IDLE_AFTER;
        stats.presented(resumed);
        assert_eq!(stats.snapshot(resumed), FrameRate::Sampling);
        assert_eq!(stats.idle_deadline(), Some(resumed + IDLE_AFTER));
        stats.presented(resumed + Duration::from_millis(20));
        assert_rate(stats.snapshot(resumed + Duration::from_millis(20)), 50.);
    }

    #[test]
    fn isolated_frames_never_reuse_an_interval_from_before_idle() {
        let start = Instant::now();
        let mut stats = FrameStats::default();
        for seconds in [0, 2, 10, 60] {
            let now = start + Duration::from_secs(seconds);
            stats.presented(now);
            assert_eq!(stats.snapshot(now), FrameRate::Sampling);
            assert_eq!(stats.snapshot(now + IDLE_AFTER), FrameRate::Idle);
        }
    }

    #[test]
    fn identical_timestamps_do_not_produce_infinite_fps() {
        let start = Instant::now();
        let mut stats = FrameStats::default();
        stats.presented(start);
        stats.presented(start);
        assert_eq!(stats.snapshot(start), FrameRate::Sampling);
    }
}
