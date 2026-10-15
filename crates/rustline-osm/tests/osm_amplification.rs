//! Builder (M4 fix round, Reviewer R28): small files whose blocks unpack to
//! ~31 MiB of 1-byte list items must not make the reader hold many times that.
//! Lists (packed varints, string tables, way refs) are capped at
//! `max_blob_bytes / 64` items per block. A counting allocator measures the
//! peak heap of each parse. Run with `--test-threads 1` (the peak is per process).

#[path = "m4_pbf/mod.rs"]
mod m4_pbf;

use m4_pbf::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::sync::Mutex;

struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
/// The peak is per process: one parse at a time.
static ONE: Mutex<()> = Mutex::new(());

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

/// Raw block size of every case (under the 32 MiB default blob limit).
const RAW: usize = 31 << 20;
/// Peak heap allowed for one parse: the unpacked block (and its buffer while
/// it grows) plus the capped lists. Measured: 48 MiB after the fix; before it,
/// 536 (way refs), 672 (dense nodes) and 416 MiB (string table).
const HEAP_LIMIT: usize = 64 << 20;

/// A file with an OSMHeader and one zlib (deflated) OSMData block.
fn file(block: &[u8]) -> Vec<u8> {
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    z.write_all(block).unwrap();
    let z = z.finish().unwrap();
    let mut b = Msg::default();
    b.uint(2, block.len() as u64);
    b.bytes(3, &z);
    let mut out = frame(
        "OSMHeader",
        &blob(
            &header_block(&["OsmSchema-V0.6", "DenseNodes"]),
            Compression::Raw,
        ),
    );
    out.extend(frame("OSMData", &b.0));
    out
}

/// PrimitiveBlock with a short string table and one group.
fn block(group: &[u8]) -> Vec<u8> {
    let mut st = Msg::default();
    st.bytes(1, b"").bytes(1, b"highway").bytes(1, b"footway");
    let mut pb = Msg::default();
    pb.bytes(1, &st.0).bytes(2, group);
    pb.0
}

fn parse(name: &str, bytes: &[u8]) -> (Result<usize, String>, usize) {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let d = TempDir::new("amp");
    let p = d.file("case.osm.pbf", bytes);
    let base = LIVE.load(Relaxed);
    PEAK.store(base, Relaxed);
    let r = rustline_osm::build_foot_graph(&p, None)
        .map(|(g, _)| g.node_coords().len())
        .map_err(|e| e.to_string());
    let peak = PEAK.load(Relaxed).saturating_sub(base);
    println!(
        "{name}: file {} KB, heap peak {} MiB, {r:?}",
        bytes.len() >> 10,
        peak >> 20
    );
    (r, peak)
}

#[test]
fn way_with_31_million_refs() {
    let mut way = Msg::default();
    way.uint(1, 1)
        .packed_uint(2, &[1])
        .packed_uint(3, &[2])
        .bytes(8, &vec![0u8; RAW]);
    let mut g = Msg::default();
    g.bytes(3, &way.0);
    let (r, peak) = parse("way refs", &file(&block(&g.0)));
    assert!(r.is_err(), "{r:?}");
    assert!(peak <= HEAP_LIMIT, "heap peak {} MiB", peak >> 20);
}

#[test]
fn dense_nodes_with_10_million_entries_per_list() {
    let n = RAW / 3;
    let mut dense = Msg::default();
    dense
        .bytes(1, &vec![0u8; n])
        .bytes(8, &vec![0u8; n])
        .bytes(9, &vec![0u8; n]);
    let mut g = Msg::default();
    g.bytes(2, &dense.0);
    let (r, peak) = parse("dense nodes", &file(&block(&g.0)));
    assert!(r.is_err(), "{r:?}");
    assert!(peak <= HEAP_LIMIT, "heap peak {} MiB", peak >> 20);
}

#[test]
fn string_table_with_15_million_entries() {
    // Each entry is 2 bytes (field 1, length 0).
    let st: Vec<u8> = [0x0Au8, 0x00].repeat(RAW / 2 - 16);
    let mut pb = Msg::default();
    pb.bytes(1, &st);
    let (r, peak) = parse("string table", &file(&pb.0));
    assert!(r.is_err(), "{r:?}");
    assert!(peak <= HEAP_LIMIT, "heap peak {} MiB", peak >> 20);
}

#[test]
fn normal_blocks_are_not_affected() {
    // The cap must not reject an ordinary file.
    let mut o = Osm::new();
    o.street(
        (1.0, 1.0),
        (1.0 + dlat(500.0), 1.0),
        50.0,
        &[("highway", "residential")],
    );
    let (r, _) = parse("normal", &to_pbf(&o, PbfOptions::default()));
    assert!(r.unwrap() > 0);
}
