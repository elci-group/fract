//! `chrono`-free timestamps using `std::time::SystemTime`.
//!
//! Provides RFC 3339 formatting and serde helpers compatible with the
//! `DateTime<Utc>` fields previously declared in `src/lib.rs`.

use std::time::{Duration, SystemTime};

pub type Timestamp = SystemTime;

/// Current wall-clock time as a `Timestamp`.
#[must_use]
pub fn now() -> Timestamp {
    SystemTime::now()
}

/// Format a timestamp as an RFC 3339 UTC string (`YYYY-MM-DDTHH:MM:SS.sssZ`).
#[must_use]
pub fn to_rfc3339(t: Timestamp) -> String {
    let duration = t.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    format_rfc3339(duration.as_secs(), duration.subsec_nanos())
}

/// Parse an RFC 3339 UTC timestamp produced by [`to_rfc3339`].
///
/// # Errors
/// Returns an error if the string lacks the `Z` suffix or `T` separator, any
/// component is non-numeric, or any component is out of its valid range
/// (including day-of-month for the given month/year).
///
/// # Panics
/// Panics only on an internal invariant violation (the day count for a
/// validated year >= 1970 is always non-negative).
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

    if !(1970..=9999).contains(&year) {
        return Err(format!("year {year} out of supported range 1970..=9999"));
    }
    if !(1..=12).contains(&month) {
        return Err(format!("month {month} out of range 1..=12"));
    }
    let dim = days_in_month(year, month);
    if day == 0 || day > dim {
        return Err(format!(
            "day {day} out of range 1..={dim} for month {month}"
        ));
    }

    let (time, frac) = if let Some((t, f)) = time.split_once('.') {
        let frac = format!("{f:0<9}");
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

    if hour > 23 {
        return Err(format!("hour {hour} out of range 0..=23"));
    }
    if minute > 59 {
        return Err(format!("minute {minute} out of range 0..=59"));
    }
    if second > 59 {
        return Err(format!("second {second} out of range 0..=59"));
    }

    let days = ymd_to_days(year, month, day);
    // `year` is validated to 1970..=9999 above, so `days` is non-negative.
    let secs = u64::try_from(days).expect("day count is non-negative for year >= 1970") * 86_400
        + hour * 3_600
        + minute * 60
        + second;
    Ok(SystemTime::UNIX_EPOCH + Duration::new(secs, frac))
}

/// Serde adapter that serializes a single `SystemTime` as an RFC 3339 string.
pub mod serde {
    use super::Timestamp;

