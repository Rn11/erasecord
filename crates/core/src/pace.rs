//! How fast a clean-up goes. Discord allows about five deletions in five
//! seconds; tools that go near that limit get rate limited all the time,
//! which is what gets accounts noticed. EraseCord stays well below it:
//!
//! - every pause varies randomly by ±25 %, so requests do not arrive like
//!   clockwork;
//! - after every [`BREAK_EVERY`] deletions it takes a longer break;
//! - whenever Discord rate limits it, the pauses get longer for the rest of
//!   the run (up to [`MAX_SLOWDOWN`] times the configured ones).
//!
//! All of it scales with the configured pauses, so pauses of 0 (as in the
//! tests) mean no waiting at all.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use rand::RngExt;

/// A longer break after this many deletions.
pub const BREAK_EVERY: u64 = 100;
/// The break lasts this many regular pauses between deletions.
const BREAK_PAUSES: u64 = 12;
/// Each rate limit makes the pauses this much longer (in percent).
const SLOWDOWN_STEP: u64 = 25;
/// The pauses never get longer than this many times the configured ones.
const MAX_SLOWDOWN: u64 = 4;

#[derive(Debug)]
pub struct Pace {
    delete_ms: u64,
    search_ms: u64,
    /// The current pauses, in percent of the configured ones.
    factor: AtomicU64,
    deletions: AtomicU64,
}

impl Pace {
    pub fn new(delete_ms: u64, search_ms: u64) -> Self {
        Pace {
            delete_ms,
            search_ms,
            factor: AtomicU64::new(100),
            deletions: AtomicU64::new(0),
        }
    }

    /// The pause after a deletion, and whether it is one of the longer
    /// breaks.
    pub fn after_delete(&self) -> (Duration, bool) {
        let count = self.deletions.fetch_add(1, Ordering::Relaxed) + 1;
        let pause = self.scaled(self.delete_ms);
        if count.is_multiple_of(BREAK_EVERY) && self.delete_ms > 0 {
            (jitter(pause * BREAK_PAUSES), true)
        } else {
            (jitter(pause), false)
        }
    }

    /// The pause before a search request.
    pub fn before_search(&self) -> Duration {
        jitter(self.scaled(self.search_ms))
    }

    /// Discord said to slow down: longer pauses from now on.
    pub fn slow_down(&self) {
        let _ = self
            .factor
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |factor| {
                Some((factor + SLOWDOWN_STEP).min(MAX_SLOWDOWN * 100))
            });
    }

    /// The current pause between deletions, in milliseconds (without the
    /// random part).
    pub fn delete_ms(&self) -> u64 {
        self.scaled(self.delete_ms)
    }

    fn scaled(&self, ms: u64) -> u64 {
        ms * self.factor.load(Ordering::Relaxed) / 100
    }
}

/// `ms` ± 25 %.
fn jitter(ms: u64) -> Duration {
    if ms == 0 {
        return Duration::ZERO;
    }
    let spread = ms / 4;
    let ms = rand::rng().random_range(ms - spread..=ms + spread);
    Duration::from_millis(ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pauses_vary_around_the_configured_value() {
        let pace = Pace::new(2000, 3000);
        let pauses: Vec<u128> = (0..50).map(|_| pace.before_search().as_millis()).collect();
        assert!(pauses.iter().all(|&ms| (2250..=3750).contains(&ms)));
        assert!(pauses.iter().any(|&ms| ms != pauses[0]));
    }

    #[test]
    fn takes_a_break_now_and_then() {
        let pace = Pace::new(1000, 0);
        let breaks: Vec<u64> = (1..=2 * BREAK_EVERY)
            .filter(|_| pace.after_delete().1)
            .collect();
        assert_eq!(breaks.len(), 2);
        let pace = Pace::new(0, 0);
        assert!((0..BREAK_EVERY).all(|_| pace.after_delete() == (Duration::ZERO, false)));
    }

    #[test]
    fn rate_limits_slow_it_down_up_to_a_limit() {
        let pace = Pace::new(1000, 1000);
        pace.slow_down();
        assert_eq!(pace.delete_ms(), 1250);
        for _ in 0..50 {
            pace.slow_down();
        }
        assert_eq!(pace.delete_ms(), 4000);
    }
}
