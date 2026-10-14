//! Turning a [`Timetable`] into the flat arrays saved in a pack (M2).
//!
//! Lives here, not in `rustline-pack`, so the phone-side pack crate never
//! depends on the GTFS parser.

use chrono::{Datelike, NaiveDate as Date};
use rustline_pack::PackMeta;
use rustline_pack::{TimetableData, NO_COORD};

use crate::{GtfsError, Service, StopKind, Timetable};

/// Copies a timetable into flat, pack-ready arrays, or explains why it doesn't fit.
pub fn try_timetable_data(tt: &Timetable) -> Result<TimetableData, GtfsError> {
    let mut data = TimetableData::default();

    for s in &tt.stops {
        data.stop_ids.push(s.id.clone());
        data.stop_names.push(s.name.clone());
        data.stop_lat
            .push(s.pos.map_or(NO_COORD, |p| p.lat.to_bits()));
        data.stop_lon
            .push(s.pos.map_or(NO_COORD, |p| p.lon.to_bits()));
        data.stop_kind.push(match s.kind {
            StopKind::Stop => 0,
            StopKind::Station => 1,
            StopKind::Other => 2,
        });
        data.stop_parent.push(s.parent.map_or(u32::MAX, |p| p.0));
    }

    for r in &tt.routes {
        data.route_ids.push(r.id.clone());
        data.route_short_names.push(r.short_name.clone());
        data.route_long_names.push(r.long_name.clone());
        // GTFS route types are never negative; a bad value becomes 0xFFFF.
        data.route_types
            .push(u16::try_from(r.route_type).unwrap_or(u16::MAX));
    }

    for t in &tt.trips {
        data.trip_ids.push(t.id.clone());
        data.trip_route.push(t.route.0);
        data.trip_service.push(t.service.0);
        data.trip_headsigns.push(t.headsign.clone());
        data.trip_approximate.push(u8::from(t.approximate));
        data.trip_pattern.push(t.pattern.0);
    }

    // Patterns, and their trips' times next to each other.
    data.trip_times_start = vec![0; tt.trips.len()];
    let mut stop_patterns: Vec<Vec<u32>> = vec![Vec::new(); tt.stops.len()];
    for (p, pattern) in tt.patterns.iter().enumerate() {
        data.pattern_stops_start.push(len_u32(&data.pattern_stops)?);
        data.pattern_trips_start.push(len_u32(&data.pattern_trips)?);
        for s in &pattern.stops {
            data.pattern_stops.push(s.0);
            let list = &mut stop_patterns[s.index()];
            let p = u32::try_from(p).map_err(|_| too_large("patterns"))?;
            if !list.contains(&p) {
                list.push(p);
            }
        }
        for &trip in &pattern.trips {
            data.pattern_trips.push(trip.0);
            data.trip_times_start[trip.index()] = len_u32(&data.stop_times)?;
            for st in tt.trip_stop_times(trip) {
                data.stop_times.push(st.arrival.0);
                data.stop_times.push(st.departure.0);
            }
        }
    }
    data.pattern_stops_start.push(len_u32(&data.pattern_stops)?);
    data.pattern_trips_start.push(len_u32(&data.pattern_trips)?);

    for list in &stop_patterns {
        data.stop_patterns_start.push(len_u32(&data.stop_patterns)?);
        data.stop_patterns.extend_from_slice(list);
    }
    data.stop_patterns_start.push(len_u32(&data.stop_patterns)?);

    // One bit per (service, day of the validity window).
    let days = (tt.valid_to - tt.valid_from).num_days() + 1;
    let days = u32::try_from(days).map_err(|_| too_large("validity window (days)"))?;
    data.service_words = days.div_ceil(64);
    for s in &tt.services {
        data.service_ids.push(s.id.clone());
        let mut words = vec![0u64; data.service_words as usize];
        for d in 0..days {
            let date = tt.valid_from + chrono::Days::new(u64::from(d));
            if runs_on(s, date) {
                words[(d / 64) as usize] |= 1 << (d % 64);
            }
        }
        data.service_bits.extend_from_slice(&words);
    }
    Ok(data)
}

/// True when a service runs on `date` (calendar.txt plus calendar_dates.txt).
fn runs_on(s: &Service, date: Date) -> bool {
    if s.removed.contains(&date) {
        return false;
    }
    if s.added.contains(&date) {
        return true;
    }
    match s.date_range {
        Some((from, to)) => {
            from <= date && date <= to && s.weekdays[date.weekday().num_days_from_monday() as usize]
        }
        None => false,
    }
}

/// Header fields for a pack of this timetable.
///
/// `feed_sha256` is the SHA-256 of the feed file; `built_at_unix` should be
/// fixed (e.g. `SOURCE_DATE_EPOCH`) when the pack must be reproducible.
pub fn pack_meta(
    tt: &Timetable,
    feed_sha256: [u8; 32],
    feed_id: &str,
    built_at_unix: i64,
) -> PackMeta {
    PackMeta {
        format_version: rustline_pack::FORMAT_VERSION,
        feed_sha256,
        osm_sha256: [0; 32],
        built_at_unix,
        valid_from: tt.valid_from,
        valid_to: tt.valid_to,
        timezone: tt.timezone.clone(),
        feed_id: feed_id.to_string(),
    }
}

/// Length of an array as a pack offset.
fn len_u32<T>(v: &[T]) -> Result<u32, GtfsError> {
    u32::try_from(v.len()).map_err(|_| too_large("offsets"))
}

fn too_large(what: &str) -> GtfsError {
    GtfsError::TooLargeForPack(what.to_string())
}
