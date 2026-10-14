//! Tester (M3 verify): gaps found while checking M3. Tester-owned.
//! Run: `cargo test --locked -p rustline-core --test tester_m3 -- --include-ignored`

mod m3_common;

use m3_common::*;
use rustline_core::{raptor_pareto, raptor_stop_to_stop};
use rustline_model::{Leg, Seconds};

/// Stops for the walk/ride label case below: O, A, B, Y, Z.
const STOPS: &str = "stop_id,stop_name,stop_lat,stop_lon\n\
                     O,O,1.0,1.0\nA,A,1.0,1.1\nB,B,1.0,1.2\nY,Y,1.0,1.3\nZ,Z,1.0,1.4\n";

/// a: O 08:00 → A 08:10, then transfers.txt A → Y (60 s): Y by walking at 08:11
/// after 1 ride. b: O 08:00 → B 08:05; c: B 08:06 → Y 08:20: Y by a ride at
/// 08:20 after 2 rides. Z is only reachable by the transfers.txt row Y → Z
/// (60 s), and (M3 decision 8: one hop after a ride, no chained walks) only
/// from the *ride* arrival at Y. Correct answer: Z at 08:21 with 2 rides.
/// Found by the CSA oracle on MBTA (seed 0xA11CE5EED0001, query #43: RAPTOR
/// 87024 s, CSA 86364 s): the earlier walk arrival at Y prunes the later ride
/// arrival, so the onward transfer from it is lost.
#[test]
fn earlier_walk_arrival_does_not_hide_the_ride_arrival_needed_to_transfer_on() {
    let f = TempFeed::new(&[
        ("stops.txt", STOPS),
        ("trips.txt", &trips(&[("R", "a"), ("R", "b"), ("Q", "c")])),
        (
            "stop_times.txt",
            &stop_times(&[
                ("a", "08:00:00", "O"),
                ("a", "08:10:00", "A"),
                ("b", "08:00:00", "O"),
                ("b", "08:05:00", "B"),
                ("c", "08:06:00", "B"),
                ("c", "08:20:00", "Y"),
            ]),
        ),
        (
            "transfers.txt",
            "from_stop_id,to_stop_id,transfer_type,min_transfer_time\nA,Y,2,60\nY,Z,2,60\n",
        ),
    ]);
    let tt = f.tt();
    let (o, z) = (stop(&tt, "O"), stop(&tt, "Z"));
    let d = date(2030, 3, 1);
    let js = raptor_pareto(&tt, o, z, d, hms(7, 0, 0), 4).unwrap();
    let best = js.iter().map(|j| j.arr.0).min();
    assert_eq!(best, Some(hms(8, 21, 0).0), "{js:?}");
    let j = js.iter().find(|j| j.arr == hms(8, 21, 0)).unwrap();
    assert_eq!(j.rides, 2);
    assert!(matches!(j.legs.last(), Some(Leg::Walk { .. })), "{j:?}");
    // The stop-to-stop answer must contain it too (it is the only journey).
    let js = raptor_stop_to_stop(&tt, o, z, d, hms(7, 0, 0), 4).unwrap();
    assert_eq!(js.len(), 1, "{js:?}");
}

/// Same feed without the A → Y row: nothing to hide, Z at 08:21 (passes today;
/// shows the case above fails only because of the walk arrival).
#[test]
fn ride_then_transfer_reaches_z_without_the_competing_walk() {
    let f = TempFeed::new(&[
        ("stops.txt", STOPS),
        ("trips.txt", &trips(&[("R", "a"), ("R", "b"), ("Q", "c")])),
        (
            "stop_times.txt",
            &stop_times(&[
                ("a", "08:00:00", "O"),
                ("a", "08:10:00", "A"),
                ("b", "08:00:00", "O"),
                ("b", "08:05:00", "B"),
                ("c", "08:06:00", "B"),
                ("c", "08:20:00", "Y"),
            ]),
        ),
        (
            "transfers.txt",
            "from_stop_id,to_stop_id,transfer_type,min_transfer_time\nY,Z,2,60\n",
        ),
    ]);
    let tt = f.tt();
    let js = raptor_pareto(
        &tt,
        stop(&tt, "O"),
        stop(&tt, "Z"),
        date(2030, 3, 1),
        hms(7, 0, 0),
        4,
    )
    .unwrap();
    assert_eq!(
        js.iter().map(|j| j.arr.0).min(),
        Some(hms(8, 21, 0).0),
        "{js:?}"
    );
}

