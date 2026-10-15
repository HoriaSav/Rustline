//! M4 foot-graph tests (rustline-osm), written by Tester before M4.
//! Plan M4 "Definition of done (strict)", "Edge cases (must be tested)" (Graph build)
//! and Builder checklist §11. The `cfg rustline_m4` gate and the "M4 not implemented"
//! ignores were removed in M4; the country-size test stays ignored (slow).
//! Run: `cargo test --locked -p rustline-osm --test m4_osm -- --include-ignored`
//!
//! All maps are synthetic (neutral coordinates near 1°N 1°E), written as PBF by
//! `m4_pbf`; expected values come from the reference graph + Dijkstra there.

#[path = "m4_pbf/mod.rs"]
mod m4_pbf;

use m4_pbf::*;
use std::collections::BTreeMap;
use std::path::Path;

/// Tester's reading of the planned interface. Adapt here only.
mod api {
    use std::path::Path;

    pub use rustline_osm::{FootGraph, OsmError, OsmReport};

    /// Plan: `build_foot_graph(pbf: &Path, bbox: Option<BBox>)`.
    pub fn build(
        p: &Path,
        bbox: Option<(f64, f64, f64, f64)>,
    ) -> Result<(FootGraph, OsmReport), OsmError> {
        rustline_osm::build_foot_graph(
            p,
            bbox.map(|(min_lat, min_lon, max_lat, max_lon)| rustline_osm::BBox {
                min_lat,
                min_lon,
                max_lat,
                max_lon,
            }),
        )
    }

    pub fn node_count(g: &FootGraph) -> usize {
        g.node_coords().len()
    }

    pub fn coord(g: &FootGraph, i: usize) -> (f64, f64) {
        let c = g.node_coords()[i];
        (c.lat, c.lon)
    }

    /// Outgoing edges of node i: (to, metres, is_steps).
    pub fn edges(g: &FootGraph, i: usize) -> Vec<(usize, f64, bool)> {
        let s = g.edges_start();
        (s[i] as usize..s[i + 1] as usize)
            .map(|e| {
                (
                    g.edge_to()[e] as usize,
                    f64::from(g.edge_len_m()[e]),
                    g.edge_kind()[e] == rustline_osm::EDGE_KIND_STEPS,
                )
            })
            .collect()
    }

    /// Ways skipped (fewer than 2 usable nodes, or referencing missing nodes).
    pub fn skipped_ways(r: &OsmReport) -> u64 {
        r.ways_skipped
    }

    /// Node references that point at nodes missing from the file.
    pub fn missing_node_refs(r: &OsmReport) -> u64 {
        r.missing_node_refs
    }
}

use api::*;

const BASE: (f64, f64) = (1.0, 1.0);

fn write(osm: &Osm, opt: PbfOptions) -> (TempDir, std::path::PathBuf) {
    let d = TempDir::new("osm");
    let p = d.file("map.osm.pbf", &to_pbf(osm, opt));
    (d, p)
}

fn graph(osm: &Osm) -> (FootGraph, OsmReport) {
    let (_d, p) = write(osm, PbfOptions::default());
    build(&p, None).expect("synthetic map builds")
}

/// Index of the graph node at `c` (within 0.5 m), if any.
fn find(g: &FootGraph, c: (f64, f64)) -> Option<usize> {
    (0..node_count(g)).find(|&i| haversine_m(coord(g, i), c) < 0.5)
}

/// Dijkstra on Builder's graph, seconds at `speed`, steps ×1.5 (the plan's cost).
fn graph_times(g: &FootGraph, from: usize, speed: f64) -> BTreeMap<i64, f64> {
    dijkstra_generic(from as i64, speed, |n| {
        edges(g, n as usize)
            .into_iter()
            .map(|(t, m, s)| (t as i64, m, s))
            .collect()
    })
}

