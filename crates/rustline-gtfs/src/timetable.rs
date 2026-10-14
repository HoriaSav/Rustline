//! Rustline's own timetable types.
//!
//! The rest of Rustline depends on these, never on the GTFS parser's types.
//! Everything is stored in plain `Vec`s and linked by typed indices
//! (`StopIdx`, `TripIdx`, …) instead of references, which keeps the borrow
//! checker happy and makes the data easy to write into a pack later (M2).

use std::ops::Range;

use chrono::NaiveDate as Date;
use rustline_model::{LatLon, PatternIdx, RouteIdx, Seconds, ServiceIdx, StopIdx, TripIdx};

/// What a row of stops.txt describes (`location_type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopKind {
    /// 0: a stop or platform where vehicles stop.
    Stop,
    /// 1: a station grouping several stops.
    Station,
    /// 2–4: entrance, generic node or boarding area.
    Other,
}

/// A stop, station or other location from stops.txt.
#[derive(Debug, Clone)]
pub struct Stop {
    /// GTFS `stop_id`.
    pub id: String,
    /// `stop_name` (may be empty).
    pub name: String,
    /// Coordinates, `None` when missing or invalid.
    pub pos: Option<LatLon>,
    /// `location_type`.
    pub kind: StopKind,
    /// `parent_station`, if it exists in the feed.
    pub parent: Option<StopIdx>,
}

/// A route (line) from routes.txt.
#[derive(Debug, Clone)]
pub struct Route {
    /// GTFS `route_id`.
    pub id: String,
    /// `route_short_name`, e.g. "4".
    pub short_name: String,
    /// `route_long_name`.
    pub long_name: String,
    /// `route_type` as the GTFS integer (0 = tram, 3 = bus, 200 = coach, …).
    pub route_type: i16,
}

/// One trip. Its stop times are `Timetable::stop_times[stop_times]`.
#[derive(Debug, Clone)]
pub struct Trip {
    /// GTFS `trip_id`; trips expanded from frequencies.txt get `"{id}@{start}"`.
    pub id: String,
    /// The route it belongs to.
    pub route: RouteIdx,
    /// The calendar that says on which days it runs.
    pub service: ServiceIdx,
    /// `trip_headsign` (may be empty).
    pub headsign: String,
    /// Where this trip's stop times are in `Timetable::stop_times`.
    pub stop_times: Range<u32>,
    /// The stop pattern this trip was grouped into.
    pub pattern: PatternIdx,
    /// True for trips made from a headway-only (`exact_times=0`) frequency.
    pub approximate: bool,
}

/// Arrival and departure of a trip at one stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StopTime {
    /// Which stop.
    pub stop: StopIdx,
    /// Arrival, seconds since service-day noon minus 12h.
    pub arrival: Seconds,
    /// Departure (≥ arrival).
    pub departure: Seconds,
}

/// A calendar: weekly pattern plus added/removed dates.
#[derive(Debug, Clone)]
pub struct Service {
    /// GTFS `service_id`.
    pub id: String,
    /// Runs on Monday … Sunday (index 0 = Monday).
    pub weekdays: [bool; 7],
    /// First and last day of the weekly pattern (`None` if only calendar_dates).
    pub date_range: Option<(Date, Date)>,
    /// calendar_dates `exception_type=1`.
    pub added: Vec<Date>,
    /// calendar_dates `exception_type=2`.
    pub removed: Vec<Date>,
}

/// Trips that visit exactly the same stops in the same order.
///
/// RAPTOR scans patterns, not routes. Trips are sorted by departure at the
/// first stop and never overtake each other inside one pattern.
#[derive(Debug, Clone)]
pub struct Pattern {
    /// The stop sequence.
    pub stops: Vec<StopIdx>,
    /// The trips, earliest first.
    pub trips: Vec<TripIdx>,
}

/// A normalised, validated timetable.
#[derive(Debug, Clone)]
pub struct Timetable {
    /// All locations from stops.txt (stops, stations, …), sorted by id.
    pub stops: Vec<Stop>,
    /// Routes, sorted by id.
    pub routes: Vec<Route>,
    /// Trips that passed validation (plus frequency expansions).
    pub trips: Vec<Trip>,
    /// Stop times of all trips, each trip a contiguous slice.
    pub stop_times: Vec<StopTime>,
    /// Calendars, sorted by id.
    pub services: Vec<Service>,
    /// Stop patterns; see [`Pattern`].
    pub patterns: Vec<Pattern>,
    /// IANA timezone from `agency_timezone`, e.g. "America/New_York".
    pub timezone: String,
    /// First day any service runs.
    pub valid_from: Date,
    /// Last day any service runs.
    pub valid_to: Date,
}

impl Timetable {
    /// The stop times of one trip.
    pub fn trip_stop_times(&self, trip: TripIdx) -> &[StopTime] {
        let r = &self.trips[trip.index()].stop_times;
        &self.stop_times[r.start as usize..r.end as usize]
    }

    /// Departure of a trip at its first stop.
    pub fn first_departure(&self, trip: TripIdx) -> Seconds {
        self.trip_stop_times(trip)[0].departure
    }
}
