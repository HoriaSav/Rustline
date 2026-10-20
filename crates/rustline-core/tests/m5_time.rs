//! M5 time-zone, DST, midnight and validity tests (Tester, written before M5).
//! Plan M5 steps 1–3, 7, UC4; DoD "UC4", "DST: both 2026-10-25 and 2027-03-28 …
//! (Toronto on its own dates)", "DateOutsideFeed … FeedExpiresSoon"; edge-case block
//! "Time and calendar". The M5 API is reached only through `m5_common::api`. The
//! `cfg rustline_m5` gate and the "M5 not implemented" ignores were removed in M5.
//! Instants are explicit `FixedOffset`s: identical under TZ=UTC and TZ=Pacific/Auckland.
//! Run: `cargo test --locked -p rustline-core --test m5_time`

mod m5_common;

use chrono::{DateTime, FixedOffset};
use m5_common::api::*;
use m5_common::*;

const H: i32 = 3600;

fn has(v: &[(chrono::NaiveDate, u32)], d: chrono::NaiveDate, secs: u32) -> bool {
    v.contains(&(d, secs))
}

/// Every leg time carries the feed zone's offset at that instant; times never go back.
fn check(it: &It, zone: &str) {
    let mut last = it.dep;
    for (a, b) in &it.spans {
        for t in [a, b] {
            assert_eq!(t.offset().local_minus_utc(), offset_at(zone, *t), "{it:?}");
        }
        assert!(*a >= last && b >= a, "time goes back: {it:?}");
        last = *b;
    }
    assert_eq!(it.arr, last);
    assert_eq!(it.dep.offset().local_minus_utc(), offset_at(zone, it.dep));
}

fn first(r: &Result<Vec<It>, PlanError>, zone: &str) -> It {
    let v = r.as_ref().expect("a route");
    assert!(!v.is_empty());
    for it in v {
        check(it, zone);
    }
    v.iter().min_by_key(|i| i.arr).unwrap().clone()
}

fn board(it: &It) -> DateTime<FixedOffset> {
    it.legs[0].3
}

// ---- to_service_time --------------------------------------------------------------

#[test]
fn to_service_time_bucharest_switch_days() {
    let z = BUCHAREST;
    let v = service_times(z, at(2026, 10, 25, 8, 0, 0, 2));
    assert!(has(&v, date(2026, 10, 25), 28_800), "{v:?}");
    assert!(
        has(&v, date(2026, 10, 24), 118_800),
        "D-1 is 33:00 (25 h day): {v:?}"
    );
    let v = service_times(z, at(2026, 10, 25, 3, 0, 0, 3));
    assert!(
        has(&v, date(2026, 10, 25), 7_200) && has(&v, date(2026, 10, 24), 97_200),
        "{v:?}"
    );
    let v = service_times(z, at(2027, 3, 28, 8, 0, 0, 3));
    assert!(has(&v, date(2027, 3, 28), 28_800), "{v:?}");
    assert!(
        has(&v, date(2027, 3, 27), 111_600),
        "D-1 is 31:00 (23 h day): {v:?}"
    );
    let v = service_times(z, at(2027, 3, 28, 1, 0, 0, 2));
    assert!(
        has(&v, date(2027, 3, 28), 7_200) && has(&v, date(2027, 3, 27), 90_000),
        "{v:?}"
    );
}

#[test]
fn to_service_time_toronto_switch_days() {
    let v = service_times(TORONTO, at(2026, 11, 1, 8, 0, 0, -5));
    assert!(
        has(&v, date(2026, 11, 1), 28_800) && has(&v, date(2026, 10, 31), 118_800),
        "{v:?}"
    );
    let v = service_times(TORONTO, at(2027, 3, 14, 8, 0, 0, -4));
    assert!(
        has(&v, date(2027, 3, 14), 28_800) && has(&v, date(2027, 3, 13), 111_600),
        "{v:?}"
    );
}