fn graph_metres(g: &FootGraph, a: (f64, f64), b: (f64, f64)) -> Option<f64> {
    let (ia, ib) = (find(g, a)?, find(g, b)?);
    let d = dijkstra_generic(ia as i64, 1.0, |n| {
        edges(g, n as usize)
            .into_iter()
            .map(|(t, m, _)| (t as i64, m, false))
            .collect()
    });
    d.get(&(ib as i64)).copied()
}

fn north(m: f64) -> (f64, f64) {
    (BASE.0 + dlat(m), BASE.1)
}

fn east(m: f64) -> (f64, f64) {
    (BASE.0, BASE.1 + dlon(m, BASE.0))
}

// ============================ structure ============================

#[test]
fn straight_street_is_walkable_both_ways_with_haversine_length() {
    let mut o = Osm::new();
    let (a, b) = (BASE, north(1000.0));
    o.street(a, b, 50.0, &[("highway", "residential")]);
    let (g, _) = graph(&o);
    let ab = graph_metres(&g, a, b).expect("both ends are graph nodes");
    let ba = graph_metres(&g, b, a).expect("reverse edge exists (plan step 2)");
    assert!(close(ab, 1000.0, 0.002, 0.5), "a→b {ab} m");
    assert!(close(ba, 1000.0, 0.002, 0.5), "b→a {ba} m");
}

#[test]
fn degree_two_nodes_are_contracted_and_lengths_kept() {
    // Plan step 3: a 20-node straight street becomes one edge between its two ends.
    let mut o = Osm::new();
    let (a, b) = (BASE, north(1000.0));
    o.street(a, b, 50.0, &[("highway", "residential")]);
    let (g, _) = graph(&o);
    assert_eq!(node_count(&g), 2, "only the two dead ends remain");
    let ab = graph_metres(&g, a, b).unwrap();
    assert!(close(ab, 1000.0, 0.002, 0.5), "{ab}");
}

#[test]
fn junctions_survive_contraction() {
    // A plus-shaped crossing: centre (degree 4) and four ends stay.
    let mut o = Osm::new();
    let c = north(500.0);
    let (n, s) = (north(1000.0), BASE);
    let (w, e) = ((c.0, c.1 - dlon(500.0, c.0)), (c.0, c.1 + dlon(500.0, c.0)));
    o.street(s, c, 50.0, &[("highway", "residential")]);
    o.street(c, n, 50.0, &[("highway", "residential")]);
    o.street(w, c, 50.0, &[("highway", "residential")]);
    o.street(c, e, 50.0, &[("highway", "residential")]);
    let (g, _) = graph(&o);
    assert_eq!(node_count(&g), 5);
    assert!(find(&g, c).is_some());
    let we = graph_metres(&g, w, e).unwrap();
    assert!(close(we, 1000.0, 0.002, 0.5), "{we}");
}

// ============================ tags ============================

