//! M9 edge cases (plan M9 "Edge cases (must be tested)"): dominance, transfer limits,
//! the option cap, ranges, arrive-by ranges and frequency trips. Stops are ≥ 111 km
//! apart (m5 `stop_pos`), so only rides connect them.
#![allow(unexpected_cfgs)]
mod m9_common;

mod m9 {
    use super::m9_common::api9::*;
    use super::m9_common::*;
    use rustline_core::TimeSpec;

    fn q(
        e: &rustline_core::Engine,
        from: &str,
        to: &str,
        t: chrono::DateTime<chrono::FixedOffset>,
        w: u32,
        f: impl Fn(&mut rustline_core::PlanRequest),
    ) -> Result<Vec<It>, rustline_core::PlanError> {
        let mut r = window(
            stop_req(stop_pos(from), stop_pos(to), TimeSpec::DepartAt(t)),
            w,
        );
        r.max_results = MAX_RESULTS;
        f(&mut r);
        plan(e, &r)
    }
    fn keys(v: &[It]) -> Vec<(String, String, usize)> {
        v.iter()
            .map(|i| {
                (
                    i.dep.format("%H:%M").to_string(),
                    i.arr.format("%H:%M").to_string(),
                    i.rides,
                )
            })
            .collect()
    }
    fn t8() -> chrono::DateTime<chrono::FixedOffset> {
        at(2031, 6, 11, 8, 0, 0, 3)
    }

    /// Front: direct A→D arrives 09:30 (0 transfers); A→B→D arrives 09:00 (1);
    /// A→B→C→D arrives 08:40 (2).
    fn front_feed() -> Feed {
        Feed::daily(BUCHAREST, "20310101", "20311231")
            .trip("S", "direct", &[("08:05:00", "A"), ("09:30:00", "D")])
            .trip("S", "ab", &[("08:05:00", "A"), ("08:15:00", "B")])
            .trip("S", "bd", &[("08:20:00", "B"), ("09:00:00", "D")])
            .trip("S", "bc", &[("08:20:00", "B"), ("08:25:00", "C")])
            .trip("S", "cd", &[("08:30:00", "C"), ("08:40:00", "D")])
    }

    #[test]
    fn full_front_is_returned() {
        let e = engine(front_feed().pack());
        let v = q(&e, "A", "D", t8(), 0, |_| {}).unwrap();
        assert_eq!(keys(&v).len(), 3, "{v:?}");
        let mut got: Vec<_> = v.iter().map(key).collect();
        got.sort();
        assert_eq!(
            got.iter().map(|k| k.1).collect::<Vec<_>>(),
            vec![2, 1, 0],
            "{v:?}"
        );
    }