#[test]
fn to_service_time_ordinary_days_any_input_offset_and_midnight() {
    let v = service_times(BUCHAREST, at(2031, 1, 15, 0, 20, 0, 2));
    assert!(
        has(&v, date(2031, 1, 15), 1_200) && has(&v, date(2031, 1, 14), 87_600),
        "{v:?}"
    );
    // 06:00Z is 08:00 local: the input offset must not leak into the service time.
    let v = service_times(BUCHAREST, at(2031, 1, 15, 6, 0, 0, 0));
    assert!(has(&v, date(2031, 1, 15), 28_800), "{v:?}");
    let v = service_times(BUCHAREST, at(2031, 1, 14, 17, 0, 0, -13));
    assert!(has(&v, date(2031, 1, 15), 28_800), "{v:?}");
    // 00:00:00 and 23:59:59.
    let v = service_times(BUCHAREST, at(2031, 1, 15, 0, 0, 0, 2));
    assert!(
        has(&v, date(2031, 1, 15), 0) && has(&v, date(2031, 1, 14), 86_400),
        "{v:?}"
    );
    let v = service_times(BUCHAREST, at(2031, 1, 15, 23, 59, 59, 2));
    assert!(has(&v, date(2031, 1, 15), 86_399), "{v:?}");
}

#[test]
fn to_service_time_never_returns_a_negative_or_wrapped_time() {
    let mut rng = Rng(0x5EED_0005);
    for _ in 0..2_000 {
        let secs = rng.below(3 * 365 * 86_400) as i64;
        let off = (rng.below(27) as i32 - 12) * H;
        let t = DateTime::from_timestamp(1_780_000_000 + secs, 0)
            .unwrap()
            .with_timezone(&FixedOffset::east_opt(off).unwrap());
        for zone in [BUCHAREST, TORONTO] {
            let v = service_times(zone, t);
            assert!(!v.is_empty() && v.len() <= 3, "{t} {v:?}");
            for (d, s) in v {
                assert!(s < 4 * 86_400, "{zone} {t} → {d} {s}");
            }
        }
    }
}

// ---- DST end to end ---------------------------------------------------------------

/// Daily A→B trips at 02:00, 03:00, 04:00, 05:00, 08:00 (10 min each); N only on
/// Saturdays: A 25:30, B 27:30, C 29:30.
fn dst_feed(tz: &str, start: &str, end: &str) -> Feed {
    let mut f = Feed::daily(tz, start, end).service("SAT", "0000010", start, end);
    for (id, h) in [
        ("T0", 0),
        ("T1", 1),
        ("T2", 2),
        ("T3", 3),
        ("T4", 4),
        ("T5", 5),
        ("T8", 8),
    ] {
        let a = format!("{h:02}:00:00");
        let b = format!("{h:02}:10:00");
        f = f.trip("S", id, &[(a.as_str(), "A"), (b.as_str(), "B")]);
    }
    f.trip(
        "SAT",
        "N",
        &[("25:30:00", "A"), ("27:30:00", "B"), ("29:30:00", "C")],
    )
    .trip("S", "T0030", &[("00:30:00", "A"), ("00:40:00", "B")])
    .trip("S", "T0130", &[("01:30:00", "A"), ("01:40:00", "B")])
}

#[test]
fn bucharest_autumn_2026_10_25_end_to_end() {
    let e = engine(dst_feed(BUCHAREST, "20261001", "20261130").pack());
    let z = BUCHAREST;
    // Service 02:00 is 03:00+03:00 (00:00Z), service 03:00 is 03:00+02:00 (01:00Z).
    let it = first(&plan_stops(&e, "A", "B", at(2026, 10, 25, 2, 50, 0, 3)), z);
    assert_eq!(it.trips(), ["T2"], "{it:?}");
    assert!(same(board(&it), at(2026, 10, 25, 3, 0, 0, 3)), "{it:?}");
    // The two 03:30 instants (the input is FixedOffset, so it is never ambiguous).
    let it = first(&plan_stops(&e, "A", "B", at(2026, 10, 25, 3, 30, 0, 3)), z);
    assert_eq!(it.trips(), ["T3"], "{it:?}");
    assert!(same(board(&it), at(2026, 10, 25, 3, 0, 0, 2)), "{it:?}");
    let it = first(&plan_stops(&e, "A", "B", at(2026, 10, 25, 3, 30, 0, 2)), z);
    assert_eq!(it.trips(), ["T4"], "{it:?}");
    assert!(same(board(&it), at(2026, 10, 25, 4, 0, 0, 2)), "{it:?}");
    let it = first(&plan_stops(&e, "A", "B", at(2026, 10, 25, 7, 0, 0, 2)), z);
    assert!(same(board(&it), at(2026, 10, 25, 8, 0, 0, 2)), "{it:?}");
}

