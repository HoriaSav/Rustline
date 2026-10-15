//! M4 pack tests (Tester, written before M4): plan M4 DoD "pack format bumped, older
//! versions rejected with a clear error", "pack without OSM → straight-line fallback,
//! PackInfo says so", "osm_sha256 in the header", "reproducible", "fuzz the new
//! sections"; plan step 7 footpaths (≤ 500 m, symmetric, precomputed).
//! The `cfg rustline_m4` gate and the "M4 not implemented" ignores were removed in M4.

mod m4_common;

use m4_common::*;
use rustline_pack::{read_pack, PackError, FORMAT_VERSION};
use sha2::{Digest, Sha256};

/// Plain CRC-32 (IEEE, reflected), the checksum at the end of a `.rlp`.
fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
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

fn rlp_bytes(c: &CityPack) -> Vec<u8> {
    std::fs::read(&c.pack).unwrap()
}

fn zst_path(c: &CityPack) -> std::path::PathBuf {
    let mut p = c.pack.clone().into_os_string();
    p.push(".zst");
    p.into()
}

#[test]
fn format_version_is_bumped() {
    // M3 shipped FORMAT_VERSION 3; the street graph and footpaths change the body.
    // black_box: a plain constant assertion is a clippy error.
    assert!(
        std::hint::black_box(FORMAT_VERSION) >= 4,
        "{FORMAT_VERSION}"
    );
    let c = city_pack(true);
    let b = rlp_bytes(&c);
    assert_eq!(&b[..4], b"RLPK");
    assert_eq!(u16::from_le_bytes([b[4], b[5]]), FORMAT_VERSION);
}

#[test]
fn older_versions_are_rejected_clearly() {
    let c = city_pack(true);
    let b = rlp_bytes(&c);
    let n = b.len();
    for v in (1..FORMAT_VERSION).chain([FORMAT_VERSION + 1, u16::MAX]) {
        let mut x = b.clone();
        x[4..6].copy_from_slice(&v.to_le_bytes());
        let crc = crc32(&x[..n - 4]);
        x[n - 4..].copy_from_slice(&crc.to_le_bytes());
        match read_pack(&x) {
            Err(PackError::UnsupportedVersion { found, supported }) => {
                assert_eq!((found, supported), (v, FORMAT_VERSION));
            }
            other => panic!("version {v}: {:?}", other.map(|_| ())),
        }
        let e = Engine::from_bytes(&x).err().expect("engine rejects it too");
        let msg = e.to_string();
        assert!(
            msg.contains(&v.to_string()) && msg.contains("version"),
            "{msg}"
        );
    }
    // Sanity: the patch method itself (same version, recomputed CRC) still loads.
    let mut x = b.clone();
    let crc = crc32(&x[..n - 4]);
    x[n - 4..].copy_from_slice(&crc.to_le_bytes());
    assert!(read_pack(&x).is_ok(), "own CRC32 disagrees with the pack's");
}

#[test]
fn osm_sha256_is_in_the_header() {
    let c = city_pack(true);
    let want: [u8; 32] = Sha256::digest(std::fs::read(&c.pbf).unwrap()).into();
    let p = read_pack(&rlp_bytes(&c)).unwrap();
    assert_eq!(p.meta().osm_sha256, want);
    assert_eq!(
        &rlp_bytes(&c)[38..70],
        &want,
        "header layout: magic|ver|feed_sha|osm_sha"
    );
    // Without --osm: no OSM hash (zeros, as in M1–M3).
    let n = city_pack(false);
    assert_eq!(
        read_pack(&rlp_bytes(&n)).unwrap().meta().osm_sha256,
        [0u8; 32]
    );
}

#[test]
fn packs_are_reproducible() {
    let a = city_pack(true);
    let b = city_pack(true);
    assert_eq!(
        rlp_bytes(&a),
        rlp_bytes(&b),
        ".rlp differs between two builds"
    );
    assert_eq!(
        std::fs::read(zst_path(&a)).unwrap(),
        std::fs::read(zst_path(&b)).unwrap(),
        ".rlp.zst differs"
    );
}

#[test]
fn every_single_bit_flip_is_rejected() {
    // CRC-32 detects every single-bit error, so each flip must be an Err (never a panic
    // and never a silently different pack). 2 000 positions spread over the file.
    let c = city_pack(true);
    let b = rlp_bytes(&c);
    let mut rng = Rng(0x4D34_F11B);
    for i in 0..2000 {
        let pos = if i < 128 {
            i % b.len()
        } else {
            rng.below(b.len() as u64) as usize
        };
        let bit = rng.below(8) as u8;
        let mut x = b.clone();
        x[pos] ^= 1 << bit;
        let r = std::panic::catch_unwind(|| read_pack(&x).map(|_| ()));
        match r {
            Ok(Err(_)) => {}
            Ok(Ok(())) => panic!("flip at {pos} bit {bit} accepted"),
            Err(_) => panic!("flip at {pos} bit {bit} panicked"),
        }
    }
}

#[test]
fn truncated_packs_are_rejected() {
    let c = city_pack(true);
    let b = rlp_bytes(&c);
    let step = (b.len() / 1500).max(1);
    for n in (0..b.len())
        .step_by(step)
        .chain(b.len().saturating_sub(64)..b.len())
    {
        let r = std::panic::catch_unwind(|| read_pack(&b[..n]).map(|_| ()));
        assert!(matches!(r, Ok(Err(_))), "prefix {n}/{}: {r:?}", b.len());
    }
}

