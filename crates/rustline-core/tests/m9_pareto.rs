//! M9 DoD: dominance property vs a per-k CSA oracle, the range property, and
//! "window 0 gives exactly the M8 results" (golden, byte-equal).
#![allow(unexpected_cfgs)]
mod m9_common;
use chrono::{Duration, NaiveTime};
use m9_common::*;

fn days_around(d: chrono::NaiveDate) -> Vec<chrono::NaiveDate> {
    vec![d - Duration::days(1), d, d + Duration::days(1)]
}

fn seeds() -> Vec<u64> {
    (0..env_n("RUSTLINE_M9_SEEDS", 3) as u64)
        .map(|i| 0x6d39 + i * 7919)
        .collect()
}

/// Random stop pair + departure (March/October 2031, around both DST changes).
fn query(r: &mut Rng, p: &Pack) -> (usize, usize, chrono::DateTime<chrono::FixedOffset>) {
    let n = p.stop_count() as u64;
    let a = r.below(n) as usize;
    let mut b = r.below(n) as usize;
    if a == b {
        b = (b + 1) % n as usize;
    }
    let base = [date(2031, 3, 28), date(2031, 10, 24), date(2031, 6, 11)][r.below(3) as usize];
    let day = base + Duration::days(r.below(4) as i64);
    let wall = day.and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap())
        + Duration::seconds(r.below(86_400) as i64);
    let t = local(BUCHAREST, wall)
        .unwrap_or_else(|| local(BUCHAREST, wall + Duration::hours(1)).unwrap());
    (a, b, t)
}

/// Golden (made from M8 at acf46b7; never re-bless without Leader approval): with
/// `window_secs = 0`, every M8 line of every query is still present, and any extra
/// line (the 5-minute rule is dropped in M9, so window 0 returns the full Pareto
/// front) is not dominated by the oracle's front. So re-blessing can only add such
/// lines. Errors must stay identical (an M8 `err` line must still be the output).
#[test]
fn window_zero_keeps_every_m8_line() {
    let path = workspace("crates/rustline-core/tests/golden/m9_window0.txt");
    let want = std::fs::read_to_string(&path).expect("golden m9_window0.txt");
    let mut blocks: std::collections::HashMap<String, Vec<String>> = Default::default();
    let mut head = String::new();
    for l in want.lines() {
        if l.starts_with("# ") {
            head = l.to_string();
            blocks.insert(head.clone(), Vec::new());
        } else {
            blocks.get_mut(&head).unwrap().push(l.to_string());
        }
    }
    let mut seen = 0;
    let mut out = String::new();
    for seed in [0x6d39_u64, 0x6d39 + 7919] {
        let p = random_feed(seed).pack();
        let e = engine(random_feed(seed).pack());
        let change = vec![Some(0); p.stop_count()];
        let mut r = Rng(seed ^ 0x90);
        for q in 0..40 {
            let (a, b, t) = query(&mut r, &p);
            let mut rq = stop_req(pos(&p, a), pos(&p, b), rustline_core::TimeSpec::DepartAt(t));
            rq.max_results = 20;
            let rq = api9::window(rq, 0);
            let h = format!(
                "# seed {seed} q{q} {}->{} {}",
                p.stop_ids[a],
                p.stop_ids[b],
                t.to_rfc3339()
            );
            let old = blocks
                .get(&h)
                .unwrap_or_else(|| panic!("{h} not in golden"));
            seen += 1;
            let res = plan(&e, &rq);
            out += &format!("{h}\n{}", render(&res));
            let got: Vec<String> = render(&res).lines().map(str::to_string).collect();
            for l in old {
                assert!(got.contains(l), "{h}: M8 line lost: {l}\ngot {got:?}");
            }
            if got.iter().all(|l| old.contains(l)) {
                continue;
            }
            let d = local_date(BUCHAREST, t);
            let conns = connections(&p, BUCHAREST, &days_around(d));
            // max_transfers 3 (default) → at most 4 rides.
            let front = oracle_front(&conns, &change, a, b, t, 4);
            for it in res.as_ref().unwrap() {
                let line = render(&Ok(vec![it.clone()]));
                if old.contains(&line.trim_end().to_string()) {
                    continue;
                }
                assert!(
                    !front.iter().any(|w| dominates(*w, key(it))),
                    "{h}: extra line {line} is dominated by the oracle front {front:?}"
                );
            }
        }
    }
    assert_eq!(seen, blocks.len(), "every golden query checked");
    // Re-bless (Builder, with Leader approval) only after every check above passed.
    if std::env::var("RUSTLINE_BLESS").is_ok() {
        std::fs::write(&path, out).unwrap();
    }
}

