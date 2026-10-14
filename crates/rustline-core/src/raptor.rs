//! RAPTOR, stop to stop (M3).
//!
//! Round k finds the earliest arrival at every stop with k rides. Round 0 is
//! the origin (plus its transfers.txt links); every later round scans the
//! patterns of the stops that improved in the round before, then follows
//! transfers.txt from the stops a ride just improved.
//!
//! Transfer rules (plan M3, decision 8):
//! - changing trips at one stop takes 0 s, or the stop's own transfers.txt
//!   row (`X → X`) when there is one (type 3 there: no change at that stop);
//! - changing to another stop only works through a transfers.txt row
//!   (types 0/1/2 with `min_transfer_time`, type 3 never). Sibling platforms
//!   of one station are not linked otherwise until M4 footpaths.
//! - `pickup_type` / `drop_off_type` 1 forbid boarding / getting off there.

use chrono::NaiveDate;
use rustline_model::{Journey, Leg, RouteIdx, Seconds, StopIdx, TripIdx};
use rustline_pack::{Pack, TimetableData};

/// Most transfers a query may use, whatever it asks for (so at most 9 rides).
pub const MAX_TRANSFERS: u8 = 8;

/// A journey with fewer rides hides one with more rides unless the latter
/// arrives at least this much earlier (plan M3 "5-minute rule").
pub const MORE_RIDES_MIN_GAIN_SECS: u32 = 300;

const NONE: u32 = u32::MAX;

/// Why a query could not be answered. "No connection" is not an error:
/// that is `Ok` with no journeys.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum QueryError {
    /// A stop index is not in the pack.
    #[error("unknown stop index {index} (the pack has {stops} stops)")]
    UnknownStop {
        /// The index asked for.
        index: u32,
        /// Number of stops in the pack.
        stops: usize,
    },
    /// The date is outside the pack's validity window.
    #[error("{date} is outside the timetable ({valid_from} to {valid_to})")]
    DateOutsideFeed {
        /// The date asked for.
        date: NaiveDate,
        /// First service day of the pack.
        valid_from: NaiveDate,
        /// Last service day of the pack.
        valid_to: NaiveDate,
    },
    /// The pack's arrays don't fit together (it did not pass `check()`).
    #[error("the pack is inconsistent: {0}")]
    BadPack(&'static str),
}

/// How a stop was reached by a ride in a round.
#[derive(Debug, Clone, Copy)]
struct RideLabel {
    /// Trip ridden, got off at pattern position `alight`.
    trip: u32,
    /// Boarded at pattern position `board`, with the boarding time coming
    /// from round `from_round`, arrival kind `from_walk`.
    board: u32,
    alight: u32,
    from_round: u8,
    from_walk: bool,
}

/// How a stop was reached without a ride in a round.
#[derive(Debug, Clone, Copy)]
enum WalkLabel {
    /// The query origin (round 0).
    Origin,
    /// One transfers.txt hop from stop `from`, reached by a ride in the same round.
    Walk { from: u32, secs: u32 },
}

/// Fastest journeys from `from` to `to` leaving at `dep` or later on service
/// day `date`, using at most `max_transfers` transfers (capped at
/// [`MAX_TRANSFERS`]).
///
/// Returns the journeys that are best in (arrival, rides), after the 5-minute
/// rule: a journey with more rides is only kept if it arrives at least
/// [`MORE_RIDES_MIN_GAIN_SECS`] earlier than the kept one with fewer rides.
/// Sorted by rides. `from == to` gives one journey without legs. A parent
/// station (`stop_kind` 1) as `from` or `to` stands for all its child
/// platforms, reached in 0 s; the legs then start/end at a platform.
pub fn raptor_stop_to_stop(
    pack: &Pack,
    from: StopIdx,
    to: StopIdx,
    date: NaiveDate,
    dep: Seconds,
    max_transfers: u8,
) -> Result<Vec<Journey>, QueryError> {
    let all = raptor_pareto(pack, from, to, date, dep, max_transfers)?;
    let mut kept: Vec<Journey> = Vec::with_capacity(all.len());
    for j in all {
        let better = kept
            .last()
            .is_none_or(|k| j.arr.0.saturating_add(MORE_RIDES_MIN_GAIN_SECS) <= k.arr.0);
        if better {
            kept.push(j);
        }
    }
    Ok(kept)
}

