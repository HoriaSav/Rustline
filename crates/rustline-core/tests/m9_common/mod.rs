//! M9 tests-first (Tester, 2026-10-23): helpers and the one adapter (`api9`) for the
//! rRAPTOR tests (`m9_*.rs`). Reuses the M8 helpers (`m8_common`, which reuses M5).
//! Builder: adapt only `api9` when names differ (the `rustline_m9` gate and the
//! `#[ignore]` lines are gone since M9 landed).
//!
//! Assumed (plan M9 step 4): `PlanRequest::window_secs: u32` (0 = M8 behaviour);
//! `Itinerary::tags: Vec<Tag>` with `Tag::{Fastest, FewestTransfers, LeavesLatest}`;
//! public caps `MAX_WINDOW_SECS` (≤ 2 h), `MAX_RANGE_DEPARTURES` (256),
//! `MAX_RESULTS` (20) and `MAX_TRANSFERS` (8, already public).
#![allow(dead_code, unused_imports, unexpected_cfgs)]

#[path = "../m8_common/mod.rs"]
pub mod m8;

pub use m8::*;

use chrono::{DateTime, Duration, FixedOffset, NaiveDate};

/// Same generator as `m8_oracle::random_feed` (copied so both stay independent):
/// 10 stops A..J ≥ 111 km apart (no walking between them), 6 lines, trips
/// 04:00–27:30, one daily and one weekday service, March–October 2031.
pub fn random_feed(seed: u64) -> Feed {
    fn gtfs(secs: u32) -> String {
        format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
    }
    let mut r = Rng(seed);
    let letters: Vec<String> = ('A'..='J').map(|c| c.to_string()).collect();
    let mut f = Feed::daily(BUCHAREST, "20310301", "20311031")
        .service("W", "1111100", "20310301", "20311031")
        .exception("S", "20310329", 2)
        .exception("S", "20311025", 2);
    for line in 0..6u32 {
        let n = 3 + r.below(3) as usize;
        let mut stops: Vec<&str> = Vec::new();
        while stops.len() < n {
            let s = letters[r.below(10) as usize].as_str();
            if !stops.contains(&s) {
                stops.push(s);
            }
        }
        let hops: Vec<u32> = (1..n).map(|_| 300 + r.below(1500) as u32).collect();
        let headway = 600 + r.below(3000) as u32;
        let svc = if line.is_multiple_of(3) { "W" } else { "S" };
        let mut t = hms(4, 0, 0) + r.below(1800) as u32;
        let mut k = 0;
        while t < hms(27, 30, 0) {
            let mut calls = vec![(gtfs(t), stops[0])];
            let mut at = t;
            for (i, h) in hops.iter().enumerate() {
                at += h;
                calls.push((gtfs(at), stops[i + 1]));
            }
            let c: Vec<(&str, &str)> = calls.iter().map(|(a, b)| (a.as_str(), *b)).collect();
            f = f.trip(svc, &format!("l{line}t{k}"), &c);
            t += headway;
            k += 1;
        }
    }
    f
}

