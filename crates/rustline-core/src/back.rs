//! RAPTOR backwards (M8): the latest departures that still arrive in time.
//!
//! The mirror of [`crate::raptor`]: round 0 starts at the destination stops
//! with the latest time one may get there; round k rides every pattern
//! through a stop improved in round k − 1, backwards, on the *latest* trip
//! that reaches that stop by then, and records the departure at each earlier
//! stop; then follows walking links backwards (one hop, into stops a ride
//! just left). The same transfer rules hold, mirrored:
//! - getting off at a stop and boarding there takes the stop's own change
//!   time (transfers.txt `X → X`, type 3: not possible);
//! - a hop is a transfers.txt row (types 0/1/2, type 3 never; type 0 takes
//!   the pack footpath when there is one) or a pack footpath, and only comes
//!   after a ride (or from a destination stop, as in round 0);
//! - `pickup_type` / `drop_off_type` 1 forbid boarding / getting off.
//!
//! Only the latest departure per number of rides is returned: the caller
//! rebuilds each itinerary with a forward search from that departure (which
//! also gives the earliest arrival from it, plan M8 step 4).

use rustline_pack::{Pack, TimetableData};

use crate::raptor::{Hops, QueryError, MAX_TRANSFERS};

const BAD: QueryError = QueryError::BadPack("index out of range");
/// No label yet.
const UNSET: i64 = i64::MIN;
const NONE: u32 = u32::MAX;

/// Walking links by the stop they lead to: transfers.txt rows and pack
/// footpaths, as (from stop, row) in CSR form. Built once per engine.
pub(crate) struct Incoming {
    tr_start: Vec<u32>,
    tr: Vec<(u32, u32)>,
    fp_start: Vec<u32>,
    fp: Vec<(u32, u32)>,
}

impl Incoming {
    pub(crate) fn new(tt: &TimetableData) -> Incoming {
        let n = tt.stop_count();
        // hop_into's binary_search needs each stop's transfers.txt targets sorted.
        debug_assert!((0..n).all(|x| tt
            .transfer_to
            .get(tt.transfers(x))
            .is_none_or(|t| t.windows(2).all(|w| w[0] <= w[1]))));
        let mut tr: Vec<Vec<(u32, u32)>> = vec![Vec::new(); n];
        let mut fp: Vec<Vec<(u32, u32)>> = vec![Vec::new(); n];
        for x in 0..n {
            // range: stop and row indexes fit u32 (check() caps the arrays).
            for e in tt.transfers(x) {
                if let Some(l) = tt.transfer_to.get(e).and_then(|&y| tr.get_mut(y as usize)) {
                    l.push((x as u32, e as u32));
                }
            }
            for f in tt.footpaths(x) {
                if let Some(l) = tt.footpath_to.get(f).and_then(|&y| fp.get_mut(y as usize)) {
                    l.push((x as u32, f as u32));
                }
            }
        }
        let (tr_start, tr) = flatten(tr);
        let (fp_start, fp) = flatten(fp);
        Incoming {
            tr_start,
            tr,
            fp_start,
            fp,
        }
    }

    fn rows<'a>(start: &[u32], items: &'a [(u32, u32)], y: usize) -> &'a [(u32, u32)] {
        match (start.get(y), start.get(y + 1)) {
            // range: CSR offsets are u32, below items.len().
            (Some(&a), Some(&b)) if a <= b => items.get(a as usize..b as usize).unwrap_or(&[]),
            _ => &[],
        }
    }
}

fn flatten(lists: Vec<Vec<(u32, u32)>>) -> (Vec<u32>, Vec<(u32, u32)>) {
    let mut start = Vec::with_capacity(lists.len() + 1);
    let mut items = Vec::new();
    start.push(0);
    for l in lists {
        items.extend(l);
        // range: at most the number of rows, which fits u32.
        start.push(items.len() as u32);
    }
    (start, items)
}

