//! M3 `rustline plan` CLI tests, written by Tester before M3.
//! Interface from the plan: `rustline plan --pack <rlp> --from-stop <gtfs_stop_id> --to-stop <id> --at <ISO>`.
//! Run: `cargo test --locked -p rustline-cli --test m3_plan`

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn rustline(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rustline"))
        .args(args)
        .output()
        .unwrap()
}

/// Temp dir removed on drop, also when an assertion fails.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Builds the tiny fixture pack once per test into its own temp dir; returns (dir, pack path).
fn tiny_pack(tag: &str) -> (Dir, PathBuf) {
    let dir = std::env::temp_dir().join(format!("rustline_m3_cli_{}_{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let tiny = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/fixtures/tiny");
    let out = rustline(&[
        "build-pack",
        "--gtfs",
        tiny.to_str().unwrap(),
        "--out",
        dir.to_str().unwrap(),
        "--feed-id",
        "tiny",
        "--built-at",
        "0",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let pack = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|e| e == "rlp"))
        .unwrap();
    (Dir(dir), pack)
}

fn text(o: &Output) -> (String, String) {
    (
        String::from_utf8_lossy(&o.stdout).into(),
        String::from_utf8_lossy(&o.stderr).into(),
    )
}

#[test]
fn plan_direct_ride_prints_the_journey() {
    let (_dir, pack) = tiny_pack("direct");
    // Wed 2030-01-02 08:00, A → D: R1_0800, arrives 08:15 (tiny stop_times.txt).
    let o = rustline(&[
        "plan",
        "--pack",
        pack.to_str().unwrap(),
        "--from-stop",
        "A",
        "--to-stop",
        "D",
        "--at",
        "2030-01-02T08:00",
    ]);
    let (out, err) = text(&o);
    assert!(o.status.success(), "{err}");
    assert!(out.contains("08:15"), "{out}");
    assert!(out.contains("08:00"), "{out}");
}

#[test]
fn plan_unknown_stop_is_a_clear_error() {
    let (_dir, pack) = tiny_pack("unknown");
    for (from, to) in [("NOPE", "D"), ("A", "NOPE"), ("", "D")] {
        let o = rustline(&[
            "plan",
            "--pack",
            pack.to_str().unwrap(),
            "--from-stop",
            from,
            "--to-stop",
            to,
            "--at",
            "2030-01-02T08:00",
        ]);
        let (_, err) = text(&o);
        assert!(!o.status.success(), "{from}->{to} should fail");
        assert_ne!(o.status.code(), Some(101), "panicked: {err}");
        assert!(!err.contains("panicked"), "{err}");
        let bad = if from == "A" { to } else { from };
        assert!(
            bad.is_empty() || err.contains(bad),
            "error should name the stop id: {err}"
        );
    }
}

#[test]
fn plan_no_route_is_success_with_no_journeys() {
    let (_dir, pack) = tiny_pack("noroute");
    // D → A: nothing leaves D. Empty result, not an error (DoD: "no route (empty result, not error)").
    let o = rustline(&[
        "plan",
        "--pack",
        pack.to_str().unwrap(),
        "--from-stop",
        "D",
        "--to-stop",
        "A",
        "--at",
        "2030-01-02T08:00",
    ]);
    let (_, err) = text(&o);
    assert!(o.status.success(), "{err}");
}

#[test]
fn plan_hostile_arguments_never_panic() {
    let (dir, pack) = tiny_pack("fuzz");
    let p = pack.to_str().unwrap().to_string();
    let missing = dir.0.join("missing.rlp");
    let garbage = dir.0.join("garbage.rlp");
    fs::write(&garbage, b"RLPK\x02\x00not really").unwrap();
    let cases: Vec<Vec<String>> = vec![
        vec!["--at".into(), "2030-13-01T08:00".into()],
        vec!["--at".into(), "garbage".into()],
        vec!["--at".into(), "2030-01-02T25:00".into()],
        vec!["--at".into(), "2030-01-02T00:00".into()],
        vec!["--at".into(), "2030-01-02T23:59:59".into()],
        vec!["--at".into(), "1970-01-01T08:00".into()],
        vec!["--at".into(), "9999-12-31T08:00".into()],
        vec!["--at".into(), "".into()],
    ];
    for c in &cases {
        let mut args = vec!["plan", "--pack", &p, "--from-stop", "A", "--to-stop", "D"];
        args.extend(c.iter().map(String::as_str));
        let o = rustline(&args);
        assert_ne!(o.status.code(), Some(101), "{c:?} panicked: {}", text(&o).1);
        assert!(!text(&o).1.contains("panicked"), "{c:?}");
    }
    for pk in [missing.to_str().unwrap(), garbage.to_str().unwrap()] {
        let o = rustline(&[
            "plan",
            "--pack",
            pk,
            "--from-stop",
            "A",
            "--to-stop",
            "D",
            "--at",
            "2030-01-02T08:00",
        ]);
        assert!(!o.status.success());
        assert_ne!(o.status.code(), Some(101), "{}", text(&o).1);
    }
}

#[test]
fn plan_help_lists_the_planned_flags() {
    let o = rustline(&["plan", "--help"]);
    let (out, _) = text(&o);
    assert!(o.status.success());
    for flag in ["--pack", "--from-stop", "--to-stop", "--at"] {
        assert!(out.contains(flag), "{flag} missing: {out}");
    }
}
