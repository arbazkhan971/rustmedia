//! Time, rational numbers, and timestamp handling.
//!
//! Media containers count time in *ticks* of a per-track **timescale** (ticks
//! per second). Keeping timestamps as integer ticks avoids the rounding drift
//! that creeps in when everything is eagerly converted to floating-point
//! seconds, so RustMedia preserves the original integers and only converts to
//! [`Duration`] or `f64` at the edges, when a human needs to read them.

use std::time::Duration;

use crate::error::{Error, Result};

/// A rational number `num / den`, used for frame rates, sample aspect ratios,
/// and other exact ratios that must not be flattened to floating point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Rational {
    /// Numerator.
    pub num: i64,
    /// Denominator. Never zero for a value produced by [`Rational::new`].
    pub den: i64,
}

impl Rational {
    /// Create a rational `num / den`, normalising the sign and reducing by the
    /// greatest common divisor. A zero denominator is clamped to `1` to keep
    /// the type total (division by zero is never useful here).
    #[must_use]
    pub fn new(num: i64, den: i64) -> Self {
        if den == 0 {
            return Rational { num, den: 1 };
        }
        let sign = if den < 0 { -1 } else { 1 };
        let (mut num, mut den) = (num * sign, den * sign);
        let g = gcd(num.unsigned_abs(), den.unsigned_abs()) as i64;
        if g > 1 {
            num /= g;
            den /= g;
        }
        Rational { num, den }
    }

    /// The value as an `f64`. Returns `0.0` for a zero denominator.
    #[must_use]
    pub fn as_f64(self) -> f64 {
        if self.den == 0 {
            0.0
        } else {
            self.num as f64 / self.den as f64
        }
    }

    /// `true` if the numerator is zero.
    #[must_use]
    pub fn is_zero(self) -> bool {
        self.num == 0
    }
}

impl std::fmt::Display for Rational {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.num, self.den)
    }
}

const fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

/// A timestamp expressed as an integer number of `ticks` in a `timescale`
/// (ticks per second).
///
/// For example, a 30 fps video frame at the two-second mark in a 15360-tick
/// timescale is `Timestamp { ticks: 30720, timescale: 15360 }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Timestamp {
    /// Number of ticks.
    pub ticks: i64,
    /// Ticks per second. Should be non-zero.
    pub timescale: u32,
}

impl Timestamp {
    /// Create a timestamp of `ticks` in `timescale`.
    #[must_use]
    pub fn new(ticks: i64, timescale: u32) -> Self {
        Timestamp { ticks, timescale }
    }

    /// The timestamp in seconds as an `f64`. Returns `0.0` if the timescale is
    /// zero (which only happens with malformed input).
    #[must_use]
    pub fn seconds(self) -> f64 {
        if self.timescale == 0 {
            0.0
        } else {
            self.ticks as f64 / f64::from(self.timescale)
        }
    }

    /// Convert to a [`Duration`]. Negative timestamps clamp to zero, since
    /// [`Duration`] cannot represent negative spans.
    #[must_use]
    pub fn to_duration(self) -> Duration {
        let secs = self.seconds();
        if secs <= 0.0 {
            Duration::ZERO
        } else {
            Duration::from_secs_f64(secs)
        }
    }

    /// Re-express this timestamp in a different `timescale`, rounding to the
    /// nearest tick. Useful when copying samples between tracks or containers
    /// with different time bases.
    #[must_use]
    pub fn rescale(self, timescale: u32) -> Timestamp {
        if self.timescale == timescale || self.timescale == 0 {
            return Timestamp {
                ticks: self.ticks,
                timescale,
            };
        }
        // Round-to-nearest using i128 to avoid overflow on large tick counts.
        let scaled = (i128::from(self.ticks) * i128::from(timescale)
            + i128::from(self.timescale) / 2)
            / i128::from(self.timescale);
        Timestamp {
            ticks: scaled as i64,
            timescale,
        }
    }
}

/// Format a [`Duration`] as `HH:MM:SS.mmm` (hours are omitted when zero, e.g.
/// `01:30.500`). This is the canonical way RustMedia renders durations.
#[must_use]
pub fn format_duration(d: Duration) -> String {
    let total_ms = d.as_millis();
    let ms = total_ms % 1000;
    let total_secs = total_ms / 1000;
    let secs = total_secs % 60;
    let mins = (total_secs / 60) % 60;
    let hours = total_secs / 3600;
    if hours > 0 {
        format!("{hours:02}:{mins:02}:{secs:02}.{ms:03}")
    } else {
        format!("{mins:02}:{secs:02}.{ms:03}")
    }
}

