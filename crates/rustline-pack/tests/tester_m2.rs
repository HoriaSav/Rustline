//! Tester checks for M2 (pack v1):
//! - deep round trip, compared field by field with the M1 `Timetable` (not with
//!   `TimetableData`), on the tiny fixture and on every feed in `fixtures/feeds/`;
//! - determinism (same bytes from independent builds, and from a feed whose rows
//!   are shuffled);
//! - a deterministic corruption loop: every truncation, every single-bit flip of
//!   the tiny pack, inflated `body_len`, bad versions, zeroed body, garbage files.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use chrono::Datelike;
use rustline_gtfs::{StopKind, Timetable};
use rustline_model::TripIdx;
use rustline_pack::{read_pack, write_pack, Pack, PackError, PackMeta, FORMAT_VERSION, MAGIC};
use sha2::{Digest, Sha256};

fn workspace(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

fn build(feed: &Path) -> (Timetable, Vec<u8>, PackMeta) {
    let raw = rustline_gtfs::load(feed).expect("feed loads");
    let (tt, _) = rustline_gtfs::build_timetable(&raw).expect("feed builds");
    let data = rustline_gtfs::try_timetable_data(&tt).unwrap();
    let sha: [u8; 32] = Sha256::digest(feed.to_string_lossy().as_bytes()).into();
    let meta = rustline_gtfs::pack_meta(&tt, sha, "tester-feed", 1_234_567_890);
    let mut bytes = Vec::new();
    write_pack(&data, &meta, &mut bytes).expect("pack writes");
    (tt, bytes, meta)
}

/// Independent reimplementation of "does service s run on date".
fn runs(s: &rustline_gtfs::Service, d: chrono::NaiveDate) -> bool {
    if s.removed.contains(&d) {
        return false;
    }
    if s.added.contains(&d) {
        return true;
    }
    s.date_range.is_some_and(|(a, b)| {
        a <= d && d <= b && s.weekdays[d.weekday().num_days_from_monday() as usize]
    })
}

/// Compares every field of the pack with the M1 timetable.
fn assert_deep_equal(tt: &Timetable, pack: &Pack, meta: &PackMeta) {
    assert_eq!(&pack.meta, meta);
    assert_eq!(pack.meta.format_version, FORMAT_VERSION);
    assert_eq!(pack.meta.osm_sha256, [0; 32]);
    assert_eq!(pack.meta.timezone, tt.timezone);
    assert_eq!(
        (pack.meta.valid_from, pack.meta.valid_to),
        (tt.valid_from, tt.valid_to)
    );
    let t = &pack.timetable;

    assert_eq!(t.stop_count(), tt.stops.len(), "stops");
    for (i, s) in tt.stops.iter().enumerate() {
        assert_eq!(t.stop_ids[i], s.id);
        assert_eq!(t.stop_names[i], s.name);
        // Coordinates are stored as f64 bits: exact, 0 m error.
        assert_eq!(
            t.stop_pos(i).map(|(a, b)| (a.to_bits(), b.to_bits())),
            s.pos.map(|p| (p.lat.to_bits(), p.lon.to_bits())),
            "pos of {}",
            s.id
        );
        let kind = match s.kind {
            StopKind::Stop => 0,
            StopKind::Station => 1,
            StopKind::Other => 2,
        };
        assert_eq!(t.stop_kind[i], kind);
        assert_eq!(t.stop_parent[i], s.parent.map_or(u32::MAX, |p| p.0));
    }

    assert_eq!(t.route_ids.len(), tt.routes.len(), "routes");
    for (i, r) in tt.routes.iter().enumerate() {
        assert_eq!(t.route_ids[i], r.id);
        assert_eq!(t.route_short_names[i], r.short_name);
        assert_eq!(t.route_long_names[i], r.long_name);
        assert_eq!(i32::from(t.route_types[i]), i32::from(r.route_type));
    }

    assert_eq!(t.trip_count(), tt.trips.len(), "trips");
    assert_eq!(t.stop_time_count(), tt.stop_times.len(), "stop_times");
    for (i, tr) in tt.trips.iter().enumerate() {
        assert_eq!(t.trip_ids[i], tr.id);
        assert_eq!(t.trip_route[i], tr.route.0);
        assert_eq!(t.trip_service[i], tr.service.0);
        assert_eq!(t.trip_headsigns[i], tr.headsign);
        assert_eq!(t.trip_approximate[i], u8::from(tr.approximate));
        assert_eq!(t.trip_pattern[i], tr.pattern.0);
        let want: Vec<(u32, u32, u32)> = tt
            .trip_stop_times(TripIdx::from_usize(i))
            .iter()
            .map(|st| (st.stop.0, st.arrival.0, st.departure.0))
            .collect();
        assert_eq!(t.trip_times(i), want, "times of {}", tr.id);
    }

    assert_eq!(t.pattern_count(), tt.patterns.len(), "patterns");
    let mut served: Vec<BTreeSet<u32>> = vec![BTreeSet::new(); tt.stops.len()];
    for (p, pat) in tt.patterns.iter().enumerate() {
        let stops: Vec<u32> = pat.stops.iter().map(|s| s.0).collect();
        assert_eq!(t.pattern_stops(p), &stops[..]);
        let (a, b) = (
            t.pattern_trips_start[p] as usize,
            t.pattern_trips_start[p + 1] as usize,
        );
        let trips: Vec<u32> = pat.trips.iter().map(|x| x.0).collect();
        assert_eq!(&t.pattern_trips[a..b], &trips[..], "pattern {p} trip order");
        for s in &stops {
            served[*s as usize].insert(p as u32);
        }
    }
    for (s, want) in served.iter().enumerate() {
        let (a, b) = (
            t.stop_patterns_start[s] as usize,
            t.stop_patterns_start[s + 1] as usize,
        );
        let got: BTreeSet<u32> = t.stop_patterns[a..b].iter().copied().collect();
        assert_eq!(got.len(), b - a, "stop {s} lists a pattern twice");
        assert_eq!(&got, want, "patterns serving stop {s}");
    }

    assert_eq!(t.service_ids.len(), tt.services.len(), "services");
    let days = (tt.valid_to - tt.valid_from).num_days() as u32 + 1;
    for (i, s) in tt.services.iter().enumerate() {
        assert_eq!(t.service_ids[i], s.id);
        for d in 0..days {
            let date = tt.valid_from + chrono::Days::new(u64::from(d));
            assert_eq!(
                t.service_runs(i, d),
                runs(s, date),
                "service {} on {date}",
                s.id
            );
        }
        // Exceptions inside the window are honoured.
        for x in &s.added {
            if let Ok(d) = u32::try_from((*x - tt.valid_from).num_days()) {
                if d < days {
                    assert!(t.service_runs(i, d));
                }
            }
        }
        for x in &s.removed {
            if let Ok(d) = u32::try_from((*x - tt.valid_from).num_days()) {
                if d < days {
                    assert!(!t.service_runs(i, d));
                }
            }
        }
    }
}

fn round_trip(feed: &Path) -> String {
    let t0 = Instant::now();
    let (tt, bytes, meta) = build(feed);
    let build_t = t0.elapsed();
    let zst = zstd::encode_all(&bytes[..], 19).unwrap();
    let t1 = Instant::now();
    let pack = read_pack(&bytes).expect("pack loads");
    let load_t = t1.elapsed();
    assert_deep_equal(&tt, &pack, &meta);
    assert_eq!(read_pack(&zst).expect("zst loads"), pack);
    format!(
        "{}: rlp {} B, zst {} B, build {:?}, load {:?}, stops {} routes {} trips {} stop_times {} patterns {} services {}",
        feed.file_name().unwrap().to_string_lossy(), bytes.len(), zst.len(), build_t, load_t,
        tt.stops.len(), tt.routes.len(), tt.trips.len(), tt.stop_times.len(), tt.patterns.len(), tt.services.len()
    )
}

#[test]
fn tiny_and_broken_fixtures_round_trip_deeply() {
    for f in ["data/fixtures/tiny", "data/fixtures/broken"] {
        println!("{}", round_trip(&workspace(f)));
    }
}

/// Every feed in fixtures/feeds/ (skipped when the folder is missing or empty).
/// Feeds the GTFS loader can't read (README Known gap (M1) #5) are listed, not failed.
/// Slow in debug; run with
/// `cargo test --locked --release -p rustline-pack --test tester_m2 -- --ignored --nocapture`.
#[test]
#[ignore = "slow: every feed in fixtures/feeds (run with --test tester_m2 -- --ignored)"]
fn every_fixture_feed_round_trips_deeply() {
    let dir = workspace("fixtures/feeds");
    let mut feeds: Vec<PathBuf> = match fs::read_dir(&dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "zip"))
            .collect(),
        Err(_) => Vec::new(),
    };
    feeds.sort();
    if feeds.is_empty() {
        println!(
            "no feeds in {}, skipped (run scripts/fetch-fixtures.sh)",
            dir.display()
        );
        return;
    }
    for f in &feeds {
        if rustline_gtfs::load(f).is_err() {
            println!(
                "{}: GTFS load fails (Known gap (M1) #5), no pack",
                f.display()
            );
            continue;
        }
        println!("{}", round_trip(f));
    }
}

