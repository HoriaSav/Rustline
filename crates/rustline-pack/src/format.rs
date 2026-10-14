//! Writing and reading `.rlp` files.

use std::io::{self, Read, Write};

use chrono::NaiveDate;
use rkyv::rancor;
use rkyv::util::AlignedVec;

use crate::{PackError, TimetableData};

/// First four bytes of every pack.
pub const MAGIC: [u8; 4] = *b"RLPK";
/// The layout version this build writes and reads.
/// v2 (M2 review): CRC32 trailer added.
pub const FORMAT_VERSION: u16 = 2;
/// First four bytes of a zstd frame.
const ZSTD_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];
/// Default limit for the pack body (256 MiB). A tiny `.rlp.zst` can claim to
/// unpack to gigabytes (a "zstd bomb"); nothing bigger than this is allocated.
/// Use [`read_pack_with_limit`] for bigger feeds.
pub const DEFAULT_MAX_BODY_BYTES: u64 = 256 << 20;

/// Header fields: where the pack came from and when it is valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackMeta {
    /// Layout version ([`FORMAT_VERSION`] when writing).
    pub format_version: u16,
    /// SHA-256 of the GTFS zip the pack was built from. The app compares it
    /// with the backend's `current.sha256` to know if the pack is current.
    pub feed_sha256: [u8; 32],
    /// SHA-256 of the OSM extract (all zeros until M4).
    pub osm_sha256: [u8; 32],
    /// Build time, unix seconds. Pass a fixed value for reproducible packs.
    pub built_at_unix: i64,
    /// First service day.
    pub valid_from: NaiveDate,
    /// Last service day.
    pub valid_to: NaiveDate,
    /// IANA timezone from the feed's `agency_timezone`.
    pub timezone: String,
    /// Free-form feed id (e.g. MobilityDatabase id or file name), used in the file name.
    pub feed_id: String,
}

/// A loaded pack.
#[derive(Debug, Clone, PartialEq)]
pub struct Pack {
    /// Header fields.
    pub meta: PackMeta,
    /// The timetable.
    pub timetable: TimetableData,
}

/// Writes a pack (uncompressed). Compress the bytes with zstd for `.rlp.zst`.
///
/// Layout, little-endian:
/// `magic | format_version u16 | feed_sha256 [32] | osm_sha256 [32] | built_at i64
/// | valid_from i32 | valid_to i32 (days since 1970-01-01) | timezone (u16 len + UTF-8)
/// | feed_id (u16 len + UTF-8) | body_len u64 | body (rkyv archive of TimetableData)
/// | crc32 u32`. The CRC32 covers every byte before it (header and body).
pub fn write_pack(
    tt: &TimetableData,
    meta: &PackMeta,
    out: &mut impl Write,
) -> Result<(), PackError> {
    let mut out = CrcWriter {
        inner: out,
        crc: crc32fast::Hasher::new(),
    };
    write_parts(tt, meta, &mut out)?;
    let crc = out.crc.finalize();
    out.inner.write_all(&crc.to_le_bytes())?;
    Ok(())
}

fn write_parts(tt: &TimetableData, meta: &PackMeta, out: &mut impl Write) -> Result<(), PackError> {
    let body = rkyv::to_bytes::<rancor::Error>(tt).map_err(std::io::Error::other)?;
    out.write_all(&MAGIC)?;
    out.write_all(&FORMAT_VERSION.to_le_bytes())?;
    out.write_all(&meta.feed_sha256)?;
    out.write_all(&meta.osm_sha256)?;
    out.write_all(&meta.built_at_unix.to_le_bytes())?;
    out.write_all(&day_number(meta.valid_from).to_le_bytes())?;
    out.write_all(&day_number(meta.valid_to).to_le_bytes())?;
    write_str(out, &meta.timezone)?;
    write_str(out, &meta.feed_id)?;
    let body_len = u64::try_from(body.len()).map_err(|_| corrupt("body too large"))?;
    out.write_all(&body_len.to_le_bytes())?;
    out.write_all(&body)?;
    Ok(())
}

fn write_str(out: &mut impl Write, s: &str) -> Result<(), PackError> {
    let len =
        u16::try_from(s.len()).map_err(|_| corrupt("header string longer than 65535 bytes"))?;
    out.write_all(&len.to_le_bytes())?;
    out.write_all(s.as_bytes())?;
    Ok(())
}

fn epoch() -> NaiveDate {
    NaiveDate::default() // 1970-01-01
}

fn day_number(d: NaiveDate) -> i32 {
    // chrono dates span about ±262 000 years, far inside i32 days.
    (d - epoch()).num_days() as i32
}

fn from_day_number(n: i32) -> Result<NaiveDate, PackError> {
    epoch()
        .checked_add_signed(chrono::TimeDelta::days(i64::from(n)))
        .ok_or_else(|| corrupt("date out of range"))
}

/// Reads a pack from `.rlp` or `.rlp.zst` bytes, with the default body
/// limit [`DEFAULT_MAX_BODY_BYTES`].
///
/// Checks the magic first, then the version, so foreign or newer files get
/// a clear error. Never panics on bad input; the input slice needs no
/// particular alignment.
pub fn read_pack(bytes: &[u8]) -> Result<Pack, PackError> {
    read_pack_with_limit(bytes, DEFAULT_MAX_BODY_BYTES)
}

