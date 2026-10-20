//! M5 calendar tests (Tester, written before M5). Plan M5 step 4 / UC5, DoD
//! "Holidays", edge-case block "Time and calendar" (calendar_dates-only feed, empty
//! calendar.txt, start_date > end_date, type 2 on a day the calendar never had,
//! types 1 and 2 on one day), "a service active only on D-1", "feed with a single
//! service day", multi-agency timezones, invalid IANA names.
//! These use only APIs that exist since M3 (service bits + stop-to-stop RAPTOR), so
//! they compile now; the "M5 not implemented" ignores were removed in M5.
//! Feed test: `RUSTLINE_M5_FEED=fixtures/feeds/brasov-latest.zip` (skips when unset).
//! Run: `cargo test --locked -p rustline-core --test m5_calendar -- --include-ignored`

mod m5_common;

use m5_common::*;

/// 2026-12-01 (Tuesday, Romania's national day) in a synthetic Bucharest feed:
/// weekday service W removed (type 2), holiday service H added (type 1, no calendar row).
fn december_feed() -> Feed {
    Feed::daily(BUCHAREST, "20261101", "20261231")
        .service("W", "1111100", "20261101", "20261231")
        .exception("W", "20261201", 2)
        .exception("H", "20261201", 1)
        .trip("W", "weekday", &[("07:30:00", "A"), ("07:50:00", "B")])
        .trip("H", "holiday", &[("09:00:00", "A"), ("09:20:00", "B")])
        .trip("S", "evening", &[("20:00:00", "A"), ("20:20:00", "B")])
}

#[test]
fn uc5_first_of_december_removes_weekday_and_adds_holiday_service() {
    let p = december_feed().pack();
    assert!(!runs(&p, "W", date(2026, 12, 1)), "type 2 removes W");
    assert!(runs(&p, "H", date(2026, 12, 1)), "type 1 adds H");
    assert!(runs(&p, "W", date(2026, 12, 2)));
    assert!(!runs(&p, "H", date(2026, 12, 2)));
    // RAPTOR at 07:00: holiday trip at 09:00 on 1 Dec, weekday trip on 2 Dec.
    let r = rides(&p, "A", "B", date(2026, 12, 1), hms(7, 0, 0)).unwrap();
    assert_eq!(r[0].0, vec!["holiday"], "{r:?}");
    assert_eq!(r[0].2, hms(9, 20, 0));
    let r = rides(&p, "A", "B", date(2026, 12, 2), hms(7, 0, 0)).unwrap();
    assert_eq!(r[0].0, vec!["weekday"], "{r:?}");
}

#[test]
fn type_1_adds_a_saturday_to_a_weekday_service() {
    // Sat 2026-11-07 gets W via type 1.
    let p = december_feed().exception("W", "20261107", 1).pack();
    assert!(runs(&p, "W", date(2026, 11, 7)));
    assert!(!runs(&p, "W", date(2026, 11, 14)));
    assert_eq!(
        earliest(&p, "A", "B", date(2026, 11, 7), hms(7, 0, 0)),
        Some(hms(7, 50, 0))
    );
}

#[test]
fn calendar_dates_only_feed() {
    // No calendar.txt at all: services come from calendar_dates type 1 rows.
    let mut f = Feed::daily(BUCHAREST, "x", "x");
    f.calendar = None;
    let f = f
        .exception("S", "20310110", 1)
        .exception("S", "20310112", 1)
        .trip("S", "t", &[("08:00:00", "A"), ("08:10:00", "B")]);
    let p = f.pack();
    assert_eq!(p.meta().valid_from, date(2031, 1, 10));
    assert_eq!(p.meta().valid_to, date(2031, 1, 12));
    assert!(runs(&p, "S", date(2031, 1, 10)));
    assert!(!runs(&p, "S", date(2031, 1, 11)));
    assert!(runs(&p, "S", date(2031, 1, 12)));
    assert_eq!(
        earliest(&p, "A", "B", date(2031, 1, 12), hms(7, 0, 0)),
        Some(hms(8, 10, 0))
    );
}

#[test]
fn empty_calendar_txt_with_calendar_dates() {
    // calendar.txt with the header only.
    let mut f = Feed::daily(BUCHAREST, "x", "x");
    f.calendar = Some(Vec::new());
    let f = f
        .exception("S", "20310110", 1)
        .trip("S", "t", &[("08:00:00", "A"), ("08:10:00", "B")]);
    let p = f.pack();
    assert!(runs(&p, "S", date(2031, 1, 10)));
    assert_eq!(
        (p.meta().valid_from, p.meta().valid_to),
        (date(2031, 1, 10), date(2031, 1, 10))
    );
}

