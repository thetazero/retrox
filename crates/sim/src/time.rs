//! Galaxy time. The sim ticks once per galaxy day (see `docs/DESIGN.md` §3).

use std::fmt;

use serde::{Deserialize, Serialize};

/// Days per galaxy year. A tidy fictional calendar: 12 months of 30 days.
pub const DAYS_PER_YEAR: u64 = 360;

/// A point in galaxy time, counted in whole days since the start of history.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct GalaxyTime {
    days: u64,
}

impl GalaxyTime {
    pub const ZERO: Self = Self { days: 0 };

    pub const fn from_days(days: u64) -> Self {
        Self { days }
    }

    pub const fn from_years(years: u64) -> Self {
        Self {
            days: years * DAYS_PER_YEAR,
        }
    }

    pub const fn days(self) -> u64 {
        self.days
    }

    pub const fn year(self) -> u64 {
        self.days / DAYS_PER_YEAR
    }

    pub const fn day_of_year(self) -> u64 {
        self.days % DAYS_PER_YEAR
    }

    pub fn advance(&mut self, days: u64) {
        self.days += days;
    }
}

impl fmt::Display for GalaxyTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Y{}.{:03}", self.year(), self.day_of_year())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar() {
        let t = GalaxyTime::from_days(DAYS_PER_YEAR * 3 + 41);
        assert_eq!(t.year(), 3);
        assert_eq!(t.day_of_year(), 41);
        assert_eq!(t.to_string(), "Y3.041");
    }
}
