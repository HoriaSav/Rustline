//! M8 tests-first (Tester, 2026-10-20): `rustline plan --arrive-by` (plan M8 CLI):
//! door to door and stop to stop, `--at` and `--arrive-by` mutually exclusive (one is
//! required), a deadline with an offset (`%:z`) or in a DST gap never panics, and the
//! arrival printed is not after the deadline.
//! Ungated in M8 (Builder removed `cfg rustline_m8` and the "M8 not implemented" ignores).
//! Run: `cargo test --locked -p rustline-cli --test m8_cli`
#![allow(unexpected_cfgs)]

mod m4_common;
use m4_common::*;

fn plan(pack: &std::path::Path, extra: &[&str]) -> std::process::Output {
    let p0 = format!("{},{}", city::P0.0, city::P0.1);
    let r = format!("{},{}", city::R.0, city::R.1);
    let mut a = vec![
        "plan",
        "--pack",
        pack.to_str().unwrap(),
        "--from",
        &p0,
        "--to",
        &r,
    ];
    a.extend_from_slice(extra);
    rustline(&a)
}

/// Every `HH:MM[:SS]` after the header line, as seconds of the day.
fn clock_times(t: &str) -> Vec<u32> {
    t.lines()
        .skip(1)
        .flat_map(|l| l.split(|c: char| !(c.is_ascii_digit() || c == ':')))
        .filter_map(|w| {
            let v: Vec<u32> = w.split(':').filter_map(|x| x.parse().ok()).collect();
            (w.contains(':') && (2..=3).contains(&v.len()) && v[0] < 48 && v[1] < 60)
                .then(|| v[0] * 3600 + v[1] * 60 + v.get(2).copied().unwrap_or(0))
        })
        .collect()
}

#[test]
fn arrive_by_door_to_door() {
    let c = city_pack(false);
    let o = plan(&c.pack, &["--arrive-by", "2030-01-02T10:00"]);
    assert!(o.status.success(), "{}", text(&o));
    let t = text(&o);
    let times = clock_times(&t);
    assert!(!times.is_empty(), "{t}");
    assert!(
        times.iter().all(|&s| s <= 10 * 3600),
        "arrival after the deadline:\n{t}"
    );
}

#[test]
fn at_and_arrive_by_are_exclusive_and_one_is_required() {
    let c = city_pack(false);
    let both = plan(
        &c.pack,
        &[
            "--at",
            "2030-01-02T08:00",
            "--arrive-by",
            "2030-01-02T10:00",
        ],
    );
    assert!(!both.status.success());
    assert!(
        text(&both).contains("cannot be used with"),
        "{}",
        text(&both)
    );
    let none = plan(&c.pack, &[]);
    assert!(!none.status.success());
    assert!(!text(&none).contains("panicked"));
}

#[test]
fn deadline_with_offset_or_in_a_gap_never_panics() {
    let c = city_pack(false);
    for at in [
        "2030-01-02T10:00+02:00",
        "2030-01-02T10:00Z",
        "2030-03-10T02:30",
        "2030-03-31T03:30",
    ] {
        let o = plan(&c.pack, &["--arrive-by", at]);
        let t = text(&o);
        assert!(!t.contains("panicked"), "{at}: {t}");
        assert!(
            o.status.success() || t.to_lowercase().contains("no route") || t.contains("outside"),
            "{at}: {t}"
        );
    }
}

#[test]
fn arrive_by_stop_to_stop() {
    let c = city_pack(false);
    let o = rustline(&[
        "plan",
        "--pack",
        c.pack.to_str().unwrap(),
        "--from-stop",
        "P0",
        "--to-stop",
        "R",
        "--arrive-by",
        "2030-01-02T10:00",
    ]);
    assert!(o.status.success(), "{}", text(&o));
}