    #[test]
    fn equal_arrival_keeps_fewer_transfers() {
        let f = Feed::daily(BUCHAREST, "20310101", "20311231")
            .trip("S", "direct", &[("08:05:00", "A"), ("09:00:00", "D")])
            .trip("S", "ab", &[("08:05:00", "A"), ("08:15:00", "B")])
            .trip("S", "bd", &[("08:20:00", "B"), ("09:00:00", "D")]);
        let v = q(&engine(f.pack()), "A", "D", t8(), 0, |_| {}).unwrap();
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].trips(), vec!["direct"]);
    }

    #[test]
    fn equal_transfers_keeps_earlier_arrival() {
        let f = Feed::daily(BUCHAREST, "20310101", "20311231")
            .trip("S", "slow", &[("08:05:00", "A"), ("09:30:00", "D")])
            .trip("S", "fast", &[("08:06:00", "A"), ("09:00:00", "D")]);
        let v = q(&engine(f.pack()), "A", "D", t8(), 0, |_| {}).unwrap();
        assert_eq!(
            v.iter().map(|i| i.trips()[0]).collect::<Vec<_>>(),
            vec!["fast"],
            "{v:?}"
        );
    }

    /// Exact ties on (arrival, transfers): one deterministic pick, the same on every
    /// build (lowest route/trip index per the M8 tie-break) and on every engine.
    #[test]
    fn exact_ties_are_deterministic() {
        let f = Feed::daily(BUCHAREST, "20310101", "20311231")
            .trip(
                "S",
                "t1",
                &[("08:05:00", "A"), ("08:15:00", "B"), ("09:00:00", "D")],
            )
            .trip(
                "S",
                "t2",
                &[("08:05:00", "A"), ("08:50:00", "C"), ("09:00:00", "D")],
            );
        let a = q(&engine(f.pack()), "A", "D", t8(), 0, |_| {}).unwrap();
        let b = q(&engine(f.pack()), "A", "D", t8(), 0, |_| {}).unwrap();
        assert_eq!(a.len(), 1, "{a:?}");
        assert_eq!(a, b);
        assert_eq!(a[0].trips(), vec!["t1"], "lowest trip index wins");
    }

    /// Walk-only vs transit in one front (Brașov, RUSTLINE_M5_FEED; skipped without
    /// it). The test pack has no street graph, so walks are straight line × 1.3 and
    /// walk-only exists up to WALK_ONLY_MAX_M (3 km) of walking: pairs are drawn at
    /// 0.8–2.2 km straight line (≤ 2.9 km walked), 07:00–19:00, until 60 such pairs.
    /// Some front must hold both a walk-only journey and a faster ride, and a ride
    /// kept beside walk-only must arrive earlier (walk-only has 0 transfers).
    /// Open decision: walk-only counts as 0 transfers in the front (assumed).
    #[test]
    fn walk_only_and_transit_share_the_front() {
        let Some(feed) = env_feed(FEED_VAR) else {
            return;
        };
        let p = pack_of(&feed);
        let e = engine(pack_of(&feed));
        let with: Vec<usize> = (0..p.stop_count())
            .filter(|&s| p.stop_pos(s).is_some())
            .collect();
        let d = p.meta().valid_from + chrono::Duration::days(7);
        let mut r = Rng(9);
        let (mut pairs, mut walk, mut both) = (0, 0, 0);
        for _ in 0..200_000 {
            if pairs == 60 {
                break;
            }
            let (a, b) = (
                with[r.below(with.len() as u64) as usize],
                with[r.below(with.len() as u64) as usize],
            );
            let (pa, pb) = (pos(&p, a), pos(&p, b));
            let dist = ((pa.0 - pb.0) * 111_000.0).hypot((pa.1 - pb.1) * 78_500.0);
            if !(800.0..2200.0).contains(&dist) {
                continue;
            }
            pairs += 1;
            let wall = d.and_hms_opt(7, 0, 0).unwrap()
                + chrono::Duration::seconds(r.below(12 * 3600) as i64);
            let t = local(BUCHAREST, wall).unwrap();
            let mut rq = window(door_req(pa, pb, TimeSpec::DepartAt(t)), 0);
            rq.max_results = MAX_RESULTS;
            let Ok(v) = plan(&e, &rq) else { continue };
            if let Some(w) = v.iter().find(|i| i.rides == 0) {
                walk += 1;
                assert!(
                    v.iter().filter(|i| i.rides > 0).all(|i| i.arr < w.arr),
                    "dominated ride kept beside walk-only: {v:?}"
                );
                if v.iter().any(|i| i.rides > 0) {
                    both += 1;
                }
            }
        }
        assert_eq!(pairs, 60, "too few pairs at 0.8–2.2 km");
        assert!(
            walk > 0,
            "no walk-only answer in {pairs} pairs (test setup)"
        );
        assert!(both > 0, "{walk} walk-only fronts, none with a faster ride");
    }

    #[test]
    fn transfer_limits() {
        let e = engine(front_feed().pack());
        let rides = |k: u8| {
            q(&e, "A", "D", t8(), 0, |r| r.max_transfers = k)
                .map(|v| v.iter().map(|i| i.rides).max().unwrap())
        };
        assert_eq!(rides(0).unwrap(), 1);
        assert_eq!(
            rides(1).unwrap(),
            2,
            "fastest needs 2 transfers, only 1 allowed"
        );
        assert_eq!(rides(MAX_TRANSFERS).unwrap(), 3);
        // Plan M9: over the hard max → invalid_request (conflicts with M8 decision:
        // 255 was capped to 8; open decision).
        assert!(is_invalid(&q(&e, "A", "D", t8(), 0, |r| r.max_transfers =
            MAX_TRANSFERS + 1)));
    }

    #[test]
    fn result_cap_keeps_fastest_and_fewest_transfers() {
        let e = engine(front_feed().pack());
        let v = q(&e, "A", "D", t8(), 0, |r| r.max_results = 2).unwrap();
        assert_eq!(v.len(), 2);
        assert!(
            v.iter().any(|i| i.trips() == vec!["direct"]),
            "fewest transfers kept: {v:?}"
        );
        assert!(v.iter().any(|i| i.rides == 3), "fastest kept: {v:?}");
        let v = q(&e, "A", "D", t8(), 0, |r| r.max_results = 1).unwrap();
        assert_eq!(v.len(), 1, "max_results 1: {v:?}");
        assert!(is_invalid(&q(&e, "A", "D", t8(), 0, |r| r.max_results = 0)));
        assert!(
            is_invalid(&q(&e, "A", "D", t8(), 0, |r| r.max_results = MAX_RESULTS + 1)),
            "hard cap 20"
        );
    }

    #[test]
    fn tags_mark_fastest_fewest_and_latest() {
        let f = front_feed().trip("S", "late", &[("08:25:00", "A"), ("09:45:00", "D")]);
        let e = engine(f.pack());
        let mut r = window(
            stop_req(stop_pos("A"), stop_pos("D"), TimeSpec::DepartAt(t8())),
            1800,
        );
        r.max_results = MAX_RESULTS;
        let v = plan_tagged(&e, &r).unwrap();
        let with = |tag: &str| {
            v.iter()
                .filter(|x| x.1.iter().any(|t| t == tag))
                .map(|x| x.0.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            with("fastest").iter().map(|i| i.rides).collect::<Vec<_>>(),
            vec![3]
        );
        assert!(
            with("fewest_transfers").iter().all(|i| i.rides == 1)
                && !with("fewest_transfers").is_empty()
        );
        assert_eq!(
            with("leaves_latest")
                .iter()
                .map(|i| i.trips())
                .collect::<Vec<_>>(),
            vec![vec!["late"]]
        );
    }

    /// Departures every 15 min 07:00–10:00, 30 min ride A→B.
    fn hourly() -> Feed {
        let mut f = Feed::daily(BUCHAREST, "20310101", "20311231");
        for i in 0..13 {
            let (h, m) = (7 + i * 15 / 60, i * 15 % 60);
            f = f.trip(
                "S",
                &format!("t{i:02}"),
                &[
                    (&format!("{h:02}:{m:02}:00"), "A"),
                    (
                        &format!("{:02}:{:02}:00", h + (m + 30) / 60, (m + 30) % 60),
                        "B",
                    ),
                ],
            );
        }
        f
    }

    #[test]
    fn window_bounds() {
        let f = Feed::daily(BUCHAREST, "20310101", "20311231")
            .trip("S", "a", &[("08:00:00", "A"), ("08:30:00", "B")])
            .trip("S", "b", &[("08:20:00", "A"), ("08:50:00", "B")])
            .trip("S", "c", &[("08:40:00", "A"), ("09:10:00", "B")]);
        let e = engine(f.pack());
        let trips = |w| {
            q(&e, "A", "B", t8(), w, |_| {}).map(|v| {
                let mut x: Vec<String> = v.iter().map(|i| i.trips()[0].to_string()).collect();
                x.sort();
                x
            })
        };
        assert_eq!(trips(0).unwrap(), vec!["a"]);
        assert_eq!(trips(1800).unwrap(), vec!["a", "b"]);
        assert!(trips(MAX_WINDOW_SECS).is_ok(), "window = cap");
        const _: () = assert!(MAX_WINDOW_SECS <= 7200, "documented cap ≤ 2 h");
        assert!(is_invalid(&q(
            &e,
            "A",
            "B",
            t8(),
            MAX_WINDOW_SECS + 1,
            |_| {}
        )));
    }

    #[test]
    fn window_crossing_midnight_uses_d_and_d1_trips() {
        let f = Feed::daily(BUCHAREST, "20310101", "20311231")
            .trip("S", "late", &[("23:50:00", "A"), ("24:20:00", "B")])
            .trip("S", "early", &[("00:10:00", "A"), ("00:40:00", "B")]);
        let e = engine(f.pack());
        let v = q(&e, "A", "B", at(2031, 6, 11, 23, 40, 0, 3), 3600, |_| {}).unwrap();
        let mut t: Vec<_> = v.iter().map(|i| i.trips()[0]).collect();
        t.sort();
        assert_eq!(t, vec!["early", "late"], "{v:?}");
        assert_eq!(
            local_date(
                BUCHAREST,
                v.iter().find(|i| i.trips()[0] == "early").unwrap().dep
            ),
            date(2031, 6, 12)
        );
    }

    /// 2031-03-30 03:00 EET → 04:00 EEST. Window 02:30+02 for 1 h spans the gap.
    #[test]
    fn window_across_dst_change() {
        let f = Feed::daily(BUCHAREST, "20310101", "20311231")
            // GTFS times count from noon − 12 h = 23:00+02 on 03-29 (a 23 h day),
            // so "03:40:00" runs at
            // 02:40+02 (after the 02:30 query) and "02:40:00" would run at 01:40+02.
            .trip("S", "before", &[("03:40:00", "A"), ("03:50:00", "B")])
            .trip("S", "after", &[("04:10:00", "A"), ("04:20:00", "B")])
            .trip("S", "toolate", &[("04:40:00", "A"), ("04:50:00", "B")]);
        let e = engine(f.pack());
        let v = q(&e, "A", "B", at(2031, 3, 30, 2, 30, 0, 2), 3600, |_| {}).unwrap();
        let mut t: Vec<_> = v.iter().map(|i| i.trips()[0]).collect();
        t.sort();
        // Window 02:30+02 .. 04:30+03 (00:30..01:30 UTC): "before" 02:40+02 and
        // "after" 04:10+03 are inside, "toolate" 04:40+03 is not.
        assert_eq!(t, vec!["after", "before"], "{v:?}");
    }

    /// Open decision: no departure in the window → the first journey after it
    /// (as window 0 does), not no_route. Assumed: same as window 0 at t0.
    #[test]
    fn no_departures_in_window() {
        let f = Feed::daily(BUCHAREST, "20310101", "20311231").trip(
            "S",
            "x",
            &[("12:00:00", "A"), ("12:30:00", "B")],
        );
        let e = engine(f.pack());
        let w = q(&e, "A", "B", t8(), 1800, |_| {}).unwrap();
        let z = q(&e, "A", "B", t8(), 0, |_| {}).unwrap();
        assert_eq!(w, z);
    }

    /// Two neighbouring departures (08:00 and 08:01 as start times) both lead to the
    /// same single trip: it appears once.
    #[test]
    fn identical_journeys_are_deduped() {
        let f = Feed::daily(BUCHAREST, "20310101", "20311231")
            .trip("S", "feeder1", &[("08:00:00", "A"), ("08:10:00", "B")])
            .trip("S", "feeder2", &[("08:05:00", "A"), ("08:12:00", "B")])
            .trip("S", "main", &[("08:30:00", "B"), ("09:00:00", "C")]);
        let v = q(&engine(f.pack()), "A", "C", t8(), 1800, |_| {}).unwrap();
        let mut seen = std::collections::HashSet::new();
        for i in &v {
            assert!(
                seen.insert(render(&Ok(vec![i.clone()]))),
                "duplicate {i:?} in {v:?}"
            );
        }
        // The latest feeder dominates (same arrival, same rides, leaves later).
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].trips(), vec!["feeder2", "main"]);
    }

    #[test]
    fn arrive_by_range_is_mirrored() {
        let e = engine(hourly().pack());
        let mut r = window(
            stop_req(
                stop_pos("A"),
                stop_pos("B"),
                TimeSpec::ArriveBy(at(2031, 6, 11, 9, 0, 0, 3)),
            ),
            1800,
        );
        r.max_results = MAX_RESULTS;
        let v = plan(&e, &r).unwrap();
        // Arrivals in [08:30, 09:00]: departures 08:00, 08:15, 08:30.
        let mut d: Vec<_> = v
            .iter()
            .map(|i| i.dep.format("%H:%M").to_string())
            .collect();
        d.sort();
        assert_eq!(d, vec!["08:00", "08:15", "08:30"], "{v:?}");
    }

    /// frequencies.txt every 20 s for 3 h: > 256 departures in a 2 h window; the
    /// range is capped (MAX_RANGE_DEPARTURES) and still answers, at most MAX_RESULTS.
    #[test]
    fn frequency_trips_in_window_are_capped() {
        let f = Feed::daily(BUCHAREST, "20310101", "20311231")
            .trip("S", "f", &[("07:00:00", "A"), ("07:30:00", "B")])
            .freq("f", "07:00:00", "10:00:00", "20", "1");
        let e = engine(f.pack());
        let t = std::time::Instant::now();
        let v = q(
            &e,
            "A",
            "B",
            at(2031, 6, 11, 7, 0, 0, 3),
            MAX_WINDOW_SECS,
            |_| {},
        )
        .unwrap();
        const _: () = assert!(MAX_RANGE_DEPARTURES <= 256);
        assert!(
            !v.is_empty() && v.len() <= MAX_RESULTS as usize,
            "{}",
            v.len()
        );
        assert!(
            t.elapsed().as_secs() < 5,
            "capped range took {:?}",
            t.elapsed()
        );
    }
}
