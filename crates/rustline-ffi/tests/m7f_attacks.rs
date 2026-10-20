//! M7 final check (Tester, 2026-10-20): try to break `install_pack` with crafted
//! downloads. Each case: the right error code, no panic, the old pack is kept
//! byte for byte, and no own temp file is left. Plus a differential check of the
//! on-disk body check against `read_pack` on a real pack with streets
//! (`RUSTLINE_M6_PACK`, prints `skip:` when unset).
//! Run: `cargo test --locked -p rustline-ffi --test m7f_attacks -- --nocapture`

mod ffi_common;
mod m7_ffi;
use ffi_common::*;
use m7_ffi::*;
use std::path::Path;

fn crc32(b: &[u8]) -> u32 {
    let mut c = !0u32;
    for &x in b {
        c ^= u32::from(x);
        for _ in 0..8 {
            c = if c & 1 != 0 {
                (c >> 1) ^ 0xEDB8_8320
            } else {
                c >> 1
            };
        }
    }
    !c
}

fn fix_crc(mut p: Vec<u8>) -> Vec<u8> {
    let n = p.len() - 4;
    let c = crc32(&p[..n]);
    p[n..].copy_from_slice(&c.to_le_bytes());
    p
}

fn z(b: &[u8]) -> Vec<u8> {
    zstd::encode_all(b, 3).unwrap()
}

/// Good pack installed at dest, then `bad_zst`: returns the error code.
fn attack(d: &Path, bad_zst: &[u8], what: &str) -> String {
    let good = tiny_pack_bytes();
    let dest = d.join("brasov.rlp").to_string_lossy().into_owned();
    install(&put(d, "good.rlp.zst", &z(&good)), &dest).expect("good install");
    let r = std::panic::catch_unwind(|| install(&put(d, "bad.rlp.zst", bad_zst), &dest))
        .unwrap_or_else(|_| panic!("{what}: panic"));
    let c = err_code(r);
    assert_eq!(
        std::fs::read(&dest).unwrap(),
        good,
        "{what}: old pack replaced"
    );
    assert!(open(&dest).is_ok(), "{what}");
    let own: Vec<_> = names(d)
        .into_iter()
        .filter(|n| is_own_tmp("brasov.rlp", n))
        .collect();
    assert!(own.is_empty(), "{what}: left {own:?}");
    println!("{what}: {c}");
    c
}

#[test]
fn crafted_downloads_keep_the_old_pack() {
    let good = tiny_pack_bytes();
    let n = good.len();
    let tz = u16::from_le_bytes([good[86], good[87]]) as usize;
    let id = u16::from_le_bytes([good[88 + tz], good[89 + tz]]) as usize;
    let len_at = 90 + tz + id; // body_len u64
    let mut cases: Vec<(&str, Vec<u8>, &[&str])> = vec![
        ("empty file", vec![], &["pack_corrupt"]),
        ("not zstd", b"hello world".to_vec(), &["pack_corrupt"]),
        (
            "truncated zst",
            z(&good)[..z(&good).len() / 2].to_vec(),
            &["pack_corrupt"],
        ),
        (
            "zst of a truncated pack",
            z(&good[..n / 2]),
            &["pack_corrupt"],
        ),
        ("zst of an empty pack", z(&[]), &["pack_corrupt"]),
        (
            "trailing garbage after pack",
            z(&[good.clone(), vec![7; 100]].concat()),
            &["pack_corrupt"],
        ),
    ];
    let mut bad_crc = good.clone();
    bad_crc[n - 1] ^= 1;
    cases.push(("bad CRC", z(&bad_crc), &["pack_corrupt"]));
    let mut magic = good.clone();
    magic[0] = b'X';
    cases.push(("bad magic", z(&magic), &["pack_corrupt"]));
    for v in [1u16, 3, 5, u16::MAX] {
        let mut b = good.clone();
        b[4..6].copy_from_slice(&v.to_le_bytes());
        cases.push((
            Box::leak(format!("version {v}").into_boxed_str()),
            z(&fix_crc(b)),
            &["pack_version"],
        ));
    }
    for (what, val) in [
        ("body_len u64::MAX", u64::MAX),
        ("body_len 1 TiB", 1 << 40),
        ("body_len 0", 0),
        ("body_len +1", (n - len_at - 12 + 1) as u64),
    ] {
        let mut b = good.clone();
        b[len_at..len_at + 8].copy_from_slice(&val.to_le_bytes());
        cases.push((what, z(&fix_crc(b)), &["pack_corrupt", "pack_too_large"]));
    }
    let mut b = good.clone();
    b[86..88].copy_from_slice(&u16::MAX.to_le_bytes());
    cases.push(("timezone len 65535", z(&fix_crc(b)), &["pack_corrupt"]));
    for (i, (what, bytes, ok)) in cases.iter().enumerate() {
        let d = scratch(&format!("m7fatk{i}"));
        let c = attack(&d, bytes, what);
        assert!(ok.contains(&c.as_str()), "{what}: {c}");
    }
}

