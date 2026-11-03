//! Transit shapes (format v5): shapes.txt and `shape_dist_traveled` read from
//! GTFS, stored in the pack, read back; v4 packs still load, without shapes.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use rustline_gtfs::Timetable;
use rustline_pack::{
    cumulative_m, read_pack, verify_pack_file, write_pack, PackError, PackMeta, TimetableData,
    TransitShapes, DEFAULT_MAX_BODY_BYTES,
};

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
            "rustline_shapes_{}_{}",
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

fn to_bytes(data: &TimetableData, meta: &PackMeta) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_pack(data, meta, &mut bytes).unwrap();
    bytes
}

/// `bytes` (a v5 pack without shapes) as v4 wrote it: version 4, no shapes
/// section, the CRC32 over the rest.
fn as_v4(mut bytes: Vec<u8>) -> Vec<u8> {
    let empty = rkyv::to_bytes::<rkyv::rancor::Error>(&TransitShapes::default()).unwrap();
    let end = bytes.len() - 4;
    let start = end - empty.len() - 8;
    assert_eq!(
        &bytes[start..start + 8],
        &(empty.len() as u64).to_le_bytes()
    );
    bytes.truncate(start);
    bytes[4..6].copy_from_slice(&4u16.to_le_bytes());
    let crc = crc32fast::hash(&bytes);
    bytes.extend_from_slice(&crc.to_le_bytes());
    bytes
}

fn trip<'a>(tt: &'a Timetable, id: &str) -> &'a rustline_gtfs::Trip {
    tt.trips.iter().find(|t| t.id == id).unwrap()
}

#[test]
fn shapes_txt_and_shape_dist_traveled_are_read() {
    let (tt, _, _) = ShapedFeed::new().build();
    assert_eq!(tt.shapes.len(), 2);
    assert!(trip(&tt, "R2_0700").shape.is_none());

    // R1: projected; Alpha at the start, Bravo past the bend.
    let r1 = trip(&tt, "R1_0800").shape.clone().unwrap();
    let line = &tt.shapes[r1.shape as usize];
    assert_eq!(line.len(), 5, "sorted by shape_pt_sequence");
    assert_eq!(line[1], (100_025_000, 200_010_000));
    let cum = cumulative_m(line);
    assert_eq!(r1.at_m.len(), 4);
    for (m, c) in r1.at_m.iter().zip([cum[0], cum[2], cum[3], cum[4]]) {
        assert!((m - c).abs() < 1.0, "{m} vs {c}");
    }

    // R3: by shape_dist_traveled, Charlie at exactly the second point.
    let r3 = trip(&tt, "R3_slow").shape.clone().unwrap();
    let cum = cumulative_m(&tt.shapes[r3.shape as usize]);
    assert_eq!(r3.at_m, cum);
}

#[test]
fn shapes_round_trip_through_the_pack() {
    let (tt, data, meta) = ShapedFeed::new().build();
    let s = &data.transit_shapes;
    assert_eq!(s.shape_count(), 2);
    assert_eq!(s.trip_cut.len(), tt.trips.len());
    // R1's four trips share one cut, R3's three another.
    assert_eq!(s.cut_shape.len(), 2);

    let bytes = to_bytes(&data, &meta);
    let pack = read_pack(&bytes).unwrap();
    let back = &pack.timetable().transit_shapes;
    assert_eq!(back, s);
    for (i, line) in tt.shapes.iter().enumerate() {
        assert_eq!(back.points(i).as_ref(), Some(line));
    }
    for (i, t) in tt.trips.iter().enumerate() {
        assert_eq!(pack.timetable().trip_ids[i], t.id);
        let dm = t.shape.as_ref().map(|s| {
            s.at_m
                .iter()
                .map(|m| (m * 10.0).round() as u32)
                .collect::<Vec<_>>()
        });
        assert_eq!(back.trip_cut(i).map(|(_, c)| c.to_vec()), dm, "{}", t.id);
    }

    let zst = zstd::encode_all(&bytes[..], 3).unwrap();
    assert_eq!(&read_pack(&zst).unwrap().timetable().transit_shapes, s);

    let file = std::env::temp_dir().join(format!("rustline_shapes_{}.rlp", std::process::id()));
    fs::write(&file, &bytes).unwrap();
    let checked = verify_pack_file(&file, DEFAULT_MAX_BODY_BYTES);
    let _ = fs::remove_file(&file);
    checked.unwrap();
}

#[test]
fn a_feed_without_shapes_has_none() {
    let tiny = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/fixtures/tiny");
    let raw = rustline_gtfs::load(&tiny).unwrap();
    let (tt, _) = rustline_gtfs::build_timetable(&raw).unwrap();
    assert!(tt.shapes.is_empty() && tt.trips.iter().all(|t| t.shape.is_none()));
    let data = rustline_gtfs::try_timetable_data(&tt).unwrap();
    assert_eq!(data.transit_shapes, TransitShapes::default());
}

