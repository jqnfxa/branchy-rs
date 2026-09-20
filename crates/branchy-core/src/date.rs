//! A calendar date, with no dependencies.
//!
//! Deliberately not `chrono` or `time`: the graph needs a day, comparison and
//! subtraction, and nothing else. No clocks, no zones, no formatting locales.
//! Reading the clock belongs to whatever is above this crate, which is why
//! there is no `today()` here.

use crate::error::Error;

/// A day in the proleptic Gregorian calendar.
///
/// The field order is what makes the derived `Ord` chronological.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    year: i32,
    month: u8,
    day: u8,
}

const fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// How many days the month has, which is the only place leap years matter.
const fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

impl Date {
    /// A date, if the calendar has one.
    ///
    /// # Errors
    ///
    /// [`Error::BadDate`] for a month outside 1-12, or a day the month does not
    /// have. 29 February is accepted only in a leap year.
    pub fn new(year: i32, month: u8, day: u8) -> Result<Self, Error> {
        let limit = days_in_month(year, month);
        if limit == 0 || day == 0 || day > limit {
            return Err(Error::BadDate(format!("{year:04}-{month:02}-{day:02}")));
        }
        Ok(Self { year, month, day })
    }

    /// The year.
    #[must_use]
    pub const fn year(self) -> i32 {
        self.year
    }

    /// The month, 1 to 12.
    #[must_use]
    pub const fn month(self) -> u8 {
        self.month
    }

    /// The day of the month.
    #[must_use]
    pub const fn day(self) -> u8 {
        self.day
    }

    /// Days since 1970-01-01, negative before it.
    ///
    /// Howard Hinnant's `days_from_civil`, which shifts the year to start in
    /// March so the leap day lands at the end and needs no special case.
    #[must_use]
    pub const fn to_days(self) -> i64 {
        let year = self.year as i64 - if self.month <= 2 { 1 } else { 0 };
        let era = if year >= 0 { year } else { year - 399 } / 400;
        let year_of_era = year - era * 400;
        let month_shifted = (self.month as i64 + 9) % 12;
        let day_of_year = (153 * month_shifted + 2) / 5 + self.day as i64 - 1;
        let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
        era * 146_097 + day_of_era - 719_468
    }

    /// The inverse of [`Date::to_days`].
    ///
    /// The narrowing casts are safe by construction: Hinnant's algorithm yields
    /// a day in 1-31 and a month in 1-12 for any input, so neither can
    /// truncate or lose a sign.
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub const fn from_days(days: i64) -> Self {
        let shifted = days + 719_468;
        let era = if shifted >= 0 {
            shifted
        } else {
            shifted - 146_096
        } / 146_097;
        let day_of_era = shifted - era * 146_097;
        let year_of_era =
            (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let year = year_of_era + era * 400;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let month_shifted = (5 * day_of_year + 2) / 153;
        let day = (day_of_year - (153 * month_shifted + 2) / 5 + 1) as u8;
        let month = (if month_shifted < 10 {
            month_shifted + 3
        } else {
            month_shifted - 9
        }) as u8;
        Self {
            year: (year + if month <= 2 { 1 } else { 0 }) as i32,
            month,
            day,
        }
    }

    /// Days from this date to `other`. Negative when `other` is earlier.
    #[must_use]
    pub const fn days_until(self, other: Self) -> i64 {
        other.to_days() - self.to_days()
    }

    /// The date `days` later, or earlier when negative.
    #[must_use]
    pub const fn plus_days(self, days: i64) -> Self {
        Self::from_days(self.to_days() + days)
    }

    /// Monday of the week this date falls in.
    ///
    /// 1970-01-01 was a Thursday, which is where the 3 comes from.
    #[must_use]
    pub const fn week_start(self) -> Self {
        let weekday = (self.to_days() + 3).rem_euclid(7); // 0 = Monday
        Self::from_days(self.to_days() - weekday)
    }
}

impl std::fmt::Display for Date {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl std::str::FromStr for Date {
    type Err = Error;

