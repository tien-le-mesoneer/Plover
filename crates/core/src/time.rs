//! Wall-clock formatting without a date library.

use std::time::{SystemTime, UNIX_EPOCH};

/// Milliseconds since the Unix epoch (0 before it).
pub fn unix_ms(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default()
}

/// `new Date().toISOString().split('Z')[0]`: UTC, milliseconds, no `Z` (Bruno's JUnit
/// timestamps).
pub fn iso_utc(t: SystemTime) -> String {
    let c = civil(t);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}",
        c.year, c.month, c.day, c.hour, c.minute, c.second, c.millis
    )
}

/// `yyyymmddThhmmssZ`, UTC.
pub fn compact_utc(t: SystemTime) -> String {
    let c = civil(t);
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        c.year, c.month, c.day, c.hour, c.minute, c.second
    )
}

struct Civil {
    year: i64,
    month: i64,
    day: i64,
    hour: u64,
    minute: u64,
    second: u64,
    millis: u32,
}

fn civil(t: SystemTime) -> Civil {
    let since = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = since.as_secs();
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // Howard Hinnant's civil_from_days.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    Civil {
        year,
        month,
        day,
        hour: rem / 3600,
        minute: rem % 3600 / 60,
        second: rem % 60,
        millis: since.subsec_millis(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn formats() {
        let t = UNIX_EPOCH + Duration::from_millis(1_761_134_400_123);
        assert_eq!(iso_utc(t), "2025-10-22T12:00:00.123");
        assert_eq!(compact_utc(t), "20251022T120000Z");
        assert_eq!(unix_ms(t), 1_761_134_400_123);
    }
}
