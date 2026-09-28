//! Frame timing and deterministic fixed-step clocks.
//!
//! All accumulation is done with integer [`Duration`]s, so step counts never drift
//! the way floating-point seconds do (e.g. 100 ms at 60 Hz is exactly 6 steps).

use std::time::Duration;

/// Per-frame timing information.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Time {
    delta: Duration,
    elapsed: Duration,
    frame: u64,
}

impl Time {
    /// Advance by `delta`, incrementing the frame counter.
    pub fn advance(&mut self, delta: Duration) {
        self.delta = delta;
        self.elapsed += delta;
        self.frame += 1;
    }

    /// Duration of the most recent frame.
    pub fn delta(&self) -> Duration {
        self.delta
    }

    /// [`Time::delta`] in seconds.
    pub fn delta_secs(&self) -> f32 {
        self.delta.as_secs_f32()
    }

    /// Total time since startup.
    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }

    /// [`Time::elapsed`] in seconds (f64 so long sessions keep precision).
    pub fn elapsed_secs(&self) -> f64 {
        self.elapsed.as_secs_f64()
    }

    /// Number of frames advanced so far.
    pub fn frame(&self) -> u64 {
        self.frame
    }
}

/// Fixed-timestep accumulator.
///
/// Feed it real frame time with [`FixedClock::accumulate`]; it returns how many fixed steps
/// to run. After a long hitch (debugger, window drag) the backlog is capped at
/// `max_steps_per_frame` and the rest is dropped, so the simulation cannot spiral.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedClock {
    step: Duration,
    accumulator: Duration,
    max_steps_per_frame: u32,
}

impl Default for FixedClock {
    fn default() -> Self {
        Self::from_hz(60)
    }
}

impl FixedClock {
    /// Default cap on steps per frame.
    pub const DEFAULT_MAX_STEPS: u32 = 8;

    /// Clock with the given step length.
    ///
    /// # Panics
    /// Panics if `step` is zero.
    pub fn new(step: Duration) -> Self {
        assert!(!step.is_zero(), "fixed step must be non-zero");
        Self {
            step,
            accumulator: Duration::ZERO,
            max_steps_per_frame: Self::DEFAULT_MAX_STEPS,
        }
    }

    /// Clock ticking `hz` times per second (step = 1s / hz, truncated to whole nanoseconds).
    ///
    /// # Panics
    /// Panics if `hz` is zero.
    pub fn from_hz(hz: u32) -> Self {
        assert!(hz > 0, "fixed rate must be non-zero");
        Self::new(Duration::from_nanos(1_000_000_000 / u64::from(hz)))
    }

    /// Override the maximum number of steps run in one frame (minimum 1).
    pub fn with_max_steps(mut self, max: u32) -> Self {
        self.max_steps_per_frame = max.max(1);
        self
    }

    /// Length of one fixed step.
    pub fn step(&self) -> Duration {
        self.step
    }

    /// Time accumulated but not yet consumed by a step.
    pub fn remainder(&self) -> Duration {
        self.accumulator
    }

    /// Interpolation factor in `[0, 1)` between the last and next fixed step, for rendering.
    pub fn overstep_fraction(&self) -> f32 {
        self.accumulator.as_secs_f32() / self.step.as_secs_f32()
    }

    /// Add `delta` and return the number of fixed steps that should run now.
    pub fn accumulate(&mut self, delta: Duration) -> u32 {
        self.accumulator += delta;
        let step_ns = self.step.as_nanos();
        let available = self.accumulator.as_nanos() / step_ns;
        let steps = available.min(u128::from(self.max_steps_per_frame)) as u32;
        if u128::from(steps) < available {
            // Hitch: drop the backlog but keep the sub-step phase.
            let rem = self.accumulator.as_nanos() % step_ns;
            self.accumulator = Duration::from_nanos(rem as u64);
        } else {
            self.accumulator -= self.step * steps;
        }
        steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: fn(u64) -> Duration = Duration::from_millis;

    /// TC-CORE-03: 60 Hz + 50 ms => 3 steps, ~0 remainder.
    #[test]
    fn tc_core_03_fixed_steps_are_exact() {
        let mut clock = FixedClock::from_hz(60);
        assert_eq!(clock.accumulate(MS(50)), 3);
        assert!(
            clock.remainder() < Duration::from_micros(1),
            "rem = {:?}",
            clock.remainder()
        );
    }

    /// TC-CORE-04: a 5 s hitch is clamped to the max step count and the backlog discarded.
    #[test]
    fn tc_core_04_hitch_is_clamped() {
        let mut clock = FixedClock::from_hz(60);
        assert_eq!(
            clock.accumulate(Duration::from_secs(5)),
            FixedClock::DEFAULT_MAX_STEPS
        );
        assert!(clock.remainder() < clock.step());
        assert_eq!(
            clock.accumulate(Duration::ZERO),
            0,
            "backlog must not carry over"
        );
    }

    #[test]
    fn small_deltas_accumulate() {
        let mut clock = FixedClock::from_hz(60);
        let total: u32 = (0..60).map(|_| clock.accumulate(MS(1))).sum();
        assert_eq!(total, 3); // 60 ms => 3 full 16.67 ms steps
    }

    #[test]
    fn time_advances() {
        let mut t = Time::default();
        t.advance(MS(16));
        t.advance(MS(17));
        assert_eq!(t.frame(), 2);
        assert_eq!(t.delta(), MS(17));
        assert_eq!(t.elapsed(), MS(33));
    }

    #[test]
    #[should_panic(expected = "non-zero")]
    fn zero_hz_panics() {
        let _ = FixedClock::from_hz(0);
    }
}
