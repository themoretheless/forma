//! Calendar date and time input with leap-year aware arithmetic.

use super::*;

/// Interactive state model for `DateTimeInput`: leap-year aware calendar math,
/// 42-cell month grid generation, day/month/time stepping, and ISO formatting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DateTimeModel {
    year: i32,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    open: bool,
}

impl DateTimeModel {
    pub fn new(year: i32, month: u8, day: u8, hour: u8, minute: u8) -> Result<Self, ModelError> {
        if !(1..=12).contains(&month)
            || day < 1
            || day > Self::days_in_month(year, month)
            || hour > 23
            || minute > 59
        {
            return Err(ModelError::InvalidRange);
        }
        Ok(Self {
            year,
            month,
            day,
            hour,
            minute,
            open: false,
        })
    }

    pub fn is_leap_year(year: i32) -> bool {
        (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
    }

    pub fn days_in_month(year: i32, month: u8) -> u8 {
        match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if Self::is_leap_year(year) => 29,
            2 => 28,
            _ => 30,
        }
    }

    /// Day of week (`0 = Monday .. 6 = Sunday`) via Sakamoto's algorithm.
    pub fn weekday(year: i32, month: u8, day: u8) -> u8 {
        let t = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
        let y = if month < 3 { year - 1 } else { year };
        let sun0 = (y + y / 4 - y / 100 + y / 400 + t[(month - 1) as usize] + i32::from(day))
            .rem_euclid(7) as u8;
        (sun0 + 6) % 7
    }

    pub fn format_iso(&self) -> String {
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute
        )
    }

    pub fn format_date(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    pub fn year(&self) -> i32 {
        self.year
    }
    pub fn month(&self) -> u8 {
        self.month
    }
    pub fn day(&self) -> u8 {
        self.day
    }
    pub fn hour(&self) -> u8 {
        self.hour
    }
    pub fn minute(&self) -> u8 {
        self.minute
    }
    pub fn is_open(&self) -> bool {
        self.open
    }
    pub fn set_open(&mut self, open: bool) {
        self.open = open;
    }

    pub fn step_months(&mut self, delta: i32) {
        let total = i64::from(self.year) * 12 + (i64::from(self.month) - 1) + i64::from(delta);
        self.year = total
            .div_euclid(12)
            .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
        self.month = (total.rem_euclid(12) + 1) as u8;
        let max_day = Self::days_in_month(self.year, self.month);
        self.day = self.day.min(max_day);
    }

    pub fn step_days(&mut self, delta: i32) {
        if delta == 0 {
            return;
        }
        let days = days_from_civil(i64::from(self.year), self.month, self.day) + i64::from(delta);
        let (y, m, d) = civil_from_days(days);
        if y < i64::from(i32::MIN) {
            self.year = i32::MIN;
            self.month = 1;
            self.day = 1;
        } else if y > i64::from(i32::MAX) {
            self.year = i32::MAX;
            self.month = 12;
            self.day = 31;
        } else {
            self.year = y as i32;
            self.month = m;
            self.day = d;
        }
    }

    /// Generates a 42-cell `(6 weeks × 7 days)` calendar grid for the current month.
    /// Cells outside the current month have `0`, cells in the current month have `1..=days_in_month`.
    pub fn calendar_grid(&self) -> [u8; 42] {
        let mut grid = [0u8; 42];
        let first_wd = Self::weekday(self.year, self.month, 1) as usize;
        let days = Self::days_in_month(self.year, self.month);
        for d in 1..=days {
            grid[first_wd + (d as usize - 1)] = d;
        }
        grid
    }
}

/// Howard Hinnant's proleptic Gregorian `days_from_civil` (`1970-01-01 == 0`).
#[inline]
fn days_from_civil(year: i64, month: u8, day: u8) -> i64 {
    let m = i64::from(month);
    let d = i64::from(day);
    let y = year - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400; // [0, 399]
    let mp = m + if m > 2 { -3 } else { 9 }; // [0, 11]
    let doy = (153 * mp + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// Howard Hinnant's proleptic Gregorian `civil_from_days` (`0 == 1970-01-01`).
#[inline]
fn civil_from_days(z: i64) -> (i64, u8, u8) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = mp + if mp < 10 { 3 } else { -9 }; // [1, 12]
    (y + i64::from(m <= 2), m as u8, d as u8)
}
