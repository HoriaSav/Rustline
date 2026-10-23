//! M9 review fixes (R39, 2026-10-23): arrive-by range steps from the latest
//! arrival (H1), the arrive-by range property, and the departure cap keeps the
//! departures nearest the requested time (M3).
#![allow(unexpected_cfgs)]
mod m9_common;
use chrono::{Duration, NaiveTime};
use m9_common::api9::*;
use m9_common::*;
use rustline_core::TimeSpec;
use std::path::PathBuf;

/// A real-feed fixture from fixtures/manifest.tsv, pinned to its sha256
/// (plan M10 step 6, M9 backlog): None (printing `skip:`) when the file is
/// missing; a different file fails instead of silently testing another feed.
fn pinned_fixture(name: &str) -> Option<PathBuf> {
    use sha2::{Digest, Sha256};
    let manifest = std::fs::read_to_string(workspace("fixtures/manifest.tsv")).unwrap();
    let row: Vec<&str> = manifest
        .lines()
        .map(|l| l.split('\t').collect::<Vec<_>>())
        .find(|c| c.first() == Some(&name))
        .unwrap_or_else(|| panic!("{name} not in fixtures/manifest.tsv"));
    let (file, sha) = (row[1], row[3]);
    let path = workspace(&format!("fixtures/feeds/{file}"));
    let Ok(bytes) = std::fs::read(&path) else {
        println!("skip: {} missing", path.display());
        return None;
    };
    let got: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(
        got,
        sha,
        "{} is not the pinned {name} (run scripts/fetch-fixtures.sh --verify)",
        path.display()
    );
    Some(path)
}

fn arrive_window(
    from: &str,
    to: &str,
    t: chrono::DateTime<chrono::FixedOffset>,
    w: u32,
) -> Vec<It> {
    let mut r = window(
        stop_req(stop_pos(from), stop_pos(to), TimeSpec::ArriveBy(t)),
        w,
    );
    r.max_results = MAX_RESULTS;
    plan(&engine(feed_h1().pack()), &r).unwrap()
}

/// Deadline 09:00: 1 ride 08:00→09:00, 1 ride 07:55→08:45, 2 rides 07:50→08:30.
fn feed_h1() -> Feed {
    Feed::daily(BUCHAREST, "20310101", "20311231")
        .trip("S", "d1", &[("08:00:00", "A"), ("09:00:00", "B")])
        .trip("S", "d2", &[("07:55:00", "A"), ("08:45:00", "B")])
        .trip("S", "x", &[("07:50:00", "A"), ("08:05:00", "C")])
        .trip("S", "y", &[("08:10:00", "C"), ("08:30:00", "B")])
}

#[test]
fn arrive_by_range_reaches_every_front_point() {
    let v = arrive_window("A", "B", at(2031, 6, 11, 9, 0, 0, 3), 1800);
    let mut got: Vec<_> = v
        .iter()
        .map(|i| {
            (
                i.dep.format("%H:%M").to_string(),
                i.arr.format("%H:%M").to_string(),
                i.rides,
            )
        })
        .collect();
    got.sort();
    let want = [
        ("07:50", "08:30", 2),
        ("07:55", "08:45", 1),
        ("08:00", "09:00", 1),
    ];
    let want: Vec<_> = want
        .iter()
        .map(|(d, a, r)| (d.to_string(), a.to_string(), *r))
        .collect();
    assert_eq!(got, want, "{v:?}");
}

fn seeds() -> Vec<u64> {
    (0..env_n("RUSTLINE_M9_SEEDS", 3) as u64)
        .map(|i| 0x6d39 + i * 7919)
        .collect()
}