/// Like [`raptor_stop_to_stop`] but without the 5-minute rule: every journey
/// that is best in (arrival, rides). The earliest arrival is the last one.
pub fn raptor_pareto(
    pack: &Pack,
    from: StopIdx,
    to: StopIdx,
    date: NaiveDate,
    dep: Seconds,
    max_transfers: u8,
) -> Result<Vec<Journey>, QueryError> {
    let tt = pack.timetable();
    let n = tt.stop_count();
    for s in [from, to] {
        if s.index() >= n {
            return Err(QueryError::UnknownStop {
                index: s.0,
                stops: n,
            });
        }
    }
    let (valid_from, valid_to) = (pack.meta().valid_from, pack.meta().valid_to);
    if date < valid_from || date > valid_to {
        return Err(QueryError::DateOutsideFeed {
            date,
            valid_from,
            valid_to,
        });
    }
    let day = u32::try_from((date - valid_from).num_days()).unwrap_or(u32::MAX);
    // A parent station stands for all its platforms (0 s to reach them).
    let origins = platforms(pack, from);
    let targets = platforms(pack, to);
    if from == to || origins.iter().any(|s| targets.contains(s)) {
        return Ok(vec![Journey {
            legs: Vec::new(),
            dep,
            arr: dep,
            rides: 0,
        }]);
    }
    let rounds = usize::from(max_transfers.min(MAX_TRANSFERS)) + 1;
    let mut search = Search::new(pack, day, rounds);
    for &t in &targets {
        search.is_target[t as usize] = true;
    }
    search.run(&origins, dep.0)
}

/// The stop itself, or for a station (`stop_kind` 1) its child platforms.
fn platforms(pack: &Pack, stop: StopIdx) -> Vec<u32> {
    let tt = pack.timetable();
    if tt.stop_kind.get(stop.index()) != Some(&1) {
        return vec![stop.0];
    }
    let children: Vec<u32> = (0..tt.stop_count() as u32)
        .filter(|&s| tt.stop_parent.get(s as usize) == Some(&stop.0) && s != stop.0)
        .collect();
    if children.is_empty() {
        vec![stop.0]
    } else {
        children
    }
}

/// State of one query. Ride and walk arrivals are kept apart (standard
/// RAPTOR): only a ride arrival may start a transfers.txt hop, and only a
/// walk arrival (or the origin) can board without the stop's own change time.
struct Search<'a> {
    tt: &'a TimetableData,
    day: u32,
    rounds: usize,
    n: usize,
    /// Destination stops (one, or a station's platforms).
    is_target: Vec<bool>,
    /// Earliest arrival at any destination stop so far.
    best_target: u32,
    /// `ride[k * n + s]` / `walk[k * n + s]`: arrival at s in round k by a
    /// ride / a hop (only where round k improved it), with its label.
    ride: Vec<u32>,
    ride_label: Vec<Option<RideLabel>>,
    walk: Vec<u32>,
    walk_label: Vec<Option<WalkLabel>>,
    /// Earliest ride / walk arrival at each stop over all rounds so far.
    best_ride: Vec<u32>,
    best_walk: Vec<u32>,
    /// Earliest time one can board at each stop, the round and arrival kind
    /// it came from.
    ready: Vec<u32>,
    ready_round: Vec<u8>,
    ready_walk: Vec<bool>,
    /// Stops whose `ready` improved in the last round.
    marked: Vec<u32>,
    is_marked: Vec<bool>,
}

const BAD: QueryError = QueryError::BadPack("index out of range");

impl<'a> Search<'a> {
    fn new(pack: &'a Pack, day: u32, rounds: usize) -> Self {
        let tt = pack.timetable();
        let n = tt.stop_count();
        let cells = (rounds + 1) * n;
        Search {
            tt,
            day,
            rounds,
            n,
            is_target: vec![false; n],
            best_target: NONE,
            ride: vec![NONE; cells],
            ride_label: vec![None; cells],
            walk: vec![NONE; cells],
            walk_label: vec![None; cells],
            best_ride: vec![NONE; n],
            best_walk: vec![NONE; n],
            ready: vec![NONE; n],
            ready_round: vec![0; n],
            ready_walk: vec![false; n],
            marked: Vec::new(),
            is_marked: vec![false; n],
        }
    }