/// The latest departure from the origin (its access walk subtracted) for
/// each number of rides that leaves later than with fewer rides: `(rides,
/// departure)`, fewer rides first. `targets`: (stop, latest arrival there,
/// i.e. the deadline minus its egress walk); `origins`: (stop, access secs).
/// Times are in the search frame of `days` (as for `raptor_multi`).
pub(crate) fn latest_departures(
    pack: &Pack,
    inc: &Incoming,
    days: &[(u32, u32)],
    max_transfers: u8,
    targets: &[(u32, u32)],
    origins: &[(u32, u32)],
    hops: Hops,
) -> Result<Vec<(u8, u32)>, QueryError> {
    let tt = pack.timetable();
    let n = tt.stop_count();
    let rounds = usize::from(max_transfers.min(MAX_TRANSFERS)) + 1;
    let mut s = Back {
        tt,
        inc,
        days: &days[..days.len().min(3)],
        n,
        hops,
        access: vec![NONE; n],
        best_origin: UNSET,
        ride: vec![UNSET; (rounds + 1) * n],
        walk: vec![UNSET; (rounds + 1) * n],
        best_ride: vec![UNSET; n],
        best_walk: vec![UNSET; n],
        need: vec![UNSET; n],
        need_round: vec![0; n],
        marked: Vec::new(),
        is_marked: vec![false; n],
    };
    for &(o, a) in origins {
        let cell = s.access.get_mut(o as usize).ok_or(BAD)?;
        *cell = (*cell).min(a);
    }
    let mut first: Vec<(u32, u32)> = targets.to_vec();
    first.sort_unstable();
    for &(t, at) in &first {
        let t = t as usize;
        let cell = s.walk.get_mut(t).ok_or(BAD)?;
        if i64::from(at) > *cell {
            *cell = i64::from(at);
            s.best_walk[t] = i64::from(at);
        }
    }
    // Hops into a destination stop, as if it had been left by a ride.
    let mut start: Vec<u32> = Vec::new();
    for &(t, _) in &first {
        let at = s.walk[t as usize];
        s.hop_into(0, t as usize, at, &mut start)?;
    }
    start.extend(first.iter().map(|t| t.0));
    start.sort_unstable();
    start.dedup();
    s.update_need(0, &start)?;
    let mut out = Vec::new();
    for k in 1..=rounds {
        if s.marked.is_empty() {
            break;
        }
        let rode = s.scan_patterns(k)?;
        let mut improved = Vec::new();
        for &y in &rode {
            let at = s.ride[k * n + y as usize];
            s.hop_into(k, y as usize, at, &mut improved)?;
        }
        improved.extend_from_slice(&rode);
        s.update_need(k, &improved)?;
        // The origin this round leaves latest from (lowest index on ties).
        let mut best: Option<i64> = None;
        for &o in &improved {
            let o = o as usize;
            let a = s.access[o];
            if a == NONE {
                continue;
            }
            // Boarding there, or a hop from there to a boarding (as forward).
            let at = s.ride[k * n + o].max(s.walk[k * n + o]);
            if at == UNSET {
                continue;
            }
            let door = at - i64::from(a);
            if door > s.best_origin && best.is_none_or(|b| door > b) {
                best = Some(door);
            }
        }
        if let Some(d) = best {
            s.best_origin = d;
            // A door departure is a label minus an access walk, inside the frame.
            debug_assert!(d >= 0, "departure before the search frame: {d}");
            // range: k ≤ MAX_TRANSFERS + 1.
            out.push((k as u8, u32::try_from(d).unwrap_or(0)));
        }
    }
    Ok(out)
}

struct Back<'a> {
    tt: &'a TimetableData,
    inc: &'a Incoming,
    days: &'a [(u32, u32)],
    n: usize,
    hops: Hops,
    /// Access walk of each origin stop (`NONE`: not an origin).
    access: Vec<u32>,
    /// Latest departure from the origin door so far.
    best_origin: i64,
    /// `ride[k * n + s]`: latest boarding at s with k rides to go;
    /// `walk[k * n + s]`: latest start of a hop from s (round 0: latest
    /// arrival at a destination stop).
    ride: Vec<i64>,
    walk: Vec<i64>,
    best_ride: Vec<i64>,
    best_walk: Vec<i64>,
    /// Latest arrival by a ride at each stop that still works, and its round.
    need: Vec<i64>,
    need_round: Vec<u8>,
    marked: Vec<u32>,
    is_marked: Vec<bool>,
}

