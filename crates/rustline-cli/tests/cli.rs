//! Runs the `rustline` binary like a user would.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn rustline(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rustline"))
        .args(args)
        .env_remove("SOURCE_DATE_EPOCH")
        .output()
        .unwrap()
}

fn tiny() -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/fixtures/tiny");
    p.to_string_lossy().to_string()
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rustline_cli_{}_{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn only_file(dir: &Path, ext: &str) -> PathBuf {
    let files: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().ends_with(ext))
        .collect();
    assert_eq!(files.len(), 1, "{files:?}");
    files[0].clone()
}

#[test]
fn build_pack_is_reproducible_and_inspectable() {
    let (a, b) = (temp_dir("a"), temp_dir("b"));
    for dir in [&a, &b] {
        let out = rustline(&[
            "build-pack",
            "--gtfs",
            &tiny(),
            "--out",
            dir.to_str().unwrap(),
            "--feed-id",
            "tiny",
            "--built-at",
            "1700000000",
        ]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let rlp = only_file(&a, ".rlp");
    assert!(rlp
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("rustline-tiny-"));
    assert_eq!(
        fs::read(&rlp).unwrap(),
        fs::read(only_file(&b, ".rlp")).unwrap()
    );
    assert_eq!(
        fs::read(only_file(&a, ".rlp.zst")).unwrap(),
        fs::read(only_file(&b, ".rlp.zst")).unwrap()
    );

    for file in [rlp, only_file(&a, ".rlp.zst")] {
        let out = rustline(&["inspect", file.to_str().unwrap(), "--json"]);
        assert!(out.status.success());
        let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(json["format_version"], rustline_pack::FORMAT_VERSION);
        assert_eq!(json["built_at_unix"], 1_700_000_000);
        assert_eq!(json["feed_sha256"].as_str().unwrap().len(), 64);
    }
    let _ = fs::remove_dir_all(a);
    let _ = fs::remove_dir_all(b);
}

#[test]
fn feed_without_valid_trips_is_a_clear_error() {
    let feed = temp_dir("notrips");
    fs::write(
        feed.join("agency.txt"),
        "agency_id,agency_name,agency_url,agency_timezone\nAG,T,https://example.invalid/,Etc/UTC\n",
    )
    .unwrap();
    fs::write(
        feed.join("stops.txt"),
        "stop_id,stop_name,stop_lat,stop_lon\nA,A,1.0,1.0\n",
    )
    .unwrap();
    fs::write(
        feed.join("routes.txt"),
        "route_id,agency_id,route_short_name,route_long_name,route_type\nR,AG,1,One,3\n",
    )
    .unwrap();
    fs::write(feed.join("calendar.txt"), "service_id,monday,tuesday,wednesday,thursday,friday,saturday,sunday,start_date,end_date\nS,1,1,1,1,1,1,1,20400101,20401231\n").unwrap();
    fs::write(
        feed.join("trips.txt"),
        "route_id,service_id,trip_id\nR,S,T1\n",
    )
    .unwrap();
    // Only one stop time: the trip is dropped, nothing is left.
    fs::write(
        feed.join("stop_times.txt"),
        "trip_id,arrival_time,departure_time,stop_id,stop_sequence\nT1,08:00:00,08:00:00,A,1\n",
    )
    .unwrap();
    let out_dir = temp_dir("notrips_out");
    let out = rustline(&[
        "build-pack",
        "--gtfs",
        feed.to_str().unwrap(),
        "--out",
        out_dir.to_str().unwrap(),
    ]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no valid trips"));
    assert_eq!(fs::read_dir(&out_dir).unwrap().count(), 0);
    let _ = fs::remove_dir_all(feed);
    let _ = fs::remove_dir_all(out_dir);
}

#[test]
fn bad_pack_file_is_an_error_not_a_panic() {
    let dir = temp_dir("badpack");
    let f = dir.join("bad.rlp");
    fs::write(&f, b"RLPK\x63\x00rest").unwrap();
    let out = rustline(&["inspect", f.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("version 99"));
    let _ = fs::remove_dir_all(dir);
}
