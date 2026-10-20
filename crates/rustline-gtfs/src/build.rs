//! Turning a [`RawFeed`] into a [`Timetable`].
//!
//! Steps, in order:
//! 1. stops, routes and services get a typed index (sorted by GTFS id so the
//!    result is the same on every run);
//! 2. each trip is checked, its missing times interpolated, and frequency
//!    trips expanded;
//! 3. trips with the same stop sequence are grouped into patterns, sorted by
//!    first departure, and split where one trip overtakes another.

use std::collections::{BTreeSet, HashMap};

use chrono::NaiveDate as Date;
use gtfs_structures::{ExactTimes, Exception, LocationType, RouteType};
use rustline_model::{LatLon, PatternIdx, RouteIdx, Seconds, ServiceIdx, StopIdx, TripIdx};

use crate::report::ReportBuilder;
use crate::timetable::{
    Pattern, Route, Service, Stop, StopKind, StopTime, Timetable, Transfer, Trip,
};
use crate::{GtfsError, IssueKind, RawFeed, ValidationReport};

/// Most trips one frequencies.txt row may generate (spec §7.2). A 1 min
/// headway over 48 h is 2 880; a 1 s headway over 48 h (172 800) is refused.
pub const MAX_FREQUENCY_TRIPS_PER_ROW: u64 = 10_000;
/// Most trips all frequencies.txt rows together may generate.
pub const MAX_FREQUENCY_TRIPS: u64 = 200_000;
/// Most stop times all generated trips together may have.
pub const MAX_FREQUENCY_STOP_TIMES: u64 = 5_000_000;

