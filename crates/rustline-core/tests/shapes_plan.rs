//! Transit legs follow the trip's shape (shapes.txt), cut at the boarding
//! and alighting stops; the stops' line when the pack has no shape.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::DateTime;
use rustline_core::{Engine, Leg, PlanRequest, TimeSpec};
use rustline_gtfs::Timetable;
use rustline_model::LatLon;
use rustline_pack::{read_pack, write_pack, PackMeta, TimetableData, TransitShapes};

/// The tiny fixture with shapes: R1 has one without distances and a bend
/// between Alpha and Bravo, R3 one with `shape_dist_traveled` in
/// shapes.txt and stop_times.txt, R2 none.
struct ShapedFeed(PathBuf);

const SHAPES: &str = "shape_id,shape_pt_lat,shape_pt_lon,shape_pt_sequence,shape_dist_traveled
SH1,10.0050,20.0000,3,
SH1,10.0000,20.0000,1,
SH1,10.0025,20.0010,2,
SH1,10.0100,20.0050,4,
SH1,10.0150,20.0100,5,
SH3,10.0000,20.0000,1,0
SH3,10.0100,20.0050,2,1.0
SH3,10.0100,20.0150,3,2.0
";

/// SH1 moved 0.01° (about 1.1 km) north: no stop of R1 is near it.
const FAR: &str = "SHFAR,10.0100,20.0000,1,
SHFAR,10.0250,20.0100,2,
";

impl ShapedFeed {
    fn new() -> Self {
        Self::with(
            "",
            |route, _| match route {
                "R1" => "SH1",
                "R3" => "SH3",
                _ => "",
            },
            ["0", "1.0", "2.0"],
        )
    }

    /// The tiny fixture plus [`SHAPES`] and `more_shapes`; trips get
    /// `shape_of(route_id, trip_id)` and R3's stops `r3_dist`.
    fn with(
        more_shapes: &str,
        shape_of: fn(&str, &str) -> &'static str,
        r3_dist: [&str; 3],
    ) -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "rustline_core_shapes_{}_{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        let tiny = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/fixtures/tiny");
        for name in [
            "agency.txt",
            "calendar.txt",
            "calendar_dates.txt",
            "routes.txt",
            "stops.txt",
        ] {
            fs::copy(tiny.join(name), dir.join(name)).unwrap();
        }
        let trips = fs::read_to_string(tiny.join("trips.txt")).unwrap();
        let trips: String = trips
            .lines()
            .enumerate()
            .map(|(i, l)| {
                let f: Vec<&str> = l.split(',').collect();
                let shape = if i == 0 {
                    "shape_id"
                } else {
                    shape_of(f[0], f[2])
                };
                format!("{l},{shape}\n")
            })
            .collect();
        let times = fs::read_to_string(tiny.join("stop_times.txt")).unwrap();
        let times: String = times
            .lines()
            .enumerate()
            .map(|(i, l)| {
                let dist = match (i, l.starts_with("R3"), l.rsplit(',').next()) {
                    (0, ..) => "shape_dist_traveled",
                    (_, true, Some("1")) => r3_dist[0],
                    (_, true, Some("2")) => r3_dist[1],
                    (_, true, _) => r3_dist[2],
                    _ => "",
                };
                format!("{l},{dist}\n")
            })
            .collect();
        fs::write(dir.join("trips.txt"), trips).unwrap();
        fs::write(dir.join("stop_times.txt"), times).unwrap();
        fs::write(dir.join("shapes.txt"), format!("{SHAPES}{more_shapes}")).unwrap();
        ShapedFeed(dir)
    }

    fn build(&self) -> (Timetable, TimetableData, PackMeta) {
        let raw = rustline_gtfs::load(&self.0).unwrap();
        let (tt, _) = rustline_gtfs::build_timetable(&raw).unwrap();
        let data = rustline_gtfs::try_timetable_data(&tt).unwrap();
        let meta = rustline_gtfs::pack_meta(&tt, [7; 32], "shapes", 1_700_000_000);
        (tt, data, meta)
    }
}