#[test]
fn bucharest_autumn_overnight_trip_from_saturday_spans_the_switch() {
    // N on Sat 2026-10-24: A 25:30 = 01:30+03:00, C 29:30 = 04:30+02:00 (4 real hours).
    // Needs D-1 shifted by the 25 h day (90 000 s), not 86 400 s.
    let e = engine(dst_feed(BUCHAREST, "20261001", "20261130").pack());
    let it = first(
        &plan_stops(&e, "A", "C", at(2026, 10, 25, 1, 0, 0, 3)),
        BUCHAREST,
    );
    assert_eq!(it.trips(), ["N"]);
    assert!(same(board(&it), at(2026, 10, 25, 1, 30, 0, 3)), "{it:?}");
    assert!(same(it.arr, at(2026, 10, 25, 4, 30, 0, 2)), "{it:?}");
    assert_eq!((it.arr - board(&it)).num_hours(), 4);
}

#[test]
fn bucharest_spring_2027_03_28_end_to_end() {
    let e = engine(dst_feed(BUCHAREST, "20270301", "20270430").pack());
    let z = BUCHAREST;
    // Service 02:00 is 01:00+02:00 (the reference is 23:00 the day before).
    let it = first(&plan_stops(&e, "A", "B", at(2027, 3, 28, 0, 50, 0, 2)), z);
    assert_eq!(it.trips(), ["T2"], "{it:?}");
    assert!(same(board(&it), at(2027, 3, 28, 1, 0, 0, 2)), "{it:?}");
    // 03:30 does not exist locally; both offsets are valid instants.
    let it = first(&plan_stops(&e, "A", "B", at(2027, 3, 28, 3, 30, 0, 2)), z);
    assert_eq!(it.trips(), ["T5"], "{it:?}");
    assert!(same(board(&it), at(2027, 3, 28, 5, 0, 0, 3)), "{it:?}");
    let it = first(&plan_stops(&e, "A", "B", at(2027, 3, 28, 3, 30, 0, 3)), z);
    assert_eq!(it.trips(), ["T4"], "{it:?}");
    assert!(same(board(&it), at(2027, 3, 28, 4, 0, 0, 3)), "{it:?}");
    // Overnight N from Sat 2027-03-27: 01:30+02:00 → 06:30+03:00.
    let it = first(&plan_stops(&e, "A", "C", at(2027, 3, 28, 1, 0, 0, 2)), z);
    assert_eq!(it.trips(), ["N"]);
    assert!(same(board(&it), at(2027, 3, 28, 1, 30, 0, 2)), "{it:?}");
    assert!(same(it.arr, at(2027, 3, 28, 6, 30, 0, 3)), "{it:?}");
}

#[test]
fn toronto_autumn_2026_11_01_and_spring_2027_03_14() {
    let z = TORONTO;
    let e = engine(dst_feed(z, "20261015", "20261115").pack());
    // Service 00:30 = 01:30-04:00, service 01:30 = 01:30-05:00 (wall time repeats).
    let it = first(&plan_stops(&e, "A", "B", at(2026, 11, 1, 1, 10, 0, -4)), z);
    assert_eq!(it.trips(), ["T0030"], "{it:?}");
    assert!(same(board(&it), at(2026, 11, 1, 1, 30, 0, -4)), "{it:?}");
    let it = first(&plan_stops(&e, "A", "B", at(2026, 11, 1, 1, 15, 0, -5)), z);
    assert_eq!(it.trips(), ["T0130"], "{it:?}");
    assert!(same(board(&it), at(2026, 11, 1, 1, 30, 0, -5)), "{it:?}");
    let it = first(&plan_stops(&e, "A", "B", at(2026, 11, 1, 7, 0, 0, -5)), z);
    assert!(same(board(&it), at(2026, 11, 1, 8, 0, 0, -5)), "{it:?}");

    let e = engine(dst_feed(z, "20270301", "20270331").pack());
    // Service 02:00 = 01:00-05:00, service 03:00 = 03:00-04:00.
    let it = first(&plan_stops(&e, "A", "B", at(2027, 3, 14, 0, 50, 0, -5)), z);
    assert_eq!(it.trips(), ["T2"], "{it:?}");
    assert!(same(board(&it), at(2027, 3, 14, 1, 0, 0, -5)), "{it:?}");
    let it = first(&plan_stops(&e, "A", "B", at(2027, 3, 14, 1, 30, 0, -5)), z);
    assert_eq!(it.trips(), ["T3"], "{it:?}");
    assert!(same(board(&it), at(2027, 3, 14, 3, 0, 0, -4)), "{it:?}");
    let it = first(&plan_stops(&e, "A", "B", at(2027, 3, 14, 7, 0, 0, -4)), z);
    assert!(same(board(&it), at(2027, 3, 14, 8, 0, 0, -4)), "{it:?}");
}

