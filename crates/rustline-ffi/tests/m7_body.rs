//! M7 (Leader decision 3, Builder): a pack whose CRC32 is right but whose body
//! is bad must not replace the installed pack. `install_pack` checks the body
//! on disk; these tests craft such packs and compare the on-disk check with
//! `read_pack` and `Engine::from_bytes` (the full in-memory checks `open` uses).
//! Run: `cargo test --locked -p rustline-ffi --test m7_body`

mod ffi_common;
use ffi_common::*;

/// CRC32 (IEEE), as the pack trailer uses.
fn crc32(bytes: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in bytes {
        c ^= u32::from(b);
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

/// `pack` with its trailer recomputed, so only the body check can catch a change.
fn fix_crc(mut pack: Vec<u8>) -> Vec<u8> {
    let n = pack.len() - 4;
    let c = crc32(&pack[..n]);
    pack[n..].copy_from_slice(&c.to_le_bytes());
    pack
}

/// Where the body starts: 98 fixed header bytes plus the two strings.
fn body_start(pack: &[u8]) -> usize {
    let tz = u16::from_le_bytes([pack[86], pack[87]]) as usize;
    let id = u16::from_le_bytes([pack[88 + tz], pack[89 + tz]]) as usize;
    98 + tz + id
}

/// Installs a good pack, then `bad`: `pack_corrupt`, and the good pack is still there.
fn assert_rejected(bad: &[u8], what: &str) {
    assert!(
        rustline_core::Engine::from_bytes(bad).is_err(),
        "{what}: open accepts it"
    );
    let d = scratch("m7body");
    let good = tiny_pack_bytes();
    let dest = d.join("brasov.rlp").to_string_lossy().into_owned();
    let z = |b: &[u8]| zstd::encode_all(b, 3).unwrap();
    install(&put(&d, "good.rlp.zst", &z(&good)), &dest).expect("good install");
    let r = install(&put(&d, "bad.rlp.zst", &z(bad)), &dest);
    assert_eq!(err_code(r), "pack_corrupt", "{what}");
    assert_eq!(
        std::fs::read(&dest).unwrap(),
        good,
        "{what}: old pack replaced"
    );
    assert!(open(&dest).is_ok());
    let mut left: Vec<String> = std::fs::read_dir(&d)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".tmp"))
        .collect();
    left.sort();
    assert!(left.is_empty(), "{what}: temp files left: {left:?}");
}

#[test]
fn crc_ok_but_inconsistent_body_keeps_the_old_pack() {
    // A trip in a pattern that does not exist: rkyv reads it, check() does not.
    let pack = rustline_pack::read_pack(&tiny_pack_bytes()).unwrap();
    let (meta, mut tt) = pack.into_parts();
    tt.trip_pattern[0] = 9_999;
    let mut bad = Vec::new();
    rustline_pack::write_pack(&tt, &meta, &mut bad).unwrap();
    assert_rejected(&bad, "trip_pattern out of range");
}

#[test]
fn crc_ok_but_broken_layout_keeps_the_old_pack() {
    let good = tiny_pack_bytes();
    let end = good.len() - 4; // the body ends before the CRC32
                              // The root's last array (service_bits): a length far past the body.
    let mut bad = good.clone();
    bad[end - 4..end].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_rejected(&fix_crc(bad), "array length past the body");
    // Its relative pointer: far before the body.
    let mut bad = good.clone();
    bad[end - 8..end - 4].copy_from_slice(&i32::MIN.to_le_bytes());
    assert_rejected(&fix_crc(bad), "array pointer before the body");
}

#[test]
fn crc_ok_but_bad_utf8_keeps_the_old_pack() {
    // A stop name longer than 8 bytes is stored out of line: break one byte.
    let (meta, mut tt) = rustline_pack::read_pack(&tiny_pack_bytes())
        .unwrap()
        .into_parts();
    let name = "Piata Sfatului (long name)";
    tt.stop_names[0] = name.to_string();
    let mut good = Vec::new();
    rustline_pack::write_pack(&tt, &meta, &mut good).unwrap();
    assert!(rustline_pack::read_pack(&good).is_ok());
    let start = body_start(&good);
    let at = good[start..]
        .windows(name.len())
        .position(|w| w == name.as_bytes())
        .expect("name in the body")
        + start;
    let mut bad = good.clone();
    bad[at] = 0xff;
    assert_rejected(&fix_crc(bad), "non-UTF-8 stop name");
}

#[test]
fn disk_check_agrees_with_read_pack_on_single_byte_changes() {
    // Every byte of the body changed in turn (CRC fixed): the on-disk check
    // accepts exactly what read_pack accepts.
    let good = tiny_pack_bytes();
    let d = scratch("m7agree");
    let path = d.join("p.rlp");
    let check = |b: &[u8]| {
        std::fs::write(&path, b).unwrap();
        rustline_pack::verify_pack_file(&path, rustline_pack::DEFAULT_MAX_BODY_BYTES).is_ok()
    };
    assert!(check(&good));
    let (start, end) = (body_start(&good), good.len() - 4);
    let mut differ = Vec::new();
    for i in start..end {
        for x in [0x00u8, 0xff, good[i] ^ 0x01, good[i] ^ 0x80] {
            if x == good[i] {
                continue;
            }
            let mut b = good.clone();
            b[i] = x;
            let b = fix_crc(b);
            let full = rustline_pack::read_pack(&b).is_ok();
            if check(&b) != full {
                differ.push((i - start, x, full));
            }
        }
    }
    println!("body {} bytes, {} disagreements", end - start, differ.len());
    assert!(
        differ.is_empty(),
        "(offset, byte, read_pack ok): {:?}",
        &differ[..differ.len().min(20)]
    );
}