/// For every deadline t in [t0 − w, t0] (arrival events at b, plus both ends),
/// the arrive-by range holds a journey at least as good as the single query at
/// t: leaves no earlier, arrives no later, no more rides.
#[test]
fn arrive_by_range_covers_every_single_query() {
    let n = env_n("RUSTLINE_M9_QUERIES", 1000) / 4;
    for seed in seeds() {
        let f = random_feed(seed);
        let p = f.pack();
        let e = engine(f.pack());
        let mut r = Rng(seed ^ 0x78);
        for q in 0..n {
            let ns = p.stop_count() as u64;
            let a = r.below(ns) as usize;
            let b = (a + 1 + r.below(ns - 1) as usize) % ns as usize;
            let base =
                [date(2031, 3, 28), date(2031, 10, 24), date(2031, 6, 11)][r.below(3) as usize];
            let day = base + Duration::days(r.below(4) as i64);
            let wall = day.and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap())
                + Duration::seconds(r.below(86_400) as i64);
            let t0 = local(BUCHAREST, wall)
                .unwrap_or_else(|| local(BUCHAREST, wall + Duration::hours(1)).unwrap());
            let w = 1800;
            let lo = t0 - Duration::seconds(w as i64);
            let mk = |t| {
                let mut x = stop_req(pos(&p, a), pos(&p, b), TimeSpec::ArriveBy(t));
                x.max_results = MAX_RESULTS;
                x
            };
            let range = plan(&e, &window(mk(t0), w)).unwrap_or_default();
            let d = local_date(BUCHAREST, t0);
            let days = [d - Duration::days(1), d, d + Duration::days(1)];
            let mut ts: Vec<_> = connections(&p, BUCHAREST, &days)
                .into_iter()
                .filter(|c| c.3 == b && c.1 >= lo && c.1 <= t0)
                .map(|c| c.1)
                .collect();
            ts.push(t0);
            ts.push(lo);
            for t in ts {
                for s in plan(&e, &mk(t))
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|s| s.arr >= lo)
                {
                    assert!(
                        range
                            .iter()
                            .any(|j| j.dep >= s.dep && j.arr <= s.arr && j.rides <= s.rides),
                        "seed {seed} q{q} t {t}: single {s:?} not covered by range {range:?}"
                    );
                }
            }
        }
    }
}

/// frequencies.txt every 20 s 07:00–10:00, window 2 h (360 departures > 256):
/// the cap keeps the 256 departures nearest 07:00, so with MAX_RESULTS the answer
/// is the first 19 departures, 20 s apart, plus the leaves-latest one, the 256th
/// departure 07:00 + 255 × 20 s = 08:25:00 (later ones are not searched).
#[test]
fn departure_cap_keeps_the_earliest_departures() {
    let f = Feed::daily(BUCHAREST, "20310101", "20311231")
        .trip("S", "f", &[("07:00:00", "A"), ("07:30:00", "B")])
        .freq("f", "07:00:00", "10:00:00", "20", "1");
    let e = engine(f.pack());
    let mut r = window(
        stop_req(
            stop_pos("A"),
            stop_pos("B"),
            TimeSpec::DepartAt(at(2031, 6, 11, 7, 0, 0, 3)),
        ),
        MAX_WINDOW_SECS,
    );
    r.max_results = MAX_RESULTS;
    let v = plan(&e, &r).unwrap();
    let got: Vec<_> = v
        .iter()
        .map(|i| i.dep.format("%H:%M:%S").to_string())
        .collect();
    let mut want: Vec<_> = (0..MAX_RESULTS as u32 - 1)
        .map(|i| format!("07:{:02}:{:02}", i * 20 / 60, i * 20 % 60))
        .collect();
    want.push("08:25:00".to_string());
    assert_eq!(got, want, "{v:?}");
    let mut late = window(
        stop_req(
            stop_pos("A"),
            stop_pos("B"),
            TimeSpec::DepartAt(at(2031, 6, 11, 8, 59, 0, 3)),
        ),
        60,
    );
    late.max_results = MAX_RESULTS;
    let v = plan(&e, &late).unwrap();
    assert!(
        v.iter()
            .any(|i| i.dep.format("%H:%M:%S").to_string() == "08:59:40"),
        "{v:?}"
    );
}

