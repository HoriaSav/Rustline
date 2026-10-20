//! M8 tests-first (Tester, 2026-10-20): plan M8 "Carried from M7 review": a pack
//! installed under the 256 MiB default can be refused by `open_with_limit` with a
//! smaller limit. Behaviour pin (ungated, passes on M7): the refusal is the
//! `pack_too_large` code, never a panic, and the installed file stays usable with
//! `open`. Doc check (ungated since M8): `open_with_limit` or
//! `install_pack` documents it (or `install_pack` takes the limit; then adapt here).
//! Run: `cargo test --locked -p rustline-ffi --test m8_open_limit`
#![allow(unexpected_cfgs)]

mod ffi_common;
use ffi_common::*;

#[test]
fn smaller_limit_after_default_install_is_a_code_not_a_panic() {
    let d = scratch("m8lim");
    let src = put(
        &d,
        "src.rlp.zst",
        &zstd::encode_all(&tiny_pack_bytes()[..], 3).unwrap(),
    );
    let dest = d.join("tiny.rlp").to_string_lossy().into_owned();
    install(&src, &dest).unwrap();
    // M7 answered `pack_corrupt` here (body check); M8 decision 2 makes it
    // `pack_too_large` (asserted exactly in the test below). Here: a code, no panic.
    let c = err_code(open_with_limit(&dest, 64));
    assert!(c == "pack_too_large" || c == "pack_corrupt", "{c}");
    assert!(open(&dest).is_ok());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn body_over_a_smaller_limit_is_pack_too_large() {
    let d = scratch("m8lim2");
    let src = put(
        &d,
        "src.rlp.zst",
        &zstd::encode_all(&tiny_pack_bytes()[..], 3).unwrap(),
    );
    let dest = d.join("tiny.rlp").to_string_lossy().into_owned();
    install(&src, &dest).unwrap();
    let c = err_code(open_with_limit(&dest, 64));
    let _ = std::fs::remove_dir_all(&d);
    assert_eq!(
        c, "pack_too_large",
        "a body over the caller's limit is not a corrupt pack"
    );
}

#[test]
fn open_with_limit_documents_the_install_mismatch() {
    let src = std::fs::read_to_string(workspace("crates/rustline-ffi/src/api.rs")).unwrap();
    let doc = |f: &str| -> String {
        let at = src.find(f).unwrap_or_else(|| panic!("{f}"));
        src[..at]
            .lines()
            .rev()
            .skip(1)
            .take_while(|l| l.trim_start().starts_with("///") || l.trim_start().starts_with("#["))
            .collect::<Vec<_>>()
            .join("\n")
            .to_lowercase()
    };
    let a = doc("pub fn open_with_limit");
    let b = doc("pub fn install_pack(");
    let takes_limit = src.contains("pub fn install_pack(")
        && src[src.find("pub fn install_pack(").unwrap()..]
            .lines()
            .next()
            .unwrap_or("")
            .contains("max_body");
    assert!(
        takes_limit || ((a.contains("install") || b.contains("open_with_limit")) && (a + &b).contains("limit")),
        "document that a pack installed under the default can be refused by a smaller open_with_limit (or pass the limit to install_pack)"
    );
}