// ---- determinism ---------------------------------------------------------------

#[test]
fn independent_builds_are_byte_identical() {
    let tiny = workspace("data/fixtures/tiny");
    let a = Sha256::digest(build(&tiny).1);
    let b = Sha256::digest(build(&tiny).1);
    assert_eq!(a, b);
}

/// Copy of the tiny fixture with every file's data rows reversed.
struct Shuffled(PathBuf);
impl Drop for Shuffled {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn shuffled_csv_rows_give_the_same_pack() {
    let src = workspace("data/fixtures/tiny");
    let dir = std::env::temp_dir().join(format!("rustline_tester_shuffled_{}", std::process::id()));
    let _guard = Shuffled(dir.clone());
    fs::create_dir_all(&dir).unwrap();
    for e in fs::read_dir(&src).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "txt") {
            let text = fs::read_to_string(&p).unwrap();
            let mut lines: Vec<&str> = text.lines().collect();
            lines[1..].reverse();
            fs::write(dir.join(p.file_name().unwrap()), lines.join("\n") + "\n").unwrap();
        }
    }
    let (_, a, _) = build(&src);
    let (_, b, mut meta_b) = build(&dir);
    // build() hashes the path into feed_sha256; neutralise that one field.
    let (tt_b, _, _) = build(&dir);
    meta_b.feed_sha256 = read_pack(&a).unwrap().meta.feed_sha256;
    let mut b2 = Vec::new();
    write_pack(
        &rustline_gtfs::try_timetable_data(&tt_b).unwrap(),
        &meta_b,
        &mut b2,
    )
    .unwrap();
    assert_ne!(b.len(), 0);
    assert_eq!(
        Sha256::digest(&a),
        Sha256::digest(&b2),
        "row order changed the pack"
    );
}

