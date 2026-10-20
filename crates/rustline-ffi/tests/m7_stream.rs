//! M7 tests-first (Tester, 2026-10-20): R5 streaming install: installing a large
//! pack adds < 16 MiB of heap (counting allocator, this process), so the whole pack
//! is never held in memory. Pack: `RUSTLINE_M7_BIG_ZST` (a .rlp.zst, e.g. MBTA), else
//! built from fixtures/feeds/mbta.zip; prints `skip:` when neither exists.
//! Run (release recommended): `cargo test --locked --release -p rustline-ffi --test m7_stream -- --nocapture`

mod ffi_common;
use ffi_common::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting;
static NOW: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let n = NOW.fetch_add(l.size(), Ordering::Relaxed) + l.size();
        PEAK.fetch_max(n, Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        NOW.fetch_sub(l.size(), Ordering::Relaxed);
        unsafe { System.dealloc(p, l) }
    }
}

#[global_allocator]
static A: Counting = Counting;

fn big_zst(dir: &std::path::Path) -> Option<String> {
    if let Ok(p) = std::env::var("RUSTLINE_M7_BIG_ZST") {
        if !p.is_empty() && std::path::Path::new(&p).is_file() {
            return Some(p);
        }
    }
    let feed = workspace("fixtures/feeds/mbta.zip");
    if !feed.is_file() {
        println!("skip: RUSTLINE_M7_BIG_ZST unset and fixtures/feeds/mbta.zip missing");
        return None;
    }
    let raw = rustline_gtfs::load(&feed).unwrap();
    let (tt, _) = rustline_gtfs::build_timetable(&raw).unwrap();
    let meta = rustline_gtfs::pack_meta(&tt, [0; 32], "mbta", 0);
    let data = rustline_gtfs::try_timetable_data(&tt).unwrap();
    let mut bytes = Vec::new();
    rustline_pack::write_pack(&data, &meta, &mut bytes).unwrap();
    let z = zstd::encode_all(&bytes[..], 3).unwrap();
    Some(put(dir, "mbta.rlp.zst", &z))
}

#[test]
fn r5_install_adds_less_than_16_mib_of_heap() {
    let d = scratch("m7stream");
    let Some(src) = big_zst(&d) else { return };
    let dest = d.join("mbta.rlp");
    let base = NOW.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    install(&src, &dest.to_string_lossy()).expect("install");
    let added = (PEAK.load(Ordering::Relaxed) - base) >> 20;
    let size = std::fs::metadata(&dest).unwrap().len() >> 20;
    println!("install of a {size} MiB pack: peak heap +{added} MiB");
    assert!(
        size >= 32,
        "pack {size} MiB is too small to tell streaming apart"
    );
    assert!(added < 16, "install added {added} MiB (pack {size} MiB)");
    assert!(open(&dest.to_string_lossy()).is_ok());
}
