//! M3 DoD: "10 golden Brașov queries checked by hand against the RATBV timetable;
//! who/when checked is noted in the test file. Uses `insta`; snapshot files committed."
//!
//! Tester can't write the golden queries before M3: the expected journeys must be
//! checked by a person against the RATBV timetable, and `insta` is not a dependency
//! yet. This test only checks that Builder delivered them.
//! Run: `cargo test --locked -p rustline-core --test m3_golden`

use std::path::Path;

#[test]
fn ten_hand_checked_golden_snapshots_are_committed() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let snaps: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "snap"))
        .collect();
    assert!(
        snaps.len() >= 10,
        "{} snapshots in {}",
        snaps.len(),
        dir.display()
    );
    // The test source that produced them must say who checked them and when.
    let src: String = std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests"))
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "rs"))
        // Skip this file, which names the markers itself.
        .filter(|p| !p.ends_with("m3_golden.rs"))
        .map(|p| std::fs::read_to_string(p).unwrap_or_default())
        .collect();
    assert!(
        src.contains("insta::assert"),
        "no insta snapshot assertions found"
    );
    let lower = src.to_lowercase();
    assert!(
        lower.contains("checked by"),
        "no 'checked by <who> on <date>' note found"
    );
}