#[test]
fn service_with_start_after_end_never_runs_and_does_not_break_the_feed() {
    let f = Feed::daily(BUCHAREST, "20310101", "20310131")
        .service("BAD", "1111111", "20310120", "20310110")
        .trip("S", "ok", &[("08:00:00", "A"), ("08:10:00", "B")])
        .trip("BAD", "bad", &[("07:00:00", "A"), ("07:10:00", "B")]);
    let tf = f.write();
    match tf.build() {
        // A clear build error is also acceptable; but no panic and no "runs every day".
        Err(e) => println!("start > end rejected: {e}"),
        Ok(_) => {
            let p = tf.pack();
            for d in 1..=31 {
                assert!(
                    !runs(&p, "BAD", date(2031, 1, d)),
                    "BAD runs on 2031-01-{d}"
                );
            }
            assert_eq!(
                earliest(&p, "A", "B", date(2031, 1, 15), hms(6, 0, 0)),
                Some(hms(8, 10, 0))
            );
        }
    }
}

#[test]
fn type_2_on_a_day_the_calendar_never_had_is_harmless() {
    // W is weekdays only; remove Saturday 2026-11-07 (it never ran) and a day after
    // its end_date.
    let p = december_feed()
        .exception("W", "20261107", 2)
        .exception("W", "20270115", 2)
        .pack();
    assert!(!runs(&p, "W", date(2026, 11, 7)));
    assert!(runs(&p, "W", date(2026, 11, 6)));
    assert!(runs(&p, "W", date(2026, 11, 9)));
}

#[test]
fn type_1_and_type_2_on_the_same_day_are_deterministic() {
    // GTFS gives no rule; whatever Rustline picks must not panic and must not depend on
    // the row order (both orders give the same answer).
    let a = december_feed()
        .exception("W", "20261110", 1)
        .exception("W", "20261110", 2)
        .pack();
    let b = december_feed()
        .exception("W", "20261110", 2)
        .exception("W", "20261110", 1)
        .pack();
    let (ra, rb) = (
        runs(&a, "W", date(2026, 11, 10)),
        runs(&b, "W", date(2026, 11, 10)),
    );
    println!("W on 2026-11-10 with type 1 and 2: runs = {ra}");
    assert_eq!(ra, rb, "row order changes the calendar");
}

#[test]
fn feed_with_a_single_service_day() {
    let f = Feed::daily(BUCHAREST, "20310110", "20310110").trip(
        "S",
        "t",
        &[("08:00:00", "A"), ("08:10:00", "B")],
    );
    let p = f.pack();
    assert_eq!(
        (p.meta().valid_from, p.meta().valid_to),
        (date(2031, 1, 10), date(2031, 1, 10))
    );
    assert_eq!(
        earliest(&p, "A", "B", date(2031, 1, 10), hms(0, 0, 0)),
        Some(hms(8, 10, 0))
    );
    assert!(matches!(
        rides(&p, "A", "B", date(2031, 1, 11), hms(8, 0, 0)),
        Err(rustline_core::QueryError::DateOutsideFeed { .. })
    ));
}

#[test]
fn date_outside_feed_at_the_exact_boundary_days_stop_to_stop() {
    // DoD: day before valid_from and day after valid_to → DateOutsideFeed; the
    // boundary days themselves work.
    let p = Feed::daily(BUCHAREST, "20310101", "20310131")
        .trip("S", "t", &[("08:00:00", "A"), ("08:10:00", "B")])
        .pack();
    let out = |d| {
        matches!(
            rides(&p, "A", "B", d, hms(7, 0, 0)),
            Err(rustline_core::QueryError::DateOutsideFeed { .. })
        )
    };
    assert!(out(date(2030, 12, 31)));
    assert!(!out(date(2031, 1, 1)));
    assert!(!out(date(2031, 1, 31)));
    assert!(out(date(2031, 2, 1)));
    assert_eq!(
        earliest(&p, "A", "B", date(2031, 1, 31), hms(7, 0, 0)),
        Some(hms(8, 10, 0))
    );
}

#[test]
fn agencies_with_different_timezones_are_rejected() {
    let mut f = Feed::daily(BUCHAREST, "20310101", "20310131").trip(
        "S",
        "t",
        &[("08:00:00", "A"), ("08:10:00", "B")],
    );
    f.more_agencies.push((s("AG2"), s(TORONTO)));
    let tf = f.write();
    let r = tf
        .load()
        .and_then(|raw| rustline_gtfs::build_timetable(&raw));
    assert!(
        matches!(r, Err(rustline_gtfs::GtfsError::MixedTimezones(_))),
        "{:?}",
        r.err()
    );
}

