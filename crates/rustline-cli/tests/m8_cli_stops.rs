//! M8 (Builder): `plan --from-stop --to-stop --arrive-by` finds the same trip
//! as stop-to-stop `--at`: L3_0840 (P0 08:40 → R 08:50) for every deadline
//! from 08:57 to 09:10.
//! Run: `cargo test --locked -p rustline-cli --test m8_cli_stops`

mod m4_common;
use m4_common::*;

#[test]
fn arrive_by_stop_to_stop_finds_the_08_40_trip() {
    let c = city_pack(false);
    for m in 57..=70 {
        let at = format!("2030-01-02T{:02}:{:02}", 8 + m / 60, m % 60);
        let o = rustline(&[
            "plan",
            "--pack",
            c.pack.to_str().unwrap(),
            "--from-stop",
            "P0",
            "--to-stop",
            "R",
            "--arrive-by",
            &at,
        ]);
        let t = text(&o);
        assert!(o.status.success(), "{at}: {t}");
        assert!(
            t.contains("08:40:00") && t.contains("08:50:00"),
            "{at}: {t}"
        );
    }
}

#[test]
fn neither_at_nor_arrive_by_names_both() {
    let c = city_pack(false);
    let o = rustline(&[
        "plan",
        "--pack",
        c.pack.to_str().unwrap(),
        "--from-stop",
        "P0",
        "--to-stop",
        "R",
    ]);
    let t = text(&o);
    assert!(!o.status.success());
    assert!(t.contains("--at") && t.contains("--arrive-by"), "{t}");
}
