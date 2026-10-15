//! M4 CLI tests (Tester, written before M4): plan M4 steps 1 (`rustline bbox`), 5
//! (unlinked stops in the build report), 10 (`plan --from/--to/--at --geojson`), DoD
//! "foot graph stats printed by build-pack", "Tester will check: GeoJSON of 3 queries,
//! walking lines follow streets; build-pack with and without --osm", §11 hostile CLI
//! arguments and OSM files (no panic, peak RSS bounded).
//! The `cfg rustline_m4` gate and the "M4 not implemented" ignores were removed in M4.

mod m4_common;

use m4_common::*;

fn no_panic(o: &std::process::Output, what: &str) {
    let t = text(o);
    assert_ne!(o.status.code(), Some(101), "{what}: panicked: {t}");
    assert!(!t.contains("panicked"), "{what}: {t}");
}

/// The numbers in one line, split at commas, semicolons and whitespace.
fn floats(s: &str) -> Vec<f64> {
    s.split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .filter(|w| !w.is_empty())
        .filter_map(|w| w.parse().ok())
        .collect()
}

fn city_gtfs() -> (TempDir, std::path::PathBuf) {
    let d = TempDir::new("cli");
    let g = d.0.join("gtfs");
    std::fs::create_dir_all(&g).unwrap();
    line_city_gtfs(&g);
    (d, g)
}

// ============================ bbox ============================

#[test]
fn bbox_is_the_stop_extent_plus_the_margin() {
    // Stops span lat 1.000–1.035, lon 1.0–1.05 (U is inside). 2 km = 0.017 99°.
    // Order assumed as for `osmium extract -b`: min_lon,min_lat,max_lon,max_lat.
    let (_d, g) = city_gtfs();
    for (margin, m) in [("2", 2000.0), ("0", 0.0)] {
        let o = rustline(&["bbox", "--gtfs", g.to_str().unwrap(), "--margin-km", margin]);
        assert!(o.status.success(), "{}", text(&o));
        let out = String::from_utf8_lossy(&o.stdout).to_string();
        let f = out
            .lines()
            .map(floats)
            .find(|f| f.len() == 4)
            .unwrap_or_else(|| panic!("no bbox line: {out}"));
        let want = [
            1.0 - dlon(m, 1.0),
            1.0 - dlat(m),
            1.05 + dlon(m, 1.035),
            1.035 + dlat(m),
        ];
        for (g, w) in f.iter().zip(want) {
            assert!(
                (g - w).abs() <= 0.02 * dlat(m.max(100.0)),
                "{f:?} vs {want:?}"
            );
        }
    }
}

#[test]
fn bbox_hostile_arguments() {
    let (d, g) = city_gtfs();
    let gs = g.to_str().unwrap();
    for m in ["-1", "NaN", "inf", "1e300", "abc", ""] {
        let o = rustline(&["bbox", "--gtfs", gs, "--margin-km", m]);
        no_panic(&o, m);
        if o.status.success() {
            // Accepting it is only fine with a valid (clamped) bbox.
            let out = String::from_utf8_lossy(&o.stdout).to_string();
            let f = out
                .lines()
                .map(floats)
                .find(|f| f.len() == 4)
                .unwrap_or_default();
            assert!(f.len() == 4 && f[0] <= f[2] && f[1] <= f[3], "{m}: {f:?}");
            assert!(
                f.iter().all(|v| v.is_finite() && v.abs() <= 180.0),
                "{m}: {f:?}"
            );
        }
    }
    let empty = d.0.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let o = rustline(&[
        "bbox",
        "--gtfs",
        empty.to_str().unwrap(),
        "--margin-km",
        "2",
    ]);
    no_panic(&o, "empty feed");
    assert!(
        !o.status.success(),
        "bbox of a feed without stops: {}",
        text(&o)
    );
}

// ============================ build-pack --osm ============================

#[test]
fn build_pack_prints_foot_graph_stats_and_unlinked_stops() {
    let c = city_pack(true);
    let low = c.output.to_lowercase();
    for word in ["nodes", "edges", "component"] {
        assert!(low.contains(word), "no '{word}' in: {}", c.output);
    }
    // One unlinked stop (U, 150 m from the meridian).
    assert!(
        low.contains("unlinked") || low.contains("not linked"),
        "{}",
        c.output
    );
    assert!(
        c.output
            .split(|ch: char| !ch.is_alphanumeric())
            .any(|w| w == "U"),
        "{}",
        c.output
    );
}