/// Normalises, validates and builds patterns for a feed.
///
/// Trips with errors are left out and listed in the report; the timetable
/// contains only trips that can be routed safely.
pub fn build_timetable(feed: &RawFeed) -> Result<(Timetable, ValidationReport), GtfsError> {
    let g = &feed.gtfs;
    let timezone = feed.timezone()?;
    // Rows the reader already dropped or padded (M7).
    let mut report = feed.issues.clone();

    let (stops, stop_idx) = build_stops(feed, &mut report);
    let (routes, route_idx) = build_routes(feed);
    let (services, service_idx) = build_services(feed, &mut report);
    let (valid_from, valid_to) = validity(&services).ok_or(GtfsError::NoServiceDates)?;

    // Trips, sorted by id for deterministic output.
    let mut trip_ids: Vec<&String> = g.trips.keys().collect();
    trip_ids.sort();
    let mut trips = Vec::new();
    let mut stop_times = Vec::new();
    // Trips and stop times generated from frequencies.txt so far (capped).
    let (mut gen_trips, mut gen_times) = (0u64, 0u64);
    for id in trip_ids {
        let raw = &g.trips[id];
        let Some(&route) = route_idx.get(raw.route_id.as_str()) else {
            report.add(IssueKind::UnknownRoute, 1, id.as_str());
            continue;
        };
        let Some(&service) = service_idx.get(raw.service_id.as_str()) else {
            report.add(IssueKind::UnknownService, 1, id.as_str());
            continue;
        };
        let Some(times) = trip_times(raw, &stop_idx, &mut report) else {
            continue;
        };
        let headsign = raw.trip_headsign.clone().unwrap_or_default();
        let mut push = |trip_id: String, times: &[StopTime], approximate: bool| {
            let start = stop_times.len() as u32;
            stop_times.extend_from_slice(times);
            trips.push(Trip {
                id: trip_id,
                route,
                service,
                headsign: headsign.clone(),
                stop_times: start..stop_times.len() as u32,
                pattern: PatternIdx(u32::MAX), // set by build_patterns
                approximate,
                block_id: raw.block_id.clone().unwrap_or_default(),
            });
        };
        if raw.frequencies.is_empty() {
            push(id.clone(), &times, false);
            continue;
        }
        // frequencies.txt: the trip's own times are a template, shifted so
        // that it starts at every headway within each window.
        let base = i64::from(times[0].departure.0);
        for f in &raw.frequencies {
            if f.headway_secs == 0 || f.end_time <= f.start_time {
                report.add(IssueKind::InvalidFrequency, 1, id.as_str());
                continue;
            }
            let approximate = !matches!(f.exact_times, Some(ExactTimes::ScheduleBased));
            // Count before generating anything, so a crafted row costs no memory.
            let rows = u64::from(f.end_time - f.start_time).div_ceil(u64::from(f.headway_secs));
            // Every shifted time must fit in u32: check the earliest (first
            // start) and the latest (last start) once, before generating.
            // ≤ end_time - 1, so no overflow.
            let last_start =
                f.start_time + (f.end_time - f.start_time - 1) / f.headway_secs * f.headway_secs;
            let lo = times
                .iter()
                .map(|st| st.arrival.0.min(st.departure.0))
                .min();
            let hi = times
                .iter()
                .map(|st| st.arrival.0.max(st.departure.0))
                .max();
            let fits = |t: Option<u32>, start: i64| {
                t.is_some_and(|t| u32::try_from(i64::from(t) + start - base).is_ok())
            };
            if !fits(lo, i64::from(f.start_time)) || !fits(hi, i64::from(last_start)) {
                report.add(IssueKind::InvalidFrequency, 1, id.as_str());
                continue;
            }
            gen_trips += rows;
            gen_times += rows * times.len() as u64;
            if rows > MAX_FREQUENCY_TRIPS_PER_ROW
                || gen_trips > MAX_FREQUENCY_TRIPS
                || gen_times > MAX_FREQUENCY_STOP_TIMES
            {
                return Err(GtfsError::TooManyFrequencyTrips(format!(
                    "trip {id}: {rows} trips from one row ({gen_trips} in all, {gen_times} stop times); \
                     caps {MAX_FREQUENCY_TRIPS_PER_ROW} per row, {MAX_FREQUENCY_TRIPS} trips, \
                     {MAX_FREQUENCY_STOP_TIMES} stop times"
                )));
            }
            let mut n = 0;
            for start in (f.start_time..f.end_time).step_by(f.headway_secs as usize) {
                let shift = i64::from(start) - base;
                // Checked above; a time that still does not fit ends the row.
                let moved = |s: Seconds| u32::try_from(i64::from(s.0) + shift).ok().map(Seconds);
                let Some(shifted) = times
                    .iter()
                    .map(|st| {
                        Some(StopTime {
                            arrival: moved(st.arrival)?,
                            departure: moved(st.departure)?,
                            ..*st
                        })
                    })
                    .collect::<Option<Vec<StopTime>>>()
                else {
                    break;
                };
                push(format!("{id}@{}", Seconds(start)), &shifted, approximate);
                n += 1;
            }
            if approximate {
                report.add(IssueKind::ApproximateFrequencyTrips, n, id.as_str());
            }
        }
    }

    let transfers = build_transfers(feed, &stops, &stop_idx, &mut report);
    let mut tt = Timetable {
        stops,
        routes,
        trips,
        stop_times,
        services,
        patterns: Vec::new(),
        transfers,
        timezone,
        valid_from,
        valid_to,
    };
    build_patterns(&mut tt, &mut report);
    Ok((tt, report.finish()))
}

/// GTFS pickup/drop-off type as its number; unknown values count as regular (0).
fn pickup_drop_off(t: gtfs_structures::PickupDropOffType) -> u8 {
    use gtfs_structures::PickupDropOffType::*;
    match t {
        Regular | Unknown(_) => 0,
        NotAvailable => 1,
        ArrangeByPhone => 2,
        CoordinateWithDriver => 3,
    }
}

