//! `chrono`-free timestamps using `std::time::SystemTime`.
//!
//! Provides RFC 3339 formatting and serde helpers compatible with the
//! `DateTime<Utc>` fields previously declared in `src/lib.rs`.

use std::time::{Duration, SystemTime};

pub type Timestamp = SystemTime;

/// Current wall-clock time as a `Timestamp`.
pub fn now() -> Timestamp {
    SystemTime::now()
}

/// Format a timestamp as an RFC 3339 UTC string (`YYYY-MM-DDTHH:MM:SS.sssZ`).
pub fn to_rfc3339(t: Timestamp) -> String {
    let duration = t.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    format_rfc3339(duration.as_secs(), duration.subsec_nanos())
}

/// Parse an RFC 3339 UTC timestamp produced by [`to_rfc3339`].
pub fn parse_rfc3339(s: &str) -> Result<Timestamp, String> {
    let s = s.strip_suffix('Z').ok_or("expected Z suffix")?;
    let (date, time) = s.split_once('T').ok_or("expected T separator")?;

    let mut dp = date.split('-');
    let year: i64 = dp
        .next()
        .ok_or("missing year")?
        .parse()
        .map_err(|e| format!("invalid year: {e}"))?;
    let month: u8 = dp
        .next()
        .ok_or("missing month")?
        .parse()
        .map_err(|e| format!("invalid month: {e}"))?;
    let day: u8 = dp
        .next()
        .ok_or("missing day")?
        .parse()
        .map_err(|e| format!("invalid day: {e}"))?;

    let (time, frac) = if let Some((t, f)) = time.split_once('.') {
        let frac = format!("{:0<9}", f);
        let frac: u32 = frac.parse().map_err(|e| format!("invalid fraction: {e}"))?;
        (t, frac)
    } else {
        (time, 0)
    };

    let mut tp = time.split(':');
    let hour: u64 = tp
        .next()
        .ok_or("missing hour")?
        .parse()
        .map_err(|e| format!("invalid hour: {e}"))?;
    let minute: u64 = tp
        .next()
        .ok_or("missing minute")?
        .parse()
        .map_err(|e| format!("invalid minute: {e}"))?;
    let second: u64 = tp
        .next()
        .ok_or("missing second")?
        .parse()
        .map_err(|e| format!("invalid second: {e}"))?;

    let days = ymd_to_days(year, month, day);
    let secs = days as u64 * 86_400 + hour * 3_600 + minute * 60 + second;
    Ok(SystemTime::UNIX_EPOCH + Duration::new(secs, frac))
}

/// Serde adapter that serializes a single `SystemTime` as an RFC 3339 string.
pub mod serde {
    use super::Timestamp;

