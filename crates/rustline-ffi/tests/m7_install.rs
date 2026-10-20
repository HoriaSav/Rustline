//! M7 tests-first (Tester, 2026-10-20): R1 stricter leftover matching, two writers
//! with a stale leftover of another name, a source that grows/shrinks during the
//! copy, disk full mid-stream (RLIMIT_FSIZE in a child), and the M6 gap: the
//! directory after an install holds exactly `dest` + `dest.lock` + what was there,
//! checked by exact names (not "contains tmp").
//! Run: `cargo test --locked -p rustline-ffi --test m7_install`

mod ffi_common;
mod m7_ffi;
use ffi_common::*;
use m7_ffi::*;
use std::path::Path;

fn zst(dir: &Path) -> String {
    put(
        dir,
        "src.rlp.zst",
        &zstd::encode_all(&tiny_pack_bytes()[..], 3).unwrap(),
    )
}

#[test]
fn r1_only_own_temp_names_are_removed() {
    let d = scratch("m7r1");
    let src = zst(&d);
    let keep = [
        "brasov.rlp.backup.tmp",
        "brasov.rlpx.1.2.tmp",
        "brasov.rlp.part",
        "brasov.rlp.1.2.tmp.keep",
        "brasov.rlp.1.x.tmp",
        "brasov.rlp..tmp",
        "brasov.rlp.1.2.3.tmp",
        "other.rlp.1.2.tmp",
        "xbrasov.rlp.1.2.tmp",
    ];
    for k in keep {
        std::fs::write(d.join(k), b"keep me").unwrap();
    }
    // Own leftovers of a killed install: removed.
    let gone = ["brasov.rlp.4242.0.tmp", "brasov.rlp.1.17.tmp"];
    for g in gone {
        std::fs::write(d.join(g), b"half").unwrap();
    }
    for n in keep.iter().chain(&gone) {
        assert_eq!(
            is_own_tmp("brasov.rlp", n),
            gone.contains(n),
            "adapter rule for {n}"
        );
    }
    install(&src, &d.join("brasov.rlp").to_string_lossy()).expect("install");
    let left = names(&d);
    for k in keep {
        assert!(left.contains(&k.to_string()), "{k} was deleted");
        assert_eq!(std::fs::read(d.join(k)).unwrap(), b"keep me", "{k} changed");
    }
    for g in gone {
        assert!(!left.contains(&g.to_string()), "{g} left behind");
    }
}

#[cfg(unix)]
#[test]
fn r1_symlink_with_an_own_name_is_left_alone_and_its_target_too() {
    let d = scratch("m7link");
    let src = zst(&d);
    let outside = scratch("m7link_target");
    let target = outside.join("precious.rlp");
    std::fs::write(&target, b"precious").unwrap();
    let link = d.join("brasov.rlp.7.7.tmp");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let dir_link = d.join("brasov.rlp.8.8.tmp");
    std::os::unix::fs::symlink(&outside, &dir_link).unwrap();
    install(&src, &d.join("brasov.rlp").to_string_lossy()).expect("install");
    assert!(link.symlink_metadata().is_ok(), "symlink removed");
    assert!(dir_link.symlink_metadata().is_ok(), "dir symlink removed");
    assert_eq!(std::fs::read(&target).unwrap(), b"precious");
}

#[test]
fn install_leaves_exactly_dest_and_lock_by_name() {
    // Closes the M6 gap: names compared exactly, so a `.part` or any other new file
    // would fail this, with or without "tmp" in the name.
    let d = scratch("m7exact");
    let pd = d.join("packs");
    std::fs::create_dir_all(&pd).unwrap();
    std::fs::write(pd.join("unrelated.txt"), b"x").unwrap();
    let src = zst(&d);
    for _ in 0..3 {
        install(&src, &pd.join("brasov.rlp").to_string_lossy()).expect("install");
        assert_eq!(
            names(&pd),
            vec!["brasov.rlp", "brasov.rlp.lock", "unrelated.txt"]
        );
    }
    // Failed installs leave nothing new either.
    let bad = put(&d, "bad.rlp.zst", b"not zstd");
    assert!(install(&bad, &pd.join("brasov.rlp").to_string_lossy()).is_err());
    assert_eq!(
        names(&pd),
        vec!["brasov.rlp", "brasov.rlp.lock", "unrelated.txt"]
    );
}

