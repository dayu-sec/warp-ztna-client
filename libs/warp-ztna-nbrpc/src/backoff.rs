use std::time::Duration;

const JITTER_RATIO: f64 = 0.25;

#[derive(Debug, Clone)]
pub struct ReconnectBackoff {
    base: Duration,
    max: Duration,
    attempt: u32,
}

impl ReconnectBackoff {
    #[must_use]
    pub fn new(base: Duration, max: Duration) -> Self {
        Self { base, max, attempt: 0 }
    }

    pub fn reset(&mut self) {
        self.attempt = 0;
    }

    #[must_use]
    pub fn delay(&mut self) -> Duration {
        let stepped = self
            .base
            .saturating_mul(1u32.checked_shl(self.attempt).unwrap_or(u32::MAX))
            .min(self.max);
        self.attempt = self.attempt.saturating_add(1);
        jittered(stepped)
    }
}

fn jittered(delay: Duration) -> Duration {
    let unit = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |elapsed| f64::from(elapsed.subsec_nanos()) / 1_000_000_000.0);
    delay.mul_f64(1.0 - JITTER_RATIO + unit * 2.0 * JITTER_RATIO)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::ReconnectBackoff;

    const EPSILON: f64 = 1e-6;

    fn assert_within(seconds: f64, step: f64) {
        assert!(seconds >= step * 0.75 - EPSILON, "{seconds} below {step}");
        assert!(seconds <= step * 1.25 + EPSILON, "{seconds} above {step}");
    }

    #[test]
    fn delay_grows_exponentially_then_caps() {
        let mut backoff = ReconnectBackoff::new(Duration::from_secs(1), Duration::from_secs(30));

        for step in [1.0, 2.0, 4.0, 8.0, 16.0, 30.0, 30.0, 30.0] {
            assert_within(backoff.delay().as_secs_f64(), step);
        }
    }

    #[test]
    fn reset_returns_to_the_first_step() {
        let mut backoff = ReconnectBackoff::new(Duration::from_secs(1), Duration::from_secs(30));

        for _ in 0..5 {
            let _ = backoff.delay();
        }
        backoff.reset();

        assert_within(backoff.delay().as_secs_f64(), 1.0);
    }

    #[test]
    fn a_cap_below_the_base_wins() {
        let mut backoff = ReconnectBackoff::new(Duration::from_secs(10), Duration::from_secs(2));

        assert_within(backoff.delay().as_secs_f64(), 2.0);
    }
}
