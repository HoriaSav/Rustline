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
use crate::timetable::{Pattern, Route, Service, Stop, StopKind, StopTime, Timetable, Trip};
use crate::{GtfsError, IssueKind, RawFeed, ValidationReport};

/// Normalises, validates and builds patterns for a feed.
///
/// Trips with errors are left out and listed in the report; the timetable
/// contains only trips that can be routed safely.
pub fn build_timetable(feed: &RawFeed) -> Result<(Timetable, ValidationReport), GtfsError> {
    let g = &feed.gtfs;
    let timezone = feed.timezone()?;
    let mut report = ReportBuilder::default();

    let (stops, stop_idx) = build_stops(feed, &mut report);
    let (routes, route_idx) = build_routes(feed);
    let (services, service_idx) = build_services(feed);
    let (valid_from, valid_to) = validity(&services).ok_or(GtfsError::NoServiceDates)?;

    // Trips, sorted by id for deterministic output.
    let mut trip_ids: Vec<&String> = g.trips.keys().collect();
    trip_ids.sort();
    let mut trips = Vec::new();
    let mut stop_times = Vec::new();
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
            let mut n = 0;
            for start in (f.start_time..f.end_time).step_by(f.headway_secs as usize) {
                let shift = i64::from(start) - base;
                let moved = |s: Seconds| Seconds((i64::from(s.0) + shift).max(0) as u32);
                let shifted: Vec<StopTime> = times
                    .iter()
                    .map(|st| StopTime {
                        arrival: moved(st.arrival),
                        departure: moved(st.departure),
                        ..*st
                    })
                    .collect();
                push(format!("{id}@{}", Seconds(start)), &shifted, approximate);
                n += 1;
            }
            if approximate {
                report.add(IssueKind::ApproximateFrequencyTrips, n, id.as_str());
            }
        }
    }

    let mut tt = Timetable {
        stops,
        routes,
        trips,
        stop_times,
        services,
        patterns: Vec::new(),
        timezone,
        valid_from,
        valid_to,
    };
    build_patterns(&mut tt, &mut report);
    Ok((tt, report.finish()))
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
fn build_services(feed: &RawFeed) -> (Vec<Service>, HashMap<&str, ServiceIdx>) {
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
                    Some((c.start_date, c.end_date)),
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
        let stop = *stop_idx.get(st.stop.id.as_str())?; // the parser already checked references
        times.push(StopTime {
            stop,
            arrival: Seconds(arr),
            departure: Seconds(dep),
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
