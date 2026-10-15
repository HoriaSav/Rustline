//! M4 hostile and fuzzed `.osm.pbf` input, written by Tester before M4.
//! Plan M4 DoD "Safety (§11): OSM reading capped (blob size, node/way counts vs a
//! configurable limit); no allocation sized from PBF headers; fuzzed .osm.pbf → no
//! panic in debug and release", edge-case block "Resources", Builder checklist §11.
//! A counting global allocator measures the peak heap of each parse, so an
//! allocation sized from a header shows up even when the machine has the RAM.
//! The `cfg rustline_m4` gate and the "M4 not implemented" ignores were removed in M4.
//! Run in both profiles:
//!   `cargo test --locked -p rustline-osm --test m4_osm_fuzz -- --include-ignored --test-threads 1`
//!   `cargo test --locked --release -p rustline-osm --test m4_osm_fuzz -- --include-ignored --test-threads 1`
//! (`--test-threads 1`: the heap peak is per process.)

#[path = "m4_pbf/mod.rs"]
mod m4_pbf;

use m4_pbf::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::{Duration, Instant};

struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: forwards to the system allocator and only counts bytes.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = System.alloc(l);
        if !p.is_null() {
            let now = LIVE.fetch_add(l.size(), Relaxed) + l.size();
            PEAK.fetch_max(now, Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l);
        LIVE.fetch_sub(l.size(), Relaxed);
    }
}

#[global_allocator]
static A: Counting = Counting;

/// Plan §11 phone-safe cap (256 MiB), used here as the heap budget and as a
/// generous `max_blob_bytes` in the limit tests. The reader's own default blob
/// limit is lower: `OsmLimits::default().max_blob_bytes` = 32 MiB.
const CAP: usize = 256 << 20;
/// Allowed heap above the baseline for one parse of a hostile file: twice the cap
/// plus slack. A blob inflated into a growing `Vec` briefly holds the old and the
/// new buffer while it reallocates (128 + 256 MiB seen with a stub), so 1× the
/// budget would fail honest implementations; unbounded inflation (516 MB here) still
/// fails. (With the 32 MiB default blob limit the reader stays far below this.)
/// The header-size cases below keep their own 64 MiB bound.
const HEAP_LIMIT: usize = 2 * CAP + (32 << 20);
/// No hostile file may take longer than this (debug build; release is much faster).
const TIME_LIMIT: Duration = Duration::from_secs(20);

fn build(p: &Path) -> Result<usize, String> {
    rustline_osm::build_foot_graph(p, None)
        .map(|(g, _)| g.node_coords().len())
        .map_err(|e| e.to_string())
}

/// Parses `bytes` from a file; returns Ok(nodes) / Err(text), the heap peak above
/// the baseline and the time. Panics (test failure) on a panic inside the parser.
fn parse(d: &TempDir, name: &str, bytes: &[u8]) -> (Result<usize, String>, usize, Duration) {
    let p = d.file("case.osm.pbf", bytes);
    let base = LIVE.load(Relaxed);
    PEAK.store(base, Relaxed);
    let t0 = Instant::now();
    let r = std::panic::catch_unwind(|| build(&p));
    let dt = t0.elapsed();
    let peak = PEAK.load(Relaxed).saturating_sub(base);
    let r = r.unwrap_or_else(|_| panic!("{name}: build_foot_graph panicked"));
    assert!(peak <= HEAP_LIMIT, "{name}: heap peak {} MiB", peak >> 20);
    assert!(dt <= TIME_LIMIT, "{name}: took {dt:?}");
    (r, peak, dt)
}

fn small_city() -> Osm {
    let mut o = Osm::new();
    let b = (1.0, 1.0);
    for i in 0..6 {
        let x = b.1 + dlon(100.0 * i as f64, b.0);
        o.street(
            (b.0, x),
            (b.0 + dlat(500.0), x),
            50.0,
            &[("highway", "residential")],
        );
        let y = b.0 + dlat(100.0 * i as f64);
        o.street(
            (y, b.1),
            (y, b.1 + dlon(500.0, b.0)),
            50.0,
            &[("highway", "footway")],
        );
    }
    o
}

fn good() -> Vec<u8> {
    to_pbf(
        &small_city(),
        PbfOptions {
            per_block: 40,
            ..Default::default()
        },
    )
}

#[test]
fn valid_small_city_parses() {
    let d = TempDir::new("fz_ok");
    let (r, peak, dt) = parse(&d, "good", &good());
    assert!(r.unwrap() > 0);
    println!("good: peak {} KiB, {dt:?}", peak >> 10);
}