    pub fn serialize<S>(t: &Timestamp, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: ::serde::ser::Serializer,
    {
        ::serde::ser::Serialize::serialize(&super::to_rfc3339(*t), serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Timestamp, D::Error>
    where
        D: ::serde::de::Deserializer<'de>,
    {
        let s = <String as ::serde::de::Deserialize>::deserialize(deserializer)?;
        super::parse_rfc3339(&s).map_err(::serde::de::Error::custom)
    }
}

/// Serde adapter for `Vec<(Timestamp, f64)>` used by `ProjectHealth.entropy_trend`.
pub mod serde_trend {
    use super::Timestamp;

    pub fn serialize<S>(v: &[(Timestamp, f64)], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: ::serde::ser::Serializer,
    {
        let flat: Vec<(String, f64)> = v
            .iter()
            .map(|(t, val)| (super::to_rfc3339(*t), *val))
            .collect();
        ::serde::ser::Serialize::serialize(&flat, serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<(Timestamp, f64)>, D::Error>
    where
        D: ::serde::de::Deserializer<'de>,
    {
        let flat: Vec<(String, f64)> =
            ::serde::de::Deserialize::deserialize(deserializer)?;
        flat.into_iter()
            .map(|(s, val)| {
                Ok((super::parse_rfc3339(&s).map_err(::serde::de::Error::custom)?, val))
            })
            .collect()
    }
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

fn days_in_month(year: i64, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => panic!("invalid month {month}"),
    }
}

pub(crate) fn ymd_to_days(year: i64, month: u8, day: u8) -> i64 {
    let mut days = 0i64;
    for y in 1970..year {
        days += if is_leap_year(y) { 366 } else { 365 };
    }
    for m in 1..month {
        days += days_in_month(year, m) as i64;
    }
    days += (day as i64) - 1;
    days
}

fn format_rfc3339(secs: u64, nanos: u32) -> String {
    let mut days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let hour = (rem / 3_600) as u8;
    let minute = ((rem % 3_600) / 60) as u8;
    let second = (rem % 60) as u8;

    let mut year = 1970i64;
    loop {
        let year_days = if is_leap_year(year) { 366 } else { 365 };
        if days < year_days {
            break;
        }
        days -= year_days;
        year += 1;
    }

    let mut month = 1u8;
    loop {
        let dim = days_in_month(year, month) as i64;
        if days < dim {
            break;
        }
        days -= dim;
        month += 1;
    }
    let day = (days + 1) as u8;

    if nanos == 0 {
        format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", year, month, day, hour, minute, second)
    } else {
        let frac = format!("{:09}", nanos);
        let frac = frac.trim_end_matches('0');
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{}Z",
            year, month, day, hour, minute, second, frac
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{now, parse_rfc3339, to_rfc3339, Timestamp, ymd_to_days};
    use ::serde::{Deserialize, Serialize};
    use std::time::{Duration, SystemTime};

    #[test]
    fn now_is_after_epoch() {
        assert!(now().duration_since(SystemTime::UNIX_EPOCH).unwrap() > Duration::from_secs(1_700_000_000));
    }

    #[test]
    fn epoch_formats_to_unix_epoch() {
        let s = to_rfc3339(SystemTime::UNIX_EPOCH);
        assert_eq!(s, "1970-01-01T00:00:00Z");
    }

    #[test]
    fn known_timestamp_formats_correctly() {
        let t = SystemTime::UNIX_EPOCH + Duration::new(946_684_800, 123_456_789);
        let s = to_rfc3339(t);
        assert_eq!(s, "2000-01-01T00:00:00.123456789Z");
    }

    #[test]
    fn roundtrip_via_parse_rfc3339() {
        let original = SystemTime::UNIX_EPOCH + Duration::new(1_700_000_000, 999_999_999);
        let s = to_rfc3339(original);
        let parsed = parse_rfc3339(&s).unwrap();
        let diff = parsed.duration_since(original).unwrap_or_else(|_| original.duration_since(parsed).unwrap());
        assert!(diff < Duration::from_nanos(1));
    }

    #[test]
    fn parse_without_fraction() {
        let t = parse_rfc3339("2024-01-15T08:30:00Z").unwrap();
        let s = to_rfc3339(t);
        assert_eq!(s, "2024-01-15T08:30:00Z");
    }

    #[test]
    fn parse_with_milliseconds() {
        let t = parse_rfc3339("2024-01-15T08:30:00.123Z").unwrap();
        let expected = SystemTime::UNIX_EPOCH
            + Duration::from_secs(ymd_to_days(2024, 1, 15) as u64 * 86_400 + 8 * 3_600 + 30 * 60)
            + Duration::from_millis(123);
        assert_eq!(t, expected);
    }

    #[test]
    fn parse_rejects_missing_z() {
        assert!(parse_rfc3339("2024-01-15T08:30:00").is_err());
    }

    #[test]
    fn serde_roundtrip() {
        #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
        struct Event {
            #[serde(with = "crate::time::serde")]
            at: Timestamp,
            msg: String,
        }

        let original = Event {
            at: now(),
            msg: "test".to_string(),
        };
        let encoded = toml::to_string(&original).unwrap();
        let parsed: Event = toml::from_str(&encoded).unwrap();
        assert_eq!(original.at, parsed.at);
        assert!(encoded.contains("T") && encoded.contains("Z"));
    }

    #[test]
    fn serde_trend_roundtrip() {
        #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
        struct Health {
            #[serde(with = "crate::time::serde_trend")]
            entropy_trend: Vec<(Timestamp, f64)>,
        }

        let original = Health {
            entropy_trend: vec![
                (SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000), 0.5),
                (SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_001), 0.6),
            ],
        };
        let encoded = toml::to_string(&original).unwrap();
        let parsed: Health = toml::from_str(&encoded).unwrap();
        assert_eq!(original.entropy_trend, parsed.entropy_trend);
        assert!(encoded.contains("2023-11-14T22:13:20Z"), "unexpected encoded: {}", encoded);
    }
}