    fn run(mut self, origins: &[u32], dep: u32) -> Result<Vec<Journey>, QueryError> {
        for &o in origins {
            let o = o as usize;
            *self.walk.get_mut(o).ok_or(BAD)? = dep;
            self.walk_label[o] = Some(WalkLabel::Origin);
            self.best_walk[o] = dep;
        }
        // Hops from the origin, as if it had been reached by a ride.
        let mut start = Vec::new();
        for &o in origins {
            self.hop_from(0, o as usize, dep, &mut start)?;
        }
        start.extend_from_slice(origins);
        self.update_ready(0, &start)?;

        let mut journeys = Vec::new();
        // A transfers.txt hop straight from the origin to the destination.
        let mut best: Option<(u32, usize)> = None;
        for &s in &start {
            let s = s as usize;
            let w = self.walk[s];
            if self.is_target[s] && w != NONE && best.is_none_or(|b| (w, s) < b) {
                best = Some((w, s));
            }
        }
        if let Some((t, s)) = best {
            self.best_target = t;
            journeys.push(self.journey(0, s, true, dep)?);
        }
        for k in 1..=self.rounds {
            if self.marked.is_empty() {
                break;
            }
            let rode = self.scan_patterns(k)?;
            let mut improved = Vec::new();
            for &s in &rode {
                let at = self.ride[k * self.n + s as usize];
                self.hop_from(k, s as usize, at, &mut improved)?;
            }
            improved.extend_from_slice(&rode);
            self.update_ready(k, &improved)?;
            // The destination stop this round reached earliest (lowest index on ties).
            let mut best: Option<(u32, usize, bool)> = None;
            for &s in &improved {
                let s = s as usize;
                if !self.is_target[s] {
                    continue;
                }
                let (r, w) = (self.ride[k * self.n + s], self.walk[k * self.n + s]);
                for (t, by_walk) in [(r, false), (w, true)] {
                    if t < self.best_target && best.is_none_or(|b| (t, s) < (b.0, b.1)) {
                        best = Some((t, s, by_walk));
                    }
                }
            }
            if let Some((t, s, by_walk)) = best {
                self.best_target = t;
                journeys.push(self.journey(k, s, by_walk, dep)?);
            }
        }
        Ok(journeys)
    }

    /// Round k: ride every pattern through a marked stop. Returns the stops
    /// whose ride arrival improved.
    fn scan_patterns(&mut self, k: usize) -> Result<Vec<u32>, QueryError> {
        let tt = self.tt;
        // Each pattern once, from its first marked position.
        let mut patterns: Vec<(u32, usize)> = Vec::new();
        for &s in &self.marked {
            let range = csr(&tt.stop_patterns_start, s as usize);
            for &p in tt.stop_patterns.get(range).unwrap_or(&[]) {
                let stops = tt.pattern_stops(p as usize);
                if let Some(pos) = stops.iter().position(|&x| x == s) {
                    patterns.push((p, pos));
                }
            }
        }
        for &s in &self.marked {
            self.is_marked[s as usize] = false;
        }
        self.marked.clear();
        patterns.sort_unstable();
        patterns.dedup_by_key(|p| p.0); // keeps the smallest position

        let mut improved = Vec::new();
        let row = k * self.n;
        for (p, first) in patterns {
            let stops = tt.pattern_stops(p as usize);
            let trips = tt
                .pattern_trips
                .get(csr(&tt.pattern_trips_start, p as usize))
                .unwrap_or(&[]);
            // (position in `trips`, boarding position, round and kind of the boarding time)
            let mut on: Option<(usize, usize, u8, bool)> = None;
            for (i, &s) in stops.iter().enumerate().skip(first) {
                let s = s as usize;
                if s >= self.n {
                    return Err(BAD);
                }
                if let Some((t, board, from_round, from_walk)) = on {
                    let trip = *trips.get(t).ok_or(BAD)?;
                    let (a, _) = self.times(trip, i);
                    let bound = self.best_ride[s].min(self.best_target);
                    if a < bound && self.drop_off(trip, i) != 1 {
                        if self.ride[row + s] == NONE {
                            improved.push(s as u32);
                        }
                        self.ride[row + s] = a;
                        self.best_ride[s] = a;
                        self.ride_label[row + s] = Some(RideLabel {
                            trip,
                            board: board as u32,
                            alight: i as u32,
                            from_round,
                            from_walk,
                        });
                    }
                }
                // Board here, or switch to an earlier trip?
                let r = self.ready[s];
                if r == NONE || usize::from(self.ready_round[s]) >= k {
                    continue;
                }
                let limit = on.map_or(trips.len(), |(t, ..)| t);
                if let Some(t) = self.earliest_trip(trips, i, r, limit) {
                    on = Some((t, i, self.ready_round[s], self.ready_walk[s]));
                }
            }
        }
        Ok(improved)
    }