#[test]
fn build_pack_without_osm_still_works() {
    let c = city_pack(false);
    assert!(c.pack.exists());
    assert!(!has_street_graph(&engine(&c.pack).info()));
}

#[test]
fn hostile_osm_files_fail_cleanly_with_bounded_memory() {
    let d = TempDir::new("cli_osm");
    let (_g, gtfs) = city_gtfs();
    let good = to_pbf(&line_city_osm(), PbfOptions::default());
    let header = frame(
        "OSMHeader",
        &blob(
            &header_block(&["OsmSchema-V0.6", "DenseNodes"]),
            Compression::Raw,
        ),
    );
    let (z, _) = zlib_zero_bomb(2_000_000);
    let mut zb = Msg::default();
    zb.uint(2, 1000).bytes(3, &z);
    let mut bomb = header.clone();
    bomb.extend(frame("OSMData", &zb.0));
    let mut huge = header.clone();
    huge.extend(frame_with("OSMData", 1 << 40, None, &[]));
    let mut rng = Rng(0x4D34_C11F);
    let random: Vec<u8> = (0..65536).map(|_| rng.next() as u8).collect();
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", vec![]),
        ("random", random),
        ("truncated", good[..good.len() / 2].to_vec()),
        ("header len u32::MAX", u32::MAX.to_be_bytes().to_vec()),
        ("datasize 2^40", huge),
        ("zlib bomb", bomb),
        (
            "gtfs as osm",
            std::fs::read(gtfs.join("stops.txt")).unwrap(),
        ),
    ];
    for (name, bytes) in cases {
        let p = d.file(&format!("{}.osm.pbf", name.replace(' ', "_")), &bytes);
        let out = d.0.join(format!("out_{}", name.replace(' ', "_")));
        let args = [
            "build-pack",
            "--gtfs",
            gtfs.to_str().unwrap(),
            "--osm",
            p.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--feed-id",
            "tester",
            "--built-at",
            "0",
        ];
        let (o, peak_kib, dt) = run_with_peak(&args);
        no_panic(&o, name);
        assert!(!o.status.success(), "{name}: accepted: {}", text(&o));
        assert!(
            text(&o).to_lowercase().contains("osm"),
            "{name}: unclear error: {}",
            text(&o)
        );
        let mib = peak_kib.unwrap_or(0) >> 10;
        println!("{name}: peak {mib} MiB, {dt:?}");
        assert!(mib < 512, "{name}: peak {mib} MiB");
        assert!(dt.as_secs() < 60, "{name}: {dt:?}");
    }
    let o = rustline(&[
        "build-pack",
        "--gtfs",
        gtfs.to_str().unwrap(),
        "--osm",
        "/nonexistent.osm.pbf",
        "--out",
        d.0.join("o").to_str().unwrap(),
    ]);
    no_panic(&o, "missing file");
    assert!(!o.status.success());
}

// ============================ plan ============================

fn plan_cli(
    pack: &std::path::Path,
    from: &str,
    to: &str,
    at: &str,
    extra: &[&str],
) -> std::process::Output {
    let mut a = vec![
        "plan",
        "--pack",
        pack.to_str().unwrap(),
        "--from",
        from,
        "--to",
        to,
        "--at",
        at,
    ];
    a.extend_from_slice(extra);
    rustline(&a)
}

#[test]
fn plan_from_coordinates_uc7() {
    // P0 → D at 08:05: walk only, arrival 08:11:24.9.
    let c = city_pack(true);
    let o = plan_cli(&c.pack, "1.0,1.0", "1.0045,1.0", "2030-01-02T08:05", &[]);
    assert!(o.status.success(), "{}", text(&o));
    let t = text(&o);
    assert!(t.contains("08:11"), "{t}");
    assert!(t.to_lowercase().contains("walk"), "{t}");
    // P0 → P3 at 08:05: ride L1_0810, arrival 08:17.
    let o = plan_cli(&c.pack, "1.0,1.0", "1.035,1.0", "2030-01-02T08:05", &[]);
    assert!(
        o.status.success() && text(&o).contains("08:17"),
        "{}",
        text(&o)
    );
}

