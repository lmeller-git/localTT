//! Clock arithmetic (RFC `engine-executor.md` 4.5, ENG-9): pure functions of
//! campaign time, no state and no I/O.

use localtt_core::types::{CampaignTime, SECONDS_PER_DAY};

/// Whole dawn instants crossed moving the clock from `old` to `new`:
/// positive forwards, negative backwards (a rewind crossing).
///
/// Exact integer arithmetic over [`SECONDS_PER_DAY`] and `dawn_hour`; the
/// `div_euclid` floor keeps pre-origin times well-behaved. The expiry window
/// over statuses lives in [`apply`](crate::commands::apply), not here.
///
/// [`SECONDS_PER_DAY`]: localtt_core::types::SECONDS_PER_DAY
#[must_use]
#[inline]
pub fn crossed_dawns(old: CampaignTime, new: CampaignTime, dawn_hour: u8) -> i64 {
    let dawn_offset = i64::from(dawn_hour) * 3_600;
    let dawns_at_or_before =
        |time: CampaignTime| (time.seconds() - dawn_offset).div_euclid(SECONDS_PER_DAY);
    dawns_at_or_before(new) - dawns_at_or_before(old)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn time(seconds: i64) -> CampaignTime {
        CampaignTime::from_seconds(seconds)
    }

    #[test]
    fn the_dawn_instant_is_in_the_window_and_the_old_instant_is_not() {
        // Dawn on day 0 is 06:00, second 21_600.
        assert_eq!(crossed_dawns(time(0), time(21_599), 6), 0);
        assert_eq!(crossed_dawns(time(0), time(21_600), 6), 1);
        assert_eq!(crossed_dawns(time(21_600), time(21_600), 6), 0);
        assert_eq!(crossed_dawns(time(21_601), time(21_600), 6), 0);
        assert_eq!(crossed_dawns(time(21_601), time(0), 6), -1);
        assert_eq!(crossed_dawns(time(0), time(0), 6), 0);
    }

    #[test]
    fn multiple_days_count_once_per_dawn() {
        assert_eq!(crossed_dawns(time(0), time(3 * SECONDS_PER_DAY), 6), 3);
        assert_eq!(crossed_dawns(time(3 * SECONDS_PER_DAY), time(0), 6), -3);
        assert_eq!(
            crossed_dawns(time(0), time(3 * SECONDS_PER_DAY + 21_600), 6),
            4
        );
    }

    #[test]
    fn pre_origin_times_floor_correctly() {
        // A dawn instant is day * SECONDS_PER_DAY + dawn_hour * 3600 for
        // negative days too: -64_800 is 06:00 on day -1.
        assert_eq!(crossed_dawns(time(-SECONDS_PER_DAY), time(0), 6), 1);
        assert_eq!(crossed_dawns(time(0), time(-SECONDS_PER_DAY), 6), -1);
        assert_eq!(crossed_dawns(time(-3 * SECONDS_PER_DAY), time(0), 6), 3);
        assert_eq!(crossed_dawns(time(-64_800), time(-64_799), 6), 0);
    }

    #[test]
    fn midnight_dawns_and_other_hours_behave() {
        assert_eq!(crossed_dawns(time(-1), time(0), 0), 1);
        assert_eq!(crossed_dawns(time(0), time(1), 0), 0);
        assert_eq!(crossed_dawns(time(0), time(SECONDS_PER_DAY), 0), 1);
        // 23:00 on day 0 is second 82_800.
        assert_eq!(crossed_dawns(time(0), time(SECONDS_PER_DAY), 23), 1);
        assert_eq!(crossed_dawns(time(0), time(82_800), 23), 1);
        assert_eq!(crossed_dawns(time(0), time(82_799), 23), 0);
    }
}
