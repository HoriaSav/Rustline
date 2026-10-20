//! Door-to-door planning (M4): walk to a stop, ride, walk to the door.
//!
//! 1. Check the request ([`PlanRequest::check`]).
//! 2. Snap both points to the nearest street within [`SNAP_MAX_M`]
//!    (else [`PlanError::OutsideCoverage`]); without a street graph walks
//!    are straight lines × [`DETOUR_FACTOR`].
//! 3. Walk from the origin to the stops within `max_access_m` and from the
//!    stops within `max_access_m` to the destination.
//! 4. RAPTOR from all those stops, with footpaths between stops.
//! 5. Walking only, if it is at most [`WALK_ONLY_MAX_M`]: kept when it
//!    arrives no more than [`WALK_BONUS_SECS`] after the best ride.
//! 6. Best in (arrival, rides), earliest first, at most `max_results`.
//!
//! Time (M5): the query instant's local date D in the feed's zone is the
//! service date (outside the feed → [`PlanError::DateOutsideFeed`], no D − 1
//! rescue). RAPTOR runs in the frame of D − 1's "noon minus 12 h" and uses
//! the trips of D − 1, D and D + 1 that are inside the feed, each shifted by
//! the real distance between the days. Every output time is an instant with
//! the feed zone's offset at that moment.

use chrono::{DateTime, Days, FixedOffset, NaiveDate};
use rustline_model::LatLon;
use rustline_pack::Pack;

use crate::back::{latest_departures, Incoming};
use crate::raptor::{raptor_multi, Found, Hops};
use crate::street::{haversine_m, Snap, Streets, SEARCH_CAP_M};
use crate::time::{day_ref, Clock};
use crate::{QueryError, MAX_TRANSFERS, MORE_RIDES_MIN_GAIN_SECS};

/// A point farther than this from every street is outside the coverage
/// (inclusive: exactly 300 m still snaps). The straight walk to the street
/// counts as walking.
pub const SNAP_MAX_M: f64 = 300.0;
/// Without a street graph, walking distance = straight line × this.
pub const DETOUR_FACTOR: f64 = 1.3;
/// A walk-only itinerary is offered up to this distance.
pub const WALK_ONLY_MAX_M: f64 = 3_000.0;
/// Walking only is kept when it arrives at most this much after the best ride.
pub const WALK_BONUS_SECS: u32 = 180;
/// Largest `max_access_m` a request may ask for.
pub const MAX_ACCESS_M: u32 = 3_000;
/// Largest `max_transfer_m`: the pack's footpaths go up to this.
pub const MAX_TRANSFER_M: u32 = 500;
/// Fastest walking speed a request may ask for (m/s).
pub const MAX_WALK_SPEED_MPS: f64 = 3.0;
/// [`Warning::FeedExpiresSoon`] when at most this many days are left from
/// the query's service date to the feed's last day.
pub const EXPIRES_SOON_DAYS: i64 = 14;

/// When to travel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeSpec {
    /// Leave at this instant or later. Any offset works; the answer is in
    /// the feed's timezone. (A local wall time without an offset: see
    /// [`crate::local_instant`].)
    DepartAt(DateTime<FixedOffset>),
    /// Arrive at this instant or earlier, leaving as late as possible (M8).
    /// Same offset rules; the service date is the deadline's local date.
    ArriveBy(DateTime<FixedOffset>),
}

/// A door-to-door query. Limits are checked by [`crate::Engine::plan`];
/// the maxima are the `MAX_*` constants of this module.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanRequest {
    /// Where the trip starts.
    pub from: LatLon,
    /// Where it ends.
    pub to: LatLon,
    /// When.
    pub time: TimeSpec,
    /// Walking speed, metres per second: above 0, at most [`MAX_WALK_SPEED_MPS`].
    pub walk_speed_mps: f64,
    /// Longest walk to the first and from the last stop, metres (≤ [`MAX_ACCESS_M`]).
    pub max_access_m: u32,
    /// Longest walk between two stops when changing, metres (≤ [`MAX_TRANSFER_M`]).
    pub max_transfer_m: u32,
    /// Most transfers (capped at [`MAX_TRANSFERS`]).
    pub max_transfers: u8,
    /// Most itineraries to return (1–255).
    pub max_results: u8,
}

