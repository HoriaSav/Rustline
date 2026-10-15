//! M4 prerequisites that hold already (Tester, written before M4; not ignored):
//! Builder checklist §1 genericity scan (no city/bbox literal in `crates/*/src`) and
//! DoD "OSM licence (ODbL) and extract date recorded in fixtures/manifest.tsv".

use std::path::Path;

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn sources(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

#[test]
fn no_city_or_bbox_literals_in_engine_sources() {
    // Same patterns as `rg -n -i 'brasov|brașov|ratbv|bucharest|europe/|45\.6|25\.5' crates/*/src`.
    let pats = [
        "brasov",
        "brașov",
        "ratbv",
        "bucharest",
        "europe/",
        "45.6",
        "25.5",
    ];
    let mut files = Vec::new();
    for c in std::fs::read_dir(root().join("crates")).unwrap().flatten() {
        let src = c.path().join("src");
        if src.is_dir() {
            sources(&src, &mut files);
        }
    }
    files.sort();
    assert!(!files.is_empty());
    let mut hits = Vec::new();
    for f in &files {
        let s = std::fs::read_to_string(f).unwrap().to_lowercase();
        for (i, line) in s.lines().enumerate() {
            for p in pats {
                if line.contains(p) {
                    hits.push(format!("{}:{}: {p}", f.display(), i + 1));
                }
            }
        }
    }
    assert!(hits.is_empty(), "{hits:#?}");
}

#[test]
fn manifest_records_the_osm_extract_with_odbl_and_date() {
    let m = std::fs::read_to_string(root().join("fixtures/manifest.tsv")).unwrap();
    let rows: Vec<Vec<&str>> = m
        .lines()
        .filter(|l| !l.starts_with('#') && !l.starts_with("name\t"))
        .map(|l| l.split('\t').collect())
        .collect();
    let osm: Vec<_> = rows
        .iter()
        .filter(|r| r.get(1).is_some_and(|f| f.ends_with(".osm.pbf")))
        .collect();
    assert!(!osm.is_empty(), "no .osm.pbf row");
    for r in osm {
        assert_eq!(r.len(), 8, "{r:?}");
        assert!(r[2].starts_with("https://"), "dated URL: {r:?}");
        assert_eq!(r[3].len(), 64, "sha256: {r:?}");
        assert!(r[3].chars().all(|c| c.is_ascii_hexdigit()));
        assert!(r[4].parse::<u64>().is_ok(), "bytes: {r:?}");
        let date = r[5];
        assert!(
            date.len() == 10 && date.as_bytes()[4] == b'-' && date.as_bytes()[7] == b'-',
            "date: {r:?}"
        );
        assert!(
            r[7].contains("ODbL") && r[7].contains("OpenStreetMap contributors"),
            "licence: {r:?}"
        );
    }
}
