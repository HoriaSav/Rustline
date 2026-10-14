//! Builder's extra M3 tests: transfers.txt types and walking from the origin.

mod m3_common;

use m3_common::*;
use rustline_core::{raptor_stop_to_stop, MAX_TRANSFERS};
use rustline_model::{Journey, Leg};

/// a: X 08:00 → Y 08:10; b: Y 08:15 → Z 08:30; c: W 08:12 → Z 08:40.
fn feed(transfers: &str) -> TempFeed {
    let st = stop_times(&[
        ("a", "08:00:00", "X"),
        ("a", "08:10:00", "Y"),
        ("b", "08:15:00", "Y"),
        ("b", "08:30:00", "Z"),
        ("c", "08:12:00", "W"),
        ("c", "08:40:00", "Z"),
    ]);
    let h = "from_stop_id,to_stop_id,transfer_type,min_transfer_time\n";
    TempFeed::new(&[
        ("trips.txt", &trips(&[("R", "a"), ("Q", "b"), ("Q", "c")])),
        ("stop_times.txt", &st),
        ("transfers.txt", &format!("{h}{transfers}")),
    ])
}

fn arrivals(tt: &Tt, from: &str, to: &str) -> Vec<(u8, u32)> {
    let js: Vec<Journey> = raptor_stop_to_stop(
        tt,
        stop(tt, from),
        stop(tt, to),
        date(2030, 3, 1),
        hms(7, 0, 0),
        MAX_TRANSFERS,
    )
    .unwrap();
    js.iter().map(|j| (j.rides, j.arr.0)).collect()
}

#[test]
fn self_transfer_type_3_forbids_changing_at_that_stop() {
    let tt = feed("Y,Y,3,\n").tt();
    assert!(arrivals(&tt, "X", "Z").is_empty());
    let tt = feed("Y,Y,2,300\n").tt();
    assert_eq!(arrivals(&tt, "X", "Z"), [(2, hms(8, 30, 0).0)]);
    let tt = feed("Y,Y,2,301\n").tt();
    assert!(arrivals(&tt, "X", "Z").is_empty());
}

#[test]
fn transfer_type_3_between_stops_is_never_used() {
    let tt = feed("Y,W,3,0\n").tt();
    assert_eq!(arrivals(&tt, "X", "Z"), [(2, hms(8, 30, 0).0)]);
    let tt = feed("Y,W,1,\n").tt();
    // Timed (type 1) counts as 0 s: c at W 08:12 is caught, but b is earlier.
    assert_eq!(arrivals(&tt, "X", "Z"), [(2, hms(8, 30, 0).0)]);
}

#[test]
fn transfer_from_the_origin_starts_with_a_walk_leg() {
    let tt = feed("W,Y,2,60\n").tt();
    let js = raptor_stop_to_stop(
        &tt,
        stop(&tt, "W"),
        stop(&tt, "Z"),
        date(2030, 3, 1),
        hms(8, 13, 0),
        0,
    )
    .unwrap();
    assert_eq!(js.len(), 1, "{js:?}");
    assert!(matches!(js[0].legs[0], Leg::Walk { secs: 60, .. }));
    assert_eq!(
        (js[0].dep, js[0].arr, js[0].rides),
        (hms(8, 13, 0), hms(8, 30, 0), 1)
    );
}

#[test]
fn walk_to_the_destination_ends_the_journey() {
    let tt = feed("Y,W,2,30\n").tt();
    let js = raptor_stop_to_stop(
        &tt,
        stop(&tt, "X"),
        stop(&tt, "W"),
        date(2030, 3, 1),
        hms(7, 0, 0),
        0,
    )
    .unwrap();
    assert_eq!(js.len(), 1, "{js:?}");
    assert_eq!(js[0].arr, hms(8, 10, 30));
    assert!(matches!(js[0].legs[1], Leg::Walk { secs: 30, .. }));
}

// ---- parent stations: all child platforms, 0 s access ----

/// Stations ST (P1, P2) and SU (Q1, Q2). a: P1 08:00 → Q2 08:20; b: P2 08:05 → Q1 08:15.
fn station_feed() -> TempFeed {
    let stops = "stop_id,stop_name,stop_lat,stop_lon,location_type,parent_station\n\
                 ST,S,1.0,1.0,1,\nP1,S 1,1.0,1.0,0,ST\nP2,S 2,1.0,1.0,0,ST\n\
                 SU,U,2.0,1.0,1,\nQ1,U 1,2.0,1.0,0,SU\nQ2,U 2,2.0,1.0,0,SU\n";
    let st = stop_times(&[
        ("a", "08:00:00", "P1"),
        ("a", "08:20:00", "Q2"),
        ("b", "08:05:00", "P2"),
        ("b", "08:15:00", "Q1"),
    ]);
    TempFeed::new(&[
        ("stops.txt", stops),
        ("trips.txt", &trips(&[("R", "a"), ("Q", "b")])),
        ("stop_times.txt", &st),
    ])
}