    /// Parses `YYYY-MM-DD`, and nothing else. A date typed by a person goes
    /// through here, so the failure has to name what was wrong with it.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let bad = || Error::BadDate(text.to_string());
        let mut parts = text.trim().split('-');
        let (Some(year), Some(month), Some(day), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(bad());
        };
        if year.len() != 4 || month.len() != 2 || day.len() != 2 {
            return Err(bad());
        }
        Self::new(
            year.parse().map_err(|_| bad())?,
            month.parse().map_err(|_| bad())?,
            day.parse().map_err(|_| bad())?,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Date;

    fn d(y: i32, m: u8, day: u8) -> Date {
        Date::new(y, m, day).expect("a real date")
    }

    #[test]
    fn round_trips_through_day_numbers() {
        // every day across four centuries, including every leap rule
        let start = d(1800, 1, 1).to_days();
        let end = d(2200, 12, 31).to_days();
        for days in start..=end {
            let date = Date::from_days(days);
            assert_eq!(date.to_days(), days, "{date} did not survive");
        }
    }

    #[test]
    fn the_epoch_is_where_it_should_be() {
        assert_eq!(d(1970, 1, 1).to_days(), 0);
        assert_eq!(d(1969, 12, 31).to_days(), -1);
        assert_eq!(Date::from_days(0), d(1970, 1, 1));
    }

    #[test]
    fn leap_days_exist_only_when_they_should() {
        assert!(Date::new(2024, 2, 29).is_ok());
        assert!(Date::new(2023, 2, 29).is_err());
        assert!(Date::new(2000, 2, 29).is_ok(), "divisible by 400");
        assert!(Date::new(1900, 2, 29).is_err(), "divisible by 100, not 400");
    }

    #[test]
    fn impossible_dates_are_refused() {
        assert!(Date::new(2026, 13, 1).is_err());
        assert!(Date::new(2026, 0, 1).is_err());
        assert!(Date::new(2026, 4, 31).is_err());
        assert!(Date::new(2026, 1, 0).is_err());
    }

    #[test]
    fn dates_compare_chronologically() {
        assert!(d(2026, 1, 31) < d(2026, 2, 1));
        assert!(d(2025, 12, 31) < d(2026, 1, 1));
        let mut all = vec![d(2026, 6, 1), d(2025, 1, 1), d(2026, 1, 1)];
        all.sort_unstable();
        assert_eq!(all, vec![d(2025, 1, 1), d(2026, 1, 1), d(2026, 6, 1)]);
    }

    #[test]
    fn subtraction_counts_days() {
        assert_eq!(d(2026, 1, 1).days_until(d(2026, 1, 8)), 7);
        assert_eq!(d(2026, 3, 1).days_until(d(2026, 1, 1)), -59);
        assert_eq!(d(2024, 2, 28).days_until(d(2024, 3, 1)), 2, "leap year");
        assert_eq!(d(2023, 2, 28).days_until(d(2023, 3, 1)), 1);
    }

    #[test]
    fn a_week_starts_on_monday() {
        // 2026-09-20 is a Sunday, so its week began on the 14th
        assert_eq!(d(2026, 9, 20).week_start(), d(2026, 9, 14));
        assert_eq!(d(2026, 9, 14).week_start(), d(2026, 9, 14));
        assert_eq!(d(2026, 9, 15).week_start(), d(2026, 9, 14));
    }

    #[test]
    fn parsing_accepts_only_the_one_shape() {
        assert_eq!(
            "2026-12-31".parse::<Date>().expect("valid"),
            d(2026, 12, 31)
        );
        for bad in [
            "2026-12-3",
            "26-12-31",
            "2026/12/31",
            "2026-13-01",
            "",
            "today",
        ] {
            assert!(bad.parse::<Date>().is_err(), "{bad} should not parse");
        }
    }

    #[test]
    fn display_round_trips_through_parsing() {
        let date = d(2026, 3, 7);
        assert_eq!(date.to_string(), "2026-03-07");
        assert_eq!(date.to_string().parse::<Date>().expect("valid"), date);
    }
}
