//! M6 tests-first (Tester, 2026-10-20): the FFI surface is plain data and every public
//! function returns `Result` (plan M6 DoD). Source scan of `src/api.rs` (FRB reads
//! that file) plus compile-time trait checks.
//! The `cfg rustline_m6` gate and the "M6 not implemented" ignores were removed in M6.
//! Run: `cargo test --locked -p rustline-ffi --test ffi_types`

mod ffi_common;
use ffi_common::*;

fn api_source() -> String {
    std::fs::read_to_string(workspace("crates/rustline-ffi/src/api.rs")).expect("src/api.rs")
}

/// `pub fn` signatures (up to the body brace), comments stripped.
fn pub_fns(src: &str) -> Vec<String> {
    let code: String = src
        .lines()
        .map(|l| l.split("//").next().unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    let mut out = Vec::new();
    let mut rest = code.as_str();
    while let Some(i) = rest.find("pub fn ") {
        let sig = &rest[i..];
        let end = sig.find('{').or_else(|| sig.find(';')).unwrap_or(sig.len());
        out.push(sig[..end].split_whitespace().collect::<Vec<_>>().join(" "));
        rest = &sig[end..];
    }
    out
}

#[test]
fn every_public_fn_returns_result() {
    let fns = pub_fns(&api_source());
    assert!(!fns.is_empty(), "no pub fn in api.rs");
    let bad: Vec<_> = fns
        .iter()
        .filter(|s| !s.contains("-> anyhow::Result<") && !s.contains("-> Result<"))
        .collect();
    assert!(bad.is_empty(), "pub fns without Result: {bad:#?}");
}

#[test]
fn public_signatures_and_fields_use_plain_data_only() {
    let src = api_source();
    let forbidden = [
        "rustline_core::",
        "rustline_model::",
        "rustline_pack::",
        "chrono::",
        "DateTime",
        "NaiveDate",
        "Path",
        "&str",
        "&[",
        "Box<dyn",
        "HashMap",
        "Tz",
    ];
    for line in src.lines().map(|l| l.split("//").next().unwrap().trim()) {
        let public =
            line.starts_with("pub fn ") || (line.starts_with("pub ") && line.contains(':'));
        if !public || line.starts_with("pub use") || line.starts_with("pub mod") {
            continue;
        }
        for f in forbidden {
            assert!(!line.contains(f), "non-plain type `{f}` in: {line}");
        }
    }
    // The engine's inner core handle must not be public.
    assert!(!src.contains("pub inner"), "RustlineEngine.inner is public");
}

#[test]
fn mirror_types_are_plain_send_clone_debug() {
    fn plain<T: Clone + Send + Sync + std::fmt::Debug + 'static>() {}
    plain::<FfiPlanRequest>();
    plain::<FfiPlanResponse>();
    plain::<rustline_ffi::api::FfiItinerary>();
    plain::<rustline_ffi::api::FfiLeg>();
    plain::<rustline_ffi::api::FfiPackInfo>();
    plain::<FfiError>();
    fn shared<T: Send + Sync>() {}
    shared::<RustlineEngine>();
}