#[test]
fn v4_pack_loads_without_shapes() {
    let (_, mut data, meta) = ShapedFeed::new().build();
    let with_shapes = read_pack(&to_bytes(&data, &meta)).unwrap();
    data.transit_shapes = TransitShapes::default();
    let v4 = as_v4(to_bytes(&data, &meta));
    let pack = read_pack(&v4).unwrap();
    assert_eq!(pack.meta().format_version, 4);
    assert_eq!(pack.timetable().transit_shapes, TransitShapes::default());
    assert_eq!(pack.timetable().trip_ids, with_shapes.timetable().trip_ids);
    assert!(pack.timetable().transit_shapes.trip_cut(0).is_none());

    let file = std::env::temp_dir().join(format!("rustline_shapes_v4_{}.rlp", std::process::id()));
    fs::write(&file, &v4).unwrap();
    let checked = verify_pack_file(&file, DEFAULT_MAX_BODY_BYTES);
    let _ = fs::remove_file(&file);
    assert_eq!(checked.unwrap().meta.format_version, 4);
}

/// Writes the shaped feed's pack with `change` applied and checks it is refused.
fn assert_refused(change: impl Fn(&mut TransitShapes)) {
    let (_, mut data, meta) = ShapedFeed::new().build();
    change(&mut data.transit_shapes);
    let bytes = to_bytes(&data, &meta);
    assert!(matches!(read_pack(&bytes), Err(PackError::Corrupt(_))));
}

#[test]
fn inconsistent_shapes_are_corrupt() {
    assert_refused(|s| s.cut_dm.swap(0, 1)); // decreasing along a trip
    assert_refused(|s| s.trip_cut[0] = 9); // no such cut
    assert_refused(|s| s.cut_shape[0] = 9); // no such shape
    assert_refused(|s| {
        s.trip_cut.pop(); // not one per trip
    });
    assert_refused(|s| s.point_start[1] = 1); // a one-point shape
    assert_refused(|s| s.lat[0] = 1_000_000_000); // latitude 100°
    assert_refused(|s| *s.cut_start.last_mut().unwrap() -= 1); // cut shorter than its trip
}

#[test]
fn a_failed_placement_does_not_spoil_the_next_shape() {
    // R1_0800's shape is far from its stops; R1_0830 has the same stops and
    // a good shape, which must still be placed (and get the first index).
    let (tt, data, _) = ShapedFeed::with(
        FAR,
        |route, trip| match (route, trip) {
            (_, "R1_0800") => "SHFAR",
            ("R1", _) => "SH1",
            _ => "",
        },
        ["", "", ""],
    )
    .build();
    assert!(trip(&tt, "R1_0800").shape.is_none());
    for id in ["R1_0830", "R1_0900", "R1_we_1000"] {
        assert_eq!(
            trip(&tt, id).shape.as_ref().map(|s| s.shape),
            Some(0),
            "{id}"
        );
    }
    assert_eq!(tt.shapes.len(), 1, "an unplaced shape is not stored");
    let i = tt.trips.iter().position(|t| t.id == "R1_0800").unwrap();
    assert!(data.transit_shapes.trip_cut(i).is_none());
}

#[test]
fn distances_that_disagree_with_the_stops_give_way_to_projection() {
    // R3's distances put Charlie and Echo at the very start of SH3.
    let (tt, _, _) = ShapedFeed::with(
        "",
        |route, _| if route == "R3" { "SH3" } else { "" },
        ["0", "0.001", "0.002"],
    )
    .build();
    let r3 = trip(&tt, "R3_slow").shape.clone().unwrap();
    let cum = cumulative_m(&tt.shapes[r3.shape as usize]);
    assert_eq!(r3.at_m.len(), 3);
    for (m, c) in r3.at_m.iter().zip(&cum) {
        assert!((m - c).abs() < 1.0, "{:?} vs {cum:?}", r3.at_m);
    }
}

#[test]
fn a_stop_far_from_every_shape_point_drops_the_shape() {
    let (tt, data, meta) = ShapedFeed::with(
        FAR,
        |route, _| if route == "R1" { "SHFAR" } else { "" },
        ["", "", ""],
    )
    .build();
    assert!(tt.trips.iter().all(|t| t.shape.is_none()));
    assert!(tt.shapes.is_empty());
    assert_eq!(data.transit_shapes, TransitShapes::default());
    read_pack(&to_bytes(&data, &meta)).unwrap();
}

#[test]
fn the_file_check_refuses_inconsistent_shapes() {
    let (_, data, meta) = ShapedFeed::new().build();
    let path = std::env::temp_dir().join(format!("rustline_shapes_bad_{}.rlp", std::process::id()));
    let cases: [fn(&mut TransitShapes); 4] = [
        |s| s.cut_dm.swap(0, 1),
        |s| s.trip_cut[0] = 9,
        |s| {
            s.trip_cut.pop();
        },
        |s| *s.cut_start.last_mut().unwrap() -= 1,
    ];
    for (k, change) in cases.iter().enumerate() {
        let mut d = data.clone();
        change(&mut d.transit_shapes);
        fs::write(&path, to_bytes(&d, &meta)).unwrap();
        let r = verify_pack_file(&path, DEFAULT_MAX_BODY_BYTES);
        assert!(matches!(r, Err(PackError::Corrupt(_))), "case {k}");
    }
    let _ = fs::remove_file(&path);
}

