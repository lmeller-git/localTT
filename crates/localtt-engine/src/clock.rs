//! Clock arithmetic (RFC `engine-executor.md` 4.5, ENG-9): pure functions of
//! campaign time, no state and no I/O.

use localtt_core::types::CampaignTime;

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
    todo!()
}
