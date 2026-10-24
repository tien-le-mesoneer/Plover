//! `#[serde(with = "crate::serde_ms")]`: a `Duration` as whole milliseconds.

use std::time::Duration;

use serde::{Deserialize, Deserializer, Serializer};

pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_u64(u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
    u64::deserialize(d).map(Duration::from_millis)
}