#[test]
fn damaged_v5_bytes_are_corrupt() {
    let (_, data, meta) = ShapedFeed::new().build();
    let bytes = to_bytes(&data, &meta);
    for len in 5..bytes.len() {
        assert!(
            matches!(read_pack(&bytes[..len]), Err(PackError::Corrupt(_))),
            "length {len}"
        );
    }
    // Every byte of the shapes section (and its length) flipped in turn.
    let shapes = rkyv::to_bytes::<rkyv::rancor::Error>(&data.transit_shapes).unwrap();
    let end = bytes.len() - 4;
    for i in end - shapes.len() - 8..end {
        let mut b = bytes.clone();
        b[i] ^= 0x10;
        assert!(
            matches!(read_pack(&b), Err(PackError::Corrupt(_))),
            "byte {i}"
        );
    }
    // A shapes length past the end, with a matching CRC32.
    let mut b = bytes.clone();
    let at = end - shapes.len() - 8;
    b[at..at + 8].copy_from_slice(&u64::MAX.to_le_bytes());
    let crc = crc32fast::hash(&b[..end]);
    b[end..].copy_from_slice(&crc.to_le_bytes());
    assert!(matches!(read_pack(&b), Err(PackError::Corrupt(_))));
}

/// A feed in a temp dir from `(name, contents)`; removed on drop.
fn feed(files: &[(&str, &str)]) -> ShapedFeed {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "rustline_shapes_feed_{}_{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    for (name, body) in files {
        fs::write(dir.join(name), body).unwrap();
    }
    ShapedFeed(dir)
}

/// Trip A crosses the ±180° meridian (stops about 100 m apart over it), trip N is
/// an ordinary one; A's shape comes first (trips are read by id), so dropping it renumbers N's.
fn meridian_feed() -> ShapedFeed {
    feed(&[
        ("agency.txt", "agency_id,agency_name,agency_url,agency_timezone\nAG,Test,https://example.invalid/,Pacific/Fiji\n"),
        ("routes.txt", "route_id,agency_id,route_short_name,route_long_name,route_type\nR,AG,1,One,3\n"),
        ("calendar.txt", "service_id,monday,tuesday,wednesday,thursday,friday,saturday,sunday,start_date,end_date\nS,1,1,1,1,1,0,0,20400101,20400131\n"),
        ("trips.txt", "route_id,service_id,trip_id,shape_id\nR,S,A,SHX\nR,S,N,SHN\n"),
        ("stops.txt", "stop_id,stop_name,stop_lat,stop_lon\nE,East,-17.0,179.9995\nW,West,-17.0,-179.9995\nP,P,-17.5,178.0\nQ,Q,-17.5,178.01\n"),
        ("stop_times.txt", "trip_id,arrival_time,departure_time,stop_id,stop_sequence\nA,08:00:00,08:00:00,E,1\nA,08:01:00,08:01:00,W,2\nN,09:00:00,09:00:00,P,1\nN,09:05:00,09:05:00,Q,2\n"),
        ("shapes.txt", "shape_id,shape_pt_lat,shape_pt_lon,shape_pt_sequence\nSHX,-17.0,179.9995,1\nSHX,-17.0,-179.9995,2\nSHN,-17.5,178.0,1\nSHN,-17.5,178.01,2\n"),
    ])
}

#[test]
fn a_shape_whose_steps_do_not_fit_is_dropped_alone() {
    let (tt, data, meta) = meridian_feed().build();
    // Placed at build time; only the pack leaves it out.
    let a = trip(&tt, "A")
        .shape
        .clone()
        .expect("A is placed on its shape");
    assert_eq!(a.shape, 0);
    assert_eq!(tt.shapes.len(), 2);
    let s = &data.transit_shapes;
    assert_eq!(s.shape_count(), 1);
    let ix = tt.trips.iter().position(|t| t.id == "A").unwrap();
    let in_ = tt.trips.iter().position(|t| t.id == "N").unwrap();
    assert_eq!(s.trip_cut[ix], rustline_pack::NO_SHAPE);
    let (shape, cut) = s.trip_cut(in_).unwrap();
    assert_eq!(shape, 0, "N's shape renumbered");
    assert_eq!(s.points(shape).as_ref(), Some(&tt.shapes[1]));
    assert_eq!(cut.len(), 2);
    let pack = read_pack(&to_bytes(&data, &meta)).unwrap();
    assert_eq!(&pack.timetable().transit_shapes, s);
}

#[test]
fn a_shape_across_the_antimeridian_passes_the_pack_checks() {
    let (_, data, meta) = meridian_feed().build();
    let bytes = to_bytes(&data, &meta);
    read_pack(&bytes).unwrap();
    let file = std::env::temp_dir().join(format!("rustline_shapes_180_{}.rlp", std::process::id()));
    fs::write(&file, &bytes).unwrap();
    let checked = verify_pack_file(&file, DEFAULT_MAX_BODY_BYTES);
    let _ = fs::remove_file(&file);
    checked.unwrap();
}
