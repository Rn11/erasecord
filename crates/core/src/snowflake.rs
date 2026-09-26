//! Discord IDs ("snowflakes") and their conversion to and from timestamps.

use std::fmt;
use std::num::ParseIntError;
use std::str::FromStr;

use chrono::{DateTime, TimeZone, Utc};
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Milliseconds between the Unix epoch and the Discord epoch (2015-01-01T00:00:00Z).
pub const DISCORD_EPOCH_MS: i64 = 1_420_070_400_000;

/// The timestamp part of a snowflake is 42 bits wide.
const MAX_TIMESTAMP: u64 = (1 << 42) - 1;

/// A Discord ID. Its upper 42 bits are the creation time in milliseconds since
/// [`DISCORD_EPOCH_MS`], which lets a date range be expressed as an ID range.
///
/// Serialized as a string, like Discord does, because JavaScript numbers
/// cannot hold 64-bit integers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Snowflake(pub u64);

impl Snowflake {
    /// The smallest ID that can be created at `time`.
    pub fn from_datetime(time: DateTime<Utc>) -> Self {
        let ms = time
            .timestamp_millis()
            .saturating_sub(DISCORD_EPOCH_MS)
            .max(0) as u64;
        Snowflake(ms.min(MAX_TIMESTAMP) << 22)
    }

    /// Unix timestamp in milliseconds at which this ID was created.
    pub fn timestamp_ms(self) -> i64 {
        (self.0 >> 22) as i64 + DISCORD_EPOCH_MS
    }

    /// The time at which this ID was created.
    pub fn created_at(self) -> DateTime<Utc> {
        Utc.timestamp_millis_opt(self.timestamp_ms())
            .single()
            .unwrap_or_default()
    }
}

impl fmt::Display for Snowflake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for Snowflake {
    type Err = ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.trim().parse().map(Snowflake)
    }
}

impl Serialize for Snowflake {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Snowflake {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct SnowflakeVisitor;

        impl Visitor<'_> for SnowflakeVisitor {
            type Value = Snowflake;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a Discord ID as string or integer")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Snowflake, E> {
                v.parse().map_err(E::custom)
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Snowflake, E> {
                Ok(Snowflake(v))
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Snowflake, E> {
                u64::try_from(v).map(Snowflake).map_err(E::custom)
            }
        }

        deserializer.deserialize_any(SnowflakeVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_id_maps_to_its_creation_time() {
        // Example from the Discord developer documentation.
        let id: Snowflake = "175928847299117063".parse().unwrap();
        assert_eq!(id.timestamp_ms(), 1_462_015_105_796);
        assert_eq!(
            id.created_at().to_rfc3339(),
            "2016-04-30T11:18:25.796+00:00"
        );
    }

    #[test]
    fn from_datetime_is_smallest_id_of_that_millisecond() {
        let id: Snowflake = "175928847299117063".parse().unwrap();
        let lower = Snowflake::from_datetime(id.created_at());
        assert!(lower <= id);
        assert_eq!(lower.timestamp_ms(), id.timestamp_ms());
        assert_eq!(lower.0 & ((1 << 22) - 1), 0);
    }

    #[test]
    fn times_outside_the_id_range_are_clamped() {
        let before_epoch = Utc.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap();
        assert_eq!(Snowflake::from_datetime(before_epoch), Snowflake(0));
        let far_future = Utc.with_ymd_and_hms(9999, 1, 1, 0, 0, 0).unwrap();
        assert_eq!(Snowflake::from_datetime(far_future).0 >> 22, MAX_TIMESTAMP);
    }

    #[test]
    fn serializes_as_string_and_reads_both_forms() {
        let id = Snowflake(42);
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"42\"");
        assert_eq!(serde_json::from_str::<Snowflake>("\"42\"").unwrap(), id);
        assert_eq!(serde_json::from_str::<Snowflake>("42").unwrap(), id);
    }
}