/// transfers.txt, read tolerantly (M5 decisions): rows whose both stops
/// exist and whose `transfer_type` is 0–3 (empty = 0), sorted by (from, to).
/// Skipped with a report warning: types 4/5 and unknown or unreadable types,
/// rows with trip or route columns (GTFS extension), unknown stops.
/// Type 0 ignores `min_transfer_time` (footpath time), type 1 is 0 s, type 2
/// without a time falls back to the footpath time (stored as type 0) with a
/// `MissingMinTransferTime` warning; an unparsable time ("x", negative, over
/// u32) counts as missing.
/// Several rows for one (from, to): the one with the longest given
/// `min_transfer_time` is kept (ties: the higher `transfer_type`, so "not
/// possible" wins), never promising a quicker change than the agency says;
/// the others are counted in the report.
fn build_transfers(
    feed: &RawFeed,
    stops: &[Stop],
    stop_idx: &HashMap<&str, StopIdx>,
    report: &mut ReportBuilder,
) -> Vec<Transfer> {
    let Some(text) = &feed.transfers else {
        return Vec::new();
    };
    let mut csv = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(text.as_bytes());
    let header: Vec<String> = match csv.headers() {
        Ok(h) => h
            .iter()
            .map(|x| x.trim_start_matches('\u{feff}').to_string())
            .collect(),
        Err(_) => return Vec::new(),
    };
    let col = |name: &str| header.iter().position(|h| h == name);
    let (c_from, c_to, c_kind, c_min) = (
        col("from_stop_id"),
        col("to_stop_id"),
        col("transfer_type"),
        col("min_transfer_time"),
    );
    let extension: Vec<usize> = ["from_trip_id", "to_trip_id", "from_route_id", "to_route_id"]
        .iter()
        .filter_map(|n| col(n))
        .collect();
    // (row, given min_transfer_time for the duplicate rule, from id, to id)
    let mut out: Vec<(Transfer, u32, String, String)> = Vec::new();
    for (line, rec) in csv.records().enumerate() {
        let example = format!("transfers.txt row {}", line + 2);
        let Ok(rec) = rec else {
            report.add(IssueKind::UnsupportedTransfer, 1, example);
            continue;
        };
        let get = |c: Option<usize>| c.and_then(|c| rec.get(c)).unwrap_or("");
        if extension
            .iter()
            .any(|&c| rec.get(c).is_some_and(|v| !v.is_empty()))
        {
            report.add(IssueKind::UnsupportedTransfer, 1, example);
            continue;
        }
        let kind: u8 = match get(c_kind) {
            "" | "0" => 0,
            "1" => 1,
            "2" => 2,
            "3" => 3,
            _ => {
                report.add(IssueKind::UnsupportedTransfer, 1, example);
                continue;
            }
        };
        let (from_id, to_id) = (get(c_from), get(c_to));
        let (Some(&from), Some(&to)) = (stop_idx.get(from_id), stop_idx.get(to_id)) else {
            report.add(
                IssueKind::UnknownTransferStop,
                1,
                format!("{from_id}→{to_id}"),
            );
            continue;
        };
        let given: Option<u32> = get(c_min).parse().ok();
        let walk = || walk_secs(stops[from.index()].pos, stops[to.index()].pos);
        let (kind, min_secs) = match (kind, given) {
            (0, _) => (0, walk()),
            (1, _) => (1, 0),
            (2, Some(t)) => (2, t),
            (2, None) => {
                report.add(
                    IssueKind::MissingMinTransferTime,
                    1,
                    format!("{from_id}→{to_id}"),
                );
                (0, walk())
            }
            (k, t) => (k, t.unwrap_or(0)),
        };
        // Types 0 and 1 ignore the given time, also for the duplicate rule.
        let rank = if matches!(kind, 2 | 3) { min_secs } else { 0 };
        let row = Transfer {
            from,
            to,
            kind,
            min_secs,
        };
        out.push((row, rank, from_id.to_string(), to_id.to_string()));
    }
    // Per (from, to), the row to keep first.
    out.sort_by_key(|r| {
        let t = r.0;
        (
            t.from,
            t.to,
            std::cmp::Reverse(r.1),
            std::cmp::Reverse(t.kind),
        )
    });
    for w in out.windows(2) {
        if (w[0].0.from, w[0].0.to) == (w[1].0.from, w[1].0.to) {
            report.add(
                IssueKind::DuplicateTransfer,
                1,
                format!("{}→{}", w[1].2, w[1].3),
            );
        }
    }
    let mut out: Vec<Transfer> = out.into_iter().map(|r| r.0).collect();
    out.dedup_by_key(|t| (t.from, t.to));
    out
}