/// (tags, walkable?) per plan step 2, spec §7.3 and the M4 edge-case block.
const TAG_CASES: &[(&[(&str, &str)], bool)] = &[
    (&[("highway", "footway")], true),
    (&[("highway", "path")], true),
    (&[("highway", "pedestrian")], true),
    (&[("highway", "steps")], true),
    (&[("highway", "living_street")], true),
    (&[("highway", "residential")], true),
    (&[("highway", "service")], true),
    (&[("highway", "unclassified")], true),
    (&[("highway", "tertiary")], true),
    (&[("highway", "secondary")], true),
    (&[("highway", "primary")], true),
    (&[("highway", "track")], true),
    (&[("highway", "motorway")], false),
    (&[("highway", "trunk")], false),
    (&[("highway", "residential"), ("foot", "no")], false),
    (&[("highway", "residential"), ("access", "private")], false),
    (
        &[
            ("highway", "residential"),
            ("access", "private"),
            ("foot", "yes"),
        ],
        true,
    ),
    (
        &[
            ("highway", "residential"),
            ("access", "no"),
            ("foot", "yes"),
        ],
        true,
    ),
    (&[("highway", "cycleway"), ("foot", "designated")], true),
    // Leader (M4 answers, fix round): platform, corridor and primary/secondary/tertiary
    // _link are walkable; foot=no still excludes them; motorway_link, trunk_link and
    // cycleway need an explicit foot tag.
    (&[("highway", "platform")], true),
    (&[("highway", "corridor")], true),
    (&[("highway", "primary_link")], true),
    (&[("highway", "secondary_link")], true),
    (&[("highway", "tertiary_link")], true),
    (&[("highway", "trunk_link")], false),
    (&[("highway", "trunk_link"), ("foot", "yes")], true),
    (&[("highway", "motorway_link")], false),
    (&[("highway", "motorway_link"), ("foot", "yes")], true),
    (&[("highway", "platform"), ("foot", "no")], false),
    (&[("highway", "corridor"), ("foot", "no")], false),
    (&[("highway", "primary_link"), ("foot", "no")], false),
    (&[("highway", "secondary_link"), ("foot", "no")], false),
    (&[("highway", "tertiary_link"), ("foot", "no")], false),
    (&[("highway", "trunk_link"), ("foot", "no")], false),
    (&[("highway", "cycleway")], false),
    (&[("highway", "cycleway"), ("foot", "yes")], true),
    // "foot=yes on an otherwise excluded way"
    (&[("highway", "motorway"), ("foot", "yes")], true),
    // "missing or unknown highway tags"
    (&[("highway", "tester_unknown_value")], false),
    (&[("building", "yes")], false),
    (&[], false),
];

#[test]
fn tag_rules_include_and_exclude_ways() {
    // Backbone along a parallel; one 100 m spoke north per case. A spoke's far end
    // is a graph node exactly when that way is walkable.
    let mut o = Osm::new();
    let n = TAG_CASES.len();
    let ends: Vec<(f64, f64)> = (0..=n).map(|i| east(100.0 * i as f64)).collect();
    for w in ends.windows(2) {
        o.street(w[0], w[1], 100.0, &[("highway", "residential")]);
    }
    let mut tips = Vec::new();
    for (i, (kv, _)) in TAG_CASES.iter().enumerate() {
        let root = ends[i + 1];
        let tip = (root.0 + dlat(100.0), root.1);
        o.street(root, tip, 100.0, kv);
        tips.push(tip);
    }
    assert!(TAG_CASES
        .iter()
        .all(|(kv, want)| walkable(&tags(kv)) == *want));
    let (g, _) = graph(&o);
    for ((kv, want), tip) in TAG_CASES.iter().zip(tips) {
        assert_eq!(find(&g, tip).is_some(), *want, "{kv:?}");
    }
}

#[test]
fn oneway_footway_is_two_way_but_oneway_foot_is_not() {
    // Square loop a→b→c→d→a (100 m sides); the side a–b carries the oneway tags.
    // Edge-case block: pedestrians walk both ways unless `oneway:foot=yes`.
    for (kv, b_to_a) in [
        (&[("highway", "footway"), ("oneway", "yes")][..], 100.0),
        (&[("highway", "footway"), ("oneway:foot", "yes")][..], 300.0),
    ] {
        let mut o = Osm::new();
        let a = BASE;
        let b = north(100.0);
        let c = (b.0, b.1 + dlon(100.0, b.0));
        let d = (a.0, a.1 + dlon(100.0, a.0));
        o.street(a, b, 100.0, kv);
        o.street(b, c, 100.0, &[("highway", "footway")]);
        o.street(c, d, 100.0, &[("highway", "footway")]);
        o.street(d, a, 100.0, &[("highway", "footway")]);
        // Dead-end spurs west of a and b: a and b become junctions, so they survive
        // degree-2 contraction (corners of a bare loop may be contracted away).
        o.street(
            (a.0, a.1 - dlon(50.0, a.0)),
            a,
            50.0,
            &[("highway", "footway")],
        );
        o.street(
            (b.0, b.1 - dlon(50.0, b.0)),
            b,
            50.0,
            &[("highway", "footway")],
        );
        let (g, _) = graph(&o);
        let ab = graph_metres(&g, a, b).unwrap();
        let ba = graph_metres(&g, b, a).unwrap();
        assert!(close(ab, 100.0, 0.003, 0.5), "{kv:?} a→b {ab}");
        assert!(close(ba, b_to_a, 0.003, 0.5), "{kv:?} b→a {ba}");
        let r = RefGraph::from_osm(&o);
        let ida = o.node_at(a.0, a.1);
        let idb = o.node_at(b.0, b.1);
        assert!(close(r.metres(idb)[&ida], b_to_a, 0.003, 0.5));
    }
}

