use std::time::Duration;

/// Exponential reconnect backoff with optional jitter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    pub initial: Duration,
    pub max: Duration,
    pub jitter: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            initial: Duration::from_millis(250),
            max: Duration::from_secs(30),
            jitter: true,
        }
    }
}

impl RetryPolicy {
    pub fn delay(&self, attempt: u32, seed: u64) -> Duration {
        let shift = attempt.min(31);
        let capped = self.initial.saturating_mul(1u32 << shift).min(self.max);
        if !self.jitter {
            return capped;
        }

        let span = (capped.as_millis() / 4).max(1) as u64;
        capped + Duration::from_millis(seed % span)
    }
}

#[derive(Debug, Clone)]
pub struct Backoff {
    policy: RetryPolicy,
    attempt: u32,
    seed: u64,
}

impl Backoff {
    pub fn new(policy: RetryPolicy, seed: u64) -> Self {
        Self {
            policy,
            attempt: 0,
            seed,
        }
    }

    pub fn next_delay(&mut self) -> Duration {
        let delay = self.policy.delay(self.attempt, self.seed);
        self.attempt = self.attempt.saturating_add(1);
        self.seed = self
            .seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        delay
    }

    pub fn reset(&mut self) {
        self.attempt = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_doubles_up_to_the_cap() {
        let policy = RetryPolicy {
            initial: Duration::from_millis(250),
            max: Duration::from_secs(30),
            jitter: false,
        };

        assert_eq!(policy.delay(0, 0), Duration::from_millis(250));
        assert_eq!(policy.delay(1, 0), Duration::from_millis(500));
        assert_eq!(policy.delay(3, 0), Duration::from_secs(2));
        assert_eq!(policy.delay(20, 0), Duration::from_secs(30));
    }

    #[test]
    fn jitter_stays_within_a_quarter_of_the_base() {
        let policy = RetryPolicy {
            initial: Duration::from_millis(1000),
            max: Duration::from_secs(30),
            jitter: true,
        };

        for seed in 0..100 {
            let delay = policy.delay(0, seed);
            assert!(delay >= Duration::from_millis(1000));
            assert!(delay < Duration::from_millis(1250));
        }
    }

    #[test]
    fn reset_starts_the_sequence_again() {
        let policy = RetryPolicy {
            initial: Duration::from_millis(250),
            max: Duration::from_secs(30),
            jitter: false,
        };
        let mut backoff = Backoff::new(policy, 0);

        assert_eq!(backoff.next_delay(), Duration::from_millis(250));
        assert_eq!(backoff.next_delay(), Duration::from_millis(500));
        backoff.reset();
        assert_eq!(backoff.next_delay(), Duration::from_millis(250));
    }
}
