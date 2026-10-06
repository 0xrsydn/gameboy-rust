//! Validated 2000–2099 RTC calendar arithmetic. No clock sampling or timezone conversion.
use std::{error::Error, fmt};

const DAYS_PER_CENTURY: u64 = 36_525;
const SECONDS_PER_DAY: u64 = 86_400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RtcError {
    NotAttached,
    InvalidDateTime,
}

impl fmt::Display for RtcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotAttached => "no cartridge RTC is attached",
            Self::InvalidDateTime => "invalid RTC date/time (expected a valid date in 2000–2099)",
        })
    }
}
impl Error for RtcError {}

/// Decimal [year since 2000, month, day, weekday, hour, minute, second].
/// Weekday is an independent 0–6 counter; the host chooses its naming convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RtcDateTime([u8; 7]);

impl Default for RtcDateTime {
    fn default() -> Self {
        Self([0, 1, 1, 0, 0, 0, 0])
    }
}

fn month_days(year: u8, month: u8) -> u8 {
    match month {
        2 => {
            if year.is_multiple_of(4) {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn bcd(value: u8) -> u8 {
    ((value / 10) << 4) | (value % 10)
}

fn decimal(value: u8) -> Result<u8, RtcError> {
    if value & 15 > 9 || value >> 4 > 9 {
        return Err(RtcError::InvalidDateTime);
    }
    Ok((value >> 4) * 10 + (value & 15))
}

impl RtcDateTime {
    pub fn new(parts: [u8; 7]) -> Result<Self, RtcError> {
        let [year, month, day, week, hour, minute, second] = parts;
        if year > 99
            || !(1..=12).contains(&month)
            || day == 0
            || day > month_days(year, month)
            || week > 6
            || hour > 23
            || minute > 59
            || second > 59
        {
            return Err(RtcError::InvalidDateTime);
        }
        Ok(Self(parts))
    }

    pub fn components(self) -> [u8; 7] {
        self.0
    }

    /// Checked epoch conversion. Uses Sunday=0, so 2000-01-01 is weekday 6.
    /// Unlike RTC ticking, this constructor rejects dates beyond 2099.
    pub fn from_seconds_since_2000(seconds: u64) -> Result<Self, RtcError> {
        if seconds >= DAYS_PER_CENTURY * SECONDS_PER_DAY {
            return Err(RtcError::InvalidDateTime);
        }
        let mut result = Self([0, 1, 1, 6, 0, 0, 0]);
        result.advance(seconds);
        Ok(result)
    }

    pub(super) fn encode(self, hour24: bool) -> [u8; 7] {
        let mut data = self.0.map(bcd);
        let hour = self.0[4];
        data[4] = bcd(if hour24 { hour } else { hour % 12 }) | if hour >= 12 { 0x80 } else { 0 };
        data
    }

    pub(super) fn decode(data: [u8; 7], hour24: bool) -> Result<Self, RtcError> {
        let mut result = [0; 7];
        let masks = [255, 31, 63, 7, 63, 127, 127];
        for i in 0..7 {
            result[i] = decimal(data[i] & masks[i])?;
        }
        if !hour24 {
            if result[4] > 11 {
                return Err(RtcError::InvalidDateTime);
            }
            result[4] += if data[4] & 128 != 0 { 12 } else { 0 };
        }
        Self::new(result)
    }

    /// Advance a bounded two-digit year calendar and the independent weekday counter.
    /// Arithmetic is bounded even for u64::MAX seconds; year 99 wraps to 00.
    pub(super) fn advance(&mut self, seconds: u64) {
        let [year, month, day, week, hour, minute, second] = self.0;
        let time = u64::from(hour) * 3600
            + u64::from(minute) * 60
            + u64::from(second)
            + seconds % SECONDS_PER_DAY;
        let days = seconds / SECONDS_PER_DAY + time / SECONDS_PER_DAY;
        let mut day_index = u64::from(day - 1);
        for y in 0..year {
            day_index += if y.is_multiple_of(4) { 366 } else { 365 };
        }
        for m in 1..month {
            day_index += u64::from(month_days(year, m));
        }
        day_index = (day_index + days % DAYS_PER_CENTURY) % DAYS_PER_CENTURY;
        let mut year: u8 = 0;
        loop {
            let count = if year.is_multiple_of(4) { 366 } else { 365 };
            if day_index < count {
                break;
            }
            day_index -= count;
            year += 1;
        }
        let mut month = 1;
        while day_index >= u64::from(month_days(year, month)) {
            day_index -= u64::from(month_days(year, month));
            month += 1;
        }
        self.0 = [
            year,
            month,
            day_index as u8 + 1,
            ((u64::from(week) + days % 7) % 7) as u8,
            ((time / 3600) % 24) as u8,
            ((time / 60) % 60) as u8,
            (time % 60) as u8,
        ];
    }
}

#[cfg(test)]
mod tests;
