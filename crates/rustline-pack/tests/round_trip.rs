//! Round-trip tests: GTFS → timetable → pack bytes → pack, nothing lost.
//! Plus robustness: bad files must give an error, never a panic.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use rustline_gtfs::Timetable;
use rustline_model::TripIdx;
use rustline_pack::{read_pack, write_pack, Pack, PackError, PackMeta, TimetableData};

fn workspace_path(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

fn build(feed: &Path) -> (Timetable, TimetableData, PackMeta) {
    let raw = rustline_gtfs::load(feed).unwrap();
    let (tt, _) = rustline_gtfs::build_timetable(&raw).unwrap();
    let data = rustline_gtfs::try_timetable_data(&tt).unwrap();
    let meta = rustline_gtfs::pack_meta(&tt, [7; 32], "test-feed", 1_700_000_000);
    (tt, data, meta)
}

fn tiny() -> (Timetable, TimetableData, PackMeta) {
    build(&workspace_path("data/fixtures/tiny"))
}

fn to_bytes(data: &TimetableData, meta: &PackMeta) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_pack(data, meta, &mut bytes).unwrap();
    bytes
}

fn tiny_bytes() -> Vec<u8> {
    let (_, data, meta) = tiny();
    to_bytes(&data, &meta)
}

fn is_corrupt(r: Result<Pack, PackError>) -> bool {
    matches!(r, Err(PackError::Corrupt(_)))
}

/// Every count equal, and **every** trip has the same stops and times.
fn assert_same(tt: &Timetable, pack: &Pack) {
    let t = &pack.timetable();
    assert_eq!(t.stop_count(), tt.stops.len());
    assert_eq!(t.route_ids.len(), tt.routes.len());
    assert_eq!(t.trip_count(), tt.trips.len());
    assert_eq!(t.stop_time_count(), tt.stop_times.len());
    assert_eq!(t.pattern_count(), tt.patterns.len());
    assert_eq!(t.service_ids.len(), tt.services.len());
    assert_eq!(pack.meta().timezone, tt.timezone);
    assert_eq!(pack.meta().valid_from, tt.valid_from);
    assert_eq!(pack.meta().valid_to, tt.valid_to);

    for (i, trip) in tt.trips.iter().enumerate() {
        let expected: Vec<(u32, u32, u32)> = tt
            .trip_stop_times(TripIdx::from_usize(i))
            .iter()
            .map(|st| (st.stop.0, st.arrival.0, st.departure.0))
            .collect();
        assert_eq!(t.trip_times(i), expected, "trip {}", trip.id);
        assert_eq!(t.trip_ids[i], trip.id);
        assert_eq!(t.trip_route[i], trip.route.0);
        assert_eq!(t.trip_service[i], trip.service.0);
        assert_eq!(t.trip_pattern[i], trip.pattern.0);
    }
    for (s, stop) in tt.stops.iter().enumerate() {
        assert_eq!(t.stop_ids[s], stop.id);
        assert_eq!(t.stop_names[s], stop.name);
        assert_eq!(t.stop_pos(s), stop.pos.map(|p| (p.lat, p.lon)));
    }
}

// ---- round trip --------------------------------------------------------------

#[test]
fn tiny_fixture_round_trips() {
    let (tt, data, meta) = tiny();
    let pack = read_pack(&to_bytes(&data, &meta)).unwrap();
    assert_eq!(pack.meta(), &meta);
    assert_eq!(pack.timetable(), &data);
    assert_same(&tt, &pack);
}

#[test]
fn compressed_pack_round_trips() {
    let (_, data, meta) = tiny();
    let zst = zstd::encode_all(&to_bytes(&data, &meta)[..], 3).unwrap();
    let pack = read_pack(&zst).unwrap();
    assert_eq!(pack.timetable(), &data);
    assert_eq!(pack.meta(), &meta);
}

#[test]
fn building_twice_gives_identical_bytes() {
    use sha2::{Digest, Sha256};
    let a = Sha256::digest(tiny_bytes());
    let b = Sha256::digest(tiny_bytes());
    assert_eq!(a, b);
}

#[test]
fn service_bits_match_the_calendar() {
    let (tt, data, _) = tiny();
    let days = (tt.valid_to - tt.valid_from).num_days() as u32 + 1;
    for s in 0..tt.services.len() {
        assert!((0..days).any(|d| data.service_runs(s, d)));
        assert!(!data.service_runs(s, days + 1000));
    }
}

#[test]
fn unaligned_input_still_loads() {
    let bytes = tiny_bytes();
    let mut shifted = vec![0u8];
    shifted.extend_from_slice(&bytes);
    let pack = read_pack(&shifted[1..]).unwrap();
    assert_eq!(pack.timetable(), &tiny().1);
}

