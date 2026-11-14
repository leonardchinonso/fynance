//! UK tax-year arithmetic: the `YYYY-YY` label and the 6 April .. 5 April
//! window it names (plan 28). Shared by the income routes, the summary
//! endpoint and the CGT lookback.

use chrono::NaiveDate;

/// A parsed UK tax year. `YYYY-YY` names the year that starts on 6 April of
/// `start_year` and ends on 5 April of the following calendar year.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaxYear {
    /// The calendar year the tax year starts in (its 6 April).
    pub start_year: i32,
    /// Inclusive first day: 6 April of `start_year`.
    pub start: NaiveDate,
    /// Inclusive last day: 5 April of `start_year + 1`.
    pub end: NaiveDate,
}

impl TaxYear {
    /// Parse a `YYYY-YY` label: four digits, a hyphen, two digits, and the
    /// second half must be `(first + 1) mod 100`. Also requires the window
    /// dates to be constructible, so a syntactically fine label near the
    /// chrono year limit is rejected rather than yielding a partial year.
    pub fn parse(s: &str) -> Option<Self> {
        let (head, tail) = s.split_once('-')?;
        if head.len() != 4 || tail.len() != 2 {
            return None;
        }
        let start_year: i32 = head.parse().ok()?;
        let end_year: i32 = tail.parse().ok()?;
        if (start_year + 1) % 100 != end_year {
            return None;
        }
        Self::from_start_year(start_year)
    }

    /// The tax year beginning 6 April of `start_year`, or `None` when either
    /// boundary date is not constructible.
    pub fn from_start_year(start_year: i32) -> Option<Self> {
        let start = NaiveDate::from_ymd_opt(start_year, 4, 6)?;
        let end = NaiveDate::from_ymd_opt(start_year + 1, 4, 5)?;
        Some(Self {
            start_year,
            start,
            end,
        })
    }

    /// The tax year containing `date`: 6 April starts a year, 5 April ends one.
    pub fn for_date(date: NaiveDate) -> Self {
        use chrono::Datelike;
        let on_or_after_six_april = (date.month(), date.day()) >= (4, 6);
        let start_year = if on_or_after_six_april {
            date.year()
        } else {
            date.year() - 1
        };
        Self::from_start_year(start_year)
            .expect("a tax year around an existing date is constructible")
    }

    /// Canonical `YYYY-YY` label for this year.
    pub fn label(&self) -> String {
        format!("{}-{:02}", self.start_year, (self.start_year + 1) % 100)
    }

    /// Whether `date` falls inside the year's inclusive window.
    pub fn contains(&self, date: NaiveDate) -> bool {
        self.start <= date && date <= self.end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn parses_canonical_labels() {
        let ty = TaxYear::parse("2025-26").unwrap();
        assert_eq!(ty.start_year, 2025);
        assert_eq!(ty.label(), "2025-26");
        assert_eq!(TaxYear::parse("1999-00").unwrap().label(), "1999-00");
    }

    #[test]
    fn rejects_second_half_not_following_the_first() {
        assert!(TaxYear::parse("2025-27").is_none());
        assert!(TaxYear::parse("2025-25").is_none());
    }

    #[test]
    fn rejects_malformed_labels() {
        for bad in [
            "", "2025", "2025-", "-26", "25-26", "2025-26x", "2025--26", "2025/26", "2-26",
            "2025-126",
        ] {
            assert!(TaxYear::parse(bad).is_none(), "{bad} must be rejected");
        }
    }

    #[test]
    fn window_is_six_april_to_five_april() {
        let ty = TaxYear::parse("2025-26").unwrap();
        assert_eq!(ty.start, date(2025, 4, 6));
        assert_eq!(ty.end, date(2026, 4, 5));
    }

    #[test]
    fn boundary_days_belong_to_opposite_years() {
        let this = TaxYear::parse("2025-26").unwrap();
        let next = TaxYear::parse("2026-27").unwrap();
        // 5 April is the last day; 6 April belongs to the next year.
        assert!(this.contains(date(2026, 4, 5)));
        assert!(!this.contains(date(2026, 4, 6)));
        assert!(next.contains(date(2026, 4, 6)));
        assert!(!next.contains(date(2026, 4, 5)));
        // Opening boundary: 5 April 2025 is in 2024-25, not 2025-26.
        assert!(this.contains(date(2025, 4, 6)));
        assert!(!this.contains(date(2025, 4, 5)));
    }

    #[test]
    fn leap_february_falls_inside_the_year_ending_in_it() {
        // 2023-24 spans 2023-04-06..2024-04-05 and contains 29 Feb 2024.
        let ty = TaxYear::parse("2023-24").unwrap();
        assert!(ty.contains(date(2024, 2, 29)));
        assert!(
            !TaxYear::parse("2024-25")
                .unwrap()
                .contains(date(2024, 2, 29))
        );
    }

    #[test]
    fn for_date_splits_at_six_april() {
        assert_eq!(TaxYear::for_date(date(2026, 4, 5)).label(), "2025-26");
        assert_eq!(TaxYear::for_date(date(2026, 4, 6)).label(), "2026-27");
        assert_eq!(TaxYear::for_date(date(2025, 12, 31)).label(), "2025-26");
        assert_eq!(TaxYear::for_date(date(2026, 1, 1)).label(), "2025-26");
        assert_eq!(TaxYear::for_date(date(2024, 2, 29)).label(), "2023-24");
    }

    #[test]
    fn start_year_constructor_agrees_with_parse() {
        assert_eq!(TaxYear::from_start_year(2025), TaxYear::parse("2025-26"));
    }
}
