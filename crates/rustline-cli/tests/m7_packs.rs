//! M7 tests-first (Tester, 2026-10-20): plan M7 DoD "packs for Brașov/Cluj/Timișoara/
//! București/MBTA byte-identical to M6 output, or every difference explained", and
//! step 5 "sample-feed-1.zip now passes inspect and build-pack".
//! Baseline: `RUSTLINE_M7_BASELINE` = a `sha256.txt` recorded with the M6 binary
//! (9f2718c, `build-pack --gtfs fixtures/feeds/<f>.zip --out <dir>/<f> --built-at 0`,
//! no --osm); Tester's copy: ~/.cache/rustline_tester/m7pre/baseline_m6/sha256.txt.
//! Prints `skip:` when the baseline or a feed is missing.
//! Slow: run in release.
//! Run: `RUSTLINE_M7_BASELINE=... cargo test --locked --release -p rustline-cli --test m7_packs -- --nocapture`

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rustline_m7_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn rustline(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rustline"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn packs_are_byte_identical_to_m6() {
    let Some(base) = std::env::var("RUSTLINE_M7_BASELINE")
        .ok()
        .filter(|p| Path::new(p).is_file())
    else {
        println!("skip: RUSTLINE_M7_BASELINE unset or missing");
        return;
    };
    let want = std::fs::read_to_string(base).unwrap();
    let out = scratch("packs");
    let mut diffs = Vec::new();
    let mut checked = 0;
    for feed in ["brasov-latest", "cluj", "timisoara", "bucuresti", "mbta"] {
        let zip = workspace(&format!("fixtures/feeds/{feed}.zip"));
        if !zip.is_file() {
            println!("skip: {} missing", zip.display());
            continue;
        }
        let dir = out.join(feed);
        let o = rustline(&[
            "build-pack",
            "--gtfs",
            &zip.to_string_lossy(),
            "--out",
            &dir.to_string_lossy(),
            "--built-at",
            "0",
        ]);
        assert!(
            o.status.success(),
            "{feed}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        for e in std::fs::read_dir(&dir).unwrap() {
            let p = e.unwrap().path();
            let rel = format!("{feed}/{}", p.file_name().unwrap().to_string_lossy());
            // sha2 0.11 digests have no `LowerHex`: hex the bytes.
            let got: String = Sha256::digest(std::fs::read(&p).unwrap())
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            match want.lines().find(|l| l.ends_with(&format!("  {rel}"))) {
                Some(l) if l.starts_with(&got) => checked += 1,
                Some(_) => diffs.push(format!("{rel}: sha differs")),
                None => diffs.push(format!("{rel}: not in the baseline (new name?)")),
            }
        }
    }
    println!("byte-identical: {checked} files");
    let _ = std::fs::remove_dir_all(&out);
    assert!(
        diffs.is_empty(),
        "explain each in the M7 report:\n{}",
        diffs.join("\n")
    );
}

#[test]
fn google_sample_feed_passes_inspect_and_build_pack() {
    let zip = workspace("fixtures/feeds/google-sample.zip");
    if !zip.is_file() {
        println!("skip: {} missing", zip.display());
        return;
    }
    let z = zip.to_string_lossy();
    let o = rustline(&["inspect", &z]);
    assert!(
        o.status.success(),
        "inspect: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    let out = scratch("sample");
    let o = rustline(&[
        "build-pack",
        "--gtfs",
        &z,
        "--out",
        &out.to_string_lossy(),
        "--built-at",
        "0",
    ]);
    assert!(
        o.status.success(),
        "build-pack: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    let n = std::fs::read_dir(&out).unwrap().count();
    let _ = std::fs::remove_dir_all(&out);
    assert_eq!(n, 2, ".rlp and .rlp.zst");
}