// ---- small hand-written feeds -------------------------------------------------

/// A feed directory under the system temp dir, removed on drop.
struct TempFeed(PathBuf);

impl TempFeed {
    fn new(stops: &str, stop_times: &str) -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "rustline_pack_{}_{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        let files = [
            ("agency.txt", "agency_id,agency_name,agency_url,agency_timezone\nAG,Test,https://example.invalid/,Etc/UTC\n"),
            ("routes.txt", "route_id,agency_id,route_short_name,route_long_name,route_type\nR,AG,Ș1,Țară 🚌,3\n"),
            ("calendar.txt", "service_id,monday,tuesday,wednesday,thursday,friday,saturday,sunday,start_date,end_date\nS,1,1,1,1,1,0,0,20400101,20400131\n"),
            ("trips.txt", "route_id,service_id,trip_id,trip_headsign\nR,S,T1,Ștrand 🏖\n"),
            ("stops.txt", stops),
            ("stop_times.txt", stop_times),
        ];
        for (name, body) in files {
            fs::write(dir.join(name), body).unwrap();
        }
        TempFeed(dir)
    }
}

impl Drop for TempFeed {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn one_stop_one_trip_with_non_ascii_names_round_trips() {
    let feed = TempFeed::new(
        "stop_id,stop_name,stop_lat,stop_lon\nA,Piața Sfatului ✨,1.0,1.0\n",
        "trip_id,arrival_time,departure_time,stop_id,stop_sequence\nT1,08:00:00,08:00:00,A,1\nT1,08:30:00,08:30:00,A,2\n",
    );
    let (tt, data, meta) = build(&feed.0);
    assert_eq!((tt.stops.len(), tt.trips.len()), (1, 1));
    let pack = read_pack(&to_bytes(&data, &meta)).unwrap();
    assert_same(&tt, &pack);
    assert_eq!(pack.timetable().stop_names[0], "Piața Sfatului ✨");
    assert_eq!(pack.timetable().route_short_names[0], "Ș1");
    assert_eq!(pack.timetable().route_long_names[0], "Țară 🚌");
    assert_eq!(pack.timetable().trip_headsigns[0], "Ștrand 🏖");
}

// ---- bad files ----------------------------------------------------------------

#[test]
fn bad_magic_is_rejected() {
    let mut bytes = tiny_bytes();
    bytes[0] = b'X';
    assert!(matches!(read_pack(&bytes), Err(PackError::BadMagic)));
    assert!(matches!(read_pack(b""), Err(PackError::BadMagic)));
    assert!(matches!(read_pack(b"R"), Err(PackError::BadMagic)));
}

#[test]
fn old_and_unknown_versions_are_rejected() {
    // The CRC no longer matches either, but the version is checked first.
    for v in [0u16, 1, 2, 99] {
        let mut bytes = tiny_bytes();
        bytes[4..6].copy_from_slice(&v.to_le_bytes());
        assert!(matches!(
            read_pack(&bytes),
            Err(PackError::UnsupportedVersion { found, supported: 3 }) if found == v
        ));
    }
}

#[test]
fn header_only_and_every_truncation_are_corrupt() {
    let bytes = tiny_bytes();
    // Everything from "magic + half a version" to "one byte short".
    for len in 5..bytes.len() {
        assert!(is_corrupt(read_pack(&bytes[..len])), "length {len}");
    }
}

#[test]
fn truncated_zstd_stream_is_corrupt() {
    let zst = zstd::encode_all(&tiny_bytes()[..], 3).unwrap();
    for len in 4..zst.len() {
        assert!(is_corrupt(read_pack(&zst[..len])), "length {len}");
    }
}

#[test]
fn extra_bytes_after_the_body_are_corrupt() {
    let mut bytes = tiny_bytes();
    bytes.push(0);
    assert!(is_corrupt(read_pack(&bytes)));
}

/// Writes `data` (made inconsistent on purpose) and checks it is refused.
fn assert_refused(change: impl Fn(&mut TimetableData)) {
    let (_, mut data, meta) = tiny();
    change(&mut data);
    assert!(is_corrupt(read_pack(&to_bytes(&data, &meta))));
}

#[test]
fn csr_offset_past_the_end_is_corrupt() {
    assert_refused(|d| *d.pattern_stops_start.last_mut().unwrap() += 1);
    assert_refused(|d| d.pattern_trips_start[1] = u32::MAX);
    assert_refused(|d| d.stop_patterns_start[0] = 1);
}

#[test]
fn decreasing_csr_offsets_are_corrupt() {
    assert_refused(|d| {
        d.pattern_stops_start.swap(1, 2);
    });
}

#[test]
fn stop_index_out_of_range_is_corrupt() {
    assert_refused(|d| d.pattern_stops[0] = d.stop_ids.len() as u32);
    assert_refused(|d| d.stop_parent[0] = d.stop_ids.len() as u32);
    assert_refused(|d| d.trip_pattern[0] = 9999);
    assert_refused(|d| d.trip_times_start[0] = d.stop_times.len() as u32);
    assert_refused(|d| {
        d.stop_names.pop();
    });
    assert_refused(|d| {
        d.service_bits.pop();
    });
}

#[test]
fn trip_in_the_wrong_pattern_is_corrupt() {
    // The fixture has more than one pattern; point trip 0 at another one.
    assert_refused(|d| d.trip_pattern[0] = (d.trip_pattern[0] + 1) % d.pattern_count() as u32);
    // A trip listed twice (and another one missing).
    assert_refused(|d| d.pattern_trips[1] = d.pattern_trips[0]);
}

#[test]
fn overlapping_trip_times_are_corrupt() {
    // Let trip 1 start where trip 0 starts (both stay inside stop_times).
    assert_refused(|d| {
        let first = d.pattern_trips[0] as usize;
        let second = d.pattern_trips[1] as usize;
        d.trip_times_start[second] = d.trip_times_start[first];
    });
}

#[test]
fn unknown_stop_kind_or_approximate_flag_is_corrupt() {
    assert_refused(|d| d.stop_kind[0] = 3);
    assert_refused(|d| d.trip_approximate[0] = 2);
}

/// A tiny fixed-seed random generator (xorshift64), so the test needs no
/// extra crate and gives the same mutations on every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[test]
fn ten_thousand_random_mutations_never_panic() {
    let bytes = tiny_bytes();
    let mut rng = Rng(0x5EED_1234_ABCD_0001);
    let mut loaded = 0;
    for _ in 0..10_000 {
        let mut bad = bytes.clone();
        // 1–4 random byte changes, sometimes also a random cut.
        for _ in 0..=rng.below(4) {
            let i = rng.below(bad.len());
            bad[i] = rng.next() as u8;
        }
        if rng.below(10) == 0 {
            bad.truncate(rng.below(bad.len()));
        }
        if let Ok(pack) = read_pack(&bad) {
            // With the checksum, only "mutations" that rewrote a byte with
            // its own value can still load.
            assert_eq!(bad, bytes, "a changed file loaded");
            loaded += 1;
            for t in 0..pack.timetable().trip_count() {
                let _ = pack.timetable().trip_times(t);
            }
            for s in 0..pack.timetable().stop_count() {
                let _ = pack.timetable().stop_pos(s);
            }
        }
    }
    println!("{loaded} of 10000 mutated packs still loaded");
}

// ---- real feed -----------------------------------------------------------------

/// Optional: the same full round trip on a real feed (see README).
#[test]
#[ignore = "needs a real feed, e.g. in fixtures/feeds (see README)"]
fn real_feed_round_trips() {
    let feed = std::env::var("RUSTLINE_MANIFEST_FEED").expect("set RUSTLINE_MANIFEST_FEED");
    let (tt, data, meta) = build(&workspace_path(&feed));
    let bytes = to_bytes(&data, &meta);
    assert_eq!(bytes, to_bytes(&data, &meta), "two writes differ");
    let zst = zstd::encode_all(&bytes[..], 19).unwrap();

    let t0 = Instant::now();
    let pack = read_pack(&bytes).unwrap();
    let load = t0.elapsed();
    let from_zst = read_pack(&zst).unwrap();

    assert_eq!(pack.timetable(), &data);
    assert_eq!(from_zst.timetable(), &data);
    assert_same(&tt, &pack);
    println!(
        "{feed}: {} trips checked, pack {} bytes, zst {} bytes, load {load:?}",
        tt.trips.len(),
        bytes.len(),
        zst.len()
    );
}

// ---- zstd bombs and checksum -------------------------------------------------

/// A valid-looking header that announces a body of `body_len` bytes.
fn bomb_header(body_len: u64) -> Vec<u8> {
    let mut h = rustline_pack::MAGIC.to_vec();
    h.extend_from_slice(&rustline_pack::FORMAT_VERSION.to_le_bytes());
    h.extend_from_slice(&[0; 64]); // feed + osm sha
    h.extend_from_slice(&0i64.to_le_bytes()); // built_at
    h.extend_from_slice(&[0; 8]); // valid_from, valid_to = 1970-01-01
    h.extend_from_slice(&[0; 4]); // empty timezone and feed_id
    h.extend_from_slice(&body_len.to_le_bytes());
    h
}

/// Header + `zeros` zero bytes, zstd-compressed in 1 MiB chunks (the input is
/// never held in memory). `declare` writes the size into the frame.
fn zstd_bomb(body_len: u64, zeros: u64, declare: bool) -> Vec<u8> {
    use std::io::Write;
    let header = bomb_header(body_len);
    let mut enc = zstd::Encoder::new(Vec::new(), 1).unwrap();
    if declare {
        enc.set_pledged_src_size(Some(header.len() as u64 + zeros))
            .unwrap();
        enc.include_contentsize(true).unwrap();
    }
    enc.write_all(&header).unwrap();
    let chunk = vec![0u8; 1 << 20];
    let mut left = zeros;
    while left > 0 {
        let n = left.min(chunk.len() as u64) as usize;
        enc.write_all(&chunk[..n]).unwrap();
        left -= n as u64;
    }
    enc.finish().unwrap()
}

#[test]
fn zstd_bomb_with_declared_size_is_refused_before_decompressing() {
    let bomb = zstd_bomb(1 << 30, 1 << 30, true); // claims and holds 1 GiB
    assert!(bomb.len() < 100_000, "bomb is {} bytes", bomb.len());
    let err = read_pack(&bomb).unwrap_err();
    assert!(err.to_string().contains("more than the limit"), "{err}");
}

#[test]
fn zstd_bomb_without_declared_size_is_refused_at_the_header() {
    let bomb = zstd_bomb(1 << 30, 1 << 30, false);
    let err = read_pack(&bomb).unwrap_err();
    assert!(err.to_string().contains("larger than the limit"), "{err}");
}

/// The worst case under the limit: 200 MiB of zeros that pass the size
/// checks, get decompressed into one buffer and then fail the checksum.
/// Peak RAM stays around one body (measured in the M2 report).
#[test]
fn zstd_bomb_under_the_limit_fails_the_checksum() {
    let bomb = zstd_bomb(200 << 20, (200 << 20) + 4, false);
    assert!(is_corrupt(read_pack(&bomb)));
}

#[test]
fn caller_can_lower_the_limit() {
    let bytes = tiny_bytes();
    assert!(rustline_pack::read_pack_with_limit(&bytes, 1 << 20).is_ok());
    assert!(is_corrupt(rustline_pack::read_pack_with_limit(&bytes, 100)));
    let zst = zstd::bulk::compress(&bytes, 3).unwrap();
    assert!(is_corrupt(rustline_pack::read_pack_with_limit(&zst, 100)));
}

#[test]
fn any_flipped_bit_fails_the_checksum() {
    let bytes = tiny_bytes();
    // Skip magic + version, which have their own errors.
    for i in 6..bytes.len() {
        let mut bad = bytes.clone();
        bad[i] ^= 0x10;
        assert!(is_corrupt(read_pack(&bad)), "byte {i}");
    }
}

/// Header claiming `body_len`, zstd-compressed with nothing after it (31 B for 255 MiB).
fn tiny_liar(body_len: u64) -> Vec<u8> {
    zstd::bulk::compress(&bomb_header(body_len), 19).unwrap()
}

#[test]
fn tiny_zst_claiming_a_huge_body_allocates_little() {
    let liar = tiny_liar(255 << 20);
    println!("liar is {} bytes", liar.len());
    assert!(liar.len() < 64);
    assert!(is_corrupt(read_pack(&liar)));
}

#[test]
fn huge_claim_with_no_limit_is_an_error_not_an_abort() {
    let claim = 1u64 << 60;
    // Plain file: the body can't be longer than the file.
    let plain = bomb_header(claim);
    assert!(is_corrupt(rustline_pack::read_pack_with_limit(
        &plain,
        u64::MAX
    )));
    // zstd, size not declared: memory only grows as data arrives.
    assert!(is_corrupt(rustline_pack::read_pack_with_limit(
        &tiny_liar(claim),
        u64::MAX
    )));
}

#[test]
fn odd_trip_times_start_is_corrupt() {
    assert_refused(|d| d.trip_times_start[0] += 1);
}

#[test]
fn accessors_never_panic_on_bad_indices() {
    let (_, data, _) = tiny();
    assert!(data.trip_times(usize::MAX).is_empty());
    assert!(data.pattern_stops(usize::MAX).is_empty());
    assert_eq!(data.stop_pos(usize::MAX), None);
    assert!(!data.service_runs(usize::MAX, 0));
}

// ---- format 3: pickup/drop-off, transfers.txt, block_id -----------------------

#[test]
fn pickup_transfers_and_blocks_round_trip() {
    let feed = TempFeed::new(
        "stop_id,stop_name,stop_lat,stop_lon\nA,A,1.0,1.0\nB,B,2.0,1.0\nC,C,3.0,1.0\n",
        "trip_id,arrival_time,departure_time,stop_id,stop_sequence,pickup_type,drop_off_type\n\
         T1,08:00:00,08:00:00,A,1,0,1\nT1,08:10:00,08:10:00,B,2,2,3\nT1,08:20:00,08:20:00,C,3,1,0\n",
    );
    fs::write(
        feed.0.join("trips.txt"),
        "route_id,service_id,trip_id,block_id\nR,S,T1,BLK\n",
    )
    .unwrap();
    // (A row naming an unknown stop makes gtfs-structures reject the whole
    // feed, see README Known gaps.)
    fs::write(
        feed.0.join("transfers.txt"),
        "from_stop_id,to_stop_id,transfer_type,min_transfer_time\nB,C,2,90\nB,B,3,\nA,C,1,\n",
    )
    .unwrap();
    let (_, data, meta) = build(&feed.0);
    let pack = read_pack(&to_bytes(&data, &meta)).unwrap();
    let t = &pack.timetable();
    assert_eq!(t.trip_block_ids, ["BLK"]);
    assert_eq!(t.stop_pickup, [0, 2, 1]);
    assert_eq!(t.stop_drop_off, [1, 3, 0]);
    assert_eq!(t.pickup_drop_off(1), (2, 3));
    assert_eq!(t.pickup_drop_off(99), (0, 0));
    assert_eq!(t.transfer_start, [0, 1, 3, 3]);
    assert_eq!(t.transfer_to, [2, 1, 2]);
    assert_eq!(t.transfer_type, [1, 3, 2]);
    assert_eq!(t.transfer_secs, [0, 0, 90]);
    assert_eq!(t.transfers(1), 1..3);
    assert_eq!(t.transfers(usize::MAX), 0..0);
}

#[test]
fn optional_format_3_arrays_may_be_empty() {
    let (_, mut data, meta) = tiny();
    data.stop_pickup.clear();
    data.stop_drop_off.clear();
    data.trip_block_ids.clear();
    data.transfer_start.clear();
    data.transfer_to.clear();
    data.transfer_type.clear();
    data.transfer_secs.clear();
    let pack = read_pack(&to_bytes(&data, &meta)).unwrap();
    assert_eq!(pack.timetable().pickup_drop_off(0), (0, 0));
    assert_eq!(pack.timetable().transfers(0), 0..0);
}

#[test]
fn bad_format_3_arrays_are_corrupt() {
    assert_refused(|d| d.stop_pickup[0] = 4);
    assert_refused(|d| d.stop_drop_off[0] = 200);
    assert_refused(|d| {
        d.stop_pickup.pop();
    });
    assert_refused(|d| d.trip_block_ids.push(String::new()));
    assert_refused(|d| {
        d.transfer_start.pop();
    });
    assert_refused(|d| {
        d.transfer_start.clear();
        d.transfer_to = vec![0];
        d.transfer_type = vec![0];
        d.transfer_secs = vec![0];
    });
    assert_refused(|d| {
        let last = d.transfer_start.len() - 1;
        d.transfer_start[last] += 1;
        d.transfer_to.push(u32::MAX);
        d.transfer_type.push(0);
        d.transfer_secs.push(0);
    });
    assert_refused(|d| {
        let last = d.transfer_start.len() - 1;
        d.transfer_start[last] += 1;
        d.transfer_to.push(0);
        d.transfer_type.push(7);
        d.transfer_secs.push(0);
    });
    assert_refused(|d| d.transfer_secs.push(1));
}

#[test]
fn pack_new_checks_its_input() {
    let (_, data, meta) = tiny();
    assert!(Pack::new(meta.clone(), data.clone()).is_ok());
    let mut bad = data.clone();
    bad.pattern_stops[0] = u32::MAX - 1;
    assert!(matches!(
        Pack::new(meta.clone(), bad),
        Err(PackError::Corrupt(_))
    ));
    let mut short = meta.clone();
    short.valid_to = short.valid_from + chrono::Days::new(u64::from(data.service_words) * 64);
    assert!(matches!(
        Pack::new(short, data.clone()),
        Err(PackError::Corrupt(_))
    ));
    let mut backwards = meta;
    backwards.valid_to = backwards.valid_from - chrono::Days::new(1);
    assert!(Pack::new(backwards, data).is_err());
}