/// Distance from p to the nearest street segment of the line city.
fn to_street(osm: &Osm, p: (f64, f64)) -> f64 {
    let mut best = f64::INFINITY;
    for w in &osm.ways {
        for s in w.refs.windows(2) {
            if let (Some(a), Some(b)) = (osm.coord(s[0]), osm.coord(s[1])) {
                best = best.min(point_segment_m(p, a, b));
            }
        }
    }
    best
}

#[test]
fn geojson_lines_follow_the_streets() {
    // Three queries whose every leg runs along streets (stops are on the meridian):
    // P0 → D (walk), 500 m south of P0 → P3 (walk + L1), P0 → spur end (corner at 1.005).
    let c = city_pack(true);
    let osm = line_city_osm();
    let se = city::spur_end();
    let queries = [
        (
            "1.0,1.0".to_string(),
            "1.0045,1.0".to_string(),
            "2030-01-02T08:05",
        ),
        (
            format!("{},{}", city::south_500().0, city::south_500().1),
            "1.035,1.0".to_string(),
            "2030-01-02T08:00",
        ),
        (
            "1.0,1.0".to_string(),
            format!("{},{}", se.0, se.1),
            "2030-01-02T08:00",
        ),
    ];
    for (i, (from, to, at)) in queries.iter().enumerate() {
        let gj = c.dir.0.join(format!("q{i}.geojson"));
        let o = plan_cli(&c.pack, from, to, at, &["--geojson", gj.to_str().unwrap()]);
        assert!(o.status.success(), "{}", text(&o));
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&gj).unwrap()).unwrap();
        assert_eq!(v["type"], "FeatureCollection");
        let mut lines = 0;
        for f in v["features"].as_array().unwrap() {
            if f["geometry"]["type"] != "LineString" {
                continue;
            }
            lines += 1;
            let pts: Vec<(f64, f64)> = f["geometry"]["coordinates"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| (c[1].as_f64().unwrap(), c[0].as_f64().unwrap()))
                .collect();
            assert!(pts.len() >= 2, "{f}");
            // Vertices and points every ~10 m along each segment stay within 2 m of a street.
            for w in pts.windows(2) {
                let n = (haversine_m(w[0], w[1]) / 10.0).ceil().max(1.0) as usize;
                for k in 0..=n {
                    let t = k as f64 / n as f64;
                    let p = (
                        w[0].0 + (w[1].0 - w[0].0) * t,
                        w[0].1 + (w[1].1 - w[0].1) * t,
                    );
                    let d = to_street(&osm, p);
                    assert!(d <= 2.0, "query {i}: {p:?} is {d:.1} m off the streets");
                }
            }
        }
        assert!(lines >= 1, "query {i}: no LineString in {v}");
    }
}

#[test]
fn plan_hostile_arguments() {
    let c = city_pack(true);
    let p = &c.pack;
    let good = ("1.0,1.0", "1.035,1.0", "2030-01-02T08:05");
    let mut cases: Vec<(String, String, String)> = Vec::new();
    for bad in [
        "", "abc", "1.0", "1,2,3", "NaN,1", "1,inf", "91,0", "0,181", "1e400,0", ",", "1.0;1.0",
        "  ",
    ] {
        cases.push((bad.into(), good.1.into(), good.2.into()));
        cases.push((good.0.into(), bad.into(), good.2.into()));
    }
    for bad in [
        "",
        "garbage",
        "2030-13-01T08:00",
        "2030-01-02T25:00",
        "99999-01-01T00:00",
        "2030-01-02",
    ] {
        cases.push((good.0.into(), good.1.into(), bad.into()));
    }
    for (f, t, a) in &cases {
        let o = plan_cli(p, f, t, a, &[]);
        no_panic(&o, &format!("{f}|{t}|{a}"));
        assert!(
            !o.status.success() || a == "2030-01-02",
            "{f}|{t}|{a} accepted: {}",
            text(&o)
        );
    }
    // Outside coverage: clear message, not success.
    let o = plan_cli(p, "1.0,1.02", "1.035,1.0", good.2, &[]);
    no_panic(&o, "outside");
    assert!(
        !o.status.success() && text(&o).to_lowercase().contains("coverage"),
        "{}",
        text(&o)
    );
    // --geojson to an unwritable path.
    let o = plan_cli(
        p,
        good.0,
        good.1,
        good.2,
        &["--geojson", "/nonexistent-dir/x.geojson"],
    );
    no_panic(&o, "geojson path");
    assert!(!o.status.success());
}
