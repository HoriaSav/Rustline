//! Zip member lookup in `load.rs` (Tester, 2026-10-27, deps-oct27: zip 8 -> 9).
//!
//! The zips are written by hand (stored, no compression), so the same bytes
//! go through any zip crate version, and archives no zip writer makes
//! (duplicate names, non-UTF-8 names, directory entries) can be built.
//! Spec (Leader, 2026-10-27): when several entries match a GTFS file name
//! (same name, or the same file name in different folders), the LAST one in
//! archive order wins, as in gtfs-structures. Main (zip 8.6) took the first
//! match by file name; see the notes on the tests where main differs.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use rustline_gtfs::{build_timetable, load, Timetable};

const AGENCY: &str = "agency_id,agency_name,agency_url,agency_timezone\nAG,Zip,https://example.invalid/,Europe/Bucharest\n";
const STOPS: &str = "stop_id,stop_name,stop_lat,stop_lon\nA,A,1.0,1.0\nB,B,1.001,1.0\n";
const ROUTES: &str =
    "route_id,agency_id,route_short_name,route_long_name,route_type\nR,AG,1,One,3\n";
const CALENDAR: &str = "service_id,monday,tuesday,wednesday,thursday,friday,saturday,sunday,start_date,end_date\nS,1,1,1,1,1,1,1,20400101,20401231\n";
const TRANSFERS: &str = "from_stop_id,to_stop_id,transfer_type,min_transfer_time\nA,B,2,120\n";

/// trips.txt with one trip `id`.
fn trips(id: &str) -> String {
    format!("route_id,service_id,trip_id\nR,S,{id}\n")
}

/// stop_times.txt: trips `ids` each A 08:00 -> B 08:05.
fn stop_times(ids: &[&str]) -> String {
    let mut s = String::from("trip_id,arrival_time,departure_time,stop_id,stop_sequence\n");
    for id in ids {
        s += &format!("{id},08:00:00,08:00:00,A,1\n{id},08:05:00,08:05:00,B,2\n");
    }
    s
}

/// One zip entry: raw name bytes, data, and whether the UTF-8 flag (bit 11) is set.
struct Entry {
    name: Vec<u8>,
    data: Vec<u8>,
    utf8: bool,
}

fn e(name: &str, data: &str) -> Entry {
    Entry {
        name: name.as_bytes().to_vec(),
        data: data.as_bytes().to_vec(),
        utf8: false,
    }
}