#[test]
fn local_0330_nonexistent_and_ambiguous_documented_choice() {
    // For naive local input (CLI `--at`): spring 03:30 does not exist; autumn 03:30
    // is ambiguous and the adapter takes the earlier (+03:00). The engine itself takes
    // a FixedOffset, so the choice is the caller's (see m5_time ambiguity note).
    let t = |y, m, d| date(y, m, d).and_hms_opt(3, 30, 0).unwrap();
    assert_eq!(local(BUCHAREST, t(2027, 3, 28)), None);
    assert!(same(
        local(BUCHAREST, t(2026, 10, 25)).unwrap(),
        at(2026, 10, 25, 3, 30, 0, 3)
    ));
    let e = engine(dst_feed(BUCHAREST, "20261001", "20261130").pack());
    let it = first(
        &plan_stops(&e, "A", "B", local(BUCHAREST, t(2026, 10, 25)).unwrap()),
        BUCHAREST,
    );
    assert_eq!(it.trips(), ["T3"]);
}

#[test]
fn query_offset_other_than_the_feed_zone_answers_in_the_feed_zone() {
    let e = engine(dst_feed(BUCHAREST, "20261001", "20261130").pack());
    for q in [
        at(2026, 10, 25, 5, 30, 0, 0),
        at(2026, 10, 25, 0, 30, 0, -5),
        at(2026, 10, 25, 18, 30, 0, 13),
    ] {
        let it = first(&plan_stops(&e, "A", "B", q), BUCHAREST);
        assert_eq!(it.trips(), ["T8"], "{q}");
        assert!(
            same(board(&it), at(2026, 10, 25, 8, 0, 0, 2)),
            "{q}: {it:?}"
        );
        assert_eq!(it.dep.offset().local_minus_utc(), 2 * H, "{q}");
    }
}

// ---- UC4 midnight -----------------------------------------------------------------

/// January 2031. N1 only on Fri 2031-01-10: A 24:10, B 24:30, C 24:50. N2 daily:
/// A 23:50, B 24:10, C 24:30. M daily: A 12:00, B 12:20, C 12:40. E24 (Fri only)
/// A 24:00:00 → D 24:10; E0 daily A 00:00:00 → D 00:10. L on Sat 2031-01-11 only:
/// A 47:50 → F 48:30.
fn uc4_feed(tz: &str, with_n2: bool) -> Feed {
    let mut f = Feed::daily(tz, "20310101", "20310131")
        .service("FRI", "0000000", "20310101", "20310131")
        .exception("FRI", "20310110", 1)
        .service("SAT", "0000000", "20310101", "20310131")
        .exception("SAT", "20310111", 1)
        .trip(
            "FRI",
            "N1",
            &[("24:10:00", "A"), ("24:30:00", "B"), ("24:50:00", "C")],
        )
        .trip(
            "S",
            "M",
            &[("12:00:00", "A"), ("12:20:00", "B"), ("12:40:00", "C")],
        )
        .trip("FRI", "E24", &[("24:00:00", "A"), ("24:10:00", "D")])
        .trip("S", "E0", &[("00:00:00", "A"), ("00:10:00", "D")])
        .trip("SAT", "L", &[("47:50:00", "A"), ("48:30:00", "F")]);
    if with_n2 {
        f = f.trip(
            "S",
            "N2",
            &[("23:50:00", "A"), ("24:10:00", "B"), ("24:30:00", "C")],
        );
    }
    f
}

