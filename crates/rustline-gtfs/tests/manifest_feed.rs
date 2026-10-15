//! Optional check against a real feed that is not in the repo.
//!
//! Set `RUSTLINE_MANIFEST_FEED` to a GTFS zip and `RUSTLINE_MANIFEST_COUNTS`
//! to the expected `stops,routes,trips,stop_times,services,exceptions`, then
//! run `cargo test --locked -p rustline-gtfs --test manifest_feed -- --ignored`. Nothing city-specific is
//! hard-coded here (paths relative to the workspace root); see the README for the RoTransit Brașov values.

#[test]
#[ignore = "needs a real feed, e.g. in fixtures/feeds (see README)"]
fn real_feed_matches_expected_counts() {
    let (Ok(path), Ok(expected)) = (
        std::env::var("RUSTLINE_MANIFEST_FEED"),
        std::env::var("RUSTLINE_MANIFEST_COUNTS"),
    ) else {
        eprintln!("skip: set RUSTLINE_MANIFEST_FEED and RUSTLINE_MANIFEST_COUNTS (see README)");
        return;
    };
    let expected: Vec<usize> = expected
        .split(',')
        .map(|n| n.trim().parse().unwrap())
        .collect();
    // Relative paths are taken from the workspace root, not this crate's directory.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path);
    let feed = rustline_gtfs::load(&path).unwrap();
    let c = feed.counts();
    let got = vec![
        c.stops,
        c.routes,
        c.trips,
        c.stop_times,
        c.services,
        c.service_exceptions,
    ];
    assert_eq!(got, expected);
    let (_, report) = rustline_gtfs::build_timetable(&feed).unwrap();
    assert!(report.is_clean(), "{report}");
}