#[test]
fn two_writers_and_a_stale_leftover_of_another_name() {
    let d = scratch("m7two");
    let src = zst(&d);
    std::fs::write(d.join("cluj.rlp.999.0.tmp"), b"other pack's leftover").unwrap();
    let dest = d.join("brasov.rlp").to_string_lossy().into_owned();
    std::thread::scope(|s| {
        let hs: Vec<_> = (0..4).map(|_| s.spawn(|| install(&src, &dest))).collect();
        for h in hs {
            h.join().expect("no panic").expect("each install succeeds");
        }
    });
    assert!(open(&dest).is_ok());
    assert_eq!(std::fs::read(&dest).unwrap(), tiny_pack_bytes());
    assert!(
        d.join("cluj.rlp.999.0.tmp").exists(),
        "another name's leftover is not ours"
    );
    assert_eq!(
        names(&d),
        vec![
            "brasov.rlp",
            "brasov.rlp.lock",
            "cluj.rlp.999.0.tmp",
            "src.rlp.zst"
        ]
    );
}

#[test]
fn source_that_grows_or_shrinks_during_the_copy() {
    // A big-ish valid .zst (pack + zstd padding frames is not valid; use a big
    // skippable frame so the source takes time to read).
    let d = scratch("m7grow");
    let good = zstd::encode_all(&tiny_pack_bytes()[..], 3).unwrap();
    let dest = d.join("brasov.rlp");
    std::fs::write(&dest, tiny_pack_bytes()).unwrap();
    for round in 0..20 {
        let src = d.join("src.rlp.zst");
        let mut b = good.clone();
        // Skippable frame (magic 0x184D2A50) of 8 MiB after the real frame.
        b.extend_from_slice(&0x184D2A50u32.to_le_bytes());
        b.extend_from_slice(&(8u32 << 20).to_le_bytes());
        b.resize(b.len() + (8 << 20), 0);
        std::fs::write(&src, &b).unwrap();
        let s = src.to_string_lossy().into_owned();
        let dst = dest.to_string_lossy().into_owned();
        let r = std::thread::scope(|sc| {
            let h = sc.spawn(|| install(&s, &dst));
            let f = std::fs::OpenOptions::new().write(true).open(&src).unwrap();
            if round % 2 == 0 {
                f.set_len(good.len() as u64 / 2).unwrap(); // shrink mid-copy
            } else {
                f.set_len(b.len() as u64 + (4 << 20)).unwrap(); // grow mid-copy
            }
            h.join().expect("no panic")
        });
        if let Err(e) = r {
            let c = code(&e);
            assert!(KNOWN_CODES.contains(&c.as_str()), "{c}");
        }
        // Whatever happened, dest is a whole valid pack (old or new), never a mix.
        assert_eq!(
            std::fs::read(&dest).unwrap(),
            tiny_pack_bytes(),
            "round {round}"
        );
        assert!(
            names(&d).iter().all(|n| !is_own_tmp("brasov.rlp", n)),
            "round {round}"
        );
    }
}

#[cfg(unix)]
#[test]
fn disk_full_mid_stream_keeps_the_old_pack() {
    // Child process: RLIMIT_FSIZE (1 KiB) below the pack size makes write() fail half-way
    // (EFBIG; SIGXFSZ ignored), a stand-in for ENOSPC mid-stream.
    const NAME: &str = "disk_full_mid_stream_keeps_the_old_pack";
    if let Some(dir) = role() {
        let d = Path::new(&dir);
        unsafe {
            libc::signal(libc::SIGXFSZ, libc::SIG_IGN);
            let lim = libc::rlimit {
                rlim_cur: 1024,
                rlim_max: libc::RLIM_INFINITY,
            };
            assert_eq!(libc::setrlimit(libc::RLIMIT_FSIZE, &lim), 0);
        }
        let r = install(
            &d.join("src.rlp.zst").to_string_lossy(),
            &d.join("brasov.rlp").to_string_lossy(),
        );
        let c = err_code(r);
        assert!(c == "no_space" || c == "pack_io", "{c}");
        return;
    }
    let d = scratch("m7full");
    let big = tiny_pack_bytes();
    assert!(
        big.len() > 1024,
        "tiny pack {} B is not over the 1 KiB limit",
        big.len()
    );
    std::fs::write(
        d.join("src.rlp.zst"),
        zstd::encode_all(&big[..], 3).unwrap(),
    )
    .unwrap();
    std::fs::write(d.join("brasov.rlp"), b"old pack bytes").unwrap();
    let (ok, out, err) = {
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", NAME, "--nocapture", "--include-ignored"])
            .env("M7_CHILD", &d)
            .output()
            .unwrap();
        (
            out.status.success(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    };
    assert!(ok, "child failed:\n{out}\n{err}");
    assert_eq!(
        std::fs::read(d.join("brasov.rlp")).unwrap(),
        b"old pack bytes"
    );
    assert!(
        names(&d).iter().all(|n| !is_own_tmp("brasov.rlp", n)),
        "{:?}",
        names(&d)
    );
}