impl Default for PlanRequest {
    /// Spec §11.1 defaults: 1.3 m/s, 1 000 m access, 500 m transfers,
    /// 3 transfers, 5 results. `from`/`to` are (0, 0) and the time is
    /// 1970-01-01 00:00 UTC: set them.
    fn default() -> Self {
        PlanRequest {
            from: LatLon::new(0.0, 0.0),
            to: LatLon::new(0.0, 0.0),
            time: TimeSpec::DepartAt(DateTime::<FixedOffset>::default()),
            walk_speed_mps: 1.3,
            max_access_m: 1000,
            max_transfer_m: 500,
            max_transfers: 3,
            max_results: 5,
        }
    }
}

impl PlanRequest {
    /// `Err(text)` when a field is out of range.
    pub fn check(&self) -> Result<(), String> {
        for (name, p) in [("from", self.from), ("to", self.to)] {
            if !p.is_valid() {
                return Err(format!(
                    "{name} is not a valid coordinate: {}, {}",
                    p.lat, p.lon
                ));
            }
        }
        let v = self.walk_speed_mps;
        if !(v.is_finite() && v > 0.0 && v <= MAX_WALK_SPEED_MPS) {
            return Err(format!(
                "walk_speed_mps must be in (0, {MAX_WALK_SPEED_MPS}], got {v}"
            ));
        }
        if self.max_access_m > MAX_ACCESS_M {
            return Err(format!("max_access_m is at most {MAX_ACCESS_M}"));
        }
        if self.max_transfer_m > MAX_TRANSFER_M {
            return Err(format!("max_transfer_m is at most {MAX_TRANSFER_M}"));
        }
        if self.max_results == 0 {
            return Err("max_results must be at least 1".to_string());
        }
        Ok(())
    }
}

/// Why no itinerary could be given.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
    /// A request field is out of range (the text says which).
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    /// A point is farther than 300 m from every street of the pack.
    #[error("a point is outside the coverage of this pack (more than 300 m from any street)")]
    OutsideCoverage,
    /// Nothing gets there: no ride, and too far to walk.
    #[error("no route found")]
    NoRoute,
    /// The query's local date (in the feed's zone) is outside the pack's
    /// timetable (`valid_from..=valid_to`).
    #[error("the date is outside the timetable of this pack")]
    DateOutsideFeed,
    /// The pack's data does not fit together.
    #[error("internal error: {0}")]
    Internal(String),
}

/// Something the user should know about an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Warning {
    /// An itinerary uses this stop (GTFS id), which is more than 100 m from
    /// any street: the walk to it is a straight line.
    StopNotLinkedToStreets(String),
    /// The pack has no street graph: every walk is a straight line × 1.3.
    NoStreetGraph,
    /// The feed ends within 14 days of the query's service date.
    FeedExpiresSoon {
        /// The feed's last service day.
        valid_to: NaiveDate,
    },
    /// An itinerary rides a trip from frequencies.txt with `exact_times=0`:
    /// the times are an estimate from the headway.
    ApproximateFrequencyTrip,
}

/// A stop as itineraries show it.
#[derive(Debug, Clone, PartialEq)]
pub struct StopRef {
    /// GTFS `stop_id`.
    pub id: String,
    /// `stop_name`.
    pub name: String,
    /// Position, if the feed has one.
    pub pos: Option<LatLon>,
}

/// A line on the map.
#[derive(Debug, Clone, PartialEq)]
pub struct Polyline(Vec<LatLon>);

impl Polyline {
    /// Wraps points (first to last).
    pub fn new(points: Vec<LatLon>) -> Self {
        Polyline(points)
    }

    /// The points, first to last.
    pub fn points(&self) -> &[LatLon] {
        &self.0
    }
}

