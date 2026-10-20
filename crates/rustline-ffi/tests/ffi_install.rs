//! M6 tests-first (Tester, 2026-10-20): installing a `.rlp.zst` (plan M6 open-pack
//! safety): free-space check, temp file + atomic rename, recovery from a partial file
//! left by a kill, and a bad download never replacing a good pack.
//! The `cfg rustline_m6` gate and the "M6 not implemented" ignores were removed in M6.
//! Run: `cargo test --locked -p rustline-ffi --test ffi_install`

mod ffi_common;
use ffi_common::*;
use std::path::Path;

fn zst(dir: &Path) -> String {
    put(
        dir,
        "tiny.rlp.zst",
        &zstd::encode_all(&tiny_pack_bytes()[..], 3).unwrap(),
    )
}

fn leftovers(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("tmp") || n.contains("part"))
        .collect()
}

#[test]
fn install_decompresses_opens_and_leaves_no_temp_file() {
    let d = scratch("inst");
    let dest = d.join("packs/brasov.rlp");
    std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
    install(&zst(&d), &dest.to_string_lossy()).expect("install");
    assert_eq!(std::fs::read(&dest).unwrap(), tiny_pack_bytes());
    assert!(open(&dest.to_string_lossy()).is_ok());
    assert!(leftovers(dest.parent().unwrap()).is_empty());
}

#[test]
fn bad_download_never_replaces_a_good_pack() {
    let d = scratch("atomic");
    let dest = put(&d, "brasov.rlp", &tiny_pack_bytes());
    let z = std::fs::read(zst(&d)).unwrap();
    for (name, bytes) in [
        ("truncated", z[..z.len() / 2].to_vec()),
        ("empty", Vec::new()),
        ("not zstd", b"hello".to_vec()),
        (
            "zst of junk",
            zstd::encode_all(&b"RLPK junk"[..], 1).unwrap(),
        ),
    ] {
        let src = put(&d, "bad.rlp.zst", &bytes);
        assert!(install(&src, &dest).is_err(), "{name} installed");
        assert_eq!(
            std::fs::read(&dest).unwrap(),
            tiny_pack_bytes(),
            "{name} touched dest"
        );
        assert!(leftovers(&d).is_empty(), "{name} left {:?}", leftovers(&d));
    }
}

#[test]
fn partial_file_from_a_kill_is_recovered() {
    // A kill during decompression leaves a temp file (or, with a non-atomic writer, a
    // half pack at dest). The next start must not open garbage, and the next install
    // must succeed.
    let d = scratch("partial");
    let dest = d.join("brasov.rlp");
    let half = &tiny_pack_bytes()[..100];
    for tmp in ["brasov.rlp.tmp", "brasov.rlp.part", ".brasov.rlp.tmp"] {
        std::fs::write(d.join(tmp), half).unwrap();
    }
    std::fs::write(&dest, half).unwrap();
    assert_eq!(err_code(open(&dest.to_string_lossy())), "pack_corrupt");
    install(&zst(&d), &dest.to_string_lossy()).expect("install over partial files");
    assert!(open(&dest.to_string_lossy()).is_ok());
}

#[cfg(debug_assertions)]
#[test]
fn not_enough_free_space_is_refused_before_writing() {
    let d = scratch("space");
    let dest = d.join("brasov.rlp");
    let need = tiny_pack_bytes().len() as u64;
    let c = err_code(install_with_free(
        &zst(&d),
        &dest.to_string_lossy(),
        need / 2,
    ));
    println!("no space -> {c}");
    assert!(!dest.exists());
    assert!(leftovers(&d).is_empty());
    install_with_free(&zst(&d), &dest.to_string_lossy(), need * 4).expect("enough space");
}

#[test]
fn two_concurrent_installs_leave_one_whole_pack() {
    // Added by Builder (M6 review MF1): two installs of the same dest at once.
    // Each must succeed (the second waits for the lock), dest must be the whole
    // pack, and only dest and its .lock may be left in the folder.
    let src_dir = scratch("conc_src");
    let z = zst(&src_dir);
    for round in 0..10 {
        let d = scratch("conc");
        let dest = d.join("brasov.rlp").to_string_lossy().into_owned();
        std::thread::scope(|s| {
            let hs: Vec<_> = (0..2).map(|_| s.spawn(|| install(&z, &dest))).collect();
            for h in hs {
                h.join().expect("no panic").expect("install");
            }
        });
        assert_eq!(
            std::fs::read(&dest).unwrap(),
            tiny_pack_bytes(),
            "round {round}"
        );
        let mut names: Vec<_> = std::fs::read_dir(&d)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["brasov.rlp", "brasov.rlp.lock"], "round {round}");
    }
}
