//! `Engine::from_bytes` / `open` on packs built from GTFS feeds.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rustline_core::Engine;
use rustline_pack::PackError;

fn workspace_path(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

fn pack_bytes(feed: &Path) -> Vec<u8> {
    let raw = rustline_gtfs::load(feed).unwrap();
    let (tt, _) = rustline_gtfs::build_timetable(&raw).unwrap();
    let data = rustline_gtfs::try_timetable_data(&tt).unwrap();
    let meta = rustline_gtfs::pack_meta(&tt, [0xAB; 32], "tiny", 0);
    let mut bytes = Vec::new();
    rustline_pack::write_pack(&data, &meta, &mut bytes).unwrap();
    bytes
}

#[test]
fn info_matches_the_pack() {
    let engine = Engine::from_bytes(&pack_bytes(&workspace_path("data/fixtures/tiny"))).unwrap();
    let info = engine.info();
    assert_eq!(info.feed_sha256, "ab".repeat(32));
    assert_eq!(info.format_version, rustline_pack::FORMAT_VERSION);
    assert_eq!(info.feed_id, "tiny");
    assert_eq!(info.trips, engine.pack().timetable().trip_count());
    assert!(info.valid_from <= info.valid_to);
}

#[test]
fn missing_file_is_an_io_error() {
    let r = Engine::open(Path::new("/nonexistent/rustline.rlp"));
    assert!(matches!(r, Err(PackError::Io(_))));
}

/// Median of 10 `Engine::from_bytes` runs on a real feed; run with `--release`.
/// `RUSTLINE_MANIFEST_FEED=fixtures/feeds/<feed>.zip cargo test --locked --release -p rustline-core --test engine -- --ignored --nocapture`
#[test]
#[ignore = "needs a real feed, e.g. in fixtures/feeds (see README)"]
fn from_bytes_median_of_10() {
    let Ok(feed) = std::env::var("RUSTLINE_MANIFEST_FEED") else {
        eprintln!(
            "skip: set RUSTLINE_MANIFEST_FEED=fixtures/feeds/brasov-manifest.zip (see README)"
        );
        return;
    };
    let bytes = pack_bytes(&workspace_path(&feed));
    let mut times: Vec<Duration> = (0..10)
        .map(|_| {
            let t = Instant::now();
            let engine = Engine::from_bytes(&bytes).unwrap();
            let elapsed = t.elapsed();
            assert!(engine.info().trips > 0);
            elapsed
        })
        .collect();
    times.sort();
    println!(
        "{feed}: {} bytes, Engine::from_bytes median {:?} (min {:?}, max {:?})",
        bytes.len(),
        times[5],
        times[0],
        times[9]
    );
}
