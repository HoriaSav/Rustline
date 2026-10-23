//! Builder (M4 fix round, Reviewer R28): small files whose blocks unpack to
//! ~31 MiB of 1-byte list items must not make the reader hold many times that.
//! Lists (packed varints, string tables, way refs) are capped at
//! `max_blob_bytes / 64` items per block. A counting allocator measures the
//! peak heap of each parse, counting only the calling thread's allocations
//! (the parse is single-threaded). A process-wide count could also see
//! other threads (libtest), the likely cause of 79 MiB on GitHub runners where
//! the parse itself is 48 MiB (plan M10 decision 5). Tests still hold `ONE`.

#[path = "m4_pbf/mod.rs"]
mod m4_pbf;

use m4_pbf::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::io::Write;
use std::sync::Mutex;

struct Counting;
thread_local! {
    // Per thread, so only the measured thread counts. Signed: a block freed
    // here may have been allocated before `parse` took its base. Const-init
    // `Cell`s never allocate, so the allocator may touch them.
    static LIVE: Cell<isize> = const { Cell::new(0) };
    static PEAK: Cell<isize> = const { Cell::new(0) };
}
/// One test at a time keeps the measured parses apart (and their output).
static ONE: Mutex<()> = Mutex::new(());

fn live() -> isize {
    LIVE.with(Cell::get)
}

fn count(delta: isize) {
    // `try_with`: thread-local storage can be gone while a thread exits.
    let _ = LIVE.try_with(|l| {
        let now = l.get() + delta;
        l.set(now);
        let _ = PEAK.try_with(|p| p.set(p.get().max(now)));
    });
}

/// Taken on the first line of every test in this file.
fn one() -> std::sync::MutexGuard<'static, ()> {
    ONE.lock().unwrap_or_else(|e| e.into_inner())
}

// SAFETY: forwards to the system allocator and only counts bytes.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = System.alloc(l);
        if !p.is_null() {
            // Layout sizes are at most isize::MAX, so this never saturates.
            count(isize::try_from(l.size()).unwrap_or(isize::MAX));
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l);
        count(-isize::try_from(l.size()).unwrap_or(isize::MAX));
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
    let d = TempDir::new("amp");
    let p = d.file("case.osm.pbf", bytes);
    let base = live();
    PEAK.with(|p| p.set(base));
    let r = rustline_osm::build_foot_graph(&p, None)
        .map(|(g, _)| g.node_coords().len())
        .map_err(|e| e.to_string());
    let peak = usize::try_from(PEAK.with(Cell::get) - base).unwrap_or(0);
    println!(
        "{name}: file {} KB, heap peak {} MiB, {r:?}",
        bytes.len() >> 10,
        peak >> 20
    );
    (r, peak)
}

#[test]
fn way_with_31_million_refs() {
    let _one = one();
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
    let _one = one();
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
    let _one = one();
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
    let _one = one();
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