/// Parse a human time expression into a [`Duration`].
///
/// Accepts the forms RustMedia's CLI and trimming operations use:
/// - plain seconds, integer or fractional: `"90"`, `"1.5"`
/// - a unit suffix: `"90s"`, `"1.5s"`, `"500ms"`, `"2m"`, `"1h"`
/// - colon-separated clock time: `"1:30"` (mm:ss), `"01:02:03"` (hh:mm:ss),
///   with an optional fractional seconds part: `"00:01:30.250"`
///
/// # Errors
/// Returns [`Error::InvalidArgument`] if the string does not match any form.
pub fn parse_duration(s: &str) -> Result<Duration> {
    let s = s.trim();
    if s.is_empty() {
        return Err(Error::invalid_argument("empty time value"));
    }

    // Clock form: one or two colons.
    if s.contains(':') {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() > 3 {
            return Err(Error::invalid_argument(format!("invalid time '{s}'")));
        }
        let mut secs = 0f64;
        for part in &parts {
            let v: f64 = part
                .parse()
                .map_err(|_| Error::invalid_argument(format!("invalid time component in '{s}'")))?;
            secs = secs * 60.0 + v;
        }
        return duration_from_secs_f64(secs, s);
    }

    // Unit-suffixed forms. Order matters: check the longer suffix first.
    let (value, mult) = if let Some(v) = s.strip_suffix("ms") {
        (v, 0.001)
    } else if let Some(v) = s.strip_suffix('s') {
        (v, 1.0)
    } else if let Some(v) = s.strip_suffix('m') {
        (v, 60.0)
    } else if let Some(v) = s.strip_suffix('h') {
        (v, 3600.0)
    } else {
        (s, 1.0)
    };

    let n: f64 = value
        .trim()
        .parse()
        .map_err(|_| Error::invalid_argument(format!("invalid time '{s}'")))?;
    duration_from_secs_f64(n * mult, s)
}

fn duration_from_secs_f64(secs: f64, original: &str) -> Result<Duration> {
    if secs.is_finite() && secs >= 0.0 {
        Ok(Duration::from_secs_f64(secs))
    } else {
        Err(Error::invalid_argument(format!(
            "time must be non-negative and finite: '{original}'"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rational_reduces_and_normalises_sign() {
        assert_eq!(
            Rational::new(30_000, 1001),
            Rational {
                num: 30_000,
                den: 1001
            }
        );
        assert_eq!(Rational::new(2, 4), Rational { num: 1, den: 2 });
        assert_eq!(Rational::new(1, -2), Rational { num: -1, den: 2 });
        assert_eq!(Rational::new(5, 0), Rational { num: 5, den: 1 });
    }

    #[test]
    fn timestamp_seconds_and_rescale() {
        let t = Timestamp::new(30_720, 15_360);
        assert!((t.seconds() - 2.0).abs() < 1e-9);
        assert_eq!(t.rescale(1000), Timestamp::new(2000, 1000));
    }

    #[test]
    fn format_duration_renders_clock() {
        assert_eq!(format_duration(Duration::from_millis(90_500)), "01:30.500");
        assert_eq!(
            format_duration(Duration::from_millis(3_661_250)),
            "01:01:01.250"
        );
    }

    #[test]
    fn parse_duration_accepts_all_forms() {
        assert_eq!(parse_duration("90").unwrap(), Duration::from_secs(90));
        assert_eq!(parse_duration("1.5s").unwrap(), Duration::from_millis(1500));
        assert_eq!(parse_duration("500ms").unwrap(), Duration::from_millis(500));
        assert_eq!(parse_duration("2m").unwrap(), Duration::from_secs(120));
        assert_eq!(parse_duration("1h").unwrap(), Duration::from_secs(3600));
        assert_eq!(parse_duration("1:30").unwrap(), Duration::from_secs(90));
        assert_eq!(
            parse_duration("01:02:03").unwrap(),
            Duration::from_secs(3723)
        );
        assert_eq!(
            parse_duration("00:01:30.250").unwrap(),
            Duration::from_millis(90_250)
        );
    }

    #[test]
    fn parse_duration_rejects_garbage() {
        assert!(parse_duration("").is_err());
        assert!(parse_duration("abc").is_err());
        assert!(parse_duration("-5").is_err());
    }
}
