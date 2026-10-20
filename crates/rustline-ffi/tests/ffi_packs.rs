//! M6 tests-first (Tester, 2026-10-20): opening bad packs and bad paths gives a stable
//! error code, never a panic or abort (corrupt, truncated, v1, zero-byte, missing,
//! empty / very long / non-ASCII paths, a lower body cap).
//! The `cfg rustline_m6` gate and the "M6 not implemented" ignores were removed in M6.
//! Run: `cargo test --locked -p rustline-ffi --test ffi_packs`

mod ffi_common;
use ffi_common::*;

fn open_code(path: &str) -> String {
    err_code(open(path))
}

#[test]
fn zero_byte_pack_is_pack_corrupt() {
    let d = scratch("zero");
    assert_eq!(open_code(&put(&d, "z.rlp", &[])), "pack_corrupt");
}

#[test]
fn truncated_packs_are_pack_corrupt() {
    let d = scratch("trunc");
    let b = tiny_pack_bytes();
    for cut in [1, 4, 6, 40, 80, b.len() / 2, b.len() - 5, b.len() - 1] {
        let p = put(&d, &format!("t{cut}.rlp"), &b[..cut]);
        assert_eq!(open_code(&p), "pack_corrupt", "cut at {cut} of {}", b.len());
    }
}

#[test]
fn flipped_bytes_are_pack_corrupt() {
    let d = scratch("flip");
    let b = tiny_pack_bytes();
    for i in (6..b.len()).step_by((b.len() / 37).max(1)) {
        let mut c = b.clone();
        c[i] ^= 0x5a;
        let r = open(&put(&d, "f.rlp", &c));
        // A flip in the version field may read as pack_version.
        let code = err_code(r);
        assert!(
            code == "pack_corrupt" || code == "pack_version",
            "byte {i}: {code}"
        );
    }
}

#[test]
fn bad_magic_is_pack_corrupt() {
    let d = scratch("magic");
    let mut b = tiny_pack_bytes();
    b[..4].copy_from_slice(b"PK\x03\x04"); // a zip by mistake
    assert_eq!(open_code(&put(&d, "m.rlp", &b)), "pack_corrupt");
}

#[test]
fn old_and_future_versions_are_pack_version() {
    let d = scratch("ver");
    for v in [1u16, 2, 3, rustline_pack::FORMAT_VERSION + 1, u16::MAX] {
        let mut b = tiny_pack_bytes();
        b[4..6].copy_from_slice(&v.to_le_bytes());
        assert_eq!(open_code(&put(&d, "v.rlp", &b)), "pack_version", "v{v}");
    }
}

#[test]
fn missing_empty_and_very_long_paths_are_errors() {
    let d = scratch("paths");
    let missing = d.join("nope.rlp").to_string_lossy().into_owned();
    for p in [
        missing,
        String::new(),
        "/".repeat(5000),
        "a".repeat(100_000),
    ] {
        let c = open_code(&p);
        println!("path of {} chars -> {c}", p.len());
        assert!(c != "internal", "a bad path is not an internal error");
    }
    // A directory is not a pack.
    assert_ne!(open_code(&d.to_string_lossy()), "internal");
}

#[test]
fn diacritics_and_emoji_in_the_path_open() {
    let d = scratch("utf8");
    let p = put(&d, "Brașov ăîșț 🚌 пак.rlp", &tiny_pack_bytes());
    assert!(open(&p).is_ok(), "{p}");
}

#[test]
fn lower_body_cap_refuses_the_pack_with_a_code() {
    let d = scratch("cap");
    let p = tiny_pack_file(&d);
    assert!(open_with_limit(&p, 64 << 20).is_ok());
    let c = err_code(open_with_limit(&p, 16));
    assert!(c == "pack_corrupt" || c == "pack_too_large", "{c}");
}

#[test]
fn a_zst_passed_to_open_is_an_error_not_a_crash() {
    let d = scratch("zst");
    let z = zstd::encode_all(&tiny_pack_bytes()[..], 3).unwrap();
    // Either open() accepts .rlp.zst (spec §8) or it refuses with a code.
    match open(&put(&d, "tiny.rlp.zst", &z)) {
        Ok(_) => println!("open() reads .rlp.zst"),
        Err(e) => println!("open(.rlp.zst) -> {}", code(&e)),
    }
}