/// Straight-line walk between two stops in seconds: metres × 1.3 (detour)
/// at 1.3 m/s, i.e. one second per straight metre (0 without coordinates).
fn walk_secs(a: Option<LatLon>, b: Option<LatLon>) -> u32 {
    let (Some(a), Some(b)) = (a, b) else { return 0 };
    let (la1, la2) = (a.lat.to_radians(), b.lat.to_radians());
    let h = ((la2 - la1) / 2.0).sin().powi(2)
        + la1.cos() * la2.cos() * ((b.lon - a.lon).to_radians() / 2.0).sin().powi(2);
    let m = 2.0 * 6_371_008.8 * h.sqrt().min(1.0).asin();
    m.ceil().min(f64::from(u32::MAX / 4)) as u32
}

/// Stops sorted by id, plus an id → index lookup used during the build.
fn build_stops<'a>(
    feed: &'a RawFeed,
    report: &mut ReportBuilder,
) -> (Vec<Stop>, HashMap<&'a str, StopIdx>) {
    let mut ids: Vec<&String> = feed.gtfs.stops.keys().collect();
    ids.sort();
    let lookup: HashMap<&str, StopIdx> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), StopIdx::from_usize(i)))
        .collect();
    let stops = ids
        .iter()
        .map(|id| {
            let s = &feed.gtfs.stops[*id];
            let pos = match (s.latitude, s.longitude) {
                (Some(lat), Some(lon)) => Some(LatLon::new(lat, lon)).filter(LatLon::is_valid),
                _ => None,
            };
            // GTFS only requires coordinates for stops, stations and entrances.
            let needs_pos = matches!(
                s.location_type,
                LocationType::StopPoint | LocationType::StopArea | LocationType::StationEntrance
            );
            if pos.is_none() && needs_pos {
                report.add(IssueKind::StopWithoutCoordinates, 1, id.as_str());
            }
            Stop {
                id: (*id).clone(),
                name: s.name.clone().unwrap_or_default(),
                pos,
                kind: match s.location_type {
                    LocationType::StopPoint => StopKind::Stop,
                    LocationType::StopArea => StopKind::Station,
                    _ => StopKind::Other,
                },
                parent: s
                    .parent_station
                    .as_deref()
                    .and_then(|p| lookup.get(p).copied()),
            }
        })
        .collect();
    (stops, lookup)
}

/// Routes sorted by id, plus an id → index lookup.
fn build_routes(feed: &RawFeed) -> (Vec<Route>, HashMap<&str, RouteIdx>) {
    let mut ids: Vec<&String> = feed.gtfs.routes.keys().collect();
    ids.sort();
    let lookup = ids
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), RouteIdx::from_usize(i)))
        .collect();
    let routes = ids
        .iter()
        .map(|id| {
            let r = &feed.gtfs.routes[*id];
            Route {
                id: (*id).clone(),
                short_name: r.short_name.clone().unwrap_or_default(),
                long_name: r.long_name.clone().unwrap_or_default(),
                route_type: route_type_code(r.route_type),
            }
        })
        .collect();
    (routes, lookup)
}

/// The GTFS integer for a route type.
///
/// The parser folds extended types into their family (e.g. 700–799 → bus),
/// so the exact extended code is not preserved.
fn route_type_code(t: RouteType) -> i16 {
    match t {
        RouteType::Tramway => 0,
        RouteType::Subway => 1,
        RouteType::Rail => 2,
        RouteType::Bus => 3,
        RouteType::Ferry => 4,
        RouteType::CableCar => 5,
        RouteType::Gondola => 6,
        RouteType::Funicular => 7,
        RouteType::Coach => 200,
        RouteType::Air => 1100,
        RouteType::Taxi => 1500,
        RouteType::Other(n) => n,
    }
}