/// One part of an itinerary.
#[derive(Debug, Clone, PartialEq)]
pub enum Leg {
    /// Walking.
    Walk {
        /// Start time.
        start: DateTime<FixedOffset>,
        /// End time.
        end: DateTime<FixedOffset>,
        /// Metres walked.
        distance_m: u32,
        /// Stop left (`None`: the origin).
        from_stop: Option<StopRef>,
        /// Stop reached (`None`: the destination).
        to_stop: Option<StopRef>,
        /// The line walked: along the streets, or straight without a street graph.
        geometry: Option<Polyline>,
    },
    /// A ride on one trip.
    Transit {
        /// GTFS `trip_id`.
        trip_id: String,
        /// GTFS `route_id`.
        route_id: String,
        /// `route_short_name` (may be empty).
        route_name: String,
        /// `trip_headsign` (may be empty).
        headsign: String,
        /// Boarding stop.
        from_stop: StopRef,
        /// Alighting stop.
        to_stop: StopRef,
        /// Departure.
        board: DateTime<FixedOffset>,
        /// Arrival.
        alight: DateTime<FixedOffset>,
        /// The stops passed, as a line (no shapes.txt yet).
        geometry: Option<Polyline>,
    },
}

/// One way to make the trip.
#[derive(Debug, Clone, PartialEq)]
pub struct Itinerary {
    /// Legs in order.
    pub legs: Vec<Leg>,
    /// When to leave the origin.
    pub departure: DateTime<FixedOffset>,
    /// When it reaches the destination.
    pub arrival: DateTime<FixedOffset>,
    /// Changes between rides (rides − 1; 0 for walking only).
    pub transfers: u8,
    /// Metres walked in total.
    pub walk_m: u32,
}

/// The answer to a [`PlanRequest`].
#[derive(Debug, Clone, PartialEq)]
pub struct PlanResponse {
    /// Best first (earliest arrival, then fewer rides).
    pub itineraries: Vec<Itinerary>,
    /// Things to tell the user.
    pub warnings: Vec<Warning>,
}

/// A walk between a point and a stop, or between two points.
#[derive(Debug, Clone)]
struct WalkTo {
    secs: u32,
    metres: f64,
}

/// One end of the trip.
struct End {
    point: LatLon,
    snap: Option<Snap>,
}