    /// First trip (position in `trips`, below `limit`) that leaves position
    /// `i` at `r` or later, runs today and lets people board there.
    fn earliest_trip(&self, trips: &[u32], i: usize, r: u32, limit: usize) -> Option<usize> {
        let trips = trips.get(..limit)?;
        // Trips of a pattern never overtake, so departures are sorted.
        let start = trips.partition_point(|&t| self.times(t, i).1 < r);
        (start..trips.len()).find(|&t| {
            let trip = trips[t] as usize;
            let service = self.tt.trip_service.get(trip).copied().unwrap_or(NONE);
            self.pickup(trips[t], i) != 1 && self.tt.service_runs(service as usize, self.day)
        })
    }

    /// One transfers.txt hop from stop `x`, which round k reached at `at` by
    /// a ride (or is the origin in round 0). Walk arrivals never hop again.
    fn hop_from(
        &mut self,
        k: usize,
        x: usize,
        at: u32,
        improved: &mut Vec<u32>,
    ) -> Result<(), QueryError> {
        let tt = self.tt;
        let row = k * self.n;
        for e in tt.transfers(x) {
            let (Some(&y), Some(&kind), Some(&secs)) = (
                tt.transfer_to.get(e),
                tt.transfer_type.get(e),
                tt.transfer_secs.get(e),
            ) else {
                return Err(BAD);
            };
            let y = y as usize;
            if y == x || kind == 3 {
                continue;
            }
            if y >= self.n {
                return Err(BAD);
            }
            let t = at.saturating_add(secs);
            if t < self.best_walk[y] && t < self.best_target {
                if self.walk[row + y] == NONE && self.ride[row + y] == NONE {
                    improved.push(y as u32);
                }
                self.walk[row + y] = t;
                self.best_walk[y] = t;
                self.walk_label[row + y] = Some(WalkLabel::Walk {
                    from: x as u32,
                    secs,
                });
            }
        }
        Ok(())
    }

    /// After round k: boarding times of the improved stops, and the marks
    /// for round k + 1.
    fn update_ready(&mut self, k: usize, stops: &[u32]) -> Result<(), QueryError> {
        let row = k * self.n;
        for &s in stops {
            let s = s as usize;
            let (r, w) = (*self.ride.get(row + s).ok_or(BAD)?, self.walk[row + s]);
            // A ride arrival boards after the stop's own change time.
            let by_ride = match (r, self.self_transfer(s)) {
                (NONE, _) | (_, None) => NONE,
                (r, Some(secs)) => r.saturating_add(secs),
            };
            let (t, by_walk) = if w <= by_ride {
                (w, true)
            } else {
                (by_ride, false)
            };
            if t < self.ready[s] {
                self.ready[s] = t;
                self.ready_round[s] = k as u8;
                self.ready_walk[s] = by_walk;
                if !self.is_marked[s] {
                    self.is_marked[s] = true;
                    self.marked.push(s as u32);
                }
            }
        }
        self.marked.sort_unstable();
        Ok(())
    }

    /// Time to change trips at stop s: its own transfers.txt row, else 0 s;
    /// `None` when that row says type 3 (not possible).
    fn self_transfer(&self, s: usize) -> Option<u32> {
        let tt = self.tt;
        for e in tt.transfers(s) {
            if tt.transfer_to.get(e) == Some(&(s as u32)) {
                return match tt.transfer_type.get(e) {
                    Some(3) => None,
                    _ => Some(tt.transfer_secs.get(e).copied().unwrap_or(0)),
                };
            }
        }
        Some(0)
    }