/// Services from calendar.txt and calendar_dates.txt, sorted by id.
fn build_services<'a>(
    feed: &'a RawFeed,
    report: &mut ReportBuilder,
) -> (Vec<Service>, HashMap<&'a str, ServiceIdx>) {
    let g = &feed.gtfs;
    let ids: BTreeSet<&str> = g
        .calendar
        .keys()
        .chain(g.calendar_dates.keys())
        .map(String::as_str)
        .collect();
    let lookup = ids
        .iter()
        .enumerate()
        .map(|(i, id)| (*id, ServiceIdx::from_usize(i)))
        .collect();
    let services = ids
        .iter()
        .map(|id| {
            let (weekdays, date_range) = match g.calendar.get(*id) {
                Some(c) => (
                    [
                        c.monday,
                        c.tuesday,
                        c.wednesday,
                        c.thursday,
                        c.friday,
                        c.saturday,
                        c.sunday,
                    ],
                    // start_date > end_date: no weekday runs (M5 decision).
                    if c.start_date > c.end_date {
                        report.add(IssueKind::EmptyDateRange, 1, *id);
                        None
                    } else {
                        Some((c.start_date, c.end_date))
                    },
                ),
                None => ([false; 7], None),
            };
            let (mut added, mut removed) = (Vec::new(), Vec::new());
            for d in g.calendar_dates.get(*id).into_iter().flatten() {
                match d.exception_type {
                    Exception::Added => added.push(d.date),
                    Exception::Deleted => removed.push(d.date),
                }
            }
            added.sort();
            removed.sort();
            // Added and removed on one day: removed wins (it does not run).
            let both = added
                .iter()
                .filter(|d| removed.binary_search(d).is_ok())
                .count();
            if both > 0 {
                report.add(IssueKind::ConflictingCalendarDates, both, *id);
            }
            Service {
                id: (*id).to_owned(),
                weekdays,
                date_range,
                added,
                removed,
            }
        })
        .collect();
    (services, lookup)
}

/// First and last date on which any service can run.
fn validity(services: &[Service]) -> Option<(Date, Date)> {
    let dates = services.iter().flat_map(|s| {
        s.date_range
            .into_iter()
            .flat_map(|(a, b)| [a, b])
            .chain(s.added.iter().copied())
    });
    let (mut lo, mut hi) = (None::<Date>, None::<Date>);
    for d in dates {
        lo = Some(lo.map_or(d, |l| l.min(d)));
        hi = Some(hi.map_or(d, |h| h.max(d)));
    }
    lo.zip(hi)
}

/// Checks one trip and returns its stop times, or `None` if it must be dropped.
///
/// Missing arrival/departure are copied from each other; stops with neither
/// are interpolated linearly between the nearest timed stops.
fn trip_times(
    trip: &gtfs_structures::Trip,
    stop_idx: &HashMap<&str, StopIdx>,
    report: &mut ReportBuilder,
) -> Option<Vec<StopTime>> {
    let id = trip.id.as_str();
    if trip.stop_times.len() < 2 {
        report.add(IssueKind::TooFewStopTimes, 1, id);
        return None;
    }
    let mut raw: Vec<&gtfs_structures::StopTime> = trip.stop_times.iter().collect();
    raw.sort_by_key(|st| st.stop_sequence);
    if raw
        .windows(2)
        .any(|w| w[0].stop_sequence == w[1].stop_sequence)
    {
        report.add(IssueKind::DuplicateStopSequence, 1, id);
        return None;
    }

    // (arrival, departure) where known.
    let known: Vec<Option<(u32, u32)>> = raw
        .iter()
        .map(|st| match (st.arrival_time, st.departure_time) {
            (Some(a), Some(d)) => Some((a, d)),
            (Some(t), None) | (None, Some(t)) => Some((t, t)),
            (None, None) => None,
        })
        .collect();
    if known[0].is_none() || known[known.len() - 1].is_none() {
        report.add(IssueKind::MissingBoundaryTime, 1, id);
        return None;
    }

    let mut times = Vec::with_capacity(raw.len());
    let mut interpolated = 0;
    let mut prev = 0; // index of the last stop with a known time
    for (k, st) in raw.iter().enumerate() {
        let (arr, dep) = match known[k] {
            Some(t) => {
                prev = k;
                t
            }
            None => {
                // The last stop is known (checked above), so `next` exists.
                let next = (k + 1..known.len())
                    .find(|&j| known[j].is_some())
                    .unwrap_or(k);
                let from = i64::from(known[prev].unwrap().1);
                let to = i64::from(known[next].unwrap().0);
                let t = from + (to - from) * (k - prev) as i64 / (next - prev) as i64;
                interpolated += 1;
                (t.max(0) as u32, t.max(0) as u32)
            }
        };
        // The parser normally rejects unknown stop ids; report it if one slips through.
        let Some(&stop) = stop_idx.get(st.stop.id.as_str()) else {
            report.add(IssueKind::UnknownStop, 1, id);
            return None;
        };
        times.push(StopTime {
            stop,
            arrival: Seconds(arr),
            departure: Seconds(dep),
            pickup: pickup_drop_off(st.pickup_type),
            drop_off: pickup_drop_off(st.drop_off_type),
        });
    }
    if interpolated > 0 {
        report.add(IssueKind::InterpolatedTimes, interpolated, id);
    }

    let decreasing = times.windows(2).any(|w| w[1].arrival < w[0].departure)
        || times.iter().any(|t| t.departure < t.arrival);
    if decreasing {
        report.add(IssueKind::DecreasingTimes, 1, id);
        return None;
    }
    Some(times)
}