/// No chained walks (M3 decision 8): O → A ride, A → Y → Z needs two hops.
#[test]
fn transfers_are_not_chained() {
    let f = TempFeed::new(&[
        ("stops.txt", STOPS),
        ("trips.txt", &trips(&[("R", "a")])),
        (
            "stop_times.txt",
            &stop_times(&[("a", "08:00:00", "O"), ("a", "08:10:00", "A")]),
        ),
        (
            "transfers.txt",
            "from_stop_id,to_stop_id,transfer_type,min_transfer_time\nA,Y,2,60\nY,Z,2,60\n",
        ),
    ]);
    let tt = f.tt();
    let d = date(2030, 3, 1);
    let y = raptor_pareto(&tt, stop(&tt, "O"), stop(&tt, "Y"), d, hms(7, 0, 0), 4).unwrap();
    assert_eq!(
        y.iter().map(|j| j.arr.0).min(),
        Some(hms(8, 11, 0).0),
        "{y:?}"
    );
    let z = raptor_pareto(&tt, stop(&tt, "O"), stop(&tt, "Z"), d, hms(7, 0, 0), 4).unwrap();
    assert!(z.is_empty(), "{z:?}");
}

/// M3 bumps the pack format (header names v3; README says format version 3).
#[test]
fn format_version_is_3() {
    assert_eq!(rustline_pack::FORMAT_VERSION, 3);
}

/// M3 DoD: golden journeys "checked by hand … who/when checked is noted".
/// m3_golden.rs only looks for "checked by", which "Checked by: PENDING" passes.
#[test]
fn golden_snapshots_are_hand_checked() {
    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/m3_golden_brasov.rs"),
    )
    .unwrap();
    let lower = src.to_lowercase();
    assert!(lower.contains("checked by"), "no 'Checked by' line");
    assert!(
        !lower.contains("checked by: pending"),
        "golden journeys not hand-checked yet"
    );
    let _ = Seconds(0);
}

/// Suspected in M3 verify: a walk that improves a stop which a ride also
/// improved in the same round must not be walked on from (that would be two
/// hops). a: O 08:00 → A 08:10; b: O 08:00 → Y 08:30. A → Y 60 s, Y → Z 60 s.
/// Z only via the ride arrival at Y: 08:31, 1 ride (chaining would give 08:12).
/// Both stop orders, so the result can't depend on scan order.
#[test]
fn walk_improving_a_ride_reached_stop_is_not_walked_on_in_the_same_round() {
    for stops in [
        "stop_id,stop_name,stop_lat,stop_lon\nO,O,1,1\nA,A,1,2\nY,Y,1,3\nZ,Z,1,4\n",
        "stop_id,stop_name,stop_lat,stop_lon\nZ,Z,1,4\nY,Y,1,3\nA,A,1,2\nO,O,1,1\n",
    ] {
        for xfers in ["A,Y,2,60\nY,Z,2,60\n", "Y,Z,2,60\nA,Y,2,60\n"] {
            let f = TempFeed::new(&[
                ("stops.txt", stops),
                ("trips.txt", &trips(&[("R", "a"), ("Q", "b")])),
                (
                    "stop_times.txt",
                    &stop_times(&[
                        ("a", "08:00:00", "O"),
                        ("a", "08:10:00", "A"),
                        ("b", "08:00:00", "O"),
                        ("b", "08:30:00", "Y"),
                    ]),
                ),
                (
                    "transfers.txt",
                    &format!("from_stop_id,to_stop_id,transfer_type,min_transfer_time\n{xfers}"),
                ),
            ]);
            let tt = f.tt();
            let d = date(2030, 3, 1);
            let js =
                raptor_pareto(&tt, stop(&tt, "O"), stop(&tt, "Z"), d, hms(7, 0, 0), 4).unwrap();
            let got: Vec<_> = js.iter().map(|j| (j.rides, j.arr.0)).collect();
            assert_eq!(got, [(1, hms(8, 31, 0).0)], "{stops:?} {xfers:?}: {js:?}");
            // Y itself: walk 08:11 (1 ride) is the earliest.
            let y = raptor_pareto(&tt, stop(&tt, "O"), stop(&tt, "Y"), d, hms(7, 0, 0), 4).unwrap();
            assert_eq!(
                y.iter().map(|j| j.arr.0).min(),
                Some(hms(8, 11, 0).0),
                "{y:?}"
            );
        }
    }
}

