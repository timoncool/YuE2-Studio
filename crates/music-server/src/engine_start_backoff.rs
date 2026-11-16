//! Bound automatic engine startup retries on all backends.
//! Persistent missing dependencies or an unusable device must not cause an
//! endless restart loop. Manual restarts are handled independently.
use std::time::{Duration, Instant};

const RETRY_DELAYS_SECONDS: [u64; 4] = [5, 15, 30, 60];
pub(super) const MAX_AUTOMATIC_FAILURES: u8 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RetryOutcome {
    Success,
    Wait(Duration),
    Suspended,
}

pub(super) struct EngineStartBackoff<K> {
    settings_key: Option<K>,
    consecutive_failures: u8,
    retry_at: Option<Instant>,
    suspended: bool,
}

impl<K: Clone + PartialEq> Default for EngineStartBackoff<K> {
    fn default() -> Self {
        Self {
            settings_key: None,
            consecutive_failures: 0,
            retry_at: None,
            suspended: false,
        }
    }
}

impl<K: Clone + PartialEq> EngineStartBackoff<K> {
    /// A change to the selected runtime/model/settings means the underlying
    /// problem may be fixed. That is the only automatic reset while down.
    pub(super) fn observe(&mut self, key: &K) {
        if self.settings_key.as_ref() != Some(key) {
            self.settings_key = Some(key.clone());
            self.reset_failures();
        }
    }

    pub(super) fn allow(&self, now: Instant) -> bool {
        !self.suspended && self.retry_at.is_none_or(|next| now >= next)
    }

    pub(super) fn reset_failures(&mut self) {
        self.consecutive_failures = 0;
        self.retry_at = None;
        self.suspended = false;
    }

    pub(super) fn finish(&mut self, now: Instant, success: bool) -> RetryOutcome {
        if success {
            self.reset_failures();
            return RetryOutcome::Success;
        }
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        if self.consecutive_failures >= MAX_AUTOMATIC_FAILURES {
            self.suspended = true;
            self.retry_at = None;
            return RetryOutcome::Suspended;
        }
        let delay = Duration::from_secs(RETRY_DELAYS_SECONDS[usize::from(self.consecutive_failures - 1)]);
        self.retry_at = Some(now + delay);
        RetryOutcome::Wait(delay)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_failure_is_bounded_and_does_not_busy_loop() {
        let now = Instant::now();
        let mut backoff = EngineStartBackoff::<String>::default();
        backoff.observe(&"backend-a".into());
        assert!(backoff.allow(now));
        for (i, seconds) in RETRY_DELAYS_SECONDS.iter().enumerate() {
            let when = now + Duration::from_secs(1000 * i as u64);
            assert_eq!(backoff.finish(when, false), RetryOutcome::Wait(Duration::from_secs(*seconds)));
            assert!(!backoff.allow(when));
            assert!(!backoff.allow(when + Duration::from_secs(*seconds - 1)));
            assert!(backoff.allow(when + Duration::from_secs(*seconds)));
        }
        assert_eq!(backoff.finish(now + Duration::from_secs(4000), false), RetryOutcome::Suspended);
        assert!(!backoff.allow(now + Duration::from_secs(100_000)));
    }

    #[test]
    fn settings_change_resets_suspension_but_identical_settings_do_not() {
        let now = Instant::now();
        let mut backoff = EngineStartBackoff::<String>::default();
        backoff.observe(&"backend-a".into());
        for _ in 0..MAX_AUTOMATIC_FAILURES { backoff.finish(now, false); }
        backoff.observe(&"backend-a".into());
        assert!(!backoff.allow(now + Duration::from_secs(86400)));
        backoff.observe(&"backend-b".into());
        assert!(backoff.allow(now));
    }

    #[test]
    fn healthy_engine_resets_backoff_for_a_later_crash() {
        let now = Instant::now();
        let mut backoff = EngineStartBackoff::<String>::default();
        backoff.observe(&"backend-a".into());
        backoff.finish(now, false);
        assert_eq!(backoff.finish(now, true), RetryOutcome::Success);
        assert!(backoff.allow(now));
        assert_eq!(backoff.finish(now, false), RetryOutcome::Wait(Duration::from_secs(5)));
    }
}