/// Tester's M9 final check: Brașov, arrive by 2026-10-27 09:00, 30 min window,
/// default max_results (5). The cap keeps the latest departures (08:36:29,
/// 08:36:31, as window 0 gives), and leaves_latest is the latest-leaving one.
#[test]
fn arrive_by_cap_keeps_the_latest_departures() {
    let Some(path) = pinned_fixture("brasov-latest") else {
        return;
    };
    let e = engine(pack_of(&path));
    let t = at(2026, 10, 27, 9, 0, 0, 2);
    let r = window(
        door_req(
            (45.6427, 25.5887),
            (45.6611, 25.6142),
            TimeSpec::ArriveBy(t),
        ),
        1800,
    );
    let v = plan_tagged(&e, &r).unwrap();
    let deps: Vec<String> = v
        .iter()
        .map(|(i, _)| i.dep.format("%H:%M:%S").to_string())
        .collect();
    for want in ["08:36:29", "08:36:31"] {
        assert!(deps.iter().any(|d| d == want), "{want} missing: {deps:?}");
    }
    let latest = v.iter().map(|(i, _)| i.dep).max().unwrap();
    let tagged: Vec<_> = v
        .iter()
        .filter(|(_, tags)| tags.iter().any(|t| t == "leaves_latest"))
        .map(|(i, _)| i.dep)
        .collect();
    assert!(
        !tagged.is_empty() && tagged.iter().all(|&d| d == latest),
        "{deps:?} {v:?}"
    );
    assert_ne!(latest.format("%H:%M:%S").to_string(), "08:27:29");
    let mut sorted = deps.clone();
    sorted.sort();
    assert_eq!(deps, sorted, "earliest first");
}

/// Tester's M9 recheck: Brașov, depart at 2026-10-27 08:00, 30 min window,
/// max 20. 08:28:31 → 08:55:17 (2 rides) is not dominated by 08:28:26 →
/// 08:55:17 (1 ride) and must be in the range answer, as in the single query
/// at 08:28:31. Root cause: `departures()` let a longer hop overwrite an
/// access walk, so that door was computed too early (outside the window).
#[test]
fn depart_at_range_keeps_later_departure_with_more_rides() {
    let Some(path) = pinned_fixture("brasov-latest") else {
        return;
    };
    let e = engine(pack_of(&path));
    let t = at(2026, 10, 27, 8, 0, 0, 2);
    let mut r = window(
        door_req(
            (45.6427, 25.5887),
            (45.6611, 25.6142),
            TimeSpec::DepartAt(t),
        ),
        1800,
    );
    r.max_results = MAX_RESULTS;
    let v = plan(&e, &r).unwrap();
    let got: Vec<_> = v
        .iter()
        .map(|i| {
            (
                i.dep.format("%H:%M:%S").to_string(),
                i.arr.format("%H:%M:%S").to_string(),
                i.rides,
            )
        })
        .collect();
    for want in [("08:28:26", "08:55:17", 1), ("08:28:31", "08:55:17", 2)] {
        let want = (want.0.to_string(), want.1.to_string(), want.2);
        assert!(got.contains(&want), "{want:?} missing: {got:?}");
    }
}

/// Arrive-by with max_results 1 keeps the latest departure (the M8 pick):
/// deadline 09:00 → the 1-ride 08:00 → 09:00, with and without a window.
#[test]
fn arrive_by_max_results_1_keeps_latest_departure() {
    let e = engine(feed_h1().pack());
    for w in [0, 1800] {
        let mut r = window(
            stop_req(
                stop_pos("A"),
                stop_pos("B"),
                TimeSpec::ArriveBy(at(2031, 6, 11, 9, 0, 0, 3)),
            ),
            w,
        );
        r.max_results = 1;
        let v = plan(&e, &r).unwrap();
        let got: Vec<_> = v
            .iter()
            .map(|i| (i.dep.format("%H:%M").to_string(), i.rides))
            .collect();
        assert_eq!(got, vec![("08:00".to_string(), 1)], "window {w}: {v:?}");
    }
}
