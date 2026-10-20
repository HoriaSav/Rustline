//! Instants ↔ service-day times (M5, spec §10). The feed's timezone is used
//! only here; inside the engine every time is plain seconds.
//!
//! GTFS times count from "noon minus 12 h" of the service day (local noon as
//! an instant, minus 12 hours), not from midnight: on 2026-11-01 in
//! America/New_York that is 01:00-04:00, on 2027-03-14 23:00-05:00 the day
//! before. Consecutive service days are 86 400 s apart, or 82 800 / 90 000 s
//! around a DST switch.

use chrono::{DateTime, Days, FixedOffset, NaiveDate, NaiveDateTime, Offset, TimeZone};
use chrono_tz::Tz;
use rustline_model::Seconds;

/// Times of a query are searched on its local date and the days around it;
/// a service time is at most this (4 days) after its day's reference.
const MAX_SERVICE_SECS: i64 = 4 * 86_400;

/// "Noon minus 12 h" of service day `d` in `tz`, as unix seconds.
pub(crate) fn day_ref(tz: Tz, d: NaiveDate) -> i64 {
    let noon = d.and_time(chrono::NaiveTime::from_hms_opt(12, 0, 0).unwrap_or_default());
    let utc = match tz.from_local_datetime(&noon).earliest() {
        Some(t) => t.timestamp(),
        // Noon does not exist (no real zone does this): use the offset at noon UTC.
        None => {
            let off = tz.offset_from_utc_datetime(&noon);
            noon.and_utc().timestamp() - i64::from(off.fix().local_minus_utc())
        }
    };
    utc - 43_200
}

/// The service-day times of instant `t` in `tz`: `(D - 1, s)`, `(D, s)` and
/// `(D + 1, s)` for the local date D of `t`, each kept only when `s` is
/// between 0 and 4 days. A 00:20 query gives D at 00:20 and D − 1 at 24:20.
pub fn to_service_time(tz: Tz, t: DateTime<FixedOffset>) -> Vec<(NaiveDate, Seconds)> {
    let local = t.with_timezone(&tz).date_naive();
    let days = [
        local.checked_sub_days(Days::new(1)),
        Some(local),
        local.checked_add_days(Days::new(1)),
    ];
    days.into_iter()
        .flatten()
        .filter_map(|d| {
            let s = t.timestamp() - day_ref(tz, d);
            ((0..MAX_SERVICE_SECS).contains(&s)).then_some((d, Seconds(s as u32)))
        })
        .collect()
}

/// A local wall time in `tz` as an instant (CLI `--at` without an offset).
/// Ambiguous (fall back): the earlier instant. Non-existent (spring
/// forward): moved forward past the gap, e.g. 03:30 → 04:30.
pub fn local_instant(tz: Tz, t: NaiveDateTime) -> DateTime<FixedOffset> {
    if let Some(x) = tz.from_local_datetime(&t).earliest() {
        return x.fixed_offset();
    }
    // In a gap: read t with the offset in force just before it.
    let before = t - chrono::Duration::hours(3);
    let off = tz
        .from_local_datetime(&before)
        .earliest()
        .map_or(0, |x| x.offset().fix().local_minus_utc());
    let utc = t.and_utc() - chrono::Duration::seconds(i64::from(off));
    utc.with_timezone(&tz).fixed_offset()
}

/// Converts the seconds of a search back to instants in the feed's zone.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Clock {
    /// Unix seconds of time 0 of the search.
    pub origin: i64,
    pub tz: Tz,
}

impl Clock {
    pub(crate) fn at(&self, secs: u32) -> DateTime<FixedOffset> {
        let t = DateTime::from_timestamp(self.origin + i64::from(secs), 0).unwrap_or_default();
        t.with_timezone(&self.tz).fixed_offset()
    }
}