/// Plan M5 decisions / M9 decision 8, by hand: a ride A→X arrives 08:10; from X a
/// trip leaves at 08:12 (→ D 08:30) and another at 08:20 (→ D 08:40). X's own
/// transfers.txt row decides which one can be caught:
/// none → 0 s → 08:30; type 1 → 0 s → 08:30; type 2 with 300 s → 08:15 needed →
/// 08:40; type 0 (time 600 given, ignored: walk time at one stop 0 s) → 08:30;
/// type 2 without a time → walk time 0 s → 08:30; type 3 → no change at X → none.
/// Checks the oracle against the hand results and the engine against both.
#[test]
fn same_stop_transfer_rows_by_hand() {
    let h = "from_stop_id,to_stop_id,transfer_type,min_transfer_time\n";
    let cases: [(&str, Option<(u32, u32)>); 6] = [
        ("", Some((8, 30))),
        ("X,X,1,\n", Some((8, 30))),
        ("X,X,2,300\n", Some((8, 40))),
        ("X,X,0,600\n", Some((8, 30))),
        ("X,X,2,\n", Some((8, 30))),
        ("X,X,3,\n", None),
    ];
    let t = at(2031, 6, 11, 8, 0, 0, 3);
    for (row, want) in cases {
        let f = Feed::daily(BUCHAREST, "20310101", "20311231")
            .trip("S", "ax", &[("08:00:00", "A"), ("08:10:00", "X")])
            .trip("S", "x1", &[("08:12:00", "X"), ("08:30:00", "D")])
            .trip("S", "x2", &[("08:20:00", "X"), ("08:40:00", "D")])
            .transfers(&format!("{h}{row}"));
        let p = f.pack();
        let change = self_changes(&format!("{h}{row}"), &p);
        let conns = connections(&p, BUCHAREST, &days_around(date(2031, 6, 11)));
        let (a, d) = (stop(&p, "A").index(), stop(&p, "D").index());
        let front = oracle_front(&conns, &change, a, d, t, 4);
        let want_t = want.map(|(hh, mm)| at(2031, 6, 11, hh, mm, 0, 3));
        assert_eq!(front.first().map(|x| x.0), want_t, "oracle, row {row:?}");
        let e = engine(f.pack());
        let mut rq = stop_req(
            stop_pos("A"),
            stop_pos("D"),
            rustline_core::TimeSpec::DepartAt(t),
        );
        rq.max_results = 20;
        let got = plan(&e, &rq)
            .ok()
            .and_then(|v| v.iter().map(|i| i.arr).min());
        assert_eq!(got, want_t, "engine, row {row:?}");
    }
}

#[test]
fn dominance_and_oracle_front() {
    let n = env_n("RUSTLINE_M9_QUERIES", 1000);
    for seed in seeds() {
        let f = random_feed(seed);
        let p = f.pack();
        let e = engine(f.pack());
        let mut r = Rng(seed);
        let mut checked = 0;
        for q in 0..n {
            let (a, b, t) = query(&mut r, &p);
            let d = local_date(BUCHAREST, t);
            let conns = connections(&p, BUCHAREST, &days_around(d));
            let want = oracle_front(&conns, &vec![Some(0); p.stop_count()], a, b, t, 4);
            let mut rq = stop_req(pos(&p, a), pos(&p, b), rustline_core::TimeSpec::DepartAt(t));
            rq.max_results = api9::MAX_RESULTS;
            rq.max_transfers = 3;
            let ctx = format!("seed {seed} q{q} {}->{} {t}", p.stop_ids[a], p.stop_ids[b]);
            let got = match plan(&e, &rq) {
                Ok(v) => v,
                Err(x) => {
                    assert!(want.is_empty(), "{ctx}: oracle {want:?}, got {x:?}");
                    continue;
                }
            };
            let keys: Vec<_> = got.iter().map(key).collect();
            for (i, x) in keys.iter().enumerate() {
                for (j, y) in keys.iter().enumerate() {
                    assert!(i == j || !dominates(*x, *y), "{ctx}: {x:?} dominates {y:?}");
                }
            }
            for w in &want {
                assert!(
                    keys.contains(w),
                    "{ctx}: Pareto {w:?} missing from {keys:?}"
                );
            }
            for k in &keys {
                assert!(
                    !want.iter().any(|w| dominates(*w, *k)),
                    "{ctx}: {k:?} not optimal ({want:?})"
                );
            }
            checked += 1;
        }
        assert!(checked > n / 3, "seed {seed}: only {checked} answered");
    }
}

#[test]
fn range_covers_every_single_query() {
    let n = env_n("RUSTLINE_M9_QUERIES", 1000) / 4;
    for seed in seeds() {
        let f = random_feed(seed);
        let p = f.pack();
        let e = engine(f.pack());
        let mut r = Rng(seed ^ 0x77);
        for q in 0..n {
            let (a, b, t0) = query(&mut r, &p);
            let w = 1800;
            let mk = |t| {
                let mut x = stop_req(pos(&p, a), pos(&p, b), rustline_core::TimeSpec::DepartAt(t));
                x.max_results = api9::MAX_RESULTS;
                x
            };
            let range = plan(&e, &api9::window(mk(t0), w)).unwrap_or_default();
            // Every departure event at `a` in the window, plus the window ends.
            let d = local_date(BUCHAREST, t0);
            let mut ts: Vec<_> = departures(&p, BUCHAREST, &[a], &days_around(d))
                .into_iter()
                .map(|x| x.0)
                .filter(|&x| x >= t0 && x <= t0 + Duration::seconds(w as i64))
                .collect();
            ts.push(t0);
            ts.push(t0 + Duration::seconds(w as i64));
            let end = t0 + Duration::seconds(w as i64);
            for t in ts {
                // A single query may return trips leaving after the window; the
                // range only owes journeys that depart by t0 + w.
                for s in plan(&e, &mk(t))
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|s| s.dep <= end)
                {
                    assert!(
                        range
                            .iter()
                            .any(|j| j.dep >= t && j.arr <= s.arr && j.rides <= s.rides),
                        "seed {seed} q{q} t {t}: single {s:?} not covered by range {range:?}"
                    );
                }
            }
        }
    }
}