fn trip_ids(tt: &Tt, js: &[Journey]) -> Vec<String> {
    js.iter()
        .flat_map(|j| &j.legs)
        .filter_map(|l| match l {
            Leg::Transit { trip, .. } => Some(trip_id(tt, *trip).to_string()),
            Leg::Walk { .. } => None,
        })
        .collect()
}

fn plan_at(tt: &Tt, from: &str, to: &str) -> Vec<Journey> {
    raptor_stop_to_stop(
        tt,
        stop(tt, from),
        stop(tt, to),
        date(2030, 3, 1),
        hms(7, 0, 0),
        MAX_TRANSFERS,
    )
    .unwrap()
}

#[test]
fn parent_station_as_origin_uses_every_platform() {
    let tt = station_feed().tt();
    let js = plan_at(&tt, "ST", "Q2");
    assert_eq!(trip_ids(&tt, &js), ["a"]);
    assert_eq!((js[0].dep, js[0].arr), (hms(8, 0, 0), hms(8, 20, 0)));
    assert!(matches!(js[0].legs[0], Leg::Transit { from, .. } if from == stop(&tt, "P1")));
    let js = plan_at(&tt, "ST", "Q1");
    assert_eq!(trip_ids(&tt, &js), ["b"]);
}

#[test]
fn parent_station_as_destination_uses_every_platform() {
    let tt = station_feed().tt();
    let js = plan_at(&tt, "P1", "SU");
    assert_eq!(trip_ids(&tt, &js), ["a"]);
    assert!(matches!(js[0].legs[0], Leg::Transit { to, .. } if to == stop(&tt, "Q2")));
    assert_eq!(trip_ids(&tt, &plan_at(&tt, "P2", "SU")), ["b"]);
}

#[test]
fn parent_station_at_both_ends_gives_the_best_platform_pair() {
    let tt = station_feed().tt();
    let js = plan_at(&tt, "ST", "SU");
    assert_eq!(js.len(), 1, "{js:?}");
    assert_eq!(trip_ids(&tt, &js), ["b"]);
    assert_eq!((js[0].arr, js[0].rides), (hms(8, 15, 0), 1));
    // Platform to its own station: already there.
    let js = plan_at(&tt, "P1", "ST");
    assert!(js.len() == 1 && js[0].legs.is_empty(), "{js:?}");
}

// ---- ride and walk arrivals are kept apart ----

/// a: X 08:00 → Y 08:10 → V 08:11; b: Y 08:15 → Z 08:30.
/// Changing at Y takes 600 s, but V → Y is a 60 s transfer: arrive Y 08:12 and
/// catch b. (Was Known gap #11; fixed by separate ride/walk labels.)
#[test]
fn self_transfer_time_does_not_hide_a_faster_walk() {
    let st = stop_times(&[
        ("a", "08:00:00", "X"),
        ("a", "08:10:00", "Y"),
        ("a", "08:11:00", "V"),
        ("b", "08:15:00", "Y"),
        ("b", "08:30:00", "Z"),
    ]);
    let f = TempFeed::new(&[
        ("trips.txt", &trips(&[("R", "a"), ("Q", "b")])),
        ("stop_times.txt", &st),
        (
            "transfers.txt",
            "from_stop_id,to_stop_id,transfer_type,min_transfer_time\nY,Y,2,600\nV,Y,2,60\n",
        ),
    ]);
    let tt = f.tt();
    assert_eq!(arrivals(&tt, "X", "Z"), [(2, hms(8, 30, 0).0)]);
}

/// transfers.txt hops never chain, neither from the origin nor after a ride:
/// O → P → Q and A → B → C are two hops each.
#[test]
fn transfer_hops_never_chain_in_one_round() {
    let st = stop_times(&[
        ("t", "08:00:00", "O"),
        ("t", "08:10:00", "A"),
        ("u", "09:00:00", "Q"),
        ("u", "09:10:00", "R"),
    ]);
    let f = TempFeed::new(&[
        ("trips.txt", &trips(&[("R", "t"), ("Q", "u")])),
        ("stop_times.txt", &st),
        (
            "stops.txt",
            "stop_id,stop_name,stop_lat,stop_lon\nO,O,1,1\nA,A,2,1\nB,B,3,1\nC,C,4,1\nP,P,5,1\nQ,Q,6,1\nR,R,7,1\n",
        ),
        (
            "transfers.txt",
            "from_stop_id,to_stop_id,transfer_type,min_transfer_time\nA,B,2,60\nB,C,2,60\nO,P,2,60\nP,Q,2,60\n",
        ),
    ]);
    let tt = f.tt();
    assert_eq!(arrivals(&tt, "O", "B"), [(1, hms(8, 11, 0).0)]);
    assert!(arrivals(&tt, "O", "C").is_empty());
    assert_eq!(arrivals(&tt, "O", "P"), [(0, hms(7, 1, 0).0)]);
    // O → P → Q would need two hops before riding u.
    assert!(arrivals(&tt, "O", "R").is_empty());
}
