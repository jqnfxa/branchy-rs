//! What day it is.
//!
//! `branchy-core` deliberately cannot answer this: a graph engine that reads
//! the clock is no longer a pure function of its input, and its tests would
//! start depending on when they ran. The question belongs one layer up, here.

use std::time::{SystemTime, UNIX_EPOCH};

use branchy_core::Date;

const SECONDS_PER_DAY: i64 = 86_400;

/// Today, in local wall-clock terms as far as the system reports them.
///
/// Falls back to the epoch if the clock is set before 1970, which is a broken
/// machine rather than a case worth an error type.
#[must_use]
pub fn today() -> Date {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
        });
    Date::from_days(seconds.div_euclid(SECONDS_PER_DAY))
}

/// How a deadline stands relative to today.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    /// The day has passed.
    Overdue,
    /// Due today.
    Today,
    /// Within the next seven days.
    Soon,
    /// Further out than that.
    Later,
}

impl Urgency {
    /// Where a deadline sits relative to a given day.
    #[must_use]
    pub fn of(due: Date, today: Date) -> Self {
        match today.days_until(due) {
            days if days < 0 => Self::Overdue,
            0 => Self::Today,
            days if days <= 7 => Self::Soon,
            _ => Self::Later,
        }
    }

    /// The word a user interface shows.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Overdue => "overdue",
            Self::Today => "today",
            Self::Soon => "soon",
            Self::Later => "later",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Urgency, today};
    use branchy_core::Date;

    fn d(text: &str) -> Date {
        text.parse().expect("a real date")
    }

    #[test]
    fn the_clock_gives_a_plausible_day() {
        let now = today();
        assert!(now > d("2020-01-01"), "clock is before this was written");
        assert!(now < d("2100-01-01"), "clock is implausibly far ahead");
    }

    #[test]
    fn urgency_has_no_gaps_and_no_overlaps() {
        let now = d("2026-09-20");
        assert_eq!(Urgency::of(d("2026-09-19"), now), Urgency::Overdue);
        assert_eq!(Urgency::of(d("2026-09-20"), now), Urgency::Today);
        assert_eq!(Urgency::of(d("2026-09-21"), now), Urgency::Soon);
        assert_eq!(
            Urgency::of(d("2026-09-27"), now),
            Urgency::Soon,
            "seven days"
        );
        assert_eq!(Urgency::of(d("2026-09-28"), now), Urgency::Later, "eight");
    }
}
