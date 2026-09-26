//! Keeps requests within Discord's rate limits.
//!
//! EraseCord sends one request at a time, so a single "not before" instant is
//! enough: it is pushed back whenever Discord reports an exhausted bucket or
//! answers with 429.

use std::sync::Mutex;
use std::time::Duration;

use reqwest::header::HeaderMap;
use tokio::time::{sleep_until, Instant};

/// Added to every wait Discord asks for, so we do not arrive a hair too early.
const MARGIN: Duration = Duration::from_millis(50);

#[derive(Debug, Default)]
pub(crate) struct RateLimiter {
    not_before: Mutex<Option<Instant>>,
}

impl RateLimiter {
    /// Waits until the next request may be sent.
    pub(crate) async fn ready(&self) {
        let until = *self.not_before.lock().unwrap();
        if let Some(until) = until {
            sleep_until(until).await;
        }
    }

    /// Holds back further requests for `wait`.
    pub(crate) fn block_for(&self, wait: Duration) {
        let until = Instant::now() + wait;
        let mut not_before = self.not_before.lock().unwrap();
        if not_before.is_none_or(|current| current < until) {
            *not_before = Some(until);
        }
    }

    /// Reads the `X-RateLimit-*` headers of a response and, if the bucket is
    /// exhausted, holds back further requests until it resets.
    pub(crate) fn observe(&self, headers: &HeaderMap) {
        let remaining = header_f64(headers, "x-ratelimit-remaining");
        let reset_after = header_f64(headers, "x-ratelimit-reset-after");
        if let (Some(remaining), Some(reset_after)) = (remaining, reset_after) {
            if remaining < 1.0 {
                self.block_for(seconds(reset_after));
            }
        }
    }
}

pub(crate) fn header_f64(headers: &HeaderMap, name: &str) -> Option<f64> {
    headers.get(name)?.to_str().ok()?.trim().parse().ok()
}

/// Converts the fractional seconds Discord uses into a duration.
pub(crate) fn seconds(value: f64) -> Duration {
    let value = if value.is_finite() {
        value.clamp(0.0, 3600.0)
    } else {
        0.0
    };
    Duration::from_secs_f64(value) + MARGIN
}