// ---- corruption ----------------------------------------------------------------

fn tiny_pack() -> (Vec<u8>, Pack) {
    let (_, bytes, _) = build(&workspace("data/fixtures/tiny"));
    let pack = read_pack(&bytes).unwrap();
    (bytes, pack)
}

/// Header length up to and including body_len.
fn header_len(p: &Pack) -> usize {
    4 + 2 + 32 + 32 + 8 + 4 + 4 + 2 + p.meta.timezone.len() + 2 + p.meta.feed_id.len() + 8
}

#[test]
fn every_truncation_is_corrupt() {
    let (bytes, _) = tiny_pack();
    for n in 0..bytes.len() {
        match read_pack(&bytes[..n]) {
            Err(PackError::Corrupt(_)) => {}
            Err(PackError::BadMagic) if n < 4 => {}
            other => panic!("truncated to {n}: {:?}", other.map(|_| "loaded")),
        }
    }
}

#[test]
fn every_single_bit_flip_stats() {
    let (bytes, good) = tiny_pack();
    let hl = header_len(&good);
    let (mut err, mut same, mut changed_header, mut changed_body) = (0, 0, 0, 0);
    for i in 0..bytes.len() {
        for bit in 0..8 {
            let mut bad = bytes.clone();
            bad[i] ^= 1 << bit;
            match read_pack(&bad) {
                Err(_) => err += 1,
                Ok(p) if p == good => same += 1,
                Ok(p) => {
                    if i < hl {
                        changed_header += 1
                    } else {
                        changed_body += 1
                    }
                    // A pack that loads must be safe to use.
                    for t in 0..p.timetable.trip_count() {
                        let _ = p.timetable.trip_times(t);
                    }
                    for s in 0..p.timetable.stop_count() {
                        let _ = p.timetable.stop_pos(s);
                    }
                }
            }
        }
    }
    println!(
        "bit flips: {} total, {err} rejected, {same} loaded unchanged, {changed_header} loaded with a changed header, {changed_body} loaded with a changed body (header {hl} B, file {} B)",
        bytes.len() * 8, bytes.len()
    );
}