pub fn env_n(var: &str, default: usize) -> usize {
    std::env::var(var)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

/// One ride between consecutive calls: `(dep, arr, from, to, trip run id)`.
pub type Conn = (
    DateTime<FixedOffset>,
    DateTime<FixedOffset>,
    usize,
    usize,
    usize,
);

/// Every elementary connection of every trip running on `days` (sorted by departure).
pub fn connections(p: &Pack, zone: &str, days: &[NaiveDate]) -> Vec<Conn> {
    let mut out = Vec::new();
    let mut run = 0;
    for t in 0..p.trip_ids.len() {
        let pat = p.trip_pattern[t] as usize;
        let (s0, s1) = (
            p.pattern_stops_start[pat] as usize,
            p.pattern_stops_start[pat + 1] as usize,
        );
        let base = p.trip_times_start[t] as usize;
        let sid = p.service_ids[p.trip_service[t] as usize].clone();
        let st = &p.pattern_stops[s0..s1];
        for &d in days {
            run += 1;
            if !runs(p, &sid, d) {
                continue;
            }
            for i in 0..st.len() - 1 {
                let dep = p.stop_times[base + 2 * i + 1];
                let arr = p.stop_times[base + 2 * (i + 1)];
                out.push((
                    service_instant(zone, d, dep),
                    service_instant(zone, d, arr),
                    st[i] as usize,
                    st[i + 1] as usize,
                    run,
                ));
            }
        }
    }
    out.sort();
    out
}

/// Change time at each stop from a transfers.txt body (header included), read
/// independently of the engine, per plan M5 decisions / M9 decision 8: no X→X row
/// = 0 s; type 1 = 0 s; type 2 = `min_transfer_time`; type 0, or type 2 without a
/// time = the walk time (0 m at one stop → 0 s); type 3 = no change at that stop
/// (`None`). Several X→X rows: the longest given time wins, ties → the higher type.
pub fn self_changes(transfers: &str, p: &Pack) -> Vec<Option<u32>> {
    // Per stop: (comparison key (given time, type), resolved change time).
    type Row = ((u32, u8), Option<u32>);
    let mut best: Vec<Option<Row>> = vec![None; p.stop_count()];
    for line in transfers.lines().skip(1) {
        let c: Vec<&str> = line.split(',').map(str::trim).collect();
        if c.len() < 3 || c[0] != c[1] {
            continue;
        }
        let Some(i) = p.stop_ids.iter().position(|x| x == c[0]) else {
            continue;
        };
        let kind = if c[2].is_empty() {
            0
        } else {
            match c[2].parse::<u8>() {
                Ok(k) => k,
                Err(_) => continue,
            }
        };
        let given = c.get(3).and_then(|x| x.parse::<u32>().ok());
        let secs = match (kind, given) {
            (1, _) | (0, _) | (2, None) => Some(0), // walk time at one stop = 0 s
            (2, Some(g)) => Some(g),
            (3, _) => None,
            _ => continue,
        };
        let key = (given.unwrap_or(0), kind);
        if best[i].is_none_or(|(k, _)| key > k) {
            best[i] = Some((key, secs));
        }
    }
    best.into_iter()
        .map(|b| b.map_or(Some(0), |(_, s)| s))
        .collect()
}

/// Brute-force per-k CSA (the M9 oracle; independent of the engine): earliest arrival
/// at `b` leaving `a` at or after `t` with at most `k` rides, for k = 1..=max_rides.
/// Changing trips at stop s needs `change[s]` seconds (None = not possible there);
/// the first ride boards at the origin with no change time.
/// Returns the Pareto front `(arrival, transfers)` (transfers = rides − 1).
pub fn oracle_front(
    conns: &[Conn],
    change: &[Option<u32>],
    a: usize,
    b: usize,
    t: DateTime<FixedOffset>,
    max_rides: usize,
) -> Vec<(DateTime<FixedOffset>, u8)> {
    let n_stops = change.len();
    let far = t + Duration::days(10);
    // Earliest arrival by ride at each stop with ≤ k−1 rides (none before round 1).
    let mut prev = vec![far; n_stops];
    let mut front = Vec::new();
    let mut best = far;
    for k in 1..=max_rides {
        let mut cur = prev.clone();
        let mut on = std::collections::HashSet::new();
        for c in conns.iter().filter(|c| c.0 >= t) {
            let from_origin = c.2 == a;
            let by_change = change[c.2]
                .is_some_and(|w| prev[c.2] < far && prev[c.2] + Duration::seconds(w as i64) <= c.0);
            if from_origin || by_change {
                on.insert(c.4);
            }
            if on.contains(&c.4) && c.1 < cur[c.3] {
                cur[c.3] = c.1;
            }
        }
        if cur[b] < best {
            best = cur[b];
            front.push((best, (k - 1) as u8));
        }
        prev = cur;
    }
    front
}

/// `x` dominates `y` on (arrival, transfers): not worse in both, better in one.
pub fn dominates(x: (DateTime<FixedOffset>, u8), y: (DateTime<FixedOffset>, u8)) -> bool {
    x.0 <= y.0 && x.1 <= y.1 && (x.0 < y.0 || x.1 < y.1)
}

pub fn key(it: &It) -> (DateTime<FixedOffset>, u8) {
    (it.arr, it.rides.saturating_sub(1) as u8)
}

/// One line per itinerary, for golden files and byte-equal comparisons.
pub fn render(r: &Result<Vec<It>, rustline_core::PlanError>) -> String {
    match r {
        Err(e) => format!("err {e:?}\n"),
        Ok(v) => v
            .iter()
            .map(|it| {
                format!(
                    "{} -> {} rides {} [{}]\n",
                    it.dep.to_rfc3339(),
                    it.arr.to_rfc3339(),
                    it.rides,
                    it.trips().join(",")
                )
            })
            .collect(),
    }
}

// ---- the M9 interfaces (gated) ---------------------------------------------------

/// Adapter over the M9 API. Builder: adapt only this module when names differ.
pub mod api9 {
    use super::*;
    use rustline_core::{Engine, PlanError, PlanRequest};
    pub use rustline_core::{MAX_RANGE_DEPARTURES, MAX_RESULTS, MAX_TRANSFERS, MAX_WINDOW_SECS};

    pub fn window(mut r: PlanRequest, secs: u32) -> PlanRequest {
        r.window_secs = secs;
        r
    }

    /// Itineraries plus their tags as lower-case names (`fastest`, `fewest_transfers`,
    /// `leaves_latest`).
    pub fn plan_tagged(e: &Engine, r: &PlanRequest) -> Result<Vec<(It, Vec<String>)>, PlanError> {
        let v = m8::m5::api::plan(e, r)?;
        let raw = e.plan(r)?;
        Ok(v.into_iter()
            .zip(raw.itineraries.iter())
            .map(|(it, x)| {
                let tags = x
                    .tags
                    .iter()
                    .map(|t| match t {
                        rustline_core::Tag::Fastest => "fastest".to_string(),
                        rustline_core::Tag::FewestTransfers => "fewest_transfers".to_string(),
                        rustline_core::Tag::LeavesLatest => "leaves_latest".to_string(),
                    })
                    .collect();
                (it, tags)
            })
            .collect())
    }

    pub fn is_invalid(r: &Result<Vec<It>, PlanError>) -> bool {
        matches!(r, Err(PlanError::InvalidRequest(_)))
    }
}
