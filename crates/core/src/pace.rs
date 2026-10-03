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
//! - attachments are downloaded one at a time, with a pause of
//!   [`DOWNLOAD_SHARE`] % of the one between deletions before each;
//!
//! All of it scales with the configured pauses, so pauses of 0 (as in the
//! tests) mean no waiting at all.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use rand::RngExt;

/// The longest pause that can be configured: ten minutes.
pub const MAX_PAUSE_MS: u64 = 10 * 60 * 1000;
/// A longer break after this many deletions.
pub const BREAK_EVERY: u64 = 100;
/// The break lasts this many regular pauses between deletions.
const BREAK_PAUSES: u64 = 12;
/// Each rate limit makes the pauses this much longer (in percent).
const SLOWDOWN_STEP: u64 = 25;
/// The pauses never get longer than this many times the configured ones.
const MAX_SLOWDOWN: u64 = 4;
/// The pause before downloading an attachment, in percent of the one
/// between deletions (1 s with the default 2.5 s). Discord's file servers
/// are not the API and have no published limit, but a backup should not
/// fetch hundreds of files as fast as it can either.
pub const DOWNLOAD_SHARE: u64 = 40;

#[derive(Debug)]
pub struct Pace {
    delete_ms: u64,
    search_ms: u64,
    /// The current pauses, in percent of the configured ones.
    factor: AtomicU64,
    deletions: AtomicU64,
}

impl Pace {
    /// Pauses above [`MAX_PAUSE_MS`] are cut to it.
    pub fn new(delete_ms: u64, search_ms: u64) -> Self {
        Pace {
            delete_ms: delete_ms.min(MAX_PAUSE_MS),
            search_ms: search_ms.min(MAX_PAUSE_MS),
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

    /// The pause before downloading an attachment.
    pub fn before_download(&self) -> Duration {
        jitter(self.scaled(self.delete_ms) * DOWNLOAD_SHARE / 100)
    }

    /// Discord said to slow down: longer pauses from now on.
    pub fn slow_down(&self) {
        // A compare-and-swap loop rather than `fetch_update`, which newer
        // Rust renamed: this builds on old and new compilers alike.
        let mut current = self.factor.load(Ordering::Relaxed);
        loop {
            let next = (current + SLOWDOWN_STEP).min(MAX_SLOWDOWN * 100);
            match self.factor.compare_exchange_weak(
                current,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
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
    fn downloads_wait_less_than_deletions() {
        let pace = Pace::new(2500, 3000);
        let pauses: Vec<u128> = (0..50)
            .map(|_| pace.before_download().as_millis())
            .collect();
        assert!(pauses.iter().all(|&ms| (750..=1250).contains(&ms)));
        pace.slow_down();
        assert!(pace.before_download() >= Duration::from_millis(937));
        assert_eq!(Pace::new(0, 0).before_download(), Duration::ZERO);
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
    fn absurd_pauses_are_cut_instead_of_overflowing() {
        let pace = Pace::new(u64::MAX, u64::MAX);
        for _ in 0..50 {
            pace.slow_down();
        }
        for _ in 0..BREAK_EVERY {
            assert!(
                pace.after_delete().0 <= Duration::from_millis(MAX_PAUSE_MS * 4 * BREAK_PAUSES * 2)
            );
        }
        assert!(pace.before_search() <= Duration::from_millis(MAX_PAUSE_MS * 5));
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