/// Groups trips by stop sequence, sorts them by first departure, and splits
/// patterns so that no trip overtakes another (RAPTOR needs this).
fn build_patterns(tt: &mut Timetable, report: &mut ReportBuilder) {
    // Stop sequence → trips, in first-seen order so the output is stable.
    let mut by_sequence: HashMap<Vec<StopIdx>, usize> = HashMap::new();
    let mut groups: Vec<(Vec<StopIdx>, Vec<TripIdx>)> = Vec::new();
    for i in 0..tt.trips.len() {
        let t = TripIdx::from_usize(i);
        let seq: Vec<StopIdx> = tt.trip_stop_times(t).iter().map(|st| st.stop).collect();
        let g = *by_sequence.entry(seq.clone()).or_insert_with(|| {
            groups.push((seq, Vec::new()));
            groups.len() - 1
        });
        groups[g].1.push(t);
    }

    for (stops, mut trips) in groups {
        trips.sort_by(|&a, &b| {
            (tt.first_departure(a), &tt.trips[a.index()].id)
                .cmp(&(tt.first_departure(b), &tt.trips[b.index()].id))
        });
        // Greedy split: put each trip in the first "lane" whose last trip it
        // doesn't overtake; open a new lane if there is none.
        let mut lanes: Vec<Vec<TripIdx>> = Vec::new();
        for t in trips {
            let lane = lanes
                .iter_mut()
                .find(|lane| !overtakes(tt, t, *lane.last().unwrap()));
            match lane {
                Some(lane) => lane.push(t),
                None => lanes.push(vec![t]),
            }
        }
        if lanes.len() > 1 {
            let example = tt.trips[lanes[1][0].index()].id.clone();
            report.add(IssueKind::OvertakingSplit, lanes.len() - 1, example);
        }
        for lane in lanes {
            let p = PatternIdx::from_usize(tt.patterns.len());
            for &t in &lane {
                tt.trips[t.index()].pattern = p;
            }
            tt.patterns.push(Pattern {
                stops: stops.clone(),
                trips: lane,
            });
        }
    }
}

/// True if `later` (which departs no earlier) arrives or departs at some stop
/// before `earlier` does.
fn overtakes(tt: &Timetable, later: TripIdx, earlier: TripIdx) -> bool {
    tt.trip_stop_times(later)
        .iter()
        .zip(tt.trip_stop_times(earlier))
        .any(|(l, e)| l.arrival < e.arrival || l.departure < e.departure)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn stop_time(stop_id: &str, seq: u32, time: u32) -> gtfs_structures::StopTime {
        let stop = gtfs_structures::Stop {
            id: stop_id.to_string(),
            ..Default::default()
        };
        gtfs_structures::StopTime {
            stop: Arc::new(stop),
            stop_sequence: seq,
            arrival_time: Some(time),
            departure_time: Some(time),
            ..Default::default()
        }
    }

    #[test]
    fn trip_with_unknown_stop_is_reported_and_dropped() {
        let trip = gtfs_structures::Trip {
            id: "T1".to_string(),
            stop_times: vec![stop_time("A", 1, 100), stop_time("NOPE", 2, 200)],
            ..Default::default()
        };
        let stop_idx: HashMap<&str, StopIdx> = HashMap::from([("A", StopIdx(0))]);
        let mut report = ReportBuilder::default();

        assert!(trip_times(&trip, &stop_idx, &mut report).is_none());
        let report = report.finish();
        assert_eq!(report.count(IssueKind::UnknownStop), 1);
    }
}
