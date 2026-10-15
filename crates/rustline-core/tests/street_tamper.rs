//! Builder (M4 fix round, Reviewer R28 MF1): a pack whose street geometry was
//! tampered with (CRC still valid) must be rejected or load in bounded memory.
//! `check()` bounds segment pieces and lengths; the snapping grid caps its entries.

use std::path::{Path, PathBuf};
use std::time::Instant;

use rustline_core::Engine;
use rustline_pack::{write_pack, Pack, PackError, TimetableData};

fn workspace_path(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

/// The tiny fixture plus a street graph: a ladder of `n` segments, each one
/// a straight piece from (lat, lon) to (lat + dlat, lon + dlon), degrees.
fn with_streets(n: u32, dlat: f64, dlon: f64) -> (rustline_pack::PackMeta, TimetableData) {
    let raw = rustline_gtfs::load(&workspace_path("data/fixtures/tiny")).unwrap();
    let (tt, _) = rustline_gtfs::build_timetable(&raw).unwrap();
    let mut data = rustline_gtfs::try_timetable_data(&tt).unwrap();
    let meta = rustline_gtfs::pack_meta(&tt, [0xAB; 32], "tiny", 0);
    let e7 = |d: f64| (d * 1e7).round() as i32;
    for i in 0..n {
        let (lat, lon) = (10.0 + 0.001 * f64::from(i), 20.0);
        let (a, b) = (
            rustline_model::LatLon::new(lat, lon),
            rustline_model::LatLon::new(lat + dlat, lon + dlon),
        );
        data.street_lat.extend([e7(a.lat), e7(b.lat)]);
        data.street_lon.extend([e7(a.lon), e7(b.lon)]);
        data.seg_a.push(2 * i);
        data.seg_b.push(2 * i + 1);
        data.seg_len_cm
            .push((haversine_m(a, b) * 100.0).round() as u32);
        data.seg_flags.push(0);
    }
    data.shape_start = vec![0; n as usize + 1];
    (meta, data)
}

fn haversine_m(a: rustline_model::LatLon, b: rustline_model::LatLon) -> f64 {
    let (la1, la2) = (a.lat.to_radians(), b.lat.to_radians());
    let h = ((la2 - la1) / 2.0).sin().powi(2)
        + la1.cos() * la2.cos() * ((b.lon - a.lon).to_radians() / 2.0).sin().powi(2);
    2.0 * 6_371_008.8 * h.sqrt().min(1.0).asin()
}

/// Moves every street node and shape point into a ~10 m square at (lat, lon)
/// and recomputes `seg_len_cm` so `check()` still passes; with `stops`, every
/// stop goes there too. Each stop's snap then sees every segment.
fn pile_up(data: &mut TimetableData, lat: f64, lon: f64, stops: bool) {
    let spot = |i: usize| {
        let (a, b) = ((i % 100) as f64, ((i / 100) % 100) as f64);
        (
            ((lat + a * 1e-6) * 1e7) as i32,
            ((lon + b * 1e-6) * 1e7) as i32,
        )
    };
    for i in 0..data.street_lat.len() {
        (data.street_lat[i], data.street_lon[i]) = spot(i);
    }
    for i in 0..data.shape_lat.len() {
        (data.shape_lat[i], data.shape_lon[i]) = spot(i + 7);
    }
    let p =
        |(a, o): (i32, i32)| rustline_model::LatLon::new(f64::from(a) * 1e-7, f64::from(o) * 1e-7);
    for g in 0..data.seg_a.len() {
        let mut pts = vec![(
            data.street_lat[data.seg_a[g] as usize],
            data.street_lon[data.seg_a[g] as usize],
        )];
        for i in data.shape_start[g] as usize..data.shape_start[g + 1] as usize {
            pts.push((data.shape_lat[i], data.shape_lon[i]));
        }
        pts.push((
            data.street_lat[data.seg_b[g] as usize],
            data.street_lon[data.seg_b[g] as usize],
        ));
        let len: f64 = pts.windows(2).map(|w| haversine_m(p(w[0]), p(w[1]))).sum();
        data.seg_len_cm[g] = (len * 100.0).round() as u32;
    }
    if stops {
        for i in 0..data.stop_lat.len() {
            if data.stop_lat[i] != rustline_pack::NO_COORD {
                data.stop_lat[i] = lat.to_bits();
                data.stop_lon[i] = lon.to_bits();
            }
        }
    }
}

fn corrupt(r: Result<Pack, PackError>) -> String {
    match r {
        Err(PackError::Corrupt(m)) => m,
        Err(e) => panic!("expected Corrupt, got {e}"),
        Ok(_) => panic!("expected Corrupt, got a pack"),
    }
}

#[test]
fn an_honest_street_graph_loads() {
    let (meta, data) = with_streets(20, 0.0005, 0.0005);
    let e = Engine::new(Pack::new(meta, data).unwrap()).unwrap();
    assert!(e.info().street_graph);
}

#[test]
fn nodes_moved_to_opposite_corners_of_the_globe_are_rejected() {
    // Reviewer R28 repro: nodes alternating between (-89, -179) and (89, 179)
    // (refused for the latitude), and the same at ±84° (refused for the length).
    let (meta, data0) = with_streets(20, 0.0005, 0.0005);
    let moved = |deg: f64| {
        let mut data = data0.clone();
        for k in 0..4 {
            let (lat, lon) = if k % 2 == 0 {
                (-deg, -179.0)
            } else {
                (deg, 179.0)
            };
            data.street_lat[k] = (lat * 1e7) as i32;
            data.street_lon[k] = (lon * 1e7) as i32;
        }
        data
    };
    let m = corrupt(Pack::new(meta.clone(), moved(89.0)));
    assert!(m.contains("out of range"), "{m}");
    let data = moved(84.0);
    let m = corrupt(Pack::new(meta.clone(), data.clone()));
    assert!(m.contains("longer than"), "{m}");
    // Also through a file with a valid CRC (write_pack does not check).
    let mut bytes = Vec::new();
    write_pack(&data, &meta, &mut bytes).unwrap();
    match Engine::from_bytes(&bytes) {
        Err(PackError::Corrupt(m)) => assert!(m.contains("longer than"), "{m}"),
        Err(e) => panic!("{e}"),
        Ok(_) => panic!("expected Corrupt"),
    }
}

#[test]
fn a_piece_just_under_10_km_is_kept_and_just_over_is_rejected() {
    // 0.089° of latitude ≈ 9.9 km; 0.091° ≈ 10.1 km.
    let (meta, data) = with_streets(1, 0.089, 0.0);
    assert!(Pack::new(meta, data).is_ok());
    let (meta, data) = with_streets(1, 0.091, 0.0);
    let m = corrupt(Pack::new(meta, data));
    assert!(m.contains("longer than"), "{m}");
}

#[test]
fn seg_len_must_match_the_geometry() {
    // A 70 m piece claiming 1 cm, or 10 km: both are rejected; ±0.5 % is fine.
    for (scale, ok) in [
        (1.0, true),
        (0.995, true),
        (1.005, true),
        (0.0, false),
        (150.0, false),
    ] {
        let (meta, mut data) = with_streets(3, 0.0005, 0.0005);
        let len = data.seg_len_cm[1];
        data.seg_len_cm[1] = (f64::from(len) * scale) as u32;
        let r = Pack::new(meta, data);
        if ok {
            assert!(r.is_ok(), "scale {scale}: {:?}", r.err());
        } else {
            let m = corrupt(r);
            assert!(m.contains("seg_len_cm"), "{m}");
        }
    }
}

#[test]
fn streets_beyond_85_degrees_are_rejected() {
    let (meta, mut data) = with_streets(2, 0.0005, 0.0005);
    data.street_lat[0] = 860_000_000;
    data.street_lat[1] = 860_050_000;
    let m = corrupt(Pack::new(meta, data));
    assert!(m.contains("85"), "{m}");
}

#[test]
fn the_grid_caps_its_entries() {
    // Every piece is a 9.9 km diagonal (passes check()), so each crosses ~70
    // grid cells: over the cap of 32 per piece. Loading fails, bounded.
    let (meta, data) = with_streets(500, 0.063, 0.064);
    let pack = Pack::new(meta, data).expect("each piece is under 10 km");
    let t = Instant::now();
    match Engine::new(pack) {
        Err(PackError::Corrupt(m)) => assert!(m.contains("grid"), "{m}"),
        Err(e) => panic!("{e}"),
        Ok(_) => panic!("expected the grid cap"),
    }
    assert!(t.elapsed().as_secs() < 5);
}

#[test]
fn all_streets_in_one_spot_is_rejected_quickly() {
    // Every street and stop in one ~10 m spot: before the cell cap each stop's
    // snap scanned every segment (Brașov 1.7 s, București 14.4 s at load).
    let (meta, mut data) = with_streets(20_000, 0.0005, 0.0005);
    pile_up(&mut data, 10.0, 20.0, true);
    let pack = Pack::new(meta, data).expect("short pieces, matching lengths");
    let t = Instant::now();
    match Engine::new(pack) {
        Err(PackError::Corrupt(m)) => assert!(m.contains("cell"), "{m}"),
        Err(e) => panic!("{e}"),
        Ok(_) => panic!("expected the cell cap"),
    }
    let dt = t.elapsed();
    println!("all streets in one spot: rejected in {dt:?}");
    assert!(dt.as_secs() < 2, "{dt:?}");
}

#[test]
fn many_streets_near_one_spot_but_under_the_cell_cap_load_quickly() {
    // 4 000 segments in one spot (half the cell cap) with every stop there:
    // each snap is bounded by the cell cap and the scan cap.
    let (meta, mut data) = with_streets(4_000, 0.0005, 0.0005);
    pile_up(&mut data, 10.0, 20.0, true);
    let t = Instant::now();
    let e = Engine::new(Pack::new(meta, data).unwrap()).expect("under the cap");
    let dt = t.elapsed();
    println!("4 000 segments in one spot: loaded in {dt:?}");
    assert!(e.info().street_graph);
    assert!(dt.as_secs() < 2, "{dt:?}");
}

/// Reviewer MF3: 5 × 5 grid cells (200 m) of 8 192 short streets each (the cell
/// cap) around `p`, every stop at `p`, valid CRC. The only street through `p` is
/// in the middle cell; it is scanned after 12 full cells, so a scan stopped at a
/// small budget answered a far street. Returns the pack bytes and `p`.
fn full_window_pack() -> (Vec<u8>, rustline_model::LatLon) {
    let (meta, mut data) = with_streets(0, 0.0, 0.0);
    let dlat = (200.0f64 / 6_371_008.8).to_degrees();
    let (lat0, lon0) = (45.0, 25.0);
    let hi = lat0 + 6.5 * dlat;
    let dlon = dlat / hi.to_radians().cos();
    let at = |y: f64, x: f64| rustline_model::LatLon::new(lat0 + y * dlat, lon0 + x * dlon);
    let street =
        |data: &mut TimetableData, a: rustline_model::LatLon, b: rustline_model::LatLon, n: u32| {
            let i = data.street_lat.len() as u32;
            for q in [a, b] {
                data.street_lat.push((q.lat * 1e7).round() as i32);
                data.street_lon.push((q.lon * 1e7).round() as i32);
            }
            for _ in 0..n {
                data.seg_a.push(i);
                data.seg_b.push(i + 1);
                data.seg_len_cm
                    .push((haversine_m(a, b) * 100.0).round() as u32);
                data.seg_flags.push(0);
            }
        };
    // Anchors that fix the grid to 7 × 7 cells, outside the 5 × 5 window.
    street(&mut data, at(0.0, 0.0), at(0.01, 0.0), 1);
    street(&mut data, at(6.5, 6.5), at(6.49, 6.5), 1);
    for y in 1..=5 {
        for x in 1..=5 {
            let (y, x) = (f64::from(y), f64::from(x));
            if (y, x) == (3.0, 3.0) {
                street(&mut data, at(3.2, 3.2), at(3.21, 3.2), 8_191);
                street(&mut data, at(3.49, 3.5), at(3.51, 3.5), 1);
            } else {
                street(
                    &mut data,
                    at(y + 0.5, x + 0.5),
                    at(y + 0.51, x + 0.5),
                    8_192,
                );
            }
        }
    }
    data.shape_start = vec![0; data.seg_a.len() + 1];
    let p = at(3.5, 3.5);
    for i in 0..data.stop_lat.len() {
        data.stop_lat[i] = p.lat.to_bits();
        data.stop_lon[i] = p.lon.to_bits();
    }
    let mut bytes = Vec::new();
    write_pack(&data, &meta, &mut bytes).expect("valid pack");
    (bytes, p)
}

#[test]
fn a_full_5x5_window_of_capped_cells_loads_and_snaps_exactly() {
    let (bytes, p) = full_window_pack();
    let t = Instant::now();
    let e = Engine::from_bytes(&bytes).expect("every stop snap fits the scan budget");
    let load = t.elapsed();
    assert!(e.info().street_graph);
    // From p to a point 1 m north on the same street: both snap at ~0 m, so the
    // walk is ~1 m (before the fix the partial scan gave a 397 m walk).
    let meta = rustline_pack::read_pack(&bytes).unwrap().into_parts().0;
    let req = rustline_core::PlanRequest {
        from: p,
        to: rustline_model::LatLon::new(p.lat + (1.0f64 / 6_371_008.8).to_degrees(), p.lon),
        time: rustline_core::TimeSpec::DepartAt(meta.valid_from.and_hms_opt(8, 0, 0).unwrap()),
        ..Default::default()
    };
    let t = Instant::now();
    let r = e.plan(&req).expect("plan");
    let query = t.elapsed();
    let walk = r
        .itineraries
        .iter()
        .map(|i| i.walk_m)
        .min()
        .expect("an itinerary");
    println!("5 x 5 x 8 192 window: loaded in {load:?}, query in {query:?}, walk {walk} m");
    assert!(walk <= 5, "walk {walk} m");
    assert!(
        load.as_secs() < 5 && query.as_secs() < 2,
        "{load:?} {query:?}"
    );
}

/// Reviewer R28 repro on a real pack, for measuring: moves `RUSTLINE_TAMPER_K`
/// street nodes of `RUSTLINE_TAMPER_PACK` to alternate corners of the globe,
/// writes the pack (fixing nothing by hand: write_pack computes the CRC) and
/// loads it. Prints the load time and VmHWM. Run one k per process:
/// `RUSTLINE_TAMPER_PACK=out/x.rlp RUSTLINE_TAMPER_K=200 cargo test --locked --release -p rustline-core --test street_tamper -- --ignored --nocapture`
#[test]
#[ignore = "measurement: needs RUSTLINE_TAMPER_PACK (a pack built with --osm)"]
fn tampered_real_pack_peak_memory() {
    let Ok(path) = std::env::var("RUSTLINE_TAMPER_PACK") else {
        eprintln!(
            "skip: set RUSTLINE_TAMPER_PACK to a pack built with --osm (see the doc comment)"
        );
        return;
    };
    let k: usize = std::env::var("RUSTLINE_TAMPER_K").map_or(200, |v| v.parse().unwrap());
    let pack = rustline_pack::read_pack(&std::fs::read(path).unwrap()).unwrap();
    let (meta, mut data) = pack.into_parts();
    // RUSTLINE_TAMPER_LAT: 89 (the review) or e.g. 84 (inside the ±85° bound).
    let deg: f64 = std::env::var("RUSTLINE_TAMPER_LAT").map_or(89.0, |v| v.parse().unwrap());
    for i in 0..k.min(data.street_lat.len()) {
        let (lat, lon) = if i % 2 == 0 {
            (-deg, -179.0)
        } else {
            (deg, 179.0)
        };
        data.street_lat[i] = (lat * 1e7) as i32;
        data.street_lon[i] = (lon * 1e7) as i32;
    }
    // RUSTLINE_TAMPER_MODE=spot: every street (and stop) in one ~10 m spot.
    let spot = std::env::var("RUSTLINE_TAMPER_MODE").is_ok_and(|m| m == "spot");
    if spot {
        let (lat, lon) = data.stop_pos(0).unwrap_or((45.0, 25.0));
        pile_up(&mut data, lat, lon, true);
    }
    let mut bytes = Vec::new();
    let written = write_pack(&data, &meta, &mut bytes);
    let t = Instant::now();
    let r = written.map_err(|e| e.to_string()).and_then(|()| {
        Engine::from_bytes(&bytes)
            .map(|e| e.info().street_segments)
            .map_err(|e| e.to_string())
    });
    let hwm = std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM:"))
                .map(str::to_string)
        });
    let what = if spot {
        "all in one spot".to_string()
    } else {
        format!("k={k} lat ±{deg}")
    };
    println!("{what}: {:?} in {:?}, {hwm:?}", r, t.elapsed());
}
