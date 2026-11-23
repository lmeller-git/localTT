//! Campaign and wall-clock time, the derived in-game calendar and its constants.

use std::{fmt, hash::Hash};

use serde::{Deserialize, Serialize};

/// Seconds in one in-game day (24 hours).
pub const SECONDS_PER_DAY: i64 = 86_400;

/// Days in one in-game week: Faerûn's ten-day *tenday*.
pub const DAYS_PER_WEEK: i64 = 10;

/// Seconds in one combat round; the initiative tracker advances the clock by
/// this much per turn.
pub const SECONDS_PER_ROUND: i64 = 6;

/// Seconds on the DM-controlled campaign clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CampaignTime(i64);

impl CampaignTime {
    /// Builds a campaign time from seconds.
    #[must_use]
    #[inline]
    pub fn from_seconds(s: i64) -> Self {
        Self(s)
    }

    /// Returns the campaign time in seconds.
    #[must_use]
    #[inline]
    pub fn seconds(self) -> i64 {
        self.0
    }

    /// Whole in-game days elapsed since the campaign start.
    ///
    /// The origin is the campaign start (`0`), 00:00 on day 0. Floors, so a time
    /// before the origin yields a negative day and [`Self::time_of_day`] stays in
    /// `00:00:00..=23:59:59`.
    #[must_use]
    #[inline]
    pub const fn days_since_start(self) -> i64 {
        self.0.div_euclid(SECONDS_PER_DAY)
    }

    /// Whole in-game weeks since the campaign start.
    ///
    /// A week in Faerûn is a *tenday* of ten days ([`DAYS_PER_WEEK`]).
    #[must_use]
    #[inline]
    pub const fn weeks_since_start(self) -> i64 {
        self.days_since_start().div_euclid(DAYS_PER_WEEK)
    }

    /// Zero-based day within the current week, in `0..`[`DAYS_PER_WEEK`].
    #[must_use]
    #[inline]
    pub const fn day_of_week(self) -> u8 {
        self.days_since_start().rem_euclid(DAYS_PER_WEEK) as u8
    }

    /// Time of day, derived from the seconds elapsed since midnight.
    #[must_use]
    #[inline]
    pub const fn time_of_day(self) -> TimeOfDay {
        let day_s = self.0.rem_euclid(SECONDS_PER_DAY);
        TimeOfDay {
            hour: (day_s / 3_600) as u8,
            minute: ((day_s % 3_600) / 60) as u8,
            second: (day_s % 60) as u8,
        }
    }
}

impl fmt::Display for CampaignTime {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// Time of day on the campaign clock, derived from [`CampaignTime`].
///
/// A view, not a stored value: it is not part of the wire contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimeOfDay {
    /// Hour, 0 to 23.
    pub hour: u8,
    /// Minute, 0 to 59.
    pub minute: u8,
    /// Second, 0 to 59.
    pub second: u8,
}

impl fmt::Display for TimeOfDay {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
    }
}

/// Unix seconds; wall clock, for audit and display only, never for durations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnixTime(i64);

impl UnixTime {
    /// Builds a wall-clock time from seconds.
    #[must_use]
    #[inline]
    pub fn from_seconds(s: i64) -> Self {
        Self(s)
    }

    /// Returns the wall-clock time in seconds.
    #[must_use]
    #[inline]
    pub fn seconds(self) -> i64 {
        self.0
    }
}

impl fmt::Display for UnixTime {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}