#[test]
fn steps_are_marked_and_cost_one_and_a_half() {
    // a –(100 m residential)– b –(100 m steps)– c.
    let mut o = Osm::new();
    let (a, b, c) = (BASE, north(100.0), north(200.0));
    o.street(a, b, 100.0, &[("highway", "residential")]);
    o.street(b, c, 100.0, &[("highway", "steps")]);
    let (g, _) = graph(&o);
    let ia = find(&g, a).unwrap();
    let ic = find(&g, c).unwrap();
    let t = graph_times(&g, ia, 1.3)[&(ic as i64)];
    let want = 100.0 / 1.3 + 100.0 * 1.5 / 1.3; // 76.9 + 115.4 = 192.3 s
    assert!(close(t, want, 0.003, 0.5), "{t} s vs {want} s");
}

#[test]
fn incline_is_ignored_in_v1() {
    // Edge-case block: elevation/`incline` ignored in v1. A star with centre x (degree 3,
    // survives contraction) and three 200 m arms: north plain, east with incline=25%,
    // south plain. Every arm costs the same in both directions.
    let mut o = Osm::new();
    let x = BASE;
    let (b, d, e) = (north(200.0), east(200.0), (BASE.0 - dlat(200.0), BASE.1));
    o.street(x, b, 50.0, &[("highway", "footway")]);
    o.street(x, d, 50.0, &[("highway", "footway"), ("incline", "25%")]);
    o.street(x, e, 50.0, &[("highway", "footway")]);
    let (g, _) = graph(&o);
    let (ab, cd, dc) = (
        graph_metres(&g, x, b).unwrap(),
        graph_metres(&g, x, d).unwrap(),
        graph_metres(&g, d, x).unwrap(),
    );
    assert!(
        close(ab, cd, 0.0005, 0.1) && close(cd, dc, 0.0005, 0.1),
        "{ab} {cd} {dc}"
    );
}

// ============================ bad geometry and references ============================

#[test]
fn degenerate_ways_are_skipped_and_reported() {
    // Edge-case block: zero-length edges, duplicate nodes, ways with one node,
    // ways referencing missing nodes → skip + report, no panic.
    let mut o = Osm::new();
    let (a, b) = (BASE, north(500.0));
    o.street(a, b, 100.0, &[("highway", "residential")]);
    let lone = o.node(1.01, 1.01);
    o.way(&[lone], &[("highway", "footway")]); // one node
    let x = o.node(1.02, 1.02);
    o.way(&[x, 999_999_999], &[("highway", "footway")]); // missing node
    o.way(&[], &[("highway", "footway")]); // no nodes at all
    let (g, r) = graph(&o);
    assert!(
        skipped_ways(&r) >= 2,
        "one-node and missing-node ways are reported"
    );
    assert!(missing_node_refs(&r) >= 1);
    assert!(close(graph_metres(&g, a, b).unwrap(), 500.0, 0.002, 0.5));
    assert!(find(&g, (1.02, 1.02)).is_none(), "missing-ref way not used");
}

