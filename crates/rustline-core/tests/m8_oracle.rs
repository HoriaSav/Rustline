//! M8 tests-first (Tester, 2026-10-20): the arrive-by oracle (plan M8 DoD 1–3).
//! For random (origin stop, destination stop, deadline T): arrive-by's best
//! itinerary arrives by T and leaves exactly at the latest departure from which the
//! depart-at engine (M5, already checked against brute force) still arrives by T,
//! scanning every departure event at the origin on D-1, D and D+1. Plus symmetry
//! (depart-at at that departure arrives by T) and the depart-late properties.
//! Feeds: a random synthetic fixture (Bucharest, DST months), and the shared feeds
//! fixtures/feeds/brasov-latest.zip and mbta.zip (no OSM; `skip:` when missing).
//! Ungated in M8 (Builder removed `cfg rustline_m8` and the "M8 not implemented" ignores).
//! Run: `cargo test --locked --release -p rustline-core --test m8_oracle -- --nocapture`
//! `RUSTLINE_M8_QUERIES` (default 1000 fixture/Brașov, MBTA a third of it) and
//! `RUSTLINE_M8_SEEDS` (default 3).
#![allow(unexpected_cfgs)]

mod m8_common;
use chrono::{DateTime, Duration, FixedOffset, NaiveTime};
use m8_common::api8::*;
use m8_common::*;