#[test]
fn every_truncation_is_an_error_or_a_smaller_graph() {
    let d = TempDir::new("fz_trunc");
    let g = good();
    let full = parse(&d, "full", &g).0.unwrap();
    let step = (g.len() / 1500).max(1);
    for n in (0..g.len()).step_by(step) {
        if let (Ok(k), _, _) = parse(&d, &format!("trunc {n}"), &g[..n]) {
            assert!(k <= full, "trunc {n}: {k} > {full} nodes");
        }
    }
}

#[test]
fn random_bit_flips_never_panic() {
    let d = TempDir::new("fz_flip");
    let g = good();
    let mut rng = Rng(0x4D34_F11B);
    let (mut ok, mut err) = (0, 0);
    for i in 0..3000 {
        let mut b = g.clone();
        for _ in 0..1 + rng.below(3) {
            let o = rng.below(b.len() as u64) as usize;
            b[o] ^= 1 << rng.below(8);
        }
        match parse(&d, &format!("flip #{i}"), &b).0 {
            Ok(_) => ok += 1,
            Err(_) => err += 1,
        }
    }
    println!("bit flips: {ok} parsed, {err} errors");
}

#[test]
fn random_bytes_and_wrong_types_are_errors() {
    let d = TempDir::new("fz_rand");
    let mut rng = Rng(0x4D34_0BAD);
    for n in [0usize, 1, 3, 4, 5, 13, 100, 4096, 1 << 20] {
        let b: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
        assert!(parse(&d, &format!("random {n}"), &b).0.is_err());
    }
    let gtfs = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/fixtures/tiny/stop_times.txt"),
    )
    .unwrap();
    assert!(parse(&d, "gtfs text", &gtfs).0.is_err());
    // A gzip/zip header, an XML file, a PNG signature.
    for (name, b) in [
        ("zip", &b"PK\x03\x04\x14\x00\x08\x00\x08\x00"[..]),
        (
            "xml",
            &b"<?xml version=\"1.0\"?><osm version=\"0.6\"></osm>"[..],
        ),
        ("png", &b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"[..]),
    ] {
        assert!(parse(&d, name, b).0.is_err(), "{name}");
    }
}

#[test]
fn huge_declared_header_and_blob_sizes_are_refused_without_allocating() {
    // §11 "no allocation sized from PBF headers": each case declares a size far
    // beyond the file; the parse must fail without reserving that size.
    let d = TempDir::new("fz_huge");
    let payload = blob(&header_block(&["OsmSchema-V0.6"]), Compression::Raw);
    for (name, bytes) in [
        (
            "header len u32::MAX",
            frame_with("OSMHeader", payload.len() as i64, Some(u32::MAX), &payload),
        ),
        (
            "header len 2^31",
            frame_with("OSMHeader", payload.len() as i64, Some(1 << 31), &payload),
        ),
        (
            "header len 64 MiB",
            frame_with("OSMHeader", payload.len() as i64, Some(64 << 20), &payload),
        ),
        (
            "datasize i32::MAX",
            frame_with("OSMHeader", i64::from(i32::MAX), None, &payload),
        ),
        (
            "datasize 2^40",
            frame_with("OSMHeader", 1 << 40, None, &payload),
        ),
        ("datasize -1", frame_with("OSMHeader", -1, None, &payload)),
        ("datasize 0", frame_with("OSMHeader", 0, None, &payload)),
    ] {
        let mut b = bytes;
        b.extend(good()); // a valid file after the bad frame
        let (r, peak, _) = parse(&d, name, &b);
        assert!(r.is_err(), "{name}: {r:?}");
        assert!(peak < 64 << 20, "{name}: heap peak {} MiB", peak >> 20);
    }
}

#[test]
fn huge_declared_raw_size_and_packed_lengths_are_refused() {
    let d = TempDir::new("fz_raw");
    // zlib blob whose raw_size claims 2 GiB (real content 50 bytes).
    let mut m = Msg::default();
    m.uint(2, 2 << 30);
    m.bytes(3, &zlib_stored(&header_block(&["OsmSchema-V0.6"])));
    let mut b = frame("OSMHeader", &m.0);
    b.extend(good());
    let (r, peak, _) = parse(&d, "raw_size 2 GiB", &b);
    assert!(
        r.is_err() || peak < 64 << 20,
        "{r:?} peak {} MiB",
        peak >> 20
    );
    // A PrimitiveBlock whose packed field declares 2^40 bytes inside a small blob.
    let mut pb = Vec::new();
    varint(&mut pb, (2 << 3) | 2); // field 2 (primitivegroup), length-delimited
    varint(&mut pb, 1 << 40);
    pb.extend_from_slice(&[0; 16]);
    let mut b = frame(
        "OSMHeader",
        &blob(&header_block(&["OsmSchema-V0.6"]), Compression::Raw),
    );
    b.extend(frame("OSMData", &blob(&pb, Compression::Raw)));
    let (r, peak, _) = parse(&d, "packed len 2^40", &b);
    assert!(r.is_err(), "{r:?}");
    assert!(peak < 64 << 20, "heap peak {} MiB", peak >> 20);
}