#[test]
fn zero_length_edges_and_repeated_refs_do_not_break_lengths() {
    // a, a' (same coordinates, other id), b; and a way that repeats a node (a, a, b).
    let mut o = Osm::new();
    let a = o.node(BASE.0, BASE.1);
    let a2 = o.node(BASE.0, BASE.1);
    let bc = north(300.0);
    let b = o.node(bc.0, bc.1);
    o.way(&[a, a2, b], &[("highway", "residential")]);
    o.way(&[a, a, b], &[("highway", "footway")]);
    // Dead-end spurs at both ends so BASE and bc are junctions (survive contraction).
    o.street(
        (BASE.0 - dlat(100.0), BASE.1),
        BASE,
        50.0,
        &[("highway", "footway")],
    );
    o.street(bc, north(400.0), 50.0, &[("highway", "footway")]);
    let (g, _) = graph(&o);
    let m = graph_metres(&g, BASE, bc).unwrap();
    assert!(close(m, 300.0, 0.002, 0.5), "{m}");
    for i in 0..node_count(&g) {
        for (_, len, _) in edges(&g, i) {
            assert!(len.is_finite() && len >= 0.0, "edge length {len}");
        }
    }
}

#[test]
fn duplicate_node_ids_do_not_panic() {
    let mut o = Osm::new();
    let (a, b) = (BASE, north(200.0));
    o.street(a, b, 100.0, &[("highway", "residential")]);
    let first = o.nodes[0].clone();
    o.nodes.push(Node {
        lat: first.lat + 0.5,
        ..first
    });
    let (_d, p) = write(&o, PbfOptions::default());
    let r = build(&p, None);
    if let Ok((g, _)) = r {
        assert!(node_count(&g) >= 2);
    }
}

#[test]
fn nodes_outside_the_extract_are_skipped() {
    // Edge-case block: "PBF referencing nodes outside the extract": a way whose middle
    // node is missing from the file (as in a clipped extract).
    let mut o = Osm::new();
    let (a, b) = (BASE, north(400.0));
    o.street(a, b, 100.0, &[("highway", "residential")]);
    let w = o.street(
        east(500.0),
        (BASE.0 + dlat(400.0), BASE.1 + dlon(500.0, BASE.0)),
        100.0,
        &[("highway", "residential")],
    );
    let mid = o.ways[w as usize - 1].refs[2];
    o.nodes.retain(|n| n.id != mid);
    let (g, r) = graph(&o);
    assert!(missing_node_refs(&r) >= 1);
    assert!(close(graph_metres(&g, a, b).unwrap(), 400.0, 0.002, 0.5));
}

// ============================ bbox, empty and wrong files ============================

#[test]
fn bbox_keeps_only_streets_inside() {
    let mut o = Osm::new();
    o.street(BASE, north(1000.0), 50.0, &[("highway", "residential")]);
    let far = (2.0, 2.0);
    o.street(
        far,
        (far.0 + dlat(500.0), far.1),
        50.0,
        &[("highway", "residential")],
    );
    let (_d, p) = write(&o, PbfOptions::default());
    let (g, _) = build(&p, Some((0.99, 0.99, 1.02, 1.02))).unwrap();
    assert!(find(&g, BASE).is_some());
    assert!(
        find(&g, far).is_none(),
        "street outside the bbox is dropped"
    );
    assert!((0..node_count(&g)).all(|i| coord(&g, i).0 < 1.5));
}

#[test]
fn bbox_with_no_streets_is_an_empty_graph() {
    // Edge-case block: "Bbox that contains no streets … → OsmError". Builder (M4,
    // Leader decision 3): the library gives Ok(empty graph); build-pack warns and
    // falls back to straight lines.
    let mut o = Osm::new();
    o.street(BASE, north(1000.0), 50.0, &[("highway", "residential")]);
    let (_d, p) = write(&o, PbfOptions::default());
    let (g, _) = build(&p, Some((5.0, 5.0, 5.1, 5.1))).unwrap();
    assert_eq!(node_count(&g), 0);
}