pub(crate) fn plan(
    pack: &Pack,
    streets: Option<&Streets>,
    incoming: &Incoming,
    tz: chrono_tz::Tz,
    req: &PlanRequest,
) -> Result<PlanResponse, PlanError> {
    req.check().map_err(PlanError::InvalidRequest)?;
    let (at, arrive_by) = match req.time {
        TimeSpec::DepartAt(t) => (t, false),
        TimeSpec::ArriveBy(t) => (t, true),
    };
    let date = at.with_timezone(&tz).date_naive();
    let meta = pack.meta();
    if date < meta.valid_from || date > meta.valid_to {
        return Err(PlanError::DateOutsideFeed);
    }
    // The search frame starts at D − 1's reference; D − 1, D and D + 1 (those
    // inside the feed) are shifted into it by their real distance. Arrive-by
    // uses the same three service days of the deadline's date.
    let prev = date
        .checked_sub_days(Days::new(1))
        .ok_or(PlanError::DateOutsideFeed)?;
    let origin = day_ref(tz, prev);
    let mut days: Vec<(u32, u32)> = Vec::with_capacity(3);
    for k in 0..3 {
        let Some(d) = prev.checked_add_days(Days::new(k)) else {
            continue;
        };
        if d < meta.valid_from || d > meta.valid_to {
            continue;
        }
        let index = u32::try_from((d - meta.valid_from).num_days()).unwrap_or(u32::MAX);
        let shift = u32::try_from(day_ref(tz, d) - origin).unwrap_or(u32::MAX);
        days.push((index, shift));
    }
    let clock = Clock { origin, tz };
    let time = |secs: u32| clock.at(secs);
    // The query instant in the frame: when to leave, or (arrive-by) the deadline.
    let when = u32::try_from(at.timestamp() - origin)
        .map_err(|_| PlanError::Internal("query time outside its search frame".into()))?;
    let snap = |p: LatLon| -> Result<End, PlanError> {
        match streets {
            Some(s) => match s.snap(p, SNAP_MAX_M) {
                Some(sn) => Ok(End {
                    point: p,
                    snap: Some(sn),
                }),
                None => Err(PlanError::OutsideCoverage),
            },
            None => Ok(End {
                point: p,
                snap: None,
            }),
        }
    };
    let (from, to) = (snap(req.from)?, snap(req.to)?);
    let q = Query {
        pack,
        streets,
        speed: req.walk_speed_mps,
    };

    // Access and egress walks.
    let access = q.stops_near(&from, f64::from(req.max_access_m), false);
    let egress = q.stops_near(&to, f64::from(req.max_access_m), true);
    let hops = Hops {
        scale: DETOUR_FACTOR / req.walk_speed_mps,
        max_m: req.max_transfer_m,
    };
    let hops = if req.walk_speed_mps == 1.3 {
        Hops { scale: 1.0, ..hops }
    } else {
        hops
    };
    let limit = req.max_transfers.min(MAX_TRANSFERS);
    let error = |e: QueryError| match e {
        QueryError::DateOutsideFeed { .. } => PlanError::DateOutsideFeed,
        e => PlanError::Internal(e.to_string()),
    };
    // Forward: journeys with at least one ride leaving the door at `door`
    // or later, at most `limit` transfers.
    let forward = |door: u32, limit: u8| -> Result<Vec<Found>, PlanError> {
        let origins: Vec<(u32, u32)> = access
            .iter()
            .map(|(s, w)| (*s, door.saturating_add(w.secs)))
            .collect();
        let targets: Vec<(u32, u32)> = egress.iter().map(|(s, w)| (*s, w.secs)).collect();
        if origins.is_empty() || targets.is_empty() {
            return Ok(Vec::new());
        }
        let mut found =
            raptor_multi(pack, &days, limit, &origins, &targets, hops).map_err(error)?;
        // Walking from stop to stop is not a ride: the direct walk covers it.
        found.retain(|f| f.journey.rides > 0);
        Ok(found)
    };
    // Backward: the latest door departure per number of rides that still
    // reaches the door by `deadline` (plan M8 step 2).
    let backward = |deadline: u32, limit: u8| -> Result<Vec<(u8, u32)>, PlanError> {
        let targets: Vec<(u32, u32)> = egress
            .iter()
            .filter_map(|(s, w)| Some((*s, deadline.checked_sub(w.secs)?)))
            .collect();
        let origins: Vec<(u32, u32)> = access.iter().map(|(s, w)| (*s, w.secs)).collect();
        if origins.is_empty() || targets.is_empty() {
            return Ok(Vec::new());
        }
        latest_departures(pack, incoming, &days, limit, &targets, &origins, hops).map_err(error)
    };
    let access_secs = |s: u32| access.iter().find(|a| a.0 == s).map_or(0, |a| a.1.secs);
    // The earliest arrival depart-at would show (after its 5-minute rule).
    let best_of = |found: Vec<Found>| {
        five_minutes(found)
            .into_iter()
            .min_by_key(|f| (f.arrival, f.journey.rides))
    };

    let mut rides: Vec<Found> = Vec::new();
    if !arrive_by {
        rides = five_minutes(forward(when, limit)?);
        // Depart-late pass (spec §8.1 step 7): leave as late as still arrives
        // as early, with no more rides.
        for f in rides.iter_mut() {
            let lim = f.journey.rides.saturating_sub(1);
            let door = door_departure(f, access_secs(f.origin));
            let Some(&(_, d)) = backward(f.arrival, lim)?.last() else {
                continue;
            };
            if d > door {
                if let Some(g) = best_of(forward(d, lim)?) {
                    if g.arrival <= f.arrival && g.journey.rides <= f.journey.rides {
                        *f = g;
                    }
                }
            }
        }
    } else {
        // Latest departure per number of rides (plan M8 DoD: no later
        // departure within the transfer limit is missed, so no 5-minute rule).
        let picks = backward(when, limit)?;
        // Each itinerary is the forward search from its departure, so it also
        // arrives as early as it can (step 4); the latest may use any rides.
        let last = picks.len().saturating_sub(1);
        for (j, &(r, d)) in picks.iter().enumerate() {
            let lim = if j == last {
                limit
            } else {
                r.saturating_sub(1)
            };
            // Only journeys that meet the deadline (no 5-minute rule: it could
            // prefer one that arrives too late, review MF1), ranked as step 5:
            // latest departure, then fewer rides, then earliest arrival.
            let ok = forward(d, lim)?.into_iter().filter(|f| f.arrival <= when);
            let rank = |f: &Found| {
                let dep = door_departure(f, access_secs(f.origin));
                (std::cmp::Reverse(dep), f.journey.rides, f.arrival)
            };
            if let Some(g) = ok.min_by_key(rank) {
                rides.push(g);
            }
        }
    }
    let walk = q.direct(&from, &to);

    // (departure, arrival, rides, itinerary, rides an approximate frequency trip)
    let mut out: Vec<(u32, u32, u8, Itinerary, bool)> = Vec::new();
    let mut unlinked: Vec<u32> = Vec::new();
    if let Some((w, line)) = walk {
        // Depart-at: leave now; arrive-by: leave as late as still arrives by the deadline.
        let (dep, arr) = if arrive_by {
            (when.saturating_sub(w.secs), when)
        } else {
            (when, when.saturating_add(w.secs))
        };
        // Offered only when competitive (spec §9), mirrored for arrive-by.
        let competitive = if arrive_by {
            rides
                .iter()
                .map(|f| door_departure(f, access_secs(f.origin)))
                .max()
                .is_none_or(|b| dep.saturating_add(WALK_BONUS_SECS) >= b)
        } else {
            rides
                .iter()
                .map(|f| f.arrival)
                .min()
                .is_none_or(|b| arr <= b.saturating_add(WALK_BONUS_SECS))
        };
        if competitive {
            let leg = Leg::Walk {
                start: time(dep),
                end: time(arr),
                distance_m: w.metres.round() as u32,
                from_stop: None,
                to_stop: None,
                geometry: Some(Polyline(line)),
            };
            out.push((
                dep,
                arr,
                0,
                Itinerary {
                    legs: vec![leg],
                    departure: time(dep),
                    arrival: time(arr),
                    transfers: 0,
                    walk_m: w.metres.round() as u32,
                },
                false,
            ));
        }
    }
    for f in &rides {
        let acc = access.iter().find(|a| a.0 == f.origin).map(|a| a.1.clone());
        let egr = egress.iter().find(|e| e.0 == f.target).map(|e| e.1.clone());
        let (Some(acc), Some(egr)) = (acc, egr) else {
            return Err(PlanError::Internal(
                "journey without its access or egress".into(),
            ));
        };
        let shifts: Vec<u32> = days.iter().map(|d| d.1).collect();
        let (it, approx) = q.itinerary(&clock, &shifts, &from, &to, f, &acc, &egr, &mut unlinked);
        let dep = door_departure(f, acc.secs);
        out.push((dep, f.arrival, f.journey.rides, it, approx));
    }
    // Depart-at: best in (arrival, rides). Arrive-by (step 5): latest
    // departure, then fewer rides, less walking, earliest arrival. Drop
    // anything another itinerary matches or beats.
    if arrive_by {
        out.sort_by_key(|o| (std::cmp::Reverse(o.0), o.2, o.3.walk_m, o.1));
    } else {
        out.sort_by_key(|o| (o.1, o.2));
    }
    let mut kept: Vec<(Itinerary, bool)> = Vec::new();
    let mut fewest = u8::MAX;
    for (_, _, r, it, approx) in out {
        if r < fewest {
            fewest = r;
            kept.push((it, approx));
        }
    }
    kept.truncate(usize::from(req.max_results));
    if kept.is_empty() {
        return Err(PlanError::NoRoute);
    }
    let approximate = kept.iter().any(|k| k.1);
    let kept: Vec<Itinerary> = kept.into_iter().map(|k| k.0).collect();
    let mut warnings = Vec::new();
    if (meta.valid_to - date).num_days() <= EXPIRES_SOON_DAYS {
        warnings.push(Warning::FeedExpiresSoon {
            valid_to: meta.valid_to,
        });
    }
    if approximate {
        warnings.push(Warning::ApproximateFrequencyTrip);
    }
    if streets.is_none() {
        warnings.push(Warning::NoStreetGraph);
    } else {
        let tt = pack.timetable();
        let used: Vec<String> = kept
            .iter()
            .flat_map(|it| &it.legs)
            .filter_map(|l| match l {
                Leg::Transit {
                    from_stop, to_stop, ..
                } => Some([from_stop.id.clone(), to_stop.id.clone()]),
                Leg::Walk { .. } => None,
            })
            .flatten()
            .collect();
        unlinked.sort_unstable();
        unlinked.dedup();
        for s in unlinked {
            let id = tt.stop_ids.get(s as usize).cloned().unwrap_or_default();
            if used.contains(&id) {
                warnings.push(Warning::StopNotLinkedToStreets(id));
            }
        }
    }
    Ok(PlanResponse {
        itineraries: kept,
        warnings,
    })
}

