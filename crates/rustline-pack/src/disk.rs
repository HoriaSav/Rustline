//! Checking a pack body on disk without loading it (M7 streaming install).
//!
//! The body is an rkyv archive of [`TimetableData`](crate::TimetableData): the
//! root struct sits at the end, and each array is a relative pointer plus a
//! length. This reads the root, checks every array and string the way rkyv's
//! validator does (in declaration order, each part aligned, after the previous
//! one and before its parent; strings UTF-8), then runs the same rules as `TimetableData::check`, reading elements
//! through a small page cache. Memory stays at a few MiB whatever the pack size.
//! The checked body is kept as [`DiskStreets`], so `rustline_core` can apply
//! its street-grid rules to it before an install renames the file.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};

use crate::data::{check_arrays, ArchivedTimetableData, Arrays, A, SERVICE_WORDS_AT};
use crate::PackError;

/// Bytes read from the file at a time.
const PAGE: u64 = 64 * 1024;
/// Pages kept (4 MiB); when full, the cache starts over.
const MAX_PAGES: usize = 64;

/// A pack body in a file, read through the page cache.
struct Body {
    file: RefCell<File>,
    /// Where the body starts in the file, and its length.
    start: u64,
    len: u64,
    pages: RefCell<HashMap<u64, Vec<u8>>>,
    /// The first read error (the [`Arrays`] calls can't return one).
    error: RefCell<Option<io::Error>>,
    /// Position, length and element size of each array (by `A` index).
    arrays: Vec<(u64, usize, u64)>,
    service_words: u32,
}

fn corrupt(what: impl Into<String>) -> PackError {
    PackError::Corrupt(format!("body: {}", what.into()))
}

impl Body {
    /// Copies `out.len()` bytes at body position `pos` into `out`.
    fn read(&self, mut pos: u64, out: &mut [u8]) -> io::Result<()> {
        // range: out is at most a few KiB; pos < len (a u64), no overflow.
        if pos + out.len() as u64 > self.len {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "past the body",
            ));
        }
        let mut done = 0;
        while done < out.len() {
            let page = pos / PAGE;
            // range: below PAGE (64 KiB).
            let at = (pos % PAGE) as usize;
            let mut pages = self.pages.borrow_mut();
            if !pages.contains_key(&page) {
                if pages.len() >= MAX_PAGES {
                    pages.clear();
                }
                let from = page * PAGE;
                // range: at most PAGE (64 KiB).
                let mut buf = vec![0; PAGE.min(self.len - from) as usize];
                let mut f = self.file.borrow_mut();
                f.seek(SeekFrom::Start(self.start + from))?;
                f.read_exact(&mut buf)?;
                pages.insert(page, buf);
            }
            let p = &pages[&page];
            let k = (p.len() - at).min(out.len() - done);
            out[done..done + k].copy_from_slice(&p[at..at + k]);
            done += k;
            // range: k ≤ out.len(), a usize.
            pos += k as u64;
        }
        Ok(())
    }

    fn bytes<const N: usize>(&self, pos: u64) -> io::Result<[u8; N]> {
        let mut b = [0; N];
        self.read(pos, &mut b)?;
        Ok(b)
    }

    /// A relative pointer at `pos` (offset from `pos`) and the length after it.
    fn rel(&self, pos: u64) -> io::Result<(i64, u32)> {
        let b: [u8; 8] = self.bytes(pos)?;
        let off = i32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        let len = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
        // range: pos < len ≤ the body limit, far below i64::MAX.
        Ok((pos as i64 + i64::from(off), len))
    }

    /// Is the string of `len` bytes at `pos` UTF-8? Read in 4 KiB pieces.
    fn utf8(&self, pos: u64, len: u64) -> io::Result<bool> {
        let mut buf = [0u8; 4096 + 4];
        let (mut carry, mut done) = (0, 0);
        while done < len {
            // range: at most 4096.
            let n = (len - done).min(4096) as usize;
            self.read(pos + done, &mut buf[carry..carry + n])?;
            // range: n ≤ 4096.
            done += n as u64;
            let have = carry + n;
            match std::str::from_utf8(&buf[..have]) {
                Ok(_) => carry = 0,
                // A character cut at the end of the piece: keep its bytes.
                Err(e) if e.error_len().is_none() && done < len => {
                    let ok = e.valid_up_to();
                    buf.copy_within(ok..have, 0);
                    carry = have - ok;
                }
                Err(_) => return Ok(false),
            }
        }
        Ok(true)
    }
}