#[test]
fn zstd_bomb_is_refused_and_bounded() {
    // 300 MiB of zeros (over the 256 MiB body cap) compresses to a few KiB.
    let mut enc = zstd::Encoder::new(Vec::new(), 19).unwrap();
    let zeros = vec![0u8; 1 << 20];
    let mut good_head = tiny_pack_bytes();
    good_head.truncate(100);
    std::io::Write::write_all(&mut enc, &good_head).unwrap();
    for _ in 0..300 {
        std::io::Write::write_all(&mut enc, &zeros).unwrap();
    }
    let bomb = enc.finish().unwrap();
    println!("bomb: {} bytes compressed", bomb.len());
    let d = scratch("m7fbomb");
    let t0 = std::time::Instant::now();
    let c = attack(&d, &bomb, "zstd bomb 300 MiB");
    println!("zstd bomb took {:?}", t0.elapsed());
    assert!(c == "pack_too_large" || c == "pack_corrupt", "{c}");
}

#[cfg(unix)]
#[test]
fn symlink_dest_does_not_write_through_to_its_target() {
    let d = scratch("m7fsym");
    let outside = scratch("m7fsym_target");
    let target = outside.join("precious");
    std::fs::write(&target, b"precious").unwrap();
    let dest = d.join("brasov.rlp");
    std::os::unix::fs::symlink(&target, &dest).unwrap();
    let r = install(
        &put(&d, "good.rlp.zst", &z(&tiny_pack_bytes())),
        &dest.to_string_lossy(),
    );
    println!(
        "install over a symlink dest: {:?}",
        r.as_ref().map(|_| ()).map_err(code)
    );
    assert_eq!(
        std::fs::read(&target).unwrap(),
        b"precious",
        "wrote through the symlink"
    );
    // A bad download over a symlink dest leaves the link alone.
    let d2 = scratch("m7fsym2");
    let dest2 = d2.join("brasov.rlp");
    std::os::unix::fs::symlink(&target, &dest2).unwrap();
    assert!(install(&put(&d2, "bad.rlp.zst", b"junk"), &dest2.to_string_lossy()).is_err());
    assert!(dest2.symlink_metadata().unwrap().file_type().is_symlink());
    assert_eq!(std::fs::read(&target).unwrap(), b"precious");
}

#[test]
fn concurrent_good_and_bad_installs_end_with_a_good_pack() {
    let d = scratch("m7fconc");
    let good = tiny_pack_bytes();
    let gz = put(&d, "good.rlp.zst", &z(&good));
    let mut bad = good.clone();
    let n = bad.len();
    bad[n / 2] ^= 0x55;
    let bz = put(&d, "bad.rlp.zst", &z(&bad));
    let dest = d.join("brasov.rlp").to_string_lossy().into_owned();
    install(&gz, &dest).unwrap();
    std::thread::scope(|s| {
        for i in 0..8 {
            let (gz, bz, dest) = (&gz, &bz, &dest);
            s.spawn(move || {
                for _ in 0..10 {
                    let r = install(if i % 2 == 0 { gz } else { bz }, dest);
                    if i % 2 == 0 {
                        r.expect("good install");
                    } else {
                        assert_eq!(err_code(r), "pack_corrupt");
                    }
                }
            });
        }
    });
    assert_eq!(std::fs::read(&dest).unwrap(), good);
    assert!(names(&d).iter().all(|n| !is_own_tmp("brasov.rlp", n)));
}

#[test]
fn disk_check_agrees_with_read_pack_on_a_real_pack() {
    let Some(p) = env_pack() else { return };
    let good = std::fs::read(&p).unwrap();
    let tz = u16::from_le_bytes([good[86], good[87]]) as usize;
    let id = u16::from_le_bytes([good[88 + tz], good[89 + tz]]) as usize;
    let (start, end) = (98 + tz + id, good.len() - 4);
    let d = scratch("m7fagree");
    let path = d.join("p.rlp");
    let check = |b: &[u8]| {
        std::fs::write(&path, b).unwrap();
        rustline_pack::verify_pack_file(&path, rustline_pack::DEFAULT_MAX_BODY_BYTES).is_ok()
    };
    assert!(check(&good));
    let n: usize = std::env::var("RUSTLINE_M7F_MUTATIONS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(300);
    let mut x = 0x6d37_u64;
    let (mut differ, mut rejected) = (Vec::new(), 0);
    for k in 0..n {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let mut b = good.clone();
        // Mostly the tail (index arrays and roots), some anywhere in the body.
        let i = if k % 2 == 0 {
            end - 1 - (x as usize % 4096.min(end - start))
        } else {
            start + x as usize % (end - start)
        };
        b[i] = [0x00, 0xff, b[i] ^ 0x01, b[i] ^ 0x80][(x >> 40) as usize % 4];
        let b = fix_crc(b);
        let full = rustline_pack::read_pack(&b).is_ok();
        if !full {
            rejected += 1;
        }
        if check(&b) != full {
            differ.push((i - start, full));
        }
    }
    println!(
        "{n} mutations of a {} MiB body: {rejected} rejected by read_pack, {} disagreements",
        (end - start) >> 20,
        differ.len()
    );
    assert!(
        differ.is_empty(),
        "(offset, read_pack ok): {:?}",
        &differ[..differ.len().min(20)]
    );
}