fn uc4(zone: &str, off: i32) {
    let e = engine(uc4_feed(zone, false).pack());
    // 23:50 on Friday → N1 at 00:10 Saturday.
    let it = first(
        &plan_stops(&e, "A", "C", at(2031, 1, 10, 23, 50, 0, off)),
        zone,
    );
    assert_eq!(it.trips(), ["N1"], "{it:?}");
    assert!(same(board(&it), at(2031, 1, 11, 0, 10, 0, off)), "{it:?}");
    assert!(same(it.arr, at(2031, 1, 11, 0, 50, 0, off)), "{it:?}");
    // 00:05 Saturday → N1 through D-1 (Saturday has nothing before 12:00).
    let it = first(
        &plan_stops(&e, "A", "C", at(2031, 1, 11, 0, 5, 0, off)),
        zone,
    );
    assert_eq!(it.trips(), ["N1"], "{it:?}");
    assert!(same(board(&it), at(2031, 1, 11, 0, 10, 0, off)));
    // 00:20 Saturday from B → N1 at 00:30 (24:30 of Friday).
    let it = first(
        &plan_stops(&e, "B", "C", at(2031, 1, 11, 0, 20, 0, off)),
        zone,
    );
    assert_eq!(it.trips(), ["N1"], "{it:?}");
    assert!(same(board(&it), at(2031, 1, 11, 0, 30, 0, off)));
    // 00:20 Saturday from A: N1 left at 00:10 → M at 12:00 Saturday.
    let it = first(
        &plan_stops(&e, "A", "C", at(2031, 1, 11, 0, 20, 0, off)),
        zone,
    );
    assert_eq!(it.trips(), ["M"], "{it:?}");
    assert!(same(board(&it), at(2031, 1, 11, 12, 0, 0, off)));
    // 00:05 Sunday: N1 did not run on Saturday → M on Sunday.
    let it = first(
        &plan_stops(&e, "A", "C", at(2031, 1, 12, 0, 5, 0, off)),
        zone,
    );
    assert_eq!(it.trips(), ["M"], "{it:?}");
}

#[test]
fn uc4_midnight_bucharest() {
    uc4(BUCHAREST, 2);
}

#[test]
fn uc4_midnight_toronto() {
    uc4(TORONTO, -5);
}

#[test]
fn uc4_results_from_d_minus_1_and_d_are_deduplicated_and_pareto() {
    let e = engine(uc4_feed(BUCHAREST, true).pack());
    for (from, q) in [
        ("A", at(2031, 1, 10, 23, 40, 0, 2)),
        ("B", at(2031, 1, 11, 0, 5, 0, 2)),
        ("A", at(2031, 1, 11, 0, 0, 0, 2)),
    ] {
        let v = plan_stops(&e, from, "C", q).expect("route");
        let mut seen = std::collections::HashSet::new();
        for it in &v {
            check(it, BUCHAREST);
            for l in &it.legs {
                assert!(
                    seen.insert((l.0.clone(), l.3)),
                    "duplicate ride {l:?} in {v:?}"
                );
            }
        }
        for a in &v {
            for b in &v {
                let dominated = b.dep >= a.dep
                    && b.arr <= a.arr
                    && b.rides <= a.rides
                    && (b.dep, b.arr, b.rides) != (a.dep, a.arr, a.rides);
                assert!(!dominated, "{a:?} dominated by {b:?}");
            }
        }
    }
    // B at 00:05 Saturday: N2 of Friday (B 24:10) is the earliest arrival, 00:30.
    let it = first(
        &plan_stops(&e, "B", "C", at(2031, 1, 11, 0, 5, 0, 2)),
        BUCHAREST,
    );
    assert_eq!(it.trips(), ["N2"]);
    assert!(same(it.arr, at(2031, 1, 11, 0, 30, 0, 2)));
}

#[test]
fn exact_midnight_and_one_second_before() {
    let e = engine(uc4_feed(BUCHAREST, false).pack());
    for q in [at(2031, 1, 10, 23, 59, 59, 2), at(2031, 1, 11, 0, 0, 0, 2)] {
        let it = first(&plan_stops(&e, "A", "D", q), BUCHAREST);
        assert!(same(board(&it), at(2031, 1, 11, 0, 0, 0, 2)), "{q}: {it:?}");
        assert!(same(it.arr, at(2031, 1, 11, 0, 10, 0, 2)), "{q}: {it:?}");
    }
}

#[test]
fn trip_at_or_after_48_00() {
    // L on Saturday 2031-01-11 at 47:50 = Sunday 23:50, arriving 48:30 = Monday 00:30.
    let e = engine(uc4_feed(BUCHAREST, false).pack());
    let it = first(
        &plan_stops(&e, "A", "F", at(2031, 1, 12, 23, 0, 0, 2)),
        BUCHAREST,
    );
    assert_eq!(it.trips(), ["L"], "{it:?}");
    assert!(same(board(&it), at(2031, 1, 12, 23, 50, 0, 2)));
    assert!(same(it.arr, at(2031, 1, 13, 0, 30, 0, 2)));
}

