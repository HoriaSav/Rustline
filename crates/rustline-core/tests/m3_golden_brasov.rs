//! 10 golden Brașov journeys (plan M3 DoD), stored as `insta` snapshots in
//! tests/snapshots/m3_golden_brasov__*.snap.
//!
//! Feed: fixtures/feeds/brasov-manifest.zip (sha256 627559bac2a3…, valid
//! 2026-01-23 … 2027-05-23). Skipped when the feed is missing.
//!
//! Checked by: Reviewer on 2026-10-14, against fixtures/feeds/brasov-manifest.zip (sha256 627559bac2a3…): every trip, stop and time exists and runs that day, and an independent Python connection scan gives the same (arrival, rides) set after the 5-minute rule. Not compared with the printed RATBV timetables.
//!
//! M3 has no walking: changing between two platforms only works through
//! transfers.txt (Brașov has none), so some journeys use more rides than a
//! person would. That is expected until M4.

mod m3_common;

use m3_common::*;
use rustline_core::raptor_stop_to_stop;
use rustline_model::{Journey, Leg, Seconds};

/// Journeys as text: one line per leg, names and ids so a person can check them.
fn render(tt: &Tt, js: &[Journey]) -> String {
    let name = |s: rustline_model::StopIdx| {
        format!("{} [{}]", tt.stop_names[s.index()], tt.stop_ids[s.index()])
    };
    let mut out = String::new();
    if js.is_empty() {
        out.push_str("no journey\n");
    }
    for (i, j) in js.iter().enumerate() {
        out.push_str(&format!(
            "{}. {} → {}, {} ride(s)\n",
            i + 1,
            j.dep,
            j.arr,
            j.rides
        ));
        for leg in &j.legs {
            match *leg {
                Leg::Transit {
                    route,
                    trip,
                    from,
                    to,
                    dep,
                    arr,
                } => out.push_str(&format!(
                    "   {dep} {} → {arr} {}  line {} (trip {})\n",
                    name(from),
                    name(to),
                    tt.route_short_names[route.index()],
                    tt.trip_ids[trip.index()]
                )),
                Leg::Walk { from, to, secs } => out.push_str(&format!(
                    "   change {} → {} ({secs} s)\n",
                    name(from),
                    name(to)
                )),
            }
        }
    }
    out
}

#[test]
fn golden_brasov_journeys() {
    let Some(feed) = shared_feed("brasov-manifest") else {
        return;
    };
    let tt = pack(&feed);
    let thu = date(2026, 10, 15);
    let sat = date(2026, 10, 17);
    // (snapshot name, from stop_id, to stop_id, day, departure)
    let queries = [
        (
            "01_line20_livada_to_poiana",
            "node/3652436629",
            "node/3708889570",
            thu,
            hms(7, 50, 0),
        ),
        (
            "02_line2_livada_to_tractorul",
            "node/9183614613",
            "node/3652479577",
            thu,
            hms(7, 30, 0),
        ),
        (
            "03_line4_terminal_gara_to_primarie",
            "node/10198827064",
            "node/2552635273",
            thu,
            hms(7, 0, 0),
        ),
        (
            "04_line5_stadion_to_teatru",
            "node/9275068611",
            "node/2655859050",
            thu,
            hms(17, 0, 0),
        ),
        (
            "05_line16_strada_mica_to_coanda",
            "node/12395944841",
            "node/3343778544",
            thu,
            hms(6, 0, 0),
        ),
        (
            "06_livada_to_gara_mare",
            "node/2375041371",
            "node/9214351438",
            thu,
            hms(8, 0, 0),
        ),
        (
            "07_autogara3_to_poiana",
            "node/3652393629",
            "node/9171470093",
            thu,
            hms(8, 0, 0),
        ),
        (
            "08_livada_to_piata_unirii_two_options",
            "node/2375041369",
            "node/3713320059",
            thu,
            hms(8, 0, 0),
        ),
        (
            "09_saturday_livada_to_poiana",
            "node/3652436629",
            "node/3708889570",
            sat,
            hms(9, 0, 0),
        ),
        (
            "10_late_evening_livada_to_tractorul",
            "node/9183614613",
            "node/3652479577",
            thu,
            hms(23, 30, 0),
        ),
    ];
    for (snap, from, to, day, dep) in queries {
        let js = raptor_stop_to_stop(&tt, stop(&tt, from), stop(&tt, to), day, dep, 8)
            .expect("valid query");
        let text = format!(
            "{from} → {to} on {day} from {}\n{}",
            Seconds(dep.0),
            render(&tt, &js)
        );
        insta::assert_snapshot!(snap, text);
    }
}