/// The base feed (trip T) with every member under `prefix`, plus `extra`.
fn base(prefix: &str, extra: Vec<Entry>) -> Vec<Entry> {
    let mut v = vec![
        e(&format!("{prefix}agency.txt"), AGENCY),
        e(&format!("{prefix}stops.txt"), STOPS),
        e(&format!("{prefix}routes.txt"), ROUTES),
        e(&format!("{prefix}calendar.txt"), CALENDAR),
        e(
            &format!("{prefix}stop_times.txt"),
            &stop_times(&["T", "U", "V"]),
        ),
    ];
    v.extend(extra);
    v
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// A stored (method 0) zip of `entries`, in order, names as given.
fn zip_bytes(entries: &[Entry]) -> Vec<u8> {
    let (mut out, mut cd) = (Vec::new(), Vec::new());
    for en in entries {
        let offset = u32::try_from(out.len()).expect("zip under 4 GiB");
        let flags: u16 = if en.utf8 { 0x0800 } else { 0 };
        let crc = crc32(&en.data);
        let len = u32::try_from(en.data.len()).expect("entry under 4 GiB");
        let common = |v: &mut Vec<u8>| {
            v.extend(20u16.to_le_bytes()); // version needed
            v.extend(flags.to_le_bytes());
            v.extend(0u16.to_le_bytes()); // stored
            v.extend(0u16.to_le_bytes()); // time
            v.extend(0x21u16.to_le_bytes()); // date 1980-01-01
            v.extend(crc.to_le_bytes());
            v.extend(len.to_le_bytes());
            v.extend(len.to_le_bytes());
            v.extend(
                u16::try_from(en.name.len())
                    .expect("name under 64 KiB")
                    .to_le_bytes(),
            );
            v.extend(0u16.to_le_bytes()); // extra
        };
        out.extend(0x0403_4b50u32.to_le_bytes());
        common(&mut out);
        out.extend(&en.name);
        out.extend(&en.data);

        cd.extend(0x0201_4b50u32.to_le_bytes());
        cd.extend(20u16.to_le_bytes()); // made by
        common(&mut cd);
        cd.extend(0u16.to_le_bytes()); // comment
        cd.extend(0u16.to_le_bytes()); // disk
        cd.extend(0u16.to_le_bytes()); // internal attrs
        let external: u32 = if en.name.ends_with(b"/") { 0x10 } else { 0 };
        cd.extend(external.to_le_bytes());
        cd.extend(offset.to_le_bytes());
        cd.extend(&en.name);
    }
    let cd_offset = u32::try_from(out.len()).expect("zip under 4 GiB");
    out.extend(&cd);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    let count = u16::try_from(entries.len()).expect("under 64 Ki entries");
    out.extend(count.to_le_bytes());
    out.extend(count.to_le_bytes());
    out.extend(
        u32::try_from(cd.len())
            .expect("central directory under 4 GiB")
            .to_le_bytes(),
    );
    out.extend(cd_offset.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out
}

/// A zip file under the temp dir, removed on drop.
struct TempZip(PathBuf);

impl TempZip {
    fn new(entries: &[Entry]) -> TempZip {
        static N: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!(
            "rustline_ziplookup_{}_{}.zip",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&p, zip_bytes(entries)).unwrap();
        TempZip(p)
    }
}

impl Drop for TempZip {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn build(p: &Path) -> Timetable {
    let raw = load(p).expect("feed loads");
    build_timetable(&raw).expect("feed builds").0
}

fn trip_ids(tt: &Timetable) -> Vec<&str> {
    tt.trips.iter().map(|t| t.id.as_str()).collect()
}

fn pack(tt: &Timetable) -> Vec<u8> {
    let meta = rustline_gtfs::pack_meta(tt, [0; 32], "zip", 0);
    let data = rustline_gtfs::try_timetable_data(tt).expect("pack arrays");
    let mut out = Vec::new();
    rustline_pack::write_pack(&data, &meta, &mut out).expect("write pack");
    out
}

// (a) Members in a folder.

#[test]
fn feed_in_a_subdirectory_loads_with_transfers() {
    let z = TempZip::new(&base(
        "feed/",
        vec![
            e("feed/trips.txt", &trips("T")),
            e("feed/transfers.txt", TRANSFERS),
        ],
    ));
    let tt = build(&z.0);
    assert_eq!(trip_ids(&tt), ["T"]);
    assert_eq!(tt.transfers.len(), 1);
}

#[test]
fn feed_two_folders_deep_with_directory_entries_loads() {
    let mut v = vec![e("gtfs/", ""), e("gtfs/2026/", "")];
    v.extend(base(
        "gtfs/2026/",
        vec![
            e("gtfs/2026/trips.txt", &trips("T")),
            e("gtfs/2026/transfers.txt", TRANSFERS),
        ],
    ));
    let tt = build(&TempZip::new(&v).0);
    assert_eq!(trip_ids(&tt), ["T"]);
    assert_eq!(tt.transfers.len(), 1);
}

#[test]
fn zip_of_the_tiny_fixture_packs_like_the_directory() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/fixtures/tiny");
    let mut names: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .map(|d| d.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.ends_with(".txt"))
        .collect();
    names.sort();
    let from_dir = pack(&build(&dir));
    for prefix in ["", "tiny/"] {
        let entries: Vec<Entry> = names
            .iter()
            .map(|n| {
                e(
                    &format!("{prefix}{n}"),
                    &fs::read_to_string(dir.join(n)).unwrap(),
                )
            })
            .collect();
        let tt = build(&TempZip::new(&entries).0);
        assert!(!tt.trips.is_empty());
        assert_eq!(pack(&tt), from_dir, "prefix {prefix:?}");
    }
}

// (b) Optional transfers.txt.

#[test]
fn zip_without_transfers_txt_loads_with_no_transfers() {
    let tt = build(&TempZip::new(&base("", vec![e("trips.txt", &trips("T"))])).0);
    assert_eq!(trip_ids(&tt), ["T"]);
    assert!(tt.transfers.is_empty());
}

#[test]
fn zip_with_root_transfers_txt_reads_it() {
    let z = TempZip::new(&base(
        "",
        vec![e("trips.txt", &trips("T")), e("transfers.txt", TRANSFERS)],
    ));
    let tt = build(&z.0);
    assert_eq!(tt.transfers.len(), 1);
    assert_eq!((tt.transfers[0].kind, tt.transfers[0].min_secs), (2, 120));
}

#[test]
fn zip_without_trips_txt_is_a_missing_file_error() {
    let z = TempZip::new(&base("", vec![]));
    let err = load(&z.0).err().expect("no trips.txt must fail");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("MissingFile") && msg.contains("trips.txt"),
        "{msg}"
    );
}

// (c) Duplicate and odd names.

#[test]
fn duplicate_trips_txt_name_uses_the_last_entry() {
    let z = TempZip::new(&base(
        "",
        vec![e("trips.txt", &trips("T")), e("trips.txt", &trips("U"))],
    ));
    assert_eq!(trip_ids(&build(&z.0)), ["U"]);
}

#[test]
fn duplicate_transfers_txt_name_uses_the_last_entry() {
    let other = "from_stop_id,to_stop_id,transfer_type,min_transfer_time\nA,B,2,300\n";
    let z = TempZip::new(&base(
        "",
        vec![
            e("trips.txt", &trips("T")),
            e("transfers.txt", TRANSFERS),
            e("transfers.txt", other),
        ],
    ));
    let tt = build(&z.0);
    assert_eq!(tt.transfers.len(), 1);
    assert_eq!(tt.transfers[0].min_secs, 300);
}

// Main took the first match here (U, then T): changed on purpose to last.
#[test]
fn trips_txt_in_two_folders_uses_the_last_in_archive_order() {
    let z = TempZip::new(&base(
        "",
        vec![e("old/trips.txt", &trips("U")), e("trips.txt", &trips("T"))],
    ));
    assert_eq!(trip_ids(&build(&z.0)), ["T"]);
    let z = TempZip::new(&base(
        "",
        vec![e("trips.txt", &trips("T")), e("old/trips.txt", &trips("U"))],
    ));
    assert_eq!(trip_ids(&build(&z.0)), ["U"]);
}

// Main took the first match here (300): changed on purpose to last.
#[test]
fn transfers_txt_in_two_folders_uses_the_last_in_archive_order() {
    let other = "from_stop_id,to_stop_id,transfer_type,min_transfer_time\nA,B,2,300\n";
    let z = TempZip::new(&base(
        "",
        vec![
            e("trips.txt", &trips("T")),
            e("x/transfers.txt", other),
            e("transfers.txt", TRANSFERS),
        ],
    ));
    assert_eq!(build(&z.0).transfers[0].min_secs, 120);
    let z = TempZip::new(&base(
        "",
        vec![
            e("trips.txt", &trips("T")),
            e("transfers.txt", TRANSFERS),
            e("x/transfers.txt", other),
        ],
    ));
    assert_eq!(build(&z.0).transfers[0].min_secs, 300);
}

#[test]
fn dot_slash_trips_txt_is_found() {
    let z = TempZip::new(&base("", vec![e("./trips.txt", &trips("T"))]));
    let r = load(&z.0).map(|raw| build_timetable(&raw).map(|(tt, _)| trip_ids(&tt).join(",")));
    assert_eq!(r.expect("loads").expect("builds"), "T");
}

// Path::file_name("trips.txt/") is "trips.txt", so a directory entry with
// that name matches too. Main took the first match: it failed the first case
// (empty "file", no header) and loaded the second.
#[test]
fn directory_entry_named_like_trips_txt_before_the_file_is_skipped_by_last_match() {
    let z = TempZip::new(&base(
        "",
        vec![e("trips.txt/", ""), e("feed/trips.txt", &trips("T"))],
    ));
    let r = load(&z.0).map(|raw| build_timetable(&raw).map(|(tt, _)| trip_ids(&tt).join(",")));
    assert_eq!(r.expect("loads").expect("builds"), "T");
}

#[test]
fn directory_entry_named_like_trips_txt_after_the_file_fails() {
    let z = TempZip::new(&base(
        "",
        vec![e("feed/trips.txt", &trips("T")), e("trips.txt/", "")],
    ));
    let r = load(&z.0);
    assert!(r.is_err());
}

#[test]
fn upper_case_trips_txt_is_not_found() {
    let z = TempZip::new(&base("", vec![e("TRIPS.TXT", &trips("T"))]));
    assert!(load(&z.0).is_err());
}

#[test]
fn cp437_folder_name_is_found() {
    // No UTF-8 flag: 0x81 is CP437 'ü', so the name decodes and matches.
    let mut v = base("", vec![]);
    v.push(Entry {
        name: b"\x81/trips.txt".to_vec(),
        data: trips("T").into_bytes(),
        utf8: false,
    });
    assert_eq!(trip_ids(&build(&TempZip::new(&v).0)), ["T"]);
}

#[test]
fn invalid_utf8_name_of_an_unrelated_entry_does_not_break_the_feed() {
    let mut v = vec![Entry {
        name: b"junk\xff\xfe.bin".to_vec(),
        data: b"x".to_vec(),
        utf8: true,
    }];
    v.extend(base(
        "",
        vec![e("trips.txt", &trips("T")), e("transfers.txt", TRANSFERS)],
    ));
    let r = load(&TempZip::new(&v).0);
    let tt = build_timetable(&r.expect("loads")).expect("builds").0;
    assert_eq!(trip_ids(&tt), ["T"]);
    assert_eq!(tt.transfers.len(), 1);
}

#[test]
fn trips_txt_under_an_invalid_utf8_folder_name() {
    // UTF-8 flag set but the folder bytes are not UTF-8.
    let mut v = base("", vec![]);
    v.push(Entry {
        name: b"bad\xff/trips.txt".to_vec(),
        data: trips("T").into_bytes(),
        utf8: true,
    });
    let r = load(&TempZip::new(&v).0)
        .map(|raw| build_timetable(&raw).map(|(tt, _)| trip_ids(&tt).join(",")));
    assert_eq!(r.expect("loads").expect("builds"), "T");
}

// (d) Real feeds from the shared fixture set (fixtures/feeds/, gitignored:
// skipped when missing, as in m7_reader.rs).

fn shared_feed(name: &str) -> Option<PathBuf> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/feeds")
        .join(name);
    if p.is_file() {
        Some(p)
    } else {
        println!("skip: {} missing", p.display());
        None
    }
}