impl Back<'_> {
    /// Round k: every pattern through a marked stop, backwards from its last
    /// marked position. Returns the stops whose boarding label improved.
    fn scan_patterns(&mut self, k: usize) -> Result<Vec<u32>, QueryError> {
        let tt = self.tt;
        let mut patterns: Vec<(u32, usize)> = Vec::new();
        for &s in &self.marked {
            let range = csr(&tt.stop_patterns_start, s as usize);
            for &p in tt.stop_patterns.get(range).unwrap_or(&[]) {
                let stops = tt.pattern_stops(p as usize);
                if let Some(pos) = stops.iter().rposition(|&x| x == s) {
                    patterns.push((p, pos));
                }
            }
        }
        for &s in &self.marked {
            self.is_marked[s as usize] = false;
        }
        self.marked.clear();
        patterns.sort_unstable_by_key(|&(p, pos)| (p, std::cmp::Reverse(pos)));
        patterns.dedup_by_key(|p| p.0); // keeps the largest position
        let mut improved = Vec::new();
        let row = k * self.n;
        for (p, last) in patterns {
            let stops = tt.pattern_stops(p as usize);
            let trips = tt
                .pattern_trips
                .get(csr(&tt.pattern_trips_start, p as usize))
                .unwrap_or(&[]);
            // (position in `trips`, day slot) of the trip we would stay on.
            let mut on: Option<(usize, u8)> = None;
            for i in (0..=last.min(stops.len().saturating_sub(1))).rev() {
                let s = *stops.get(i).ok_or(BAD)? as usize;
                if s >= self.n {
                    return Err(BAD);
                }
                if let Some((t, slot)) = on {
                    let trip = *trips.get(t).ok_or(BAD)?;
                    let d = self.shifted(trip, i, slot).1;
                    let bound = self.best_ride[s].max(self.best_origin);
                    if d != NONE && i64::from(d) > bound && self.pickup(trip, i) != 1 {
                        if self.ride[row + s] == UNSET {
                            improved.push(s as u32);
                        }
                        self.ride[row + s] = i64::from(d);
                        self.best_ride[s] = i64::from(d);
                    }
                }
                // Get off here, or switch to a later trip?
                let need = self.need[s];
                if need == UNSET || usize::from(self.need_round[s]) >= k {
                    continue;
                }
                let now = on.map(|(t, slot)| (self.shifted(trips[t], i, slot).0, slot, t));
                if let Some((a, slot, t)) = self.latest_trip(trips, i, need) {
                    let later = now.is_none_or(|(na, nslot, nt)| {
                        (a, std::cmp::Reverse(slot), std::cmp::Reverse(t))
                            > (na, std::cmp::Reverse(nslot), std::cmp::Reverse(nt))
                    });
                    if later {
                        on = Some((t, slot));
                    }
                }
            }
        }
        Ok(improved)
    }

    /// The latest trip that reaches position `i` by `need` (in the search
    /// frame), runs on its service day and lets people get off there:
    /// (arrival in the frame, day slot, position in `trips`).
    fn latest_trip(&self, trips: &[u32], i: usize, need: i64) -> Option<(u32, u8, usize)> {
        let mut best: Option<(u32, u8, usize)> = None;
        for (slot, &(day, shift)) in self.days.iter().enumerate() {
            // range: at most 3 slots.
            let slot = slot as u8;
            let local = need - i64::from(shift);
            if local < 0 {
                continue;
            }
            // Trips of a pattern never overtake, so arrivals are sorted.
            let end = trips.partition_point(|&t| i64::from(self.times(t, i).0) <= local);
            let found = (0..end).rev().find(|&t| {
                let trip = trips[t] as usize;
                let service = self.tt.trip_service.get(trip).copied().unwrap_or(NONE);
                self.drop_off(trips[t], i) != 1 && self.tt.service_runs(service as usize, day)
            });
            if let Some(t) = found {
                let a = self.shifted(trips[t], i, slot).0;
                if a != NONE
                    && i64::from(a) <= need
                    && best.is_none_or(|b| {
                        (a, std::cmp::Reverse(slot)) > (b.0, std::cmp::Reverse(b.1))
                    })
                {
                    best = Some((a, slot, t));
                }
            }
        }
        best
    }

    /// One hop backwards into stop `y`, boarded (or, round 0, reached) at
    /// `at`: every walking link x → y. Hop starts never hop again.
    fn hop_into(
        &mut self,
        k: usize,
        y: usize,
        at: i64,
        improved: &mut Vec<u32>,
    ) -> Result<(), QueryError> {
        let tt = self.tt;
        let row = k * self.n;
        for &(x, e) in Incoming::rows(&self.inc.tr_start, &self.inc.tr, y) {
            let (x, e) = (x as usize, e as usize);
            let (Some(&kind), Some(&secs)) = (tt.transfer_type.get(e), tt.transfer_secs.get(e))
            else {
                return Err(BAD);
            };
            if x == y || kind == 3 {
                continue;
            }
            if x >= self.n {
                return Err(BAD);
            }
            let secs = if kind == 0 {
                self.footpath_secs(x, y).unwrap_or(secs)
            } else {
                secs
            };
            self.walk_from(row, x, at - i64::from(secs), improved);
        }
        for &(x, f) in Incoming::rows(&self.inc.fp_start, &self.inc.fp, y) {
            let (x, f) = (x as usize, f as usize);
            let (Some(&secs), Some(&m)) = (tt.footpath_secs.get(f), tt.footpath_m.get(f)) else {
                return Err(BAD);
            };
            if x >= self.n {
                return Err(BAD);
            }
            // A transfers.txt row for the pair wins over the footpath.
            let listed = tt.transfer_to.get(tt.transfers(x)).unwrap_or(&[]);
            // range: y < n, a stop index (u32).
            if m > self.hops.max_m || x == y || listed.binary_search(&(y as u32)).is_ok() {
                continue;
            }
            let secs = self.hops.secs(secs);
            self.walk_from(row, x, at - i64::from(secs), improved);
        }
        Ok(())
    }

    fn walk_from(&mut self, row: usize, x: usize, t: i64, improved: &mut Vec<u32>) {
        if t > self.best_walk[x] && t > self.best_origin {
            if self.walk[row + x] == UNSET && self.ride[row + x] == UNSET {
                // range: x < n, a stop index (u32).
                improved.push(x as u32);
            }
            self.walk[row + x] = t;
            self.best_walk[x] = t;
        }
    }

    /// The pack footpath x → y at this query's speed, if there is one.
    fn footpath_secs(&self, x: usize, y: usize) -> Option<u32> {
        let tt = self.tt;
        tt.footpaths(x).find_map(|f| {
            // range: y < n, a stop index (u32).
            (tt.footpath_to.get(f) == Some(&(y as u32)))
                .then(|| tt.footpath_secs.get(f).map(|&s| self.hops.secs(s)))
                .flatten()
        })
    }

    /// After round k: the latest ride arrival that works at each improved
    /// stop (a hop start, or boarding after the stop's change time), and the
    /// marks for round k + 1.
    fn update_need(&mut self, k: usize, stops: &[u32]) -> Result<(), QueryError> {
        let row = k * self.n;
        for &s in stops {
            let s = s as usize;
            let (r, w) = (*self.ride.get(row + s).ok_or(BAD)?, self.walk[row + s]);
            let by_ride = match (r, self.self_transfer(s)) {
                (UNSET, _) | (_, None) => UNSET,
                (r, Some(secs)) => r - i64::from(secs),
            };
            let t = w.max(by_ride);
            if t > self.need[s] {
                self.need[s] = t;
                // range: k ≤ MAX_TRANSFERS + 1.
                self.need_round[s] = k as u8;
                if !self.is_marked[s] {
                    self.is_marked[s] = true;
                    // range: s < n, a stop index (u32).
                    self.marked.push(s as u32);
                }
            }
        }
        self.marked.sort_unstable();
        Ok(())
    }

    /// Change time at stop s (its own transfers.txt row, else 0 s; `None`
    /// for type 3), as in the forward search.
    fn self_transfer(&self, s: usize) -> Option<u32> {
        let tt = self.tt;
        for e in tt.transfers(s) {
            // range: s < n, a stop index (u32).
            if tt.transfer_to.get(e) == Some(&(s as u32)) {
                return match tt.transfer_type.get(e) {
                    Some(3) => None,
                    _ => Some(tt.transfer_secs.get(e).copied().unwrap_or(0)),
                };
            }
        }
        Some(0)
    }

    fn time_index(&self, trip: u32, i: usize) -> usize {
        let start = self
            .tt
            .trip_times_start
            .get(trip as usize)
            .copied()
            .unwrap_or(NONE);
        (start as usize / 2).saturating_add(i)
    }

    fn times(&self, trip: u32, i: usize) -> (u32, u32) {
        let k = self.time_index(trip, i).saturating_mul(2);
        let st = &self.tt.stop_times;
        match (st.get(k), st.get(k.saturating_add(1))) {
            (Some(&a), Some(&d)) => (a, d),
            _ => (NONE, NONE),
        }
    }

    fn shifted(&self, trip: u32, i: usize, slot: u8) -> (u32, u32) {
        let shift = self.days.get(usize::from(slot)).map_or(0, |d| d.1);
        let (a, d) = self.times(trip, i);
        let up = |t: u32| {
            if t == NONE {
                NONE
            } else {
                t.checked_add(shift).unwrap_or(NONE)
            }
        };
        (up(a), up(d))
    }

    fn pickup(&self, trip: u32, i: usize) -> u8 {
        self.tt.pickup_drop_off(self.time_index(trip, i)).0
    }

    fn drop_off(&self, trip: u32, i: usize) -> u8 {
        self.tt.pickup_drop_off(self.time_index(trip, i)).1
    }
}

fn csr(start: &[u32], i: usize) -> std::ops::Range<usize> {
    match (start.get(i), start.get(i.wrapping_add(1))) {
        // range: CSR offsets are u32.
        (Some(&a), Some(&b)) if a <= b => a as usize..b as usize,
        _ => 0..0,
    }
}