/// The 5-minute rule: a journey with more rides is kept only when it arrives
/// at least 5 minutes earlier than every one with fewer rides.
fn five_minutes(mut found: Vec<Found>) -> Vec<Found> {
    found.sort_by_key(|f| (f.journey.rides, f.arrival));
    let mut kept: Vec<Found> = Vec::new();
    for f in found {
        let better = kept
            .last()
            .is_none_or(|k| f.arrival.saturating_add(MORE_RIDES_MIN_GAIN_SECS) <= k.arrival);
        if better {
            kept.push(f);
        }
    }
    kept
}

/// When a journey leaves the door: its first boarding minus the walks
/// before it (no waiting at the origin).
fn door_departure(f: &Found, access_secs: u32) -> u32 {
    let first_board = f
        .journey
        .legs
        .iter()
        .find_map(|l| match l {
            rustline_model::Leg::Transit { dep, .. } => Some(dep.0),
            _ => None,
        })
        .unwrap_or(f.journey.dep.0);
    let pre: u32 = f
        .journey
        .legs
        .iter()
        .take_while(|l| matches!(l, rustline_model::Leg::Walk { .. }))
        .map(|l| match l {
            rustline_model::Leg::Walk { secs, .. } => *secs,
            _ => 0,
        })
        .sum();
    first_board.saturating_sub(pre).saturating_sub(access_secs)
}