    /// Serialize as an RFC 3339 string.
    ///
    /// # Errors
    /// Returns an error if the underlying serializer fails.
    pub fn serialize<S>(t: &Timestamp, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: ::serde::ser::Serializer,
    {
        ::serde::ser::Serialize::serialize(&super::to_rfc3339(*t), serializer)
    }

    /// Deserialize from an RFC 3339 string.
    ///
    /// # Errors
    /// Returns an error if the input is not a string or is not a valid
    /// RFC 3339 timestamp as accepted by [`super::parse_rfc3339`].
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

    /// Serialize the trend as RFC 3339 string/value pairs.
    ///
    /// # Errors
    /// Returns an error if the underlying serializer fails.
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

    /// Deserialize the trend from RFC 3339 string/value pairs.
    ///
    /// # Errors
    /// Returns an error if the input is not a list of string/number pairs or
    /// any timestamp fails to parse.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<(Timestamp, f64)>, D::Error>
    where
        D: ::serde::de::Deserializer<'de>,
    {
        let flat: Vec<(String, f64)> = ::serde::de::Deserialize::deserialize(deserializer)?;
        flat.into_iter()
            .map(|(s, val)| {
                Ok((
                    super::parse_rfc3339(&s).map_err(::serde::de::Error::custom)?,
                    val,
                ))
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
        days += i64::from(days_in_month(year, m));
    }
    days += i64::from(day) - 1;
    days
}

fn format_rfc3339(secs: u64, nanos: u32) -> String {
    // Timestamps this formatter receives are at most thousands of years past
    // the epoch, so every conversion below is far inside the target range.
    let mut days = i64::try_from(secs / 86_400).expect("day count fits in i64");
    let rem = secs % 86_400;
    let hour = u8::try_from(rem / 3_600).expect("hour is 0..=23");
    let minute = u8::try_from((rem % 3_600) / 60).expect("minute is 0..=59");
    let second = u8::try_from(rem % 60).expect("second is 0..=59");

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
        let dim = i64::from(days_in_month(year, month));
        if days < dim {
            break;
        }
        days -= dim;
        month += 1;
    }
    let day = u8::try_from(days + 1).expect("day of month is 1..=31");

    if nanos == 0 {
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
    } else {
        let frac = format!("{nanos:09}");
        let frac = frac.trim_end_matches('0');
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{frac}Z")
    }
}

#[cfg(test)]
mod tests {
    use super::{now, parse_rfc3339, to_rfc3339, ymd_to_days, Timestamp};
    use ::serde::{Deserialize, Serialize};
    use std::time::{Duration, SystemTime};

    #[test]
    fn now_is_after_epoch() {
        assert!(
            now().duration_since(SystemTime::UNIX_EPOCH).unwrap()
                > Duration::from_secs(1_700_000_000)
        );
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
        let diff = parsed
            .duration_since(original)
            .unwrap_or_else(|_| original.duration_since(parsed).unwrap());
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
            + Duration::from_secs(
                u64::try_from(ymd_to_days(2024, 1, 15)).unwrap() * 86_400 + 8 * 3_600 + 30 * 60,
            )
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
        assert!(encoded.contains('T') && encoded.contains('Z'));
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
                (
                    SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
                    0.5,
                ),
                (
                    SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_001),
                    0.6,
                ),
            ],
        };
        let encoded = toml::to_string(&original).unwrap();
        let parsed: Health = toml::from_str(&encoded).unwrap();
        assert_eq!(original.entropy_trend, parsed.entropy_trend);
        assert!(
            encoded.contains("2023-11-14T22:13:20Z"),
            "unexpected encoded: {encoded}"
        );
    }

    #[test]
    fn parse_rejects_out_of_range_month() {
        assert!(parse_rfc3339("2024-00-15T08:30:00Z").is_err());
        assert!(parse_rfc3339("2024-13-15T08:30:00Z").is_err());
    }

    #[test]
    fn parse_rejects_out_of_range_day() {
        assert!(parse_rfc3339("2024-01-00T08:30:00Z").is_err());
        assert!(parse_rfc3339("2024-01-32T08:30:00Z").is_err());
        assert!(parse_rfc3339("2023-02-29T08:30:00Z").is_err()); // non-leap
        assert!(parse_rfc3339("2024-04-31T08:30:00Z").is_err());
    }

    #[test]
    fn parse_accepts_leap_day() {
        assert!(parse_rfc3339("2024-02-29T08:30:00Z").is_ok());
        assert!(parse_rfc3339("2000-02-29T00:00:00Z").is_ok());
    }

    #[test]
    fn parse_rejects_out_of_range_time() {
        assert!(parse_rfc3339("2024-01-15T24:00:00Z").is_err());
        assert!(parse_rfc3339("2024-01-15T08:60:00Z").is_err());
        assert!(parse_rfc3339("2024-01-15T08:30:60Z").is_err());
    }

    #[test]
    fn parse_rejects_year_out_of_range() {
        assert!(parse_rfc3339("1969-12-31T23:59:59Z").is_err());
        assert!(parse_rfc3339("10000-01-01T00:00:00Z").is_err());
    }

    #[test]
    fn boundary_years_roundtrip() {
        for secs in [0u64, 86_400, 31_536_000, 1_700_000_000] {
            let t = SystemTime::UNIX_EPOCH + Duration::from_secs(secs);
            let s = to_rfc3339(t);
            let back = parse_rfc3339(&s).unwrap();
            assert_eq!(back, t, "roundtrip failed for {s}");
        }
    }

    #[test]
    fn property_roundtrip_over_epoch_range() {
        // Deterministic LCG so we sample many seconds without a `rand` dep.
        let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
        for _ in 0..2_000 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            // Keep within a range the formatter can represent (1970..~2500).
            let secs = state % 17_000_000_000;
            let t = SystemTime::UNIX_EPOCH + Duration::from_secs(secs);
            let s = to_rfc3339(t);
            let back = parse_rfc3339(&s).expect(&s);
            assert_eq!(back, t, "roundtrip drift for {s}");
        }
    }
}