/// A 0-ride journey (one hop from the origin straight to the destination) and
/// a ride that arrives earlier are both Pareto-optimal; a ride arriving later
/// than the walk is dominated and must not be returned.
#[test]
fn zero_ride_hop_to_destination_is_pareto_sound() {
    let stops = "stop_id,stop_name,stop_lat,stop_lon\nO,O,1,1\nD,D,1,2\n";
    for (walk, want) in [
        (600u32, vec![(0u8, 7 * 3600 + 600), (1, 7 * 3600 + 300)]),
        (60, vec![(0, 7 * 3600 + 60)]),
    ] {
        let f = TempFeed::new(&[
            ("stops.txt", stops),
            ("trips.txt", &trips(&[("R", "a")])),
            (
                "stop_times.txt",
                &stop_times(&[("a", "07:01:00", "O"), ("a", "07:05:00", "D")]),
            ),
            (
                "transfers.txt",
                &format!("from_stop_id,to_stop_id,transfer_type,min_transfer_time\nO,D,2,{walk}\n"),
            ),
        ]);
        let tt = f.tt();
        let mut js = raptor_pareto(
            &tt,
            stop(&tt, "O"),
            stop(&tt, "D"),
            date(2030, 3, 1),
            hms(7, 0, 0),
            4,
        )
        .unwrap();
        js.sort_by_key(|j| j.rides);
        let got: Vec<_> = js.iter().map(|j| (j.rides, j.arr.0)).collect();
        assert_eq!(got, want, "walk {walk}: {js:?}");
        let z = &js[0];
        assert_eq!(z.dep, hms(7, 0, 0));
        assert!(
            matches!(z.legs[..], [Leg::Walk { secs, .. }] if secs == walk),
            "{z:?}"
        );
    }
}

/// Pack::new must reject an inconsistent timetable with Err (never panic).
#[test]
fn pack_new_rejects_inconsistent_timetables() {
    let good = tiny();
    let meta = good.meta().clone();
    let base = good.timetable().clone();
    let mut cases: Vec<(&str, rustline_pack::TimetableData)> = Vec::new();
    let mut t = base.clone();
    t.stop_ids.pop();
    cases.push(("stop_ids short", t));
    let mut t = base.clone();
    t.pattern_stops_start.push(u32::MAX);
    cases.push(("pattern_stops_start extra", t));
    let mut t = base.clone();
    if let Some(x) = t.pattern_stops.first_mut() {
        *x = u32::MAX;
    }
    cases.push(("pattern stop out of range", t));
    let mut t = base.clone();
    t.stop_times.pop();
    cases.push(("stop_times odd", t));
    let mut t = base.clone();
    t.trip_pattern[0] = u32::MAX;
    cases.push(("trip pattern out of range", t));
    let mut t = base.clone();
    t.service_words = 0;
    cases.push(("no service words", t));
    let mut t = base.clone();
    t.transfer_start = vec![0, 5];
    cases.push(("transfer_start wrong length", t));
    for (name, t) in cases {
        let r = std::panic::catch_unwind(|| rustline_pack::Pack::new(meta.clone(), t));
        match r {
            Ok(Err(_)) => {}
            Ok(Ok(_)) => panic!("{name}: accepted"),
            Err(_) => panic!("{name}: panicked"),
        }
    }
    assert!(rustline_pack::Pack::new(meta, base).is_ok());
}