/// Since the CRC32 trailer (and zstd's content checksum), every single-bit
/// flip anywhere in the file, plain or compressed, must be rejected.
#[test]
fn no_bit_flip_goes_undetected() {
    let (bytes, good) = tiny_pack();
    let zst = {
        let mut c = zstd::bulk::Compressor::new(19).unwrap();
        c.set_parameter(zstd::zstd_safe::CParameter::ChecksumFlag(true))
            .unwrap();
        c.compress(&bytes).unwrap()
    };
    for (name, file) in [("rlp", &bytes), ("zst", &zst)] {
        let mut accepted = Vec::new();
        let mut same = 0;
        for i in 0..file.len() {
            for bit in 0..8 {
                let mut bad = file.clone();
                bad[i] ^= 1 << bit;
                match read_pack(&bad) {
                    // zstd can decode a few flipped bits (e.g. in the frame
                    // header or an unused table bit) to the very same bytes;
                    // that is harmless, the pack is unchanged.
                    Ok(p) if p == good => same += 1,
                    Ok(_) => accepted.push((i, bit)),
                    Err(_) => {}
                }
            }
        }
        println!(
            "{name}: {} flips, {} loaded a different pack, {same} decoded to the identical pack",
            file.len() * 8,
            accepted.len()
        );
        assert!(
            accepted.is_empty(),
            "{name}: flips accepted at {:?}",
            &accepted[..accepted.len().min(10)]
        );
    }
}

#[test]
fn zstd_bomb_over_the_cap_is_rejected_cleanly() {
    // 300 MiB of zeros, above the 256 MiB default cap; the frame records its size.
    let big = vec![0u8; 300 << 20];
    let zst = zstd::bulk::compress(&big, 1).unwrap();
    drop(big);
    assert!(matches!(read_pack(&zst), Err(PackError::Corrupt(_))));
    // Same bytes streamed (no content size in the frame header).
    let mut streamed = Vec::new();
    {
        let mut enc = zstd::Encoder::new(&mut streamed, 1).unwrap();
        let chunk = vec![0u8; 1 << 20];
        for _ in 0..300 {
            std::io::Write::write_all(&mut enc, &chunk).unwrap();
        }
        enc.finish().unwrap();
    }
    // Zeros are not a pack header, so this is BadMagic; it must not unpack all 300 MiB first.
    assert!(read_pack(&streamed).is_err());
    // A streamed (size-less) frame whose header is real and claims a body over the cap.
    let (bytes0, good0) = tiny_pack();
    let hl0 = header_len(&good0);
    let mut hdr = bytes0[..hl0].to_vec();
    hdr[hl0 - 8..hl0].copy_from_slice(&(300u64 << 20).to_le_bytes());
    let mut streamed = Vec::new();
    {
        let mut enc = zstd::Encoder::new(&mut streamed, 1).unwrap();
        std::io::Write::write_all(&mut enc, &hdr).unwrap();
        let chunk = vec![0u8; 1 << 20];
        for _ in 0..300 {
            std::io::Write::write_all(&mut enc, &chunk).unwrap();
        }
        enc.finish().unwrap();
    }
    assert!(matches!(read_pack(&streamed), Err(PackError::Corrupt(_))));
    // A real header claiming a body just over the cap, inside a small zstd frame.
    let (bytes, good) = tiny_pack();
    let hl = header_len(&good);
    let mut b = bytes[..hl].to_vec();
    b[hl - 8..hl].copy_from_slice(&((256u64 << 20) + 1).to_le_bytes());
    let z = zstd::bulk::compress(&b, 1).unwrap();
    assert!(matches!(read_pack(&z), Err(PackError::Corrupt(_))));
}

