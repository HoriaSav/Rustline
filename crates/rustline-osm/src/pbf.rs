//! A small reader for the `.osm.pbf` format: just what the foot graph needs
//! (node coordinates, way tags and node lists), written so that a broken or
//! hostile file can only give an error.
//!
//! The file is a sequence of frames: `u32 BE header length | BlobHeader | Blob`.
//! A Blob holds a protobuf `HeaderBlock` or `PrimitiveBlock`, raw or zlib.
//! Sizes from the file are checked against [`OsmLimits`] and against what is
//! left of the file before anything is read, and buffers grow as data
//! arrives, so a header claiming gigabytes allocates nothing.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use crate::{OsmError, OsmLimits};

/// The PBF spec's maximum `BlobHeader` size.
const MAX_HEADER_BYTES: u32 = 64 * 1024;

/// Most items in one list of a block (packed varints, the string table, a
/// way's refs): `max_blob_bytes / 64`, at least 4 096. A 1-byte varint becomes
/// an 8-byte value, so without a cap a 32 MiB block of them would need over
/// 256 MiB. Real blocks hold about 8 000 entities; ways have at most 2 000 refs.
fn max_items(limits: &OsmLimits) -> usize {
    usize::try_from(limits.max_blob_bytes / 64)
        .unwrap_or(usize::MAX)
        .max(4096)
}

fn too_many(cap: usize) -> OsmError {
    OsmError::Limit(format!(
        "a block list has more than {cap} items (max_blob_bytes / 64)"
    ))
}

/// What a data block gives the graph builder.
pub(crate) trait Sink {
    /// One node (degrees, not checked).
    fn node(&mut self, id: i64, lat: f64, lon: f64) -> Result<(), OsmError>;
    /// One way: `tag(key)` looks up a tag value; `refs` are node ids.
    fn way(&mut self, tags: &Tags<'_>, refs: &[i64]) -> Result<(), OsmError>;
}

/// The tags of one way, as indices into the block's string table.
pub(crate) struct Tags<'a> {
    strings: &'a [&'a [u8]],
    keys: Vec<u64>,
    vals: Vec<u64>,
}

impl Tags<'_> {
    /// Value of tag `key`, if the way has it (and it is UTF-8).
    pub(crate) fn get(&self, key: &str) -> Option<&str> {
        let i = self
            .keys
            .iter()
            .position(|&k| self.strings.get(k as usize) == Some(&key.as_bytes()))?;
        let v = self.strings.get(*self.vals.get(i)? as usize)?;
        std::str::from_utf8(v).ok()
    }
}

/// Reads every frame of `path` and feeds nodes and ways to `sink`.
pub(crate) fn read(path: &Path, limits: &OsmLimits, sink: &mut impl Sink) -> Result<(), OsmError> {
    let file = File::open(path).map_err(|e| OsmError::Io(format!("{}: {e}", path.display())))?;
    let size = file
        .metadata()
        .map_err(|e| OsmError::Io(format!("{}: {e}", path.display())))?
        .len();
    if size == 0 {
        return Err(bad("the file is empty"));
    }
    let mut r = BufReader::new(file);
    let mut left = size;
    let mut seen_header = false;
    loop {
        let mut len = [0u8; 4];
        if left == 0 {
            break;
        }
        read_exact(&mut r, &mut len, &mut left)?;
        let header_len = u32::from_be_bytes(len);
        if header_len == 0 || header_len > MAX_HEADER_BYTES || u64::from(header_len) > left {
            return Err(bad(&format!(
                "blob header length {header_len} is invalid (not an .osm.pbf file?)"
            )));
        }
        let header = read_vec(&mut r, u64::from(header_len), &mut left)?;
        let (kind, data_size) = blob_header(&header)?;
        if data_size == 0 || data_size > limits.max_blob_bytes || data_size > left {
            return Err(OsmError::Limit(format!(
                "blob of {data_size} bytes is empty, truncated or above the limit of {} bytes",
                limits.max_blob_bytes
            )));
        }
        let blob = read_vec(&mut r, data_size, &mut left)?;
        match kind.as_str() {
            "OSMHeader" => {
                header_block(&unpack(&blob, limits)?)?;
                seen_header = true;
            }
            "OSMData" => {
                if !seen_header {
                    return Err(bad("data block before the OSMHeader block"));
                }
                primitive_block(&unpack(&blob, limits)?, max_items(limits), sink)?;
            }
            // The spec says readers skip blob types they don't know.
            _ => {}
        }
    }
    if !seen_header {
        return Err(bad("no OSMHeader block"));
    }
    Ok(())
}

fn bad(what: &str) -> OsmError {
    OsmError::Corrupt(what.to_string())
}

fn read_exact(r: &mut impl Read, buf: &mut [u8], left: &mut u64) -> Result<(), OsmError> {
    r.read_exact(buf)
        .map_err(|e| OsmError::Corrupt(format!("file is truncated ({e})")))?;
    *left = left.saturating_sub(buf.len() as u64);
    Ok(())
}