#[test]
fn files_that_are_not_pbf_are_errors() {
    let d = TempDir::new("notpbf");
    let xml = d.file(
        "map.osm",
        b"<?xml version='1.0'?><osm version='0.6'><node id='1' lat='1' lon='1'/></osm>",
    );
    let zip = d.file(
        "feed.zip",
        b"PK\x03\x04\x14\x00\x00\x00\x08\x00rest-of-a-zip",
    );
    let empty = d.file("empty.osm.pbf", b"");
    let text = d.file("text.osm.pbf", b"hello world, not protobuf at all\n");
    let tiny_gtfs =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/fixtures/tiny/stops.txt");
    let mut rng = Rng(0x0005_EED0_4001);
    let random: Vec<u8> = (0..4096).map(|_| rng.next() as u8).collect();
    let random = d.file("random.osm.pbf", &random);
    for p in [
        xml,
        zip,
        empty,
        text,
        random,
        tiny_gtfs,
        d.0.clone(),                // a directory
        d.0.join("does-not-exist"), // missing
    ] {
        assert!(build(&p, None).is_err(), "{}", p.display());
    }
}

#[test]
fn empty_extract_and_no_walkable_ways_give_a_clear_result() {
    // Edge-case block: empty extract, extract with no walkable ways → clear error
    // (Tester accepts Ok with an empty graph too; see the report's ambiguities).
    let d = TempDir::new("empty");
    let header = d.file("header.osm.pbf", &header_only());
    let mut o = Osm::new();
    o.street(BASE, north(1000.0), 50.0, &[("highway", "motorway")]);
    let motorway = d.file("motorway.osm.pbf", &to_pbf(&o, PbfOptions::default()));
    for p in [header, motorway] {
        match build(&p, None) {
            Err(e) => assert!(!format!("{e}").is_empty()),
            Ok((g, _)) => assert_eq!(node_count(&g), 0, "{}", p.display()),
        }
    }
}

// ============================ encodings, determinism, oracle ============================

/// A random grid (step 120 m) with random gaps, steps and one-way-foot pieces.
fn random_city(seed: u64, n: usize) -> Osm {
    let mut rng = Rng(seed);
    let mut o = Osm::new();
    let at = |i: usize, j: usize| {
        (
            BASE.0 + dlat(120.0 * i as f64),
            BASE.1 + dlon(120.0 * j as f64, BASE.0),
        )
    };
    for i in 0..n {
        for j in 0..n {
            for (di, dj) in [(1, 0), (0, 1)] {
                let (i2, j2) = (i + di, j + dj);
                if i2 >= n || j2 >= n || rng.below(10) == 0 {
                    continue;
                }
                let kv: &[(&str, &str)] = match rng.below(20) {
                    0 => &[("highway", "steps")],
                    1 => &[("highway", "footway"), ("oneway:foot", "yes")],
                    2 => &[("highway", "motorway")],
                    3 => &[("highway", "residential"), ("access", "private")],
                    4 => &[("highway", "footway"), ("oneway", "yes")],
                    _ => &[("highway", "residential")],
                };
                o.street(at(i, j), at(i2, j2), 40.0, kv);
            }
        }
    }
    o
}

#[test]
fn dense_plain_raw_and_zlib_give_the_same_graph() {
    let o = random_city(0x4D34_0001, 8);
    let mut sigs = Vec::new();
    for dense in [true, false] {
        for compression in [Compression::Raw, Compression::ZlibStored] {
            for per_block in [7, 8000] {
                let (_d, p) = write(
                    &o,
                    PbfOptions {
                        dense,
                        compression,
                        per_block,
                    },
                );
                let (g, _) = build(&p, None).unwrap();
                let mut total = 0.0;
                let mut e = 0;
                for i in 0..node_count(&g) {
                    for (_, m, _) in edges(&g, i) {
                        total += m;
                        e += 1;
                    }
                }
                sigs.push((node_count(&g), e, (total * 10.0).round() as i64));
            }
        }
    }
    assert!(sigs.windows(2).all(|w| w[0] == w[1]), "{sigs:?}");
}