    /// Index of (trip, position) in `stop_pickup`/`stop_drop_off`, and
    /// `2 ×` that in `stop_times`.
    fn time_index(&self, trip: u32, i: usize) -> usize {
        let start = self
            .tt
            .trip_times_start
            .get(trip as usize)
            .copied()
            .unwrap_or(NONE);
        (start as usize / 2).saturating_add(i)
    }

    /// (arrival, departure) of a trip at a pattern position (`NONE` if missing).
    fn times(&self, trip: u32, i: usize) -> (u32, u32) {
        let k = self.time_index(trip, i).saturating_mul(2);
        let st = &self.tt.stop_times;
        match (st.get(k), st.get(k.saturating_add(1))) {
            (Some(&a), Some(&d)) => (a, d),
            _ => (NONE, NONE),
        }
    }

    fn pickup(&self, trip: u32, i: usize) -> u8 {
        self.tt.pickup_drop_off(self.time_index(trip, i)).0
    }

    fn drop_off(&self, trip: u32, i: usize) -> u8 {
        self.tt.pickup_drop_off(self.time_index(trip, i)).1
    }

    /// Walks the labels back from destination stop `target` in round k.
    fn journey(
        &self,
        k: usize,
        target: usize,
        by_walk: bool,
        dep: u32,
    ) -> Result<Journey, QueryError> {
        let tt = self.tt;
        let arr = if by_walk {
            self.walk[k * self.n + target]
        } else {
            self.ride[k * self.n + target]
        };
        let mut legs = Vec::new();
        let (mut s, mut round, mut walk) = (target, k, by_walk);
        let mut rides = 0u8;
        // Each round adds at most one ride and one hop.
        for _ in 0..2 * (self.rounds + 1) + 1 {
            let cell = round * self.n + s;
            if walk {
                match self.walk_label.get(cell).copied().flatten() {
                    Some(WalkLabel::Origin) => {
                        legs.reverse();
                        let first_dep = match legs.first() {
                            Some(Leg::Transit { dep, .. }) => *dep,
                            _ => Seconds(dep),
                        };
                        return Ok(Journey {
                            legs,
                            dep: first_dep,
                            arr: Seconds(arr),
                            rides,
                        });
                    }
                    Some(WalkLabel::Walk { from, secs }) => {
                        legs.push(Leg::Walk {
                            from: StopIdx(from),
                            to: StopIdx(s as u32),
                            secs,
                        });
                        s = from as usize;
                        // Round 0 hops start at the origin itself.
                        walk = round == 0;
                    }
                    None => return Err(QueryError::BadPack("broken journey labels")),
                }
            } else {
                let l = self
                    .ride_label
                    .get(cell)
                    .copied()
                    .flatten()
                    .ok_or(QueryError::BadPack("broken journey labels"))?;
                let stops = tt
                    .trip_pattern
                    .get(l.trip as usize)
                    .map_or(&[][..], |&p| tt.pattern_stops(p as usize));
                let b = *stops.get(l.board as usize).ok_or(BAD)?;
                legs.push(Leg::Transit {
                    route: RouteIdx(*tt.trip_route.get(l.trip as usize).ok_or(BAD)?),
                    trip: TripIdx(l.trip),
                    from: StopIdx(b),
                    to: StopIdx(s as u32),
                    dep: Seconds(self.times(l.trip, l.board as usize).1),
                    arr: Seconds(self.times(l.trip, l.alight as usize).0),
                });
                rides += 1;
                s = b as usize;
                round = usize::from(l.from_round);
                walk = l.from_walk;
            }
        }
        Err(QueryError::BadPack("journey labels loop"))
    }
}

/// `start[i]..start[i + 1]` as a range (empty if out of range).
fn csr(start: &[u32], i: usize) -> std::ops::Range<usize> {
    match (start.get(i), start.get(i.wrapping_add(1))) {
        (Some(&a), Some(&b)) if a <= b => a as usize..b as usize,
        _ => 0..0,
    }
}