/// Reads `n` bytes (already checked against the file size and the limit),
/// growing the buffer as bytes arrive.
fn read_vec(r: &mut impl Read, n: u64, left: &mut u64) -> Result<Vec<u8>, OsmError> {
    let mut v = Vec::new();
    r.take(n)
        .read_to_end(&mut v)
        .map_err(|e| OsmError::Io(e.to_string()))?;
    if v.len() as u64 != n {
        return Err(bad("file is truncated"));
    }
    *left = left.saturating_sub(n);
    Ok(v)
}

/// `BlobHeader`: type (1) and datasize (3).
fn blob_header(b: &[u8]) -> Result<(String, u64), OsmError> {
    let (mut kind, mut size) = (None, None);
    let mut pb = Pb::new(b);
    while let Some((field, v)) = pb.next()? {
        match (field, v) {
            (1, Val::Bytes(s)) => {
                kind =
                    Some(String::from_utf8(s.to_vec()).map_err(|_| bad("blob type is not UTF-8"))?)
            }
            (3, Val::Int(n)) => size = Some(n),
            _ => {}
        }
    }
    let (Some(kind), Some(size)) = (kind, size) else {
        return Err(bad("blob header without type or datasize"));
    };
    // datasize is an int32: a negative value arrives as a huge u64.
    if size > i32::MAX as u64 {
        return Err(OsmError::Limit(format!("blob datasize {size} is invalid")));
    }
    Ok((kind, size))
}

/// `Blob`: raw (1), or zlib (3) with raw_size (2). The zlib stream is never
/// inflated past `raw_size`, which must be within the limit.
fn unpack(b: &[u8], limits: &OsmLimits) -> Result<Vec<u8>, OsmError> {
    let (mut raw, mut raw_size, mut zlib) = (None, None, None);
    let mut pb = Pb::new(b);
    while let Some((field, v)) = pb.next()? {
        match (field, v) {
            (1, Val::Bytes(s)) => raw = Some(s),
            (2, Val::Int(n)) => raw_size = Some(n),
            (3, Val::Bytes(s)) => zlib = Some(s),
            (4 | 6 | 7, _) => return Err(bad("unsupported blob compression (only raw and zlib)")),
            _ => {}
        }
    }
    if let Some(raw) = raw {
        return Ok(raw.to_vec());
    }
    let Some(zlib) = zlib else {
        return Err(bad("blob without data"));
    };
    let Some(size) = raw_size else {
        return Err(bad("zlib blob without raw_size"));
    };
    if size > limits.max_blob_bytes {
        return Err(OsmError::Limit(format!(
            "blob unpacks to {size} bytes, above the limit of {} bytes",
            limits.max_blob_bytes
        )));
    }
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(zlib)
        .take(size + 1)
        .read_to_end(&mut out)
        .map_err(|e| OsmError::Corrupt(format!("zlib: {e}")))?;
    if out.len() as u64 != size {
        return Err(bad("zlib blob does not unpack to its raw_size"));
    }
    Ok(out)
}

/// `HeaderBlock`: every required feature (4) must be one we understand.
fn header_block(b: &[u8]) -> Result<(), OsmError> {
    let mut pb = Pb::new(b);
    while let Some((field, v)) = pb.next()? {
        if let (4, Val::Bytes(f)) = (field, v) {
            if f != b"OsmSchema-V0.6" && f != b"DenseNodes" {
                return Err(OsmError::Unsupported(format!(
                    "required feature {:?}",
                    String::from_utf8_lossy(f)
                )));
            }
        }
    }
    Ok(())
}

