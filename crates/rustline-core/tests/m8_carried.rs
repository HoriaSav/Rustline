//! M8 tests-first (Tester, 2026-10-20): plan M8 "Carried from M7 review".
//! 1. The in-memory grid check (`Engine::from_bytes` → `build_grid`) and the on-disk
//!    one (`verify_pack_file` + `check_installable` → `check_disk_grid`) agree, for a
//!    street graph spanning the globe (±85° lat, ±180° lon) and for one cell just
//!    under / just over the per-cell cap (8191 / 8193 segments). Ungated: a pin that
//!    must stay green (it already passes on M7).
//! 2. `Grid::for_box` carries a comment that the snapping budget depends on the
//!    highest-latitude choice (source scan; ungated since M8).
//!
//! Run: `cargo test --locked -p rustline-core --test m8_carried`

#![allow(unexpected_cfgs)]

use std::path::{Path, PathBuf};

use rustline_core::Engine;
use rustline_pack::{write_pack, TimetableData};

fn workspace_path(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

fn haversine_m(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (la1, la2) = (a.0.to_radians(), b.0.to_radians());
    let h = ((la2 - la1) / 2.0).sin().powi(2)
        + la1.cos() * la2.cos() * ((b.1 - a.1).to_radians() / 2.0).sin().powi(2);
    2.0 * 6_371_008.8 * h.sqrt().min(1.0).asin()
}

/// The tiny fixture plus the given straight segments (degrees).
type Seg = ((f64, f64), (f64, f64));

fn pack_bytes(segs: &[Seg]) -> Vec<u8> {
    let raw = rustline_gtfs::load(&workspace_path("data/fixtures/tiny")).unwrap();
    let (tt, _) = rustline_gtfs::build_timetable(&raw).unwrap();
    let mut data: TimetableData = rustline_gtfs::try_timetable_data(&tt).unwrap();
    let meta = rustline_gtfs::pack_meta(&tt, [0xAB; 32], "tiny", 0);
    let e7 = |d: f64| (d * 1e7).round() as i32;
    for (i, (a, b)) in segs.iter().enumerate() {
        let i = i as u32;
        data.street_lat.extend([e7(a.0), e7(b.0)]);
        data.street_lon.extend([e7(a.1), e7(b.1)]);
        data.seg_a.push(2 * i);
        data.seg_b.push(2 * i + 1);
        data.seg_len_cm
            .push((haversine_m(*a, *b) * 100.0).round().max(1.0) as u32);
        data.seg_flags.push(0);
    }
    data.shape_start = vec![0; segs.len() + 1];
    let mut out = Vec::new();
    write_pack(&data, &meta, &mut out).unwrap();
    out
}

/// (in-memory accepts, on-disk accepts).
fn both(name: &str, bytes: &[u8]) -> (bool, bool) {
    let path = std::env::temp_dir().join(format!(
        "rustline_m8_carried_{}_{name}.rlp",
        std::process::id()
    ));
    std::fs::write(&path, bytes).unwrap();
    let mem = Engine::from_bytes(bytes).is_ok();
    let disk = rustline_pack::verify_pack_file(&path, rustline_pack::DEFAULT_MAX_BODY_BYTES)
        .map_err(|e| format!("{e:?}"))
        .and_then(|f| rustline_core::check_installable(&f).map_err(|e| format!("{e:?}")))
        .is_ok();
    let _ = std::fs::remove_file(&path);
    println!("{name}: in-memory {mem}, on-disk {disk}");
    (mem, disk)
}

#[test]
fn grid_checks_agree_on_a_globe_spanning_graph() {
    let segs = vec![
        ((-85.0, -180.0), (-84.999, -179.999)),
        ((85.0, 180.0), (84.999, 179.999)),
        ((0.0, 0.0), (0.001, 0.001)),
        ((-85.0, 180.0), (-84.999, 179.999)),
        ((85.0, -180.0), (84.999, -179.999)),
    ];
    let (m, d) = both("globe", &pack_bytes(&segs));
    assert_eq!(
        m, d,
        "build_grid and check_disk_grid disagree on the globe graph"
    );
}

fn one_cell(n: usize) -> Vec<Seg> {
    // All segments inside one 200 m cell near (10, 20): tiny, distinct pieces.
    (0..n)
        .map(|i| {
            let (a, b) = ((i % 90) as f64 * 1e-5, (i / 90) as f64 * 1e-5);
            ((10.0 + a, 20.0 + b), (10.0 + a + 5e-6, 20.0 + b))
        })
        .collect()
}

#[test]
fn grid_checks_agree_just_under_and_over_the_cell_cap() {
    for n in [8191, 8192, 8193] {
        let (m, d) = both(&format!("cell{n}"), &pack_bytes(&one_cell(n)));
        assert_eq!(m, d, "{n} segments in one cell: in-memory {m}, on-disk {d}");
    }
}

#[test]
fn for_box_documents_the_latitude_budget() {
    let src =
        std::fs::read_to_string(workspace_path("crates/rustline-core/src/street.rs")).unwrap();
    let at = src.find("fn for_box").expect("Grid::for_box");
    let before = &src[src[..at].rfind("\n\n").unwrap_or(0)..at];
    let body_head: String = src[at..].lines().take(8).collect::<Vec<_>>().join("\n");
    let text = format!("{before}\n{body_head}").to_lowercase();
    assert!(
        text.contains("budget") && text.contains("latitude"),
        "Grid::for_box needs a comment that the snapping budget depends on the highest-latitude choice:\n{text}"
    );
}