impl Arrays for Body {
    fn len(&self, a: A) -> usize {
        // range: an enum index, below A::ALL.len().
        self.arrays[a as usize].1
    }

    fn get(&self, a: A, i: usize) -> i64 {
        // range: an enum index; i < len ≤ u32::MAX and size ≤ 8, so the offset fits u64.
        let (pos, _, size) = self.arrays[a as usize];
        let mut b = [0u8; 8];
        if let Err(e) = self.read(pos + i as u64 * size, &mut b[..size as usize]) {
            self.error.borrow_mut().get_or_insert(e);
            return 0;
        }
        if a.is_strings() {
            return 0;
        }
        match a {
            A::StreetLat | A::StreetLon | A::ShapeLat | A::ShapeLon => {
                i64::from(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            }
            // range: may wrap for u64 values above i64::MAX; none is compared (as in check()).
            _ => u64::from_le_bytes(b) as i64,
        }
    }

    fn service_words(&self) -> u32 {
        self.service_words
    }
}

/// A pack body on disk that passed every check, read through the page cache
/// (from [`verify_pack_file`](crate::verify_pack_file)). Coordinates are in
/// 1e-7 degrees, as stored.
pub struct DiskStreets(Body);

impl DiskStreets {
    /// Number of street segments (0: the pack has no street graph).
    pub fn segments(&self) -> usize {
        self.0.len(A::SegA)
    }

    /// Number of straight segment pieces (segments plus shape points).
    pub fn pieces(&self) -> usize {
        self.0.len(A::SegA) + self.0.len(A::ShapeLat)
    }

    /// `[lo_lat, hi_lat, lo_lon, hi_lon]` over street nodes and shape points.
    pub fn bbox_e7(&self) -> Result<[i32; 4], PackError> {
        let mut b = [i32::MAX, i32::MIN, i32::MAX, i32::MIN];
        for (lat, lon) in [(A::StreetLat, A::StreetLon), (A::ShapeLat, A::ShapeLon)] {
            for i in 0..self.0.len(lat) {
                let (y, x) = (self.0.i32_at(lat, i)?, self.0.i32_at(lon, i)?);
                b = [b[0].min(y), b[1].max(y), b[2].min(x), b[3].max(x)];
            }
        }
        Ok(b)
    }

    /// Calls `f(a, b)` for every piece (`(lat, lon)` end points), segment by
    /// segment, from its first node through its shape points to its last,
    /// until `f` returns false.
    pub fn each_piece(
        &self,
        mut f: impl FnMut((i32, i32), (i32, i32)) -> bool,
    ) -> Result<(), PackError> {
        let b = &self.0;
        if b.len(A::SegA) == 0 {
            return Ok(());
        }
        let node = |i: u32| -> Result<(i32, i32), PackError> {
            // range: u32 fits usize on every target we build (≥ 32-bit).
            let i = i as usize;
            Ok((b.i32_at(A::StreetLat, i)?, b.i32_at(A::StreetLon, i)?))
        };
        let mut start = b.u32_at(A::ShapeStart, 0)?;
        for g in 0..b.len(A::SegA) {
            let end = b.u32_at(A::ShapeStart, g + 1)?;
            let mut p = node(b.u32_at(A::SegA, g)?)?;
            for j in start..end {
                // range: as above; check() bounds shape_start by the shape arrays.
                let j = j as usize;
                let q = (b.i32_at(A::ShapeLat, j)?, b.i32_at(A::ShapeLon, j)?);
                if !f(p, q) {
                    return Ok(());
                }
                p = q;
            }
            if !f(p, node(b.u32_at(A::SegB, g)?)?) {
                return Ok(());
            }
            start = end;
        }
        Ok(())
    }
}

impl Body {
    /// Element `i` of an i32 array.
    fn i32_at(&self, a: A, i: usize) -> io::Result<i32> {
        // range: an enum index, below A::ALL.len().
        let (pos, _, _) = self.arrays[a as usize];
        // range: i < len ≤ u32::MAX, so 4 * i fits u64.
        Ok(i32::from_le_bytes(self.bytes(pos + 4 * i as u64)?))
    }