fn env_n(var: &str, default: usize) -> usize {
    std::env::var(var)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

fn gtfs(secs: u32) -> String {
    format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

/// Random network: 10 stops A..J, 6 lines of 3–5 stops, trips 04:00–27:30 with
/// random headways and run times, one service daily (with two days removed) and
/// one weekdays-only, across March and October 2031 (both DST changes).
fn random_feed(seed: u64) -> Feed {
    let mut r = Rng(seed);
    let letters: Vec<String> = ('A'..='J').map(|c| c.to_string()).collect();
    let mut f = Feed::daily(BUCHAREST, "20310301", "20311031")
        .service("W", "1111100", "20310301", "20311031")
        .exception("S", "20310329", 2)
        .exception("S", "20311025", 2);
    for line in 0..6 {
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
        let svc = if line % 3 == 0 { "W" } else { "S" };
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

struct Stats {
    checked: usize,
    no_route: usize,
    skipped: usize,
    hidden: usize,
}

/// One oracle run over `n` random queries on pack `p` (zone `zone`).
fn run_oracle(
    p: &Pack,
    e: &rustline_core::Engine,
    zone: &str,
    seed: u64,
    n: usize,
    days: &[chrono::NaiveDate],
) -> Stats {
    let mut r = Rng(seed);
    let with_pos: Vec<usize> = (0..p.stop_count())
        .filter(|&s| p.stop_pos(s).is_some())
        .collect();
    let mut st = Stats {
        checked: 0,
        no_route: 0,
        skipped: 0,
        hidden: 0,
    };
    for q in 0..n {
        let a = with_pos[r.below(with_pos.len() as u64) as usize];
        let b = with_pos[r.below(with_pos.len() as u64) as usize];
        let (pa, pb) = (pos(p, a), pos(p, b));
        // Walks are modelled (the oracle uses the depart-at engine door to door), so
        // only identical stops are skipped.
        if a == b {
            st.skipped += 1;
            continue;
        }
        let day = days[r.below(days.len() as u64) as usize];
        let wall = day.and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap())
            + Duration::seconds(r.below(86_400) as i64);
        let Some(deadline) = local(zone, wall) else {
            st.skipped += 1;
            continue;
        };
        let want = oracle_latest_leave(e, zone, pa, pb, deadline);
        let got = plan(e, &door_req(pa, pb, arrive_by(deadline)));
        let ctx = format!("seed {seed} q{q} {a}->{b} by {deadline}");
        let day_ago = deadline - Duration::hours(24);
        match (want, &got) {
            (None, Err(rustline_core::PlanError::NoRoute)) => st.no_route += 1,
            (None, Ok(its)) if its.iter().all(|i| i.dep < day_ago) => {
                // Out of scope of the oracle (decision 7): only journeys leaving more
                // than 24 h before the deadline; still checked against the timetable.
                for it in its {
                    assert!(
                        it.arr <= deadline && journey_is_real(p, zone, it),
                        "{ctx}: {it:?}"
                    );
                }
                st.skipped += 1;
            }
            (None, other) => panic!("{ctx}: oracle none, got {other:?}"),
            (Some((_, fwd)), _)
                if fwd
                    .iter()
                    .all(|i| uses_day_before_window(p, zone, i, deadline)) =>
            {
                // Out of scope (decision 7): the only way rides a day before D − 1.
                st.skipped += 1;
            }
            (Some((w, fwd)), Ok(its)) => {
                assert!(!its.is_empty());
                // Every answer is real (timetable only) and arrives by the deadline.
                for it in its {
                    assert!(
                        it.arr <= deadline,
                        "{ctx}: arrives {} after the deadline",
                        it.arr
                    );
                    assert!(
                        journey_is_real(p, zone, it),
                        "{ctx}: not in the timetable: {it:?}"
                    );
                }
                let best = its.iter().max_by_key(|i| i.dep).unwrap();
                let latest = best.dep;
                // No later departure is missed. The depart-at journey at w is real, so
                // arrive-by leaves at w or later, except where the mirrored M3
                // "5-minute rule" hides it: w's journey has more rides than every
                // arrive-by answer and leaves less than 300 s later.
                let fewest_w = fwd.iter().map(|i| i.rides).min().unwrap_or(0);
                let hidden = its.iter().all(|i| i.rides < fewest_w)
                    && w - latest
                        < Duration::seconds(rustline_core::MORE_RIDES_MIN_GAIN_SECS as i64);
                assert!(
                    latest + Duration::seconds(1) > w || hidden,
                    "{ctx}: leaves {latest}, but depart-at at {w} arrives by the deadline: {fwd:?}; got {its:?}"
                );
                // Symmetry (DoD), under the same rule: depart-at at that departure
                // arrives by the deadline, or offers fewer rides arriving < 300 s later
                // (then it hid the arrive-by journey, which is real, see above).
                let (fa, fits) = depart_at_door(e, pa, pb, latest).expect("depart-at finds it");
                let gain = Duration::seconds(rustline_core::MORE_RIDES_MIN_GAIN_SECS as i64);
                assert!(
                    fa <= deadline
                        || (fa < deadline + gain && fits.iter().all(|i| i.rides < best.rides)),
                    "{ctx}: symmetry: depart-at {latest} arrives {fa}: {fits:?}; arrive-by {best:?}"
                );
                if fa > deadline {
                    st.hidden += 1;
                }
                // Leaving later than w (depart-at hides journeys with more rides that
                // gain < 5 min, so its earliest arrival can be later than the true one)
                // is fine: the journey was checked against the timetable above.
                st.checked += 1;
            }
            (Some((w, _)), Err(x)) => panic!("{ctx}: oracle {w}, got {x:?}"),
        }
    }
    st
}

fn seeds() -> Vec<u64> {
    (0..env_n("RUSTLINE_M8_SEEDS", 3) as u64)
        .map(|i| 0x6d38 + i * 7919)
        .collect()
}

#[test]
fn oracle_on_random_fixtures_across_dst() {
    let n = env_n("RUSTLINE_M8_QUERIES", 1000);
    let days: Vec<_> = (26..=31)
        .map(|d| date(2031, 3, d))
        .chain((23..=28).map(|d| date(2031, 10, d)))
        .collect();
    for seed in seeds() {
        let f = random_feed(seed);
        let (p, e) = (f.pack(), engine(f.pack()));
        let s = run_oracle(&p, &e, BUCHAREST, seed, n, &days);
        println!(
            "fixture seed {seed}: {} checked, {} no route, {} skipped, {} hidden by the 5-minute rule",
            s.checked, s.no_route, s.skipped, s.hidden
        );
        assert!(s.checked >= n / 3, "too few routed queries: {}", s.checked);
    }
}

fn shared(name: &str) -> Option<(Pack, rustline_core::Engine, String)> {
    let path = workspace(&format!("fixtures/feeds/{name}"));
    if !path.is_file() {
        println!("skip: {} missing", path.display());
        return None;
    }
    let p = pack_of(&path);
    let zone = p.meta().timezone.clone();
    let e = engine(pack_of(&path));
    Some((p, e, zone))
}

fn feed_days(p: &Pack) -> Vec<chrono::NaiveDate> {
    let (a, b) = (p.meta().valid_from, p.meta().valid_to);
    (1..=14)
        .map(|i| a + Duration::days(i))
        .filter(|d| *d < b)
        .collect()
}

#[test]
fn oracle_on_brasov() {
    let Some((p, e, zone)) = shared("brasov-latest.zip") else {
        return;
    };
    let n = env_n("RUSTLINE_M8_QUERIES", 1000);
    for seed in seeds() {
        let s = run_oracle(&p, &e, &zone, seed, n, &feed_days(&p));
        println!(
            "brasov seed {seed}: {} checked, {} no route, {} skipped, {} hidden by the 5-minute rule",
            s.checked, s.no_route, s.skipped, s.hidden
        );
        assert!(s.checked >= n / 4, "too few routed queries: {}", s.checked);
    }
}

#[test]
fn oracle_on_mbta_non_romanian_timezone() {
    let Some((p, e, zone)) = shared("mbta.zip") else {
        return;
    };
    assert_eq!(zone, "America/New_York");
    let n = env_n("RUSTLINE_M8_QUERIES", 1000) / 3;
    for seed in seeds() {
        let s = run_oracle(&p, &e, &zone, seed, n, &feed_days(&p));
        println!(
            "mbta seed {seed}: {} checked, {} no route, {} skipped, {} hidden by the 5-minute rule",
            s.checked, s.no_route, s.skipped, s.hidden
        );
        assert!(s.checked >= n / 5, "too few routed queries: {}", s.checked);
    }
}

#[test]
fn depart_late_pass_property_on_depart_at() {
    // Depart-at with the M8 depart-late pass: its departure is the latest one that
    // keeps its arrival, i.e. arrive-by at that arrival leaves no later; arrival
    // never gets later than the M5 earliest arrival.
    for seed in seeds() {
        let f = random_feed(seed);
        let (p, e) = (f.pack(), engine(f.pack()));
        let mut r = Rng(seed ^ 0xdead);
        let mut checked = 0;
        let mut skipped = 0;
        for _ in 0..300 {
            let (a, b) = (
                r.below(p.stop_count() as u64) as usize,
                r.below(p.stop_count() as u64) as usize,
            );
            if a == b {
                continue;
            }
            let day = date(2031, 3, 26 + r.below(6) as u32);
            let Some(t) = local(
                BUCHAREST,
                day.and_time(NaiveTime::from_hms_opt(5, 0, 0).unwrap())
                    + Duration::seconds(r.below(80_000) as i64),
            ) else {
                continue;
            };
            let Ok(its) = plan(
                &e,
                &stop_req(pos(&p, a), pos(&p, b), rustline_core::TimeSpec::DepartAt(t)),
            ) else {
                continue;
            };
            let d0 = local_date(BUCHAREST, t);
            let mut seen = false;
            for it in &its {
                // Out of scope (plan M8 decision 7): a ride on a service day before
                // D − 1 of the arrival, which the arrive-by window cannot see.
                if uses_day_before_window(&p, BUCHAREST, it, it.arr) {
                    skipped += 1;
                    continue;
                }
                let _ = d0;
                // Same transfer limit as the journey (DoD "within the same transfer
                // limit"): arrive-by with max_transfers = rides − 1.
                let mut rq = stop_req(pos(&p, a), pos(&p, b), arrive_by(it.arr));
                rq.max_transfers = it.rides.saturating_sub(1) as u8;
                let back = plan(&e, &rq)
                    .unwrap_or_else(|x| panic!("arrive-by at a reachable arrival: {x:?} {it:?}"));
                let latest: DateTime<FixedOffset> = back.iter().map(first_board).max().unwrap();
                assert!(first_board(it) >= t);
                assert_eq!(
                    first_board(it),
                    latest,
                    "depart-at does not leave as late as possible: {it:?} vs {back:?}"
                );
                seen = true;
            }
            if !seen {
                continue;
            }
            checked += 1;
        }
        println!("depart-late seed {seed}: {checked} queries checked, {skipped} journeys outside the window");
        assert!(checked > 50, "{checked}");
        assert!(
            skipped * 10 <= checked,
            "too many out-of-window journeys: {skipped}"
        );
    }
}