#[test]
fn damaged_zst_never_panics() {
    // A flip may only be accepted when it changes nothing that was read (e.g. the
    // zstd frame's checksum flag): the loaded pack must then equal the original.
    let c = city_pack(true);
    let z = std::fs::read(zst_path(&c)).unwrap();
    let orig = Engine::from_bytes(&z).unwrap();
    let mut rng = Rng(0x4D34_25D7);
    for _ in 0..1000 {
        let mut x = z.clone();
        let pos = rng.below(x.len() as u64) as usize;
        x[pos] ^= 1 << rng.below(8);
        match std::panic::catch_unwind(|| Engine::from_bytes(&x)) {
            Ok(Err(_)) => {}
            Ok(Ok(e)) => {
                assert_eq!(
                    e.info(),
                    orig.info(),
                    "flip at {pos} accepted and changed the header"
                );
                assert!(
                    e.pack().timetable() == orig.pack().timetable(),
                    "flip at {pos} accepted and changed the body"
                );
            }
            Err(_) => panic!("flip at {pos} panicked"),
        }
    }
}

#[test]
fn pack_info_says_whether_there_is_a_street_graph() {
    let with = engine(&city_pack(true).pack);
    let without = engine(&city_pack(false).pack);
    assert!(has_street_graph(&with.info()));
    assert!(!has_street_graph(&without.info()));
}

#[test]
fn pack_without_osm_uses_the_straight_line_fallback() {
    // Spec §8.1: straight line × 1.3 detour factor. P0 → D is 500.4 m straight:
    // 650.5 m of walking at 1.3 m/s = 500.4 s, 08:05 → 08:13:20.4. Transit (L1_0810 to
    // P0b 08:11 + 55.6 × 1.3 / 1.3 s) arrives 08:11:55.6, so the walk is within 3 min.
    let e = engine(&city_pack(false).pack);
    let its = plan(&e, &req(city::P0, city::D, at(wed(), 8, 5, 0))).unwrap();
    let straight = haversine_m(city::P0, city::D);
    let walk = its
        .iter()
        .find(|i| i.rides == 0)
        .unwrap_or_else(|| panic!("{its:?}"));
    let (dep, want) = (8.0 * 3600.0 + 300.0, straight * 1.3 / WALK);
    assert!(
        close(secs(wed(), walk.arr) - dep, want, 0.003, 1.5),
        "{walk:?} vs {want}"
    );
    let ride = its
        .iter()
        .find(|i| i.rides == 1)
        .unwrap_or_else(|| panic!("{its:?}"));
    let (alight, want) = (
        8.0 * 3600.0 + 660.0,
        haversine_m(city::P0B, city::D) * 1.3 / WALK,
    );
    assert!(
        close(secs(wed(), ride.arr) - alight, want, 0.003, 1.5),
        "{ride:?} vs {want}"
    );
}

#[test]
fn footpaths_are_symmetric_short_and_match_the_walk_oracle() {
    let c = city_pack(true);
    let e = engine(&c.pack);
    let fp = footpaths(&e);
    let get = |a: &str, b: &str| fp.iter().find(|(x, y, _)| x == a && y == b).map(|f| f.2);
    // Short (≤ 500 m at 1.3 m/s = 384.6 s), symmetric within 1 s.
    for (a, b, s) in &fp {
        assert!(f64::from(*s) <= 500.0 / WALK + 1.0, "{a}->{b}: {s}");
        let back = get(b, a).unwrap_or_else(|| panic!("{a}->{b} has no reverse"));
        assert!(back.abs_diff(*s) <= 1, "{a}->{b}: {s} vs {back}");
    }
    // Oracle: every pair of linked stops within 500 m of walking has a footpath of the
    // reference walking time; farther pairs have none.
    let osm = line_city_osm();
    let rg = RefGraph::from_osm(&osm);
    for (a, ac) in LINKED_STOPS {
        for (b, bc) in LINKED_STOPS {
            if a == b {
                continue;
            }
            match walk_metres(&rg, &osm, *ac, *bc) {
                Some(m) if m <= 495.0 => {
                    let w = walk_secs(&rg, &osm, *ac, *bc).unwrap();
                    let s = get(a, b).unwrap_or_else(|| panic!("{a}->{b} ({m:.1} m) missing"));
                    assert!(
                        close(f64::from(s), w, 0.005, 1.0),
                        "{a}->{b}: {s} vs {w:.1}"
                    );
                }
                Some(m) if m > 505.0 => assert!(get(a, b).is_none(), "{a}->{b} is {m:.0} m"),
                None => assert!(get(a, b).is_none(), "{a}->{b} across components"),
                _ => {}
            }
        }
    }
    // Hand values: P0–P0b 444.8 m → 342.1 s; P1–P1a same place → 0 s; P0–P1 1 112 m → none.
    assert!(
        get("P0", "P0b").is_some_and(|s| s.abs_diff(342) <= 1),
        "{:?}",
        get("P0", "P0b")
    );
    assert!(
        get("P1", "P1a").is_some_and(|s| s <= 1),
        "{:?}",
        get("P1", "P1a")
    );
    assert!(get("P0", "P1").is_none());
}