/// The tiny pack with `meta` changed by `m` and these street pieces added: one
/// segment of about 55 m going north from each point (lat, lon).
fn tiny_with(m: impl Fn(&mut rustline_pack::PackMeta), points: &[(f64, f64)]) -> Vec<u8> {
    let (mut meta, mut tt) = rustline_pack::read_pack(&tiny_pack_bytes())
        .unwrap()
        .into_parts();
    m(&mut meta);
    let e7 = |v: f64| (v * 1e7).round() as i32;
    for &(lat, lon) in points {
        let a = tt.street_lat.len() as u32;
        for d in [0.0, 0.0005] {
            tt.street_lat.push(e7(lat + d));
            tt.street_lon.push(e7(lon));
        }
        tt.seg_a.push(a);
        tt.seg_b.push(a + 1);
        tt.seg_len_cm.push(5_560);
        tt.seg_flags.push(0);
    }
    tt.shape_start = vec![0; tt.seg_a.len() + 1];
    let mut out = Vec::new();
    rustline_pack::write_pack(&tt, &meta, &mut out).unwrap();
    out
}

/// `n` short streets in a square of about 1 km at (45.6, 25.6).
fn city(n: usize) -> Vec<(f64, f64)> {
    (0..n)
        .map(|i| {
            (
                45.6 + (i % 100) as f64 * 1e-4,
                25.6 + (i / 100) as f64 * 1e-4,
            )
        })
        .collect()
}

#[test]
fn crc_ok_but_not_an_iana_timezone_keeps_the_old_pack() {
    // Reviewer MF1: read_pack accepts it, Engine::new does not.
    let bad = tiny_with(|m| m.timezone = "Mars/Olympus".into(), &[]);
    assert!(rustline_pack::read_pack(&bad).is_ok());
    assert_rejected(&bad, "timezone Mars/Olympus");
}

#[test]
fn crc_ok_but_streets_spread_over_the_globe_keeps_the_old_pack() {
    // Reviewer MF1: two streets at far corners of the globe grow the grid
    // cells until the whole city (9 000 streets) falls in one cell, more
    // than Engine::new allows. read_pack accepts it.
    let mut points = city(9_000);
    points.extend([(-84.0, -179.0), (84.0, 179.0)]);
    let bad = tiny_with(|_| {}, &points);
    assert!(rustline_pack::read_pack(&bad).is_ok());
    assert_rejected(&bad, "streets spread over the globe");
    // The same city without the far streets installs and opens.
    let good = tiny_with(|_| {}, &city(9_000));
    assert!(rustline_core::Engine::from_bytes(&good).is_ok());
    let d = scratch("m7globe");
    let dest = d.join("p.rlp").to_string_lossy().into_owned();
    let z = zstd::encode_all(&good[..], 3).unwrap();
    install(&put(&d, "p.rlp.zst", &z), &dest).expect("city installs");
}

#[test]
fn install_check_agrees_with_open_on_a_body_of_several_pages() {
    // A body over two 64 KiB pages (the page cache), changed one byte at a
    // time at every page edge, every 1 999th byte and the whole root (CRC fixed):
    // the install check accepts exactly what Engine::from_bytes accepts.
    let good = tiny_with(|_| {}, &city(5_000));
    let (start, end) = (body_start(&good), good.len() - 4);
    let page = 64 * 1024;
    assert!(end - start > 2 * page, "body is {} bytes", end - start);
    let d = scratch("m7pages");
    let path = d.join("p.rlp");
    let check = |b: &[u8]| {
        std::fs::write(&path, b).unwrap();
        rustline_pack::verify_pack_file(&path, rustline_pack::DEFAULT_MAX_BODY_BYTES)
            .is_ok_and(|f| rustline_core::check_installable(&f).is_ok())
    };
    assert!(check(&good));
    let mut at: Vec<usize> = (0..end - start).step_by(1_999).collect();
    for k in 1..=(end - start) / page {
        at.extend(k * page - 4..k * page + 4);
    }
    at.extend(end - start - 400..end - start);
    at.sort_unstable();
    at.dedup();
    let mut differ = Vec::new();
    for &i in &at {
        let i = start + i;
        for x in [0x00u8, 0xff, good[i] ^ 0x01, good[i] ^ 0x80] {
            if x == good[i] {
                continue;
            }
            let mut b = good.clone();
            b[i] = x;
            let b = fix_crc(b);
            let full = rustline_core::Engine::from_bytes(&b).is_ok();
            if check(&b) != full {
                differ.push((i - start, x, full));
            }
        }
    }
    println!(
        "body {} bytes, {} places, {} disagreements",
        end - start,
        at.len(),
        differ.len()
    );
    assert!(
        differ.is_empty(),
        "(offset, byte, open ok): {:?}",
        &differ[..differ.len().min(20)]
    );
}