/// Like [`read_pack`], but the body may be at most `max_body_bytes` long.
///
/// A `.rlp.zst` is decompressed while reading, straight into the buffer the
/// body is parsed from, so memory stays at about one body plus zstd's
/// own small buffers.
pub fn read_pack_with_limit(bytes: &[u8], max_body_bytes: u64) -> Result<Pack, PackError> {
    if !bytes.starts_with(&ZSTD_MAGIC) {
        // Plain file: the body can't be longer than the file itself.
        let limit = max_body_bytes.min(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        return read_from(bytes, limit, false);
    }
    // If the frame says how big it unpacks, refuse a bomb before decompressing.
    if let Ok(Some(size)) = zstd::zstd_safe::get_frame_content_size(bytes) {
        if size > max_body_bytes.saturating_add(64 * 1024) {
            return Err(PackError::Corrupt(format!(
                "compressed pack unpacks to {size} bytes, more than the limit of {max_body_bytes}"
            )));
        }
    }
    let decoder = zstd::Decoder::new(bytes).map_err(zstd_error)?;
    read_from(decoder, max_body_bytes, true)
}

fn zstd_error(e: io::Error) -> PackError {
    PackError::Corrupt(format!("zstd: {e}"))
}

/// Reads header, body and checksum from a stream.
fn read_from(input: impl Read, max_body_bytes: u64, zstd: bool) -> Result<Pack, PackError> {
    let mut r = CrcReader {
        inner: input,
        crc: crc32fast::Hasher::new(),
    };
    let mut magic = [0; 4];
    if let Err(e) = r.read_exact(&mut magic) {
        // A short plain file is "not a pack"; a broken zstd stream is damage.
        return Err(if zstd {
            zstd_error(e)
        } else {
            PackError::BadMagic
        });
    }
    if magic != MAGIC {
        return Err(PackError::BadMagic);
    }
    let version = u16::from_le_bytes(r.array()?);
    if version != FORMAT_VERSION {
        return Err(PackError::UnsupportedVersion {
            found: version,
            supported: FORMAT_VERSION,
        });
    }
    let meta = PackMeta {
        format_version: version,
        feed_sha256: r.array()?,
        osm_sha256: r.array()?,
        built_at_unix: i64::from_le_bytes(r.array()?),
        valid_from: from_day_number(i32::from_le_bytes(r.array()?))?,
        valid_to: from_day_number(i32::from_le_bytes(r.array()?))?,
        timezone: r.string()?,
        feed_id: r.string()?,
    };
    if meta.valid_from > meta.valid_to {
        return Err(corrupt("valid_from is after valid_to"));
    }
    let body_len = u64::from_le_bytes(r.array()?);
    if body_len > max_body_bytes {
        return Err(PackError::Corrupt(format!(
            "body of {body_len} bytes is truncated or larger than the limit of {max_body_bytes}"
        )));
    }
    let body_len = usize::try_from(body_len).map_err(|_| corrupt("body too large"))?;
    // rkyv needs the body at an aligned address, so read it straight into an
    // AlignedVec. It grows 1 MiB at a time as data arrives: the length in the
    // header is untrusted, so a tiny file can't make us allocate it up front.
    let mut body = AlignedVec::<16>::new();
    let mut left = body_len;
    while left > 0 {
        let n = left.min(1 << 20);
        let old = body.len();
        body.resize(old + n, 0);
        r.fill(&mut body.as_mut_slice()[old..])?;
        left -= n;
    }

    let expected = r.crc.clone().finalize();
    let mut stored = [0; 4];
    read_all(&mut r.inner, &mut stored)?;
    if u32::from_le_bytes(stored) != expected {
        return Err(corrupt("body checksum mismatch (file damaged)"));
    }
    if r.inner.read(&mut [0; 1]).map_err(zstd_error)? != 0 {
        return Err(corrupt("extra bytes after the checksum"));
    }

    let timetable = rkyv::from_bytes::<TimetableData, rancor::Error>(&body)
        .map_err(|e| PackError::Corrupt(format!("body: {e}")))?;
    timetable.check().map_err(PackError::Corrupt)?;
    let days = (meta.valid_to - meta.valid_from).num_days() + 1;
    if i64::from(timetable.service_words) * 64 < days {
        return Err(corrupt(
            "service bitsets are shorter than the validity window",
        ));
    }
    Ok(Pack { meta, timetable })
}

/// `read_exact`, with a short file reported as `Corrupt`.
fn read_all(r: &mut impl Read, buf: &mut [u8]) -> Result<(), PackError> {
    r.read_exact(buf).map_err(|e| match e.kind() {
        io::ErrorKind::UnexpectedEof => corrupt("file is truncated"),
        _ => zstd_error(e),
    })
}

fn corrupt(what: &str) -> PackError {
    PackError::Corrupt(what.to_string())
}

/// Passes writes through and keeps a running CRC32 of them.
struct CrcWriter<'a, W: Write> {
    inner: &'a mut W,
    crc: crc32fast::Hasher,
}

impl<W: Write> Write for CrcWriter<'_, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.crc.update(&buf[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Reads header fields and the body, keeping a running CRC32 of every byte.
struct CrcReader<R: Read> {
    inner: R,
    crc: crc32fast::Hasher,
}

impl<R: Read> Read for CrcReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.crc.update(&buf[..n]);
        Ok(n)
    }
}

impl<R: Read> CrcReader<R> {
    fn fill(&mut self, buf: &mut [u8]) -> Result<(), PackError> {
        read_all(self, buf)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], PackError> {
        let mut a = [0; N];
        self.fill(&mut a)?;
        Ok(a)
    }

    fn string(&mut self) -> Result<String, PackError> {
        let len = usize::from(u16::from_le_bytes(self.array()?));
        let mut bytes = vec![0; len];
        self.fill(&mut bytes)?;
        String::from_utf8(bytes).map_err(|_| corrupt("header string is not UTF-8"))
    }
}