// ---- validity window ---------------------------------------------------------------

#[test]
fn date_outside_feed_and_feed_expires_soon_at_the_boundaries() {
    // Feed valid 2031-01-01..2031-01-31. Expiry counts from the query date (not the
    // wall clock); "within 14 days" read as valid_to - date <= 14.
    let e = engine(uc4_feed(BUCHAREST, false).pack());
    let q = |d: u32, m: u32, y: i32| req(stop_pos("A"), stop_pos("C"), at(y, m, d, 8, 0, 0, 2));
    assert!(is_date_outside(&plan(&e, &q(31, 12, 2030))));
    assert!(is_date_outside(&plan(&e, &q(1, 2, 2031))));
    assert!(plan(&e, &q(1, 1, 2031)).is_ok());
    assert_eq!(expires_soon(&e, &q(16, 1, 2031)), None, "15 days left");
    assert_eq!(
        expires_soon(&e, &q(17, 1, 2031)),
        Some(date(2031, 1, 31)),
        "14 days left"
    );
    assert_eq!(
        expires_soon(&e, &q(31, 1, 2031)),
        Some(date(2031, 1, 31)),
        "last day"
    );
    assert_eq!(expires_soon(&e, &q(5, 1, 2031)), None);
}

#[test]
fn last_valid_day_late_query_is_no_route_not_date_outside() {
    let e = engine(uc4_feed(BUCHAREST, false).pack());
    let r = plan_stops(&e, "B", "C", at(2031, 1, 31, 23, 59, 0, 2));
    assert!(is_no_route(&r), "{r:?}");
}

#[test]
fn evening_query_finds_the_next_morning_trip_d_plus_1() {
    let e = engine(uc4_feed(BUCHAREST, false).pack());
    let it = first(
        &plan_stops(&e, "B", "C", at(2031, 1, 15, 23, 30, 0, 2)),
        BUCHAREST,
    );
    assert_eq!(it.trips(), ["M"]);
    assert!(same(board(&it), at(2031, 1, 16, 12, 20, 0, 2)), "{it:?}");
}

#[test]
fn just_after_valid_to_only_a_d_minus_1_trip_can_serve() {
    // 2031-02-01 00:05 from B: N2 of 2031-01-31 leaves B at 24:10, but the query's
    // local date is after valid_to, so it is DateOutsideFeed with no D-1 rescue
    // (Leader, M5 decisions; narrowed by Builder after the M5 review).
    let e = engine(uc4_feed(BUCHAREST, true).pack());
    let r = plan_stops(&e, "B", "C", at(2031, 2, 1, 0, 5, 0, 2));
    assert!(is_date_outside(&r), "{r:?}");
}

// ---- warnings from frequencies and transfers ---------------------------------------

#[test]
fn approximate_frequency_trip_warning() {
    for (exact, want) in [("0", true), ("1", false)] {
        let p = Feed::daily(BUCHAREST, "20310101", "20310131")
            .trip("S", "F", &[("00:00:00", "A"), ("00:10:00", "B")])
            .freq("F", "06:00:00", "09:00:00", "600", exact)
            .pack();
        let e = engine(p);
        let r = req(stop_pos("A"), stop_pos("B"), at(2031, 1, 15, 7, 0, 0, 2));
        assert_eq!(approximate_warning(&e, &r), want, "exact_times={exact}");
    }
}

#[test]
fn unknown_and_unsupported_transfer_rows_are_report_warnings() {
    let t = "from_stop_id,to_stop_id,transfer_type,min_transfer_time\nA,B,4,\nA,B,5,\nA,B,9,\nA,B,2,60\n";
    let tf = Feed::daily(BUCHAREST, "20310101", "20310131")
        .trip("S", "t", &[("08:00:00", "A"), ("08:10:00", "B")])
        .transfers(t)
        .write();
    let (_, report) = tf
        .build()
        .expect("unknown transfer types must not fail the build");
    assert_eq!(unsupported_transfers(&report), 3);
    assert!(unsupported_transfer_is_warning());
}