#[test]
fn decompression_bombs_stop_at_the_cap() {
    // ~3.3 MB of deflate that inflates to ~516 MB of zeros (> the 256 MiB budget and
    // far above the reader's 32 MiB default blob limit).
    let d = TempDir::new("fz_bomb");
    let (z, total) = zlib_zero_bomb(2_000_000);
    assert!(total > CAP as u64);
    let header = frame(
        "OSMHeader",
        &blob(
            &header_block(&["OsmSchema-V0.6", "DenseNodes"]),
            Compression::Raw,
        ),
    );
    for (name, raw_size) in [
        ("honest raw_size", Some(total)),
        ("lying raw_size", Some(1000)),
        ("no raw_size", None),
    ] {
        let mut m = Msg::default();
        if let Some(n) = raw_size {
            m.uint(2, n);
        }
        m.bytes(3, &z);
        let mut b = header.clone();
        b.extend(frame("OSMData", &m.0));
        let (r, peak, dt) = parse(&d, name, &b);
        assert!(r.is_err(), "{name}: {r:?}");
        println!("bomb {name}: peak {} MiB, {dt:?}", peak >> 20);
    }
}

#[test]
fn unknown_blob_types_and_order_never_panic() {
    let d = TempDir::new("fz_types");
    let o = small_city();
    let data = to_pbf(&o, PbfOptions::default());
    // to_pbf writes this exact OSMHeader frame first.
    let header_len = frame(
        "OSMHeader",
        &blob(
            &header_block(&["OsmSchema-V0.6", "DenseNodes"]),
            Compression::ZlibStored,
        ),
    )
    .len();
    let body = &data[header_len..];
    let unknown = frame("OSMFutureType", &blob(b"whatever", Compression::Raw));
    let historical = frame(
        "OSMHeader",
        &blob(
            &header_block(&[
                "OsmSchema-V0.6",
                "HistoricalInformation",
                "Tester-Unknown-Feature",
            ]),
            Compression::Raw,
        ),
    );
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("data before header", [body, &data[..header_len]].concat()),
        ("data only", body.to_vec()),
        (
            "unknown blob type first",
            [&unknown[..], &data[..]].concat(),
        ),
        (
            "unknown blob type in the middle",
            [&data[..header_len], &unknown[..], body].concat(),
        ),
        ("unknown required feature", [&historical[..], body].concat()),
        ("two headers", [&data[..header_len], &data[..]].concat()),
    ];
    for (name, b) in cases {
        let _ = parse(&d, name, &b); // Ok or Err: only no panic, cap and time
    }
}

#[test]
fn node_and_way_count_limits_are_enforced() {
    // Plan §11: "node/way counts vs a configurable limit". small_city has > 100 nodes.
    let d = TempDir::new("fz_limits");
    let p = d.file("city.osm.pbf", &good());
    let nodes = small_city().nodes.len() as u64;
    let ways = small_city().ways.len() as u64;
    assert!(rustline_osm::build_foot_graph_with_limits(
        &p,
        None,
        &rustline_osm::OsmLimits {
            max_blob_bytes: CAP as u64,
            max_nodes: nodes / 2,
            max_ways: ways * 10
        }
    )
    .is_err());
    assert!(rustline_osm::build_foot_graph_with_limits(
        &p,
        None,
        &rustline_osm::OsmLimits {
            max_blob_bytes: CAP as u64,
            max_nodes: nodes * 10,
            max_ways: ways / 2
        }
    )
    .is_err());
    assert!(rustline_osm::build_foot_graph_with_limits(
        &p,
        None,
        &rustline_osm::OsmLimits {
            max_blob_bytes: 64,
            max_nodes: nodes * 10,
            max_ways: ways * 10
        }
    )
    .is_err());
    assert!(rustline_osm::build_foot_graph_with_limits(
        &p,
        None,
        &rustline_osm::OsmLimits {
            max_blob_bytes: CAP as u64,
            max_nodes: nodes * 10,
            max_ways: ways * 10
        }
    )
    .is_ok());
}

#[test]
fn nan_and_extreme_coordinates_in_the_file_never_panic() {
    // Raw coordinates near i64 limits and lat/lon out of range (±90/±180).
    let d = TempDir::new("fz_coord");
    let mut o = small_city();
    for (lat, lon) in [
        (91.0, 1.0),
        (-91.0, 1.0),
        (1.0, 181.0),
        (1.0, -181.0),
        (900_000.0, 1.0),
        (90.0, 180.0),
        (-90.0, -180.0),
    ] {
        let a = o.node(lat, lon);
        let b = o.node(1.0, 1.0001);
        o.way(&[a, b], &[("highway", "residential")]);
    }
    let _ = parse(&d, "extreme coords", &to_pbf(&o, PbfOptions::default()));
}