struct Query<'a> {
    pack: &'a Pack,
    streets: Option<&'a Streets>,
    speed: f64,
}

impl Query<'_> {
    fn secs(&self, cost: f64) -> u32 {
        (cost / self.speed).ceil().min(f64::from(u32::MAX / 4)) as u32
    }

    fn stop_pos(&self, s: usize) -> Option<LatLon> {
        let (lat, lon) = self.pack.timetable().stop_pos(s)?;
        Some(LatLon::new(lat, lon))
    }

    /// Stops within `max_m` of walking from `e` (or, `to_end`, to it), with the walk.
    fn stops_near(&self, e: &End, max_m: f64, to_end: bool) -> Vec<(u32, WalkTo)> {
        let mut v: Vec<(u32, WalkTo)> = Vec::new();
        if let (Some(st), Some(sn)) = (self.streets, &e.snap) {
            let seeds = if to_end { st.reach(sn) } else { st.leave(sn) };
            let tree = st.tree(&seeds, to_end, max_m);
            for (s, c, m) in st.stops_in(&tree, sn, to_end, max_m) {
                v.push((
                    s,
                    WalkTo {
                        secs: self.secs(c),
                        metres: m,
                    },
                ));
            }
        }
        // Stops without a street link: straight line × 1.3.
        let tt = self.pack.timetable();
        for s in 0..tt.stop_count() {
            if self.streets.is_some_and(|st| st.stop_snap(s).is_some()) {
                continue;
            }
            let Some(p) = self.stop_pos(s) else { continue };
            let m = haversine_m(e.point, p) * DETOUR_FACTOR;
            if m <= max_m {
                v.push((
                    s as u32,
                    WalkTo {
                        secs: self.secs(m),
                        metres: m,
                    },
                ));
            }
        }
        v.sort_by_key(|a| a.0);
        v
    }

    /// The walk-only trip, if it is at most [`WALK_ONLY_MAX_M`].
    fn direct(&self, a: &End, b: &End) -> Option<(WalkTo, Vec<LatLon>)> {
        let (w, line) = self.walk(
            a.point,
            a.snap.as_ref(),
            b.point,
            b.snap.as_ref(),
            WALK_ONLY_MAX_M,
        )?;
        Some((w, line))
    }

    /// Walk between two points (each snapped, or not linked), at most `max_m`.
    fn walk(
        &self,
        a: LatLon,
        sa: Option<&Snap>,
        b: LatLon,
        sb: Option<&Snap>,
        max_m: f64,
    ) -> Option<(WalkTo, Vec<LatLon>)> {
        // Already there: no walk to the street and back.
        if a == b {
            let w = WalkTo {
                secs: 0,
                metres: 0.0,
            };
            return Some((w, vec![a, b]));
        }
        if let (Some(st), Some(sa), Some(sb)) = (self.streets, sa, sb) {
            let w = st.walk(a, sa, b, sb, max_m.min(SEARCH_CAP_M))?;
            return Some((
                WalkTo {
                    secs: self.secs(w.cost),
                    metres: w.metres,
                },
                w.points,
            ));
        }
        let m = haversine_m(a, b) * DETOUR_FACTOR;
        (m <= max_m).then(|| {
            (
                WalkTo {
                    secs: self.secs(m),
                    metres: m,
                },
                vec![a, b],
            )
        })
    }

    fn stop_ref(&self, s: u32) -> StopRef {
        let tt = self.pack.timetable();
        let i = s as usize;
        StopRef {
            id: tt.stop_ids.get(i).cloned().unwrap_or_default(),
            name: tt.stop_names.get(i).cloned().unwrap_or_default(),
            pos: self.stop_pos(i),
        }
    }

    /// The line and length of the walk from stop x to stop y (a footpath or a
    /// transfers.txt link).
    fn stop_walk(&self, x: u32, y: u32) -> (u32, Option<Polyline>) {
        let (Some(a), Some(b)) = (self.stop_pos(x as usize), self.stop_pos(y as usize)) else {
            return (0, None);
        };
        let snap = |s: u32| self.streets.and_then(|st| st.stop_snap(s as usize));
        if let Some((w, line)) = self.walk(
            a,
            snap(x).as_ref(),
            b,
            snap(y).as_ref(),
            f64::from(MAX_TRANSFER_M) * 4.0,
        ) {
            return (w.metres.round() as u32, Some(Polyline(line)));
        }
        let m = haversine_m(a, b) * DETOUR_FACTOR;
        (m.round() as u32, Some(Polyline(vec![a, b])))
    }

    /// Builds the itinerary of a RAPTOR journey with its access and egress
    /// walks; true when it rides an approximate (headway-only) trip.
    /// `shifts`: the day shifts of the search (to find a ride's stop times).
    #[allow(clippy::too_many_arguments)]
    fn itinerary(
        &self,
        clock: &Clock,
        shifts: &[u32],
        from: &End,
        to: &End,
        f: &Found,
        acc: &WalkTo,
        egr: &WalkTo,
        unlinked: &mut Vec<u32>,
    ) -> (Itinerary, bool) {
        let tt = self.pack.timetable();
        let time = |secs: u32| clock.at(secs);
        let mut approximate = false;
        let linked = |s: u32| {
            self.streets
                .is_some_and(|st| st.stop_snap(s as usize).is_some())
        };
        for s in [f.origin, f.target] {
            if !linked(s) {
                unlinked.push(s);
            }
        }
        let line = |a: &End, s: u32, rev: bool| -> Option<Polyline> {
            let p = self.stop_pos(s as usize)?;
            let sn = self.streets.and_then(|st| st.stop_snap(s as usize));
            let w = if rev {
                self.walk(p, sn.as_ref(), a.point, a.snap.as_ref(), SEARCH_CAP_M)
            } else {
                self.walk(a.point, a.snap.as_ref(), p, sn.as_ref(), SEARCH_CAP_M)
            };
            Some(Polyline(w.map(|x| x.1).unwrap_or_else(|| vec![a.point, p])))
        };
        // Walks before the first ride end when it leaves: work back from there.
        let mut legs: Vec<Leg> = Vec::new();
        let first_board = f
            .journey
            .legs
            .iter()
            .find_map(|l| match l {
                rustline_model::Leg::Transit { dep, .. } => Some(dep.0),
                _ => None,
            })
            .unwrap_or(f.journey.dep.0);
        let pre: u32 = f
            .journey
            .legs
            .iter()
            .take_while(|l| matches!(l, rustline_model::Leg::Walk { .. }))
            .map(|l| match l {
                rustline_model::Leg::Walk { secs, .. } => *secs,
                _ => 0,
            })
            .sum();
        let mut t = first_board.saturating_sub(pre).saturating_sub(acc.secs);
        let departure = time(t);
        let mut walk_m = acc.metres.round() as u32;
        if acc.metres > 0.0 || acc.secs > 0 {
            legs.push(Leg::Walk {
                start: time(t),
                end: time(t + acc.secs),
                distance_m: acc.metres.round() as u32,
                from_stop: None,
                to_stop: Some(self.stop_ref(f.origin)),
                geometry: line(from, f.origin, false),
            });
        }
        t += acc.secs;
        let mut rides = 0u8;
        for l in &f.journey.legs {
            match *l {
                rustline_model::Leg::Walk {
                    from: x,
                    to: y,
                    secs,
                } => {
                    let (m, geometry) = self.stop_walk(x.0, y.0);
                    walk_m += m;
                    legs.push(Leg::Walk {
                        start: time(t),
                        end: time(t + secs),
                        distance_m: m,
                        from_stop: Some(self.stop_ref(x.0)),
                        to_stop: Some(self.stop_ref(y.0)),
                        geometry,
                    });
                    t += secs;
                }
                rustline_model::Leg::Transit {
                    route,
                    trip,
                    from: x,
                    to: y,
                    dep,
                    arr,
                } => {
                    rides += 1;
                    let i = trip.index();
                    approximate |= tt.trip_approximate.get(i) == Some(&1);
                    let pts: Vec<LatLon> = {
                        let times = tt.trip_times(i);
                        // The trip's own times are shifted by its day's shift.
                        let at = |t: u32, sh: u32| t.checked_add(sh);
                        let span = shifts.iter().find_map(|&sh| {
                            let a = times
                                .iter()
                                .position(|s| s.0 == x.0 && at(s.2, sh) == Some(dep.0))?;
                            let b = times
                                .iter()
                                .rposition(|s| s.0 == y.0 && at(s.1, sh) == Some(arr.0))?;
                            (a <= b).then_some((a, b))
                        });
                        match span {
                            Some((a, b)) => times[a..=b]
                                .iter()
                                .filter_map(|s| self.stop_pos(s.0 as usize))
                                .collect(),
                            None => Vec::new(),
                        }
                    };
                    legs.push(Leg::Transit {
                        trip_id: tt.trip_ids.get(i).cloned().unwrap_or_default(),
                        route_id: tt.route_ids.get(route.index()).cloned().unwrap_or_default(),
                        route_name: tt
                            .route_short_names
                            .get(route.index())
                            .cloned()
                            .unwrap_or_default(),
                        headsign: tt.trip_headsigns.get(i).cloned().unwrap_or_default(),
                        from_stop: self.stop_ref(x.0),
                        to_stop: self.stop_ref(y.0),
                        board: time(dep.0),
                        alight: time(arr.0),
                        geometry: (pts.len() >= 2).then_some(Polyline(pts)),
                    });
                    t = arr.0;
                }
            }
        }
        let t = f.journey.arr.0.max(t);
        if egr.metres > 0.0 || egr.secs > 0 {
            legs.push(Leg::Walk {
                start: time(t),
                end: time(t + egr.secs),
                distance_m: egr.metres.round() as u32,
                from_stop: Some(self.stop_ref(f.target)),
                to_stop: None,
                geometry: line(to, f.target, true),
            });
        }
        walk_m += egr.metres.round() as u32;
        let it = Itinerary {
            legs,
            departure,
            arrival: time(t + egr.secs),
            transfers: rides.saturating_sub(1),
            walk_m,
        };
        (it, approximate)
    }
}