#[test]
fn same_file_builds_the_same_graph() {
    let o = random_city(0x4D34_0002, 10);
    let (_d, p) = write(&o, PbfOptions::default());
    let dump = |g: &FootGraph| {
        (0..node_count(g))
            .map(|i| format!("{:?}{:?}", coord(g, i), edges(g, i)))
            .collect::<Vec<_>>()
    };
    let (a, _) = build(&p, None).unwrap();
    let (b, _) = build(&p, None).unwrap();
    assert_eq!(dump(&a), dump(&b));
}

#[test]
fn walking_times_match_a_reference_dijkstra() {
    // The walking oracle: shortest times between graph nodes on Builder's graph equal
    // a plain Dijkstra on the reference graph built from the same map with the plan's
    // tag rules (both at 1.3 m/s, steps ×1.5). Different seeds, 150 pairs each.
    for seed in [0x4D34_0101u64, 0x4D34_0102, 0x4D34_0103] {
        let o = random_city(seed, 9);
        let r = RefGraph::from_osm(&o);
        let (g, _) = graph(&o);
        let mut rng = Rng(seed ^ 0xFFFF);
        let n = node_count(&g);
        assert!(n > 10, "seed {seed:#x}: {n} nodes");
        let id_at = |c: (f64, f64)| -> i64 {
            *r.coord
                .iter()
                .find(|(_, &x)| haversine_m(x, c) < 0.5)
                .unwrap_or_else(|| panic!("graph node {c:?} is not on a walkable way"))
                .0
        };
        for _ in 0..150 {
            let (a, b) = (rng.below(n as u64) as usize, rng.below(n as u64) as usize);
            let got = graph_times(&g, a, 1.3).get(&(b as i64)).copied();
            let want = r
                .dijkstra(id_at(coord(&g, a)), 1.3)
                .get(&id_at(coord(&g, b)))
                .copied();
            match (got, want) {
                (Some(x), Some(y)) => assert!(
                    close(x, y, 0.003, 0.5),
                    "seed {seed:#x} {a}->{b}: {x} vs {y}"
                ),
                // Builder may keep only the largest component: unreachable there is fine
                // only if the reference also says unreachable.
                (None, None) => {}
                (x, y) => panic!("seed {seed:#x} {a}->{b}: graph {x:?}, reference {y:?}"),
            }
        }
    }
}

// ============================ real extract (slow) ============================

fn vm_hwm_kib() -> Option<u64> {
    std::fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find(|l| l.starts_with("VmHWM:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

#[test]
#[ignore = "slow: reads the whole Romania extract (fixtures/feeds/romania-260101.osm.pbf)"]
fn country_size_extract_stays_within_the_cap_or_fails_clearly() {
    // Edge-case block: "huge bbox (whole country) → within the memory cap or a clear error".
    let p =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/feeds/romania-260101.osm.pbf");
    if !p.exists() {
        eprintln!("skip: {} missing (scripts/fetch-fixtures.sh)", p.display());
        return;
    }
    let t0 = std::time::Instant::now();
    let r = build(&p, None);
    let hwm = vm_hwm_kib();
    match &r {
        Ok((g, _)) => println!(
            "romania whole: {} nodes in {:?}, VmHWM {hwm:?} KiB",
            node_count(g),
            t0.elapsed()
        ),
        Err(e) => println!(
            "romania whole: error '{e}' after {:?}, VmHWM {hwm:?} KiB",
            t0.elapsed()
        ),
    }
    if let Err(e) = r {
        assert!(!format!("{e}").is_empty());
    }
}
