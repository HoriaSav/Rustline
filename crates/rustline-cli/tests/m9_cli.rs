//! M9 tests-first (Tester, 2026-10-23): `rustline plan --window` (plan M9 step 4).
//! Assumed syntax: `--window <SECONDS>` (open decision: also accept `30m`?), works
//! with `--at` and `--arrive-by`, over the cap → non-zero exit naming the limit.
//! Run: `RUSTFLAGS="--cfg rustline_m9" cargo test -p rustline-cli --test m9_cli -- --ignored`
#![allow(unexpected_cfgs)]

mod m4_common;

mod m9 {
    use super::m4_common::*;

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

    fn count(o: &std::process::Output) -> usize {
        text(o)
            .lines()
            .filter(|l| l.contains("→") || l.contains("->"))
            .count()
    }

    #[test]
    fn window_zero_matches_no_window() {
        let c = city_pack(false);
        let a = plan(&c.pack, &["--at", "2030-01-02T08:00"]);
        let b = plan(&c.pack, &["--at", "2030-01-02T08:00", "--window", "0"]);
        assert!(a.status.success() && b.status.success(), "{}", text(&b));
        assert_eq!(text(&a), text(&b));
    }

    #[test]
    fn window_gives_more_options_and_tags() {
        let c = city_pack(false);
        let a = plan(&c.pack, &["--at", "2030-01-02T08:00"]);
        let b = plan(&c.pack, &["--at", "2030-01-02T08:00", "--window", "3600"]);
        assert!(b.status.success(), "{}", text(&b));
        assert!(count(&b) >= count(&a), "{}", text(&b));
        let t = text(&b);
        assert!(
            t.contains("fastest") && t.contains("leaves_latest"),
            "tags shown: {t}"
        );
        let r = plan(
            &c.pack,
            &["--arrive-by", "2030-01-02T10:00", "--window", "1800"],
        );
        assert!(r.status.success(), "arrive-by range: {}", text(&r));
    }

    #[test]
    fn window_over_cap_is_rejected() {
        let c = city_pack(false);
        let o = plan(&c.pack, &["--at", "2030-01-02T08:00", "--window", "7201"]);
        assert!(!o.status.success(), "{}", text(&o));
        assert!(
            text(&o).contains("window") && !text(&o).contains("unexpected argument"),
            "{}",
            text(&o)
        );
    }
}