    /// Element `i` of a u32 array.
    fn u32_at(&self, a: A, i: usize) -> io::Result<u32> {
        // range: an enum index, below A::ALL.len().
        let (pos, _, _) = self.arrays[a as usize];
        // range: i < len ≤ u32::MAX, so 4 * i fits u64.
        Ok(u32::from_le_bytes(self.bytes(pos + 4 * i as u64)?))
    }
}

/// Checks the body of `len` bytes at `start` in `file`; returns `service_words`
/// and the checked body.
pub(crate) fn check_body(
    file: File,
    start: u64,
    len: u64,
) -> Result<(u32, DiskStreets), PackError> {
    let mut body = Body {
        file: RefCell::new(file),
        start,
        len,
        pages: RefCell::new(HashMap::new()),
        error: RefCell::new(None),
        arrays: Vec::new(),
        service_words: 0,
    };
    // range: size and alignment of the root, a few hundred bytes.
    let size = std::mem::size_of::<ArchivedTimetableData>() as u64;
    let align = std::mem::align_of::<ArchivedTimetableData>() as u64;
    if len < size || !(len - size).is_multiple_of(align) {
        return Err(corrupt("no room for the root"));
    }
    let root = len - size;
    // rkyv's order rule: the root's arrays lie before it, one after the other
    // in declaration order; a string's bytes lie after the previous part and
    // before its own entry array.
    let mut lo = 0u64;
    for &a in A::ALL {
        let (at, elem, align) = a.layout();
        // range: at, elem and align come from offset_of/size_of/align_of (small).
        let (pos, n) = body.rel(root + at as u64)?;
        let (elem, align) = (elem as u64, align as u64);
        let bytes = u64::from(n) * elem;
        let entries = place(pos, bytes, align, lo, root)
            .ok_or_else(|| corrupt(format!("{a:?} points outside its place")))?;
        if a.is_strings() {
            for k in 0..u64::from(n) {
                lo = string(&body, a, k, entries + k * elem, lo, entries)?;
            }
        }
        lo = entries + bytes;
        // range: u32 fits usize on every target we build (≥ 32-bit).
        body.arrays.push((entries, n as usize, elem));
    }
    // range: an offset inside the root.
    let w: [u8; 4] = body.bytes(root + SERVICE_WORDS_AT as u64)?;
    body.service_words = u32::from_le_bytes(w);
    let checked = check_arrays(&body);
    if let Some(e) = body.error.borrow_mut().take() {
        return Err(PackError::Io(e));
    }
    checked.map_err(PackError::Corrupt)?;
    Ok((body.service_words, DiskStreets(body)))
}

/// Where a part at `pos` of `bytes` may sit: aligned, at or after `lo` (the
/// end of the previous part) and ending by `hi` (its parent).
fn place(pos: i64, bytes: u64, align: u64, lo: u64, hi: u64) -> Option<u64> {
    let p = u64::try_from(pos).ok()?;
    let end = p.checked_add(bytes)?;
    (p >= lo && p.is_multiple_of(align) && end <= hi).then_some(p)
}

/// Checks string entry `k` at `at`; its bytes, when out of line, must sit in
/// `lo..hi` and be UTF-8. Returns the new `lo`.
fn string(body: &Body, a: A, k: u64, at: u64, lo: u64, hi: u64) -> Result<u64, PackError> {
    let e: [u8; 8] = body.bytes(at)?;
    if e[0] & 0xc0 != 0x80 {
        // Inline: up to 8 bytes, ended by 0xff.
        let l = e.iter().position(|&b| b == 0xff).unwrap_or(8);
        if std::str::from_utf8(&e[..l]).is_err() {
            return Err(corrupt(format!("{a:?}[{k}] is not UTF-8")));
        }
        return Ok(lo);
    }
    let raw = u32::from_le_bytes([e[0], e[1], e[2], e[3]]);
    let l = u64::from((raw & 0b0011_1111) | ((raw & !0xff) >> 2));
    let off = i32::from_le_bytes([e[4], e[5], e[6], e[7]]);
    // range: at < root ≤ the body limit, far below i64::MAX.
    let from = place(at as i64 + i64::from(off), l, 1, lo, hi)
        .filter(|_| l > 8)
        .ok_or_else(|| corrupt(format!("{a:?}[{k}] points outside its place")))?;
    if !body.utf8(from, l)? {
        return Err(corrupt(format!("{a:?}[{k}] is not UTF-8")));
    }
    Ok(from + l)
}