#[test]
fn shared_real_feeds_load_with_pinned_counts() {
    // (feed, stops, stations, routes, trips, stop_times, services,
    //  service_exceptions, frequencies, timetable trips, transfers), as on main.
    let want: [(&str, [usize; 10]); 4] = [
        ("google-sample.zip", [9, 0, 5, 11, 28, 2, 1, 11, 144, 0]),
        (
            "brasov-manifest.zip",
            [811, 9, 84, 8075, 118534, 5, 110, 0, 8075, 0],
        ),
        ("cluj.zip", [767, 0, 140, 17186, 231429, 4, 0, 0, 17186, 0]),
        (
            "timisoara.zip",
            [892, 0, 59, 13530, 206136, 215, 1914, 0, 13530, 0],
        ),
    ];
    for (name, want) in want {
        let Some(p) = shared_feed(name) else { continue };
        let raw = load(&p).expect("real feed loads");
        let (tt, _) = build_timetable(&raw).expect("real feed builds");
        let c = raw.counts();
        let got = [
            c.stops,
            c.stations,
            c.routes,
            c.trips,
            c.stop_times,
            c.services,
            c.service_exceptions,
            c.frequencies,
            tt.trips.len(),
            tt.transfers.len(),
        ];
        assert_eq!(got, want, "{name}");
    }
}