impl Drop for ShapedFeed {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn engine(data: &TimetableData, meta: &PackMeta) -> Engine {
    let mut bytes = Vec::new();
    write_pack(data, meta, &mut bytes).unwrap();
    Engine::new(read_pack(&bytes).unwrap()).unwrap()
}

/// The transit leg's geometry for a ride from `from` to `to` at `at`.
fn ride(e: &Engine, from: (f64, f64), to: (f64, f64), at: &str) -> Vec<(f64, f64)> {
    let r = PlanRequest {
        from: LatLon::new(from.0, from.1),
        to: LatLon::new(to.0, to.1),
        time: TimeSpec::DepartAt(DateTime::parse_from_rfc3339(at).unwrap()),
        max_access_m: 100,
        max_results: 1,
        ..PlanRequest::default()
    };
    let plan = e.plan(&r).unwrap();
    let it = plan.itineraries.first().expect("an itinerary");
    it.legs
        .iter()
        .find_map(|l| match l {
            Leg::Transit { geometry, .. } => Some(geometry.clone().unwrap()),
            _ => None,
        })
        .expect("a transit leg")
        .points()
        .iter()
        .map(|p| (p.lat, p.lon))
        .collect()
}

/// Within about a metre (cuts are stored in decimetres).
fn near(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() < 1e-5 && (a.1 - b.1).abs() < 1e-5
}

const ALPHA: (f64, f64) = (10.0, 20.0);
const BRAVO: (f64, f64) = (10.005, 20.0);
const CHARLIE: (f64, f64) = (10.01, 20.005);
const DELTA: (f64, f64) = (10.015, 20.01);
const ECHO: (f64, f64) = (10.01, 20.015);
const BEND: (f64, f64) = (10.0025, 20.001);

#[test]
fn a_ride_follows_the_shape() {
    let (_, data, meta) = ShapedFeed::new().build();
    let line = ride(
        &engine(&data, &meta),
        ALPHA,
        DELTA,
        "2030-01-02T07:55:00-05:00",
    );
    assert_eq!(line.len(), 5);
    for (p, q) in line.iter().zip([ALPHA, BEND, BRAVO, CHARLIE, DELTA]) {
        assert!(near(*p, q), "{p:?} vs {q:?}");
    }
}

#[test]
fn a_ride_is_cut_at_its_stops() {
    let (_, data, meta) = ShapedFeed::new().build();
    let line = ride(
        &engine(&data, &meta),
        BRAVO,
        CHARLIE,
        "2030-01-02T08:04:00-05:00",
    );
    assert_eq!(line.len(), 2, "{line:?}");
    assert!(near(line[0], BRAVO) && near(line[1], CHARLIE), "{line:?}");
}

#[test]
fn a_trip_without_a_shape_uses_its_stops() {
    let (_, data, meta) = ShapedFeed::new().build();
    let line = ride(
        &engine(&data, &meta),
        ECHO,
        BRAVO,
        "2030-01-02T06:55:00-05:00",
    );
    assert_eq!(line, vec![ECHO, CHARLIE, BRAVO]);
}

#[test]
fn a_pack_without_shapes_uses_the_stops() {
    let (_, mut data, meta) = ShapedFeed::new().build();
    data.transit_shapes = TransitShapes::default();
    let line = ride(
        &engine(&data, &meta),
        ALPHA,
        DELTA,
        "2030-01-02T07:55:00-05:00",
    );
    assert_eq!(line, vec![ALPHA, BRAVO, CHARLIE, DELTA]);
}

#[test]
fn a_ride_starts_and_ends_exactly_at_its_stops() {
    let (_, data, meta) = ShapedFeed::new().build();
    let e = engine(&data, &meta);
    for (from, to, at) in [
        (ALPHA, DELTA, "2030-01-02T07:55:00-05:00"),
        (BRAVO, CHARLIE, "2030-01-02T08:04:00-05:00"),
        (ALPHA, ECHO, "2030-01-02T07:55:00-05:00"),
    ] {
        let line = ride(&e, from, to, at);
        assert_eq!(line.first(), Some(&from), "{line:?}");
        assert_eq!(line.last(), Some(&to), "{line:?}");
    }
}

#[test]
fn a_shape_too_far_from_the_stops_gives_the_stop_line() {
    let (_, data, meta) = ShapedFeed::with(
        FAR,
        |route, _| if route == "R1" { "SHFAR" } else { "" },
        ["", "", ""],
    )
    .build();
    let line = ride(
        &engine(&data, &meta),
        ALPHA,
        DELTA,
        "2030-01-02T07:55:00-05:00",
    );
    assert_eq!(line, vec![ALPHA, BRAVO, CHARLIE, DELTA]);
}