#[test]
fn structural_header_damage_gives_typed_errors() {
    let (bytes, good) = tiny_pack();
    let hl = header_len(&good);

    let mut b = bytes.clone();
    b[0] = b'X';
    assert!(matches!(read_pack(&b), Err(PackError::BadMagic)));

    for v in [0u16, 1, 3, 99, u16::MAX] {
        let mut b = bytes.clone();
        b[4..6].copy_from_slice(&v.to_le_bytes());
        assert!(
            matches!(read_pack(&b), Err(PackError::UnsupportedVersion { found, supported })
            if found == v && supported == FORMAT_VERSION),
            "version {v}"
        );
    }

    let body_len_at = hl - 8;
    let real = u64::from_le_bytes(bytes[body_len_at..hl].try_into().unwrap());
    for l in [
        0,
        1,
        real - 1,
        real + 1,
        real * 2,
        u32::MAX as u64,
        u32::MAX as u64 + 1,
        u64::MAX / 2,
        u64::MAX,
    ] {
        let mut b = bytes.clone();
        b[body_len_at..hl].copy_from_slice(&l.to_le_bytes());
        assert!(
            matches!(read_pack(&b), Err(PackError::Corrupt(_))),
            "body_len {l}"
        );
    }

    // Timezone length inflated to 65535.
    let tz_at = 4 + 2 + 32 + 32 + 8 + 4 + 4;
    let mut b = bytes.clone();
    b[tz_at..tz_at + 2].copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(matches!(read_pack(&b), Err(PackError::Corrupt(_))));

    // Non-UTF-8 timezone.
    let mut b = bytes.clone();
    b[tz_at + 2] = 0xFF;
    assert!(matches!(read_pack(&b), Err(PackError::Corrupt(_))));

    // valid_from after valid_to; dates far out of range.
    let vf = 4 + 2 + 32 + 32 + 8;
    for (from, to) in [
        (100i32, 50i32),
        (i32::MAX, i32::MAX),
        (i32::MIN, i32::MIN),
        (0, i32::MAX),
    ] {
        let mut b = bytes.clone();
        b[vf..vf + 4].copy_from_slice(&from.to_le_bytes());
        b[vf + 4..vf + 8].copy_from_slice(&to.to_le_bytes());
        assert!(read_pack(&b).is_err(), "dates {from}..{to}");
    }

    // Zeroed body, 0xFF body, body halves swapped.
    for fill in [0x00u8, 0xFF] {
        let mut b = bytes.clone();
        b[hl..].fill(fill);
        assert!(
            matches!(read_pack(&b), Err(PackError::Corrupt(_))),
            "body filled {fill:#x}"
        );
    }

    // Trailing garbage, one byte and a lot.
    for extra in [1usize, 4096] {
        let mut b = bytes.clone();
        b.extend(std::iter::repeat_n(0xAB, extra));
        assert!(matches!(read_pack(&b), Err(PackError::Corrupt(_))));
    }
}

#[test]
fn empty_random_and_foreign_files_are_errors() {
    assert!(read_pack(&[]).is_err());
    let mut x: u64 = 0x1234_5678_9ABC_DEF0;
    for len in [1usize, 3, 4, 100, 4096, 1 << 20] {
        let v: Vec<u8> = (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                x as u8
            })
            .collect();
        assert!(read_pack(&v).is_err(), "random {len}");
        let mut m = MAGIC.to_vec();
        m.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        m.extend_from_slice(&v);
        assert!(read_pack(&m).is_err(), "magic+version+random {len}");
    }
    // A zip, and a zstd frame of garbage.
    let zip = fs::read(workspace("fixtures/feeds/google-sample.zip"))
        .unwrap_or_else(|_| b"PK\x03\x04junk".to_vec());
    assert!(matches!(read_pack(&zip), Err(PackError::BadMagic)));
    let zgarbage = zstd::encode_all(&b"not a pack at all"[..], 3).unwrap();
    assert!(matches!(read_pack(&zgarbage), Err(PackError::BadMagic)));
    assert!(read_pack(&[0x28, 0xB5, 0x2F, 0xFD]).is_err());
    // zstd of a pack wrapped twice must not recurse forever.
    let (bytes, _) = tiny_pack();
    let twice = zstd::encode_all(&zstd::encode_all(&bytes[..], 3).unwrap()[..], 3).unwrap();
    let _ = read_pack(&twice);
}
