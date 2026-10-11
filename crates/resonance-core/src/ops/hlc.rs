//! Hybrid logical clock: wall-clock milliseconds plus a counter (MVP-010, §9.1).

use std::time::{SystemTime, UNIX_EPOCH};

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Nanoseconds since the Unix epoch.
pub fn now_ns() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0)
}

/// An HLC timestamp. Its string form, `<ms:013>.<counter:05>`, sorts in clock order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Hlc {
    pub ms: u64,
    pub counter: u32,
}

impl Hlc {
    pub fn parse(s: &str) -> Option<Hlc> {
        let (ms, counter) = s.split_once('.')?;
        Some(Hlc {
            ms: ms.parse().ok()?,
            counter: counter.parse().ok()?,
        })
    }

    /// The next timestamp after `prev`, given the wall clock `now_ms`.
    pub fn next(prev: Option<Hlc>, now_ms: u64) -> Hlc {
        match prev {
            Some(p) if p.ms >= now_ms => Hlc {
                ms: p.ms,
                counter: p.counter + 1,
            },
            _ => Hlc {
                ms: now_ms,
                counter: 0,
            },
        }
    }
}

impl std::fmt::Display for Hlc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:013}.{:05}", self.ms, self.counter)
    }
}

/// Render milliseconds since the epoch as an RFC 3339 UTC timestamp, without a date library.
pub fn rfc3339(ms: u64) -> String {
    let secs = ms / 1000;
    let millis = ms % 1000;
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}.{millis:03}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_is_monotonic_even_if_the_wall_clock_goes_back() {
        let a = Hlc::next(None, 1000);
        let b = Hlc::next(Some(a), 1000);
        let c = Hlc::next(Some(b), 900);
        let d = Hlc::next(Some(c), 2000);
        assert!(a < b && b < c && c < d);
        assert!(a.to_string() < b.to_string() && c.to_string() < d.to_string());
        assert_eq!(Hlc::parse(&c.to_string()), Some(c));
    }

    #[test]
    fn formats_rfc3339() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(rfc3339(1_760_197_607_123), "2025-10-11T15:46:47.123Z");
    }
}