/// `PrimitiveBlock`: string table (1), groups (2), granularity (17),
/// lat/lon offsets (19/20). Groups are read after the scalars, which may
/// come later in the message.
fn primitive_block(b: &[u8], cap: usize, sink: &mut impl Sink) -> Result<(), OsmError> {
    let mut strings: Vec<&[u8]> = Vec::new();
    let mut groups = Vec::new();
    let (mut gran, mut lat_off, mut lon_off) = (100i64, 0i64, 0i64);
    let mut pb = Pb::new(b);
    while let Some((field, v)) = pb.next()? {
        match (field, v) {
            (1, Val::Bytes(t)) => {
                let mut st = Pb::new(t);
                while let Some((f, v)) = st.next()? {
                    if let (1, Val::Bytes(s)) = (f, v) {
                        if strings.len() >= cap {
                            return Err(too_many(cap));
                        }
                        strings.push(s);
                    }
                }
            }
            (2, Val::Bytes(g)) => groups.push(g),
            (17, Val::Int(n)) => gran = n as i64,
            (19, Val::Int(n)) => lat_off = n as i64,
            (20, Val::Int(n)) => lon_off = n as i64,
            _ => {}
        }
    }
    let deg = |off: i64, raw: i64| (off as f64 + gran as f64 * raw as f64) * 1e-9;
    for g in groups {
        let mut pg = Pb::new(g);
        while let Some((field, v)) = pg.next()? {
            let Val::Bytes(m) = v else { continue };
            match field {
                1 => {
                    let (mut id, mut lat, mut lon) = (0, 0, 0);
                    let mut pn = Pb::new(m);
                    while let Some((f, v)) = pn.next()? {
                        match (f, v) {
                            (1, Val::Int(n)) => id = unzig(n),
                            (8, Val::Int(n)) => lat = unzig(n),
                            (9, Val::Int(n)) => lon = unzig(n),
                            _ => {}
                        }
                    }
                    sink.node(id, deg(lat_off, lat), deg(lon_off, lon))?;
                }
                2 => {
                    let (mut ids, mut lats, mut lons) = (Vec::new(), Vec::new(), Vec::new());
                    let mut pd = Pb::new(m);
                    while let Some((f, v)) = pd.next()? {
                        match f {
                            1 => varints(v, &mut ids, cap)?,
                            8 => varints(v, &mut lats, cap)?,
                            9 => varints(v, &mut lons, cap)?,
                            _ => {}
                        }
                    }
                    if ids.len() != lats.len() || ids.len() != lons.len() {
                        return Err(bad("dense nodes: id/lat/lon counts differ"));
                    }
                    let (mut id, mut lat, mut lon) = (0i64, 0i64, 0i64);
                    for i in 0..ids.len() {
                        id = id.wrapping_add(unzig(ids[i]));
                        lat = lat.wrapping_add(unzig(lats[i]));
                        lon = lon.wrapping_add(unzig(lons[i]));
                        sink.node(id, deg(lat_off, lat), deg(lon_off, lon))?;
                    }
                }
                3 => {
                    let (mut keys, mut vals, mut deltas) = (Vec::new(), Vec::new(), Vec::new());
                    let mut pw = Pb::new(m);
                    while let Some((f, v)) = pw.next()? {
                        match f {
                            2 => varints(v, &mut keys, cap)?,
                            3 => varints(v, &mut vals, cap)?,
                            8 => varints(v, &mut deltas, cap)?,
                            _ => {}
                        }
                    }
                    if keys.len() != vals.len() {
                        return Err(bad("way: key/value counts differ"));
                    }
                    if keys
                        .iter()
                        .chain(&vals)
                        .any(|&k| k as usize >= strings.len())
                    {
                        return Err(bad("way: tag index outside the string table"));
                    }
                    let mut refs = Vec::with_capacity(deltas.len());
                    let mut id = 0i64;
                    for d in deltas {
                        id = id.wrapping_add(unzig(d));
                        refs.push(id);
                    }
                    let tags = Tags {
                        strings: &strings,
                        keys,
                        vals,
                    };
                    sink.way(&tags, &refs)?;
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn unzig(n: u64) -> i64 {
    ((n >> 1) as i64) ^ -((n & 1) as i64)
}

/// Appends a repeated varint field: packed (bytes) or one plain value.
/// `out` grows as values arrive, up to `cap` values.
fn varints(v: Val<'_>, out: &mut Vec<u64>, cap: usize) -> Result<(), OsmError> {
    match v {
        Val::Int(n) => {
            if out.len() >= cap {
                return Err(too_many(cap));
            }
            out.push(n)
        }
        Val::Bytes(b) => {
            let mut p = Pb::new(b);
            while p.pos < b.len() {
                if out.len() >= cap {
                    return Err(too_many(cap));
                }
                out.push(p.varint()?);
            }
        }
        Val::Other => return Err(bad("unexpected wire type in a packed field")),
    }
    Ok(())
}

/// One protobuf field value.
enum Val<'a> {
    Int(u64),
    Bytes(&'a [u8]),
    /// fixed32/fixed64 (not used by the fields we read).
    Other,
}

/// A protobuf message reader over a byte slice. Every length is checked
/// against what is left, so a bad length is an error, never a big allocation.
struct Pb<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Pb<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Pb { buf, pos: 0 }
    }

    fn varint(&mut self) -> Result<u64, OsmError> {
        let mut v = 0u64;
        for i in 0..10 {
            let Some(&b) = self.buf.get(self.pos) else {
                return Err(bad("protobuf: truncated varint"));
            };
            self.pos += 1;
            v |= u64::from(b & 0x7F) << (7 * i);
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err(bad("protobuf: varint too long"))
    }

    /// The next (field number, value), or `None` at the end.
    fn next(&mut self) -> Result<Option<(u32, Val<'a>)>, OsmError> {
        if self.pos >= self.buf.len() {
            return Ok(None);
        }
        let key = self.varint()?;
        let field = u32::try_from(key >> 3).map_err(|_| bad("protobuf: bad field number"))?;
        let v = match key & 7 {
            0 => Val::Int(self.varint()?),
            1 => {
                self.skip(8)?;
                Val::Other
            }
            2 => {
                let len = self.varint()?;
                let len = usize::try_from(len).map_err(|_| bad("protobuf: bad length"))?;
                let start = self.pos;
                self.skip(len)?;
                Val::Bytes(&self.buf[start..self.pos])
            }
            5 => {
                self.skip(4)?;
                Val::Other
            }
            _ => return Err(bad("protobuf: unknown wire type")),
        };
        Ok(Some((field, v)))
    }

    fn skip(&mut self, n: usize) -> Result<(), OsmError> {
        if n > self.buf.len() - self.pos {
            return Err(bad("protobuf: field runs past the end"));
        }
        self.pos += n;
        Ok(())
    }
}