#[test]
fn invalid_iana_timezone_is_rejected_at_build_time() {
    // DoD "Invalid IANA timezone rejected at build time (M1 gap #4)". Stricter than
    // the M1 test: the build itself fails, not only `RawFeed::timezone()`.
    for bad in ["Mars/Olympus", "Europe/Bucuresti", "UTC+2", ""] {
        let tf = Feed::daily(bad, "20310101", "20310131")
            .trip("S", "t", &[("08:00:00", "A"), ("08:10:00", "B")])
            .write();
        let r = tf
            .load()
            .and_then(|raw| rustline_gtfs::build_timetable(&raw));
        assert!(r.is_err(), "{bad:?} accepted");
    }
}

#[test]
fn valid_iana_names_are_still_accepted() {
    // Guard against an over-strict check: real IANA names, including the less common
    // forms, must build.
    for good in [
        BUCHAREST,
        TORONTO,
        "America/Argentina/Buenos_Aires",
        "Etc/GMT+5",
        "UTC",
        "Pacific/Auckland",
    ] {
        let tf = Feed::daily(good, "20310101", "20310131")
            .trip("S", "t", &[("08:00:00", "A"), ("08:10:00", "B")])
            .write();
        let r = tf
            .load()
            .and_then(|raw| rustline_gtfs::build_timetable(&raw));
        assert!(r.is_ok(), "{good}: {:?}", r.err());
        assert_eq!(tf.pack().meta().timezone, good);
    }
}

#[test]
fn engine_refuses_a_pack_with_an_invalid_timezone() {
    // Plan M5 step 1: the zone is parsed once in Engine::open; invalid name → PackError.
    // A pack written by an older build (or tampered) can carry any string.
    let p = Feed::daily(BUCHAREST, "20310101", "20310131")
        .trip("S", "t", &[("08:00:00", "A"), ("08:10:00", "B")])
        .pack();
    let (mut meta, data) = p.into_parts();
    meta.timezone = s("Mars/Olympus");
    let mut bytes = Vec::new();
    rustline_pack::write_pack(&data, &meta, &mut bytes).unwrap();
    assert!(rustline_core::Engine::from_bytes(&bytes).is_err());
}

#[test]
fn brasov_first_of_december_and_an_exception_date_from_the_real_feed() {
    // DoD "Holidays … + a Brașov date with an exception". brasov-latest (2026-08-10 to
    // 2027-12-09): on Tue 2026-12-01 Mo-Fr and TE:Mo-Fr are removed (type 2), Sa-Su and
    // Su are added (type 1); on Mon 2026-10-05 only TE:Mo-Fr is removed.
    let Some(feed) = env_feed(FEED_VAR) else {
        return;
    };
    let p = pack_of(&feed);
    if !p.service_ids.iter().any(|x| x == "TE:Mo-Fr") {
        println!(
            "skip: {} is not brasov-latest (no TE:Mo-Fr service)",
            feed.display()
        );
        return;
    }
    let dec1 = date(2026, 12, 1);
    assert!(!runs(&p, "Mo-Fr", dec1));
    assert!(!runs(&p, "TE:Mo-Fr", dec1));
    assert!(runs(&p, "Sa-Su", dec1));
    assert!(runs(&p, "Su", dec1));
    assert!(!runs(&p, "Sa", dec1));
    let dec2 = date(2026, 12, 2);
    assert!(runs(&p, "Mo-Fr", dec2) && !runs(&p, "Sa-Su", dec2));
    let oct5 = date(2026, 10, 5);
    assert!(runs(&p, "Mo-Fr", oct5) && !runs(&p, "TE:Mo-Fr", oct5));
    // Every ride RAPTOR offers on 1 December belongs to a service running that day.
    let mut rng = Rng(0x4D35_DEC1);
    let mut checked = 0;
    for _ in 0..200 {
        let a = rng.below(p.stop_count() as u64) as usize;
        let b = rng.below(p.stop_count() as u64) as usize;
        let js = rustline_core::raptor_stop_to_stop(
            &p,
            rustline_model::StopIdx::from_usize(a),
            rustline_model::StopIdx::from_usize(b),
            dec1,
            rustline_model::Seconds(hms(7, 0, 0)),
            3,
        )
        .unwrap();
        for j in &js {
            for l in &j.legs {
                if let rustline_model::Leg::Transit { trip, .. } = l {
                    let sv = &p.service_ids[p.trip_service[trip.index()] as usize];
                    assert!(
                        sv == "Sa-Su" || sv == "Su",
                        "trip {} of {sv} used on 1 December",
                        p.trip_ids[trip.index()]
                    );
                    checked += 1;
                }
            }
        }
    }
    println!("brasov 1 December: {checked} rides checked");
    assert!(checked > 50, "{checked}");
}
