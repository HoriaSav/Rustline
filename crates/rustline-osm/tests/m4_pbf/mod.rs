//! Tester (M4, written before M4): shared test helpers, no engine code.
//! - a hand-written OSM PBF writer (protobuf by hand: raw or zlib "stored" blobs,
//!   dense or plain nodes), so tests need no extra dev-dependency;
//! - a hand-written deflate "bomb" (fixed Huffman, length-258 matches) for
//!   decompression-cap tests;
//! - a reference walking graph built from the plan's tag rules (spec §7.3) and a plain
//!   Dijkstra over it: the walking oracle;
//! - haversine distances (R = 6 371 008.8 m) and metre ↔ degree helpers.
//!
//! Included with `#[path = ...] mod m4_pbf;` from the rustline-osm and rustline-cli
//! M4 tests. Not a test target itself.
#![allow(dead_code)] // Each test file uses a different subset.

use std::collections::{BTreeMap, BinaryHeap};

// ============================ geometry ============================

/// Mean Earth radius used by the tests. Builder may use another radius or an
/// equirectangular approximation; tests compare with a relative tolerance.
pub const R_EARTH_M: f64 = 6_371_008.8;

pub fn haversine_m(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (la1, lo1, la2, lo2) = (
        a.0.to_radians(),
        a.1.to_radians(),
        b.0.to_radians(),
        b.1.to_radians(),
    );
    let h = ((la2 - la1) / 2.0).sin().powi(2)
        + la1.cos() * la2.cos() * ((lo2 - lo1) / 2.0).sin().powi(2);
    2.0 * R_EARTH_M * h.sqrt().asin()
}

/// Degrees of latitude for `m` metres along a meridian.
pub fn dlat(m: f64) -> f64 {
    (m / R_EARTH_M).to_degrees()
}

/// Degrees of longitude for `m` metres along the parallel at `lat` (exact for haversine).
pub fn dlon(m: f64, lat: f64) -> f64 {
    (2.0 * ((m / (2.0 * R_EARTH_M)).sin() / lat.to_radians().cos()).asin()).to_degrees()
}

/// Distance from p to segment a–b in metres (local equirectangular projection; the
/// test maps are a few km wide, so this is within a few cm of the true value).
pub fn point_segment_m(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let k = p.0.to_radians().cos();
    let xy = |q: (f64, f64)| {
        (
            (q.1 - p.1).to_radians() * k * R_EARTH_M,
            (q.0 - p.0).to_radians() * R_EARTH_M,
        )
    };
    let (ax, ay) = xy(a);
    let (bx, by) = xy(b);
    let (dx, dy) = (bx - ax, by - ay);
    let l2 = dx * dx + dy * dy;
    let t = if l2 == 0.0 {
        0.0
    } else {
        (-(ax * dx + ay * dy) / l2).clamp(0.0, 1.0)
    };
    let (cx, cy) = (ax + t * dx, ay + t * dy);
    (cx * cx + cy * cy).sqrt()
}

/// |a - b| <= abs + rel * max(|a|, |b|).
pub fn close(a: f64, b: f64, rel: f64, abs: f64) -> bool {
    (a - b).abs() <= abs + rel * a.abs().max(b.abs())
}

// ============================ OSM model ============================

#[derive(Clone, Debug)]
pub struct Node {
    pub id: i64,
    pub lat: f64,
    pub lon: f64,
    pub tags: Vec<(String, String)>,
}

#[derive(Clone, Debug)]
pub struct Way {
    pub id: i64,
    pub refs: Vec<i64>,
    pub tags: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default)]
pub struct Osm {
    pub nodes: Vec<Node>,
    pub ways: Vec<Way>,
    next_node: i64,
    next_way: i64,
}

pub fn tags(kv: &[(&str, &str)]) -> Vec<(String, String)> {
    kv.iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

impl Osm {
    pub fn new() -> Self {
        Osm {
            next_node: 1,
            next_way: 1,
            ..Default::default()
        }
    }

    /// Adds a node; returns its id.
    pub fn node(&mut self, lat: f64, lon: f64) -> i64 {
        let id = self.next_node;
        self.next_node += 1;
        self.nodes.push(Node {
            id,
            lat,
            lon,
            tags: vec![],
        });
        id
    }

    /// The id of an existing node at exactly (lat, lon), or a new one.
    pub fn node_at(&mut self, lat: f64, lon: f64) -> i64 {
        if let Some(n) = self.nodes.iter().find(|n| n.lat == lat && n.lon == lon) {
            return n.id;
        }
        self.node(lat, lon)
    }

    pub fn way(&mut self, refs: &[i64], kv: &[(&str, &str)]) -> i64 {
        let id = self.next_way;
        self.next_way += 1;
        self.ways.push(Way {
            id,
            refs: refs.to_vec(),
            tags: tags(kv),
        });
        id
    }

    /// A straight way from a to b with a node every `step_m` metres (≥ 2 nodes).
    /// End nodes are shared with other ways at the same coordinates.
    pub fn street(
        &mut self,
        a: (f64, f64),
        b: (f64, f64),
        step_m: f64,
        kv: &[(&str, &str)],
    ) -> i64 {
        let len = haversine_m(a, b);
        let n = ((len / step_m).ceil() as usize).max(1);
        let mut refs = vec![self.node_at(a.0, a.1)];
        for i in 1..n {
            let t = i as f64 / n as f64;
            refs.push(self.node(a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1)));
        }
        refs.push(self.node_at(b.0, b.1));
        self.way(&refs, kv)
    }

    pub fn coord(&self, id: i64) -> Option<(f64, f64)> {
        self.nodes
            .iter()
            .find(|n| n.id == id)
            .map(|n| (n.lat, n.lon))
    }
}

// ============================ protobuf writer ============================

pub fn varint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

fn zigzag(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

#[derive(Default, Clone)]
pub struct Msg(pub Vec<u8>);

impl Msg {
    pub fn key(&mut self, field: u32, wire: u8) {
        varint(&mut self.0, (u64::from(field) << 3) | u64::from(wire));
    }
    pub fn uint(&mut self, field: u32, v: u64) -> &mut Self {
        self.key(field, 0);
        varint(&mut self.0, v);
        self
    }
    /// int32/int64 (two's complement as u64, so -1 takes 10 bytes).
    pub fn int(&mut self, field: u32, v: i64) -> &mut Self {
        self.uint(field, v as u64)
    }
    pub fn sint(&mut self, field: u32, v: i64) -> &mut Self {
        self.uint(field, zigzag(v))
    }
    pub fn bytes(&mut self, field: u32, b: &[u8]) -> &mut Self {
        self.key(field, 2);
        varint(&mut self.0, b.len() as u64);
        self.0.extend_from_slice(b);
        self
    }
    pub fn packed_uint(&mut self, field: u32, v: &[u64]) -> &mut Self {
        let mut p = Vec::new();
        for &x in v {
            varint(&mut p, x);
        }
        self.bytes(field, &p)
    }
    pub fn packed_sint(&mut self, field: u32, v: &[i64]) -> &mut Self {
        let z: Vec<u64> = v.iter().map(|&x| zigzag(x)).collect();
        self.packed_uint(field, &z)
    }
}

fn deltas(v: &[i64]) -> Vec<i64> {
    let mut prev = 0;
    v.iter()
        .map(|&x| {
            let d = x - prev;
            prev = x;
            d
        })
        .collect()
}

// ============================ zlib / deflate ============================

pub fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + u32::from(x)) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}

/// Valid zlib stream using only "stored" (uncompressed) deflate blocks.
pub fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let chunks: Vec<&[u8]> = if data.is_empty() {
        vec![&[][..]]
    } else {
        data.chunks(65_535).collect()
    };
    for (i, c) in chunks.iter().enumerate() {
        out.push(u8::from(i + 1 == chunks.len()));
        let n = c.len() as u16;
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&(!n).to_le_bytes());
        out.extend_from_slice(c);
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

struct Bits {
    out: Vec<u8>,
    acc: u32,
    n: u32,
}

impl Bits {
    fn bit(&mut self, b: u32) {
        self.acc |= (b & 1) << self.n;
        self.n += 1;
        if self.n == 8 {
            self.out.push(self.acc as u8);
            self.acc = 0;
            self.n = 0;
        }
    }
    /// Huffman codes go most-significant bit first.
    fn code(&mut self, code: u32, len: u32) {
        for i in (0..len).rev() {
            self.bit(code >> i);
        }
    }
    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push(self.acc as u8);
        }
        self.out
    }
}

/// zlib stream that inflates to `1 + 258 * matches` zero bytes (one literal 0, then
/// `matches` copies of length 258 at distance 1, fixed Huffman). About 13 bits per
/// 258 output bytes: 2 million matches ≈ 3.3 MB in, 516 MB out.
pub fn zlib_zero_bomb(matches: u64) -> (Vec<u8>, u64) {
    let mut b = Bits {
        out: vec![0x78, 0x01],
        acc: 0,
        n: 0,
    };
    b.bit(1); // BFINAL
    b.bit(1); // BTYPE = 01 (fixed), LSB first
    b.bit(0);
    b.code(0x30, 8); // literal 0
    for _ in 0..matches {
        b.code(0xC5, 8); // length symbol 285 = 258, no extra bits
        b.code(0, 5); // distance code 0 = 1
    }
    b.code(0, 7); // end of block
    let mut out = b.finish();
    let total = 1 + 258 * matches;
    // Adler-32 of `total` zero bytes: a stays 1, b = total mod 65521.
    let adler = (((total % 65_521) as u32) << 16) | 1;
    out.extend_from_slice(&adler.to_be_bytes());
    (out, total)
}

// ============================ PBF writer ============================

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Compression {
    Raw,
    ZlibStored,
}

#[derive(Clone, Copy, Debug)]
pub struct PbfOptions {
    pub dense: bool,
    pub compression: Compression,
    /// Nodes (and ways) per PrimitiveBlock.
    pub per_block: usize,
}

impl Default for PbfOptions {
    fn default() -> Self {
        PbfOptions {
            dense: true,
            compression: Compression::ZlibStored,
            per_block: 8000,
        }
    }
}

/// Blob message: raw bytes or zlib with raw_size.
pub fn blob(payload: &[u8], c: Compression) -> Vec<u8> {
    let mut b = Msg::default();
    match c {
        Compression::Raw => {
            b.bytes(1, payload);
        }
        Compression::ZlibStored => {
            b.uint(2, payload.len() as u64);
            b.bytes(3, &zlib_stored(payload));
        }
    }
    b.0
}

/// `u32 BE header length | BlobHeader | Blob`; the header's datasize is the blob length.
pub fn frame(kind: &str, blob: &[u8]) -> Vec<u8> {
    frame_with(kind, blob.len() as i64, None, blob)
}

/// Like `frame` but with a chosen datasize and (optionally) a lying header length.
pub fn frame_with(kind: &str, datasize: i64, header_len: Option<u32>, blob: &[u8]) -> Vec<u8> {
    let mut h = Msg::default();
    h.bytes(1, kind.as_bytes());
    h.int(3, datasize);
    let mut out = header_len
        .unwrap_or(h.0.len() as u32)
        .to_be_bytes()
        .to_vec();
    out.extend_from_slice(&h.0);
    out.extend_from_slice(blob);
    out
}

pub fn header_block(features: &[&str]) -> Vec<u8> {
    let mut h = Msg::default();
    for f in features {
        h.bytes(4, f.as_bytes());
    }
    h.bytes(16, b"rustline-tester");
    h.0
}

struct Strings(Vec<String>, BTreeMap<String, u64>);

impl Strings {
    fn new() -> Self {
        Strings(vec![String::new()], BTreeMap::new())
    }
    fn id(&mut self, s: &str) -> u64 {
        if let Some(&i) = self.1.get(s) {
            return i;
        }
        let i = self.0.len() as u64;
        self.0.push(s.to_string());
        self.1.insert(s.to_string(), i);
        i
    }
    fn table(&self) -> Vec<u8> {
        let mut t = Msg::default();
        for s in &self.0 {
            t.bytes(1, s.as_bytes());
        }
        t.0
    }
}

fn raw_coord(deg: f64) -> i64 {
    (deg * 1e7).round() as i64 // granularity 100 nanodegrees (default)
}

fn primitive_block(st: &Strings, group: &[u8]) -> Vec<u8> {
    let mut b = Msg::default();
    b.bytes(1, &st.table());
    b.bytes(2, group);
    b.0
}

fn node_block(nodes: &[Node], dense: bool) -> Vec<u8> {
    let mut st = Strings::new();
    let mut g = Msg::default();
    if dense {
        let ids: Vec<i64> = nodes.iter().map(|n| n.id).collect();
        let lats: Vec<i64> = nodes.iter().map(|n| raw_coord(n.lat)).collect();
        let lons: Vec<i64> = nodes.iter().map(|n| raw_coord(n.lon)).collect();
        let mut kv = Vec::new();
        for n in nodes {
            for (k, v) in &n.tags {
                kv.push(st.id(k));
                kv.push(st.id(v));
            }
            kv.push(0);
        }
        let mut d = Msg::default();
        d.packed_sint(1, &deltas(&ids));
        d.packed_sint(8, &deltas(&lats));
        d.packed_sint(9, &deltas(&lons));
        if nodes.iter().any(|n| !n.tags.is_empty()) {
            d.packed_uint(10, &kv);
        }
        g.bytes(2, &d.0);
    } else {
        for n in nodes {
            let mut m = Msg::default();
            m.sint(1, n.id);
            let k: Vec<u64> = n.tags.iter().map(|(k, _)| st.id(k)).collect();
            let v: Vec<u64> = n.tags.iter().map(|(_, v)| st.id(v)).collect();
            if !k.is_empty() {
                m.packed_uint(2, &k);
                m.packed_uint(3, &v);
            }
            m.sint(8, raw_coord(n.lat));
            m.sint(9, raw_coord(n.lon));
            g.bytes(1, &m.0);
        }
    }
    primitive_block(&st, &g.0)
}

fn way_block(ways: &[Way]) -> Vec<u8> {
    let mut st = Strings::new();
    let mut g = Msg::default();
    for w in ways {
        let mut m = Msg::default();
        m.int(1, w.id);
        let k: Vec<u64> = w.tags.iter().map(|(k, _)| st.id(k)).collect();
        let v: Vec<u64> = w.tags.iter().map(|(_, v)| st.id(v)).collect();
        if !k.is_empty() {
            m.packed_uint(2, &k);
            m.packed_uint(3, &v);
        }
        m.packed_sint(8, &deltas(&w.refs));
        g.bytes(3, &m.0);
    }
    primitive_block(&st, &g.0)
}

/// Writes `osm` as a complete .osm.pbf: OSMHeader, node blocks, way blocks.
pub fn to_pbf(osm: &Osm, opt: PbfOptions) -> Vec<u8> {
    let mut features = vec!["OsmSchema-V0.6"];
    if opt.dense {
        features.push("DenseNodes");
    }
    let mut out = frame(
        "OSMHeader",
        &blob(&header_block(&features), opt.compression),
    );
    for c in osm.nodes.chunks(opt.per_block.max(1)) {
        out.extend(frame(
            "OSMData",
            &blob(&node_block(c, opt.dense), opt.compression),
        ));
    }
    for c in osm.ways.chunks(opt.per_block.max(1)) {
        out.extend(frame("OSMData", &blob(&way_block(c), opt.compression)));
    }
    out
}

/// Just the OSMHeader frame (an extract with no data).
pub fn header_only() -> Vec<u8> {
    frame(
        "OSMHeader",
        &blob(
            &header_block(&["OsmSchema-V0.6", "DenseNodes"]),
            Compression::Raw,
        ),
    )
}

// ============================ reference walking graph ============================

/// Plan/spec §7.3 rules as Tester reads them (Builder's list may be longer; tests
/// only use the values below):
/// - walkable `highway`: footway, path, pedestrian, steps, living_street, residential,
///   service, unclassified, tertiary, secondary, primary, track, cycleway (only with
///   foot=yes/designated), crossing; platform, corridor, primary_link, secondary_link,
///   tertiary_link (Leader, M4 answers and fix round);
/// - excluded: motorway, motorway_link, trunk, trunk_link, unknown values, no highway
///   tag; `foot=no`;
///   `access=private`/`access=no` unless `foot=yes|designated|permissive`;
/// - `foot=yes|designated` makes any highway walkable (also motorway);
/// - `oneway:foot=yes` makes it one-way for pedestrians; `oneway=yes` does not;
/// - `highway=steps` costs ×1.5; `incline` is ignored.
pub fn walkable(t: &[(String, String)]) -> bool {
    let g = |k: &str| t.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str());
    let Some(hw) = g("highway") else {
        return false;
    };
    let foot = g("foot");
    let foot_ok = matches!(foot, Some("yes" | "designated" | "permissive"));
    if foot == Some("no") {
        return false;
    }
    if foot_ok {
        return true;
    }
    if matches!(g("access"), Some("private" | "no")) {
        return false;
    }
    matches!(
        hw,
        "footway"
            | "path"
            | "pedestrian"
            | "steps"
            | "living_street"
            | "residential"
            | "service"
            | "unclassified"
            | "tertiary"
            | "secondary"
            | "primary"
            | "track"
            | "crossing"
            | "platform"
            | "corridor"
            | "primary_link"
            | "secondary_link"
            | "tertiary_link"
    )
}

pub fn is_steps(t: &[(String, String)]) -> bool {
    t.iter().any(|(k, v)| k == "highway" && v == "steps")
}

pub fn oneway_foot(t: &[(String, String)]) -> bool {
    t.iter().any(|(k, v)| k == "oneway:foot" && v == "yes")
}

/// Directed reference graph: node id → [(to id, metres, steps)].
#[derive(Default, Debug, Clone)]
pub struct RefGraph {
    pub adj: BTreeMap<i64, Vec<(i64, f64, bool)>>,
    pub coord: BTreeMap<i64, (f64, f64)>,
}

impl RefGraph {
    pub fn from_osm(osm: &Osm) -> Self {
        let mut g = RefGraph::default();
        let coords: BTreeMap<i64, (f64, f64)> =
            osm.nodes.iter().map(|n| (n.id, (n.lat, n.lon))).collect();
        for w in &osm.ways {
            if !walkable(&w.tags) || w.refs.len() < 2 {
                continue;
            }
            if w.refs.iter().any(|r| !coords.contains_key(r)) {
                continue; // way referencing a missing node: skipped whole
            }
            let st = is_steps(&w.tags);
            let one = oneway_foot(&w.tags);
            for p in w.refs.windows(2) {
                let (a, b) = (coords[&p[0]], coords[&p[1]]);
                let m = haversine_m(a, b);
                g.coord.insert(p[0], a);
                g.coord.insert(p[1], b);
                g.adj.entry(p[0]).or_default().push((p[1], m, st));
                g.adj.entry(p[1]).or_default();
                if !one {
                    g.adj.entry(p[1]).or_default().push((p[0], m, st));
                }
            }
        }
        g
    }

    /// Plain Dijkstra in seconds at `speed` m/s, steps ×1.5. Returns time per node id.
    pub fn dijkstra(&self, from: i64, speed: f64) -> BTreeMap<i64, f64> {
        dijkstra_generic(from, speed, |n| {
            self.adj.get(&n).cloned().unwrap_or_default()
        })
    }

    /// Walking metres (no steps factor) from one node to every node.
    pub fn metres(&self, from: i64) -> BTreeMap<i64, f64> {
        dijkstra_generic(from, 1.0, |n| {
            self.adj
                .get(&n)
                .map(|v| v.iter().map(|&(t, m, _)| (t, m, false)).collect())
                .unwrap_or_default()
        })
    }

    /// Node ids of the weakly connected component of `start`.
    pub fn component(&self, start: i64) -> Vec<i64> {
        let mut und: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
        for (&a, v) in &self.adj {
            for &(b, _, _) in v {
                und.entry(a).or_default().push(b);
                und.entry(b).or_default().push(a);
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut stack = vec![start];
        while let Some(n) = stack.pop() {
            if seen.insert(n) {
                stack.extend(und.get(&n).into_iter().flatten().copied());
            }
        }
        seen.into_iter().collect()
    }
}

/// Min-heap entry: smallest cost first, ties by smallest node id.
struct Item(f64, i64);
impl PartialEq for Item {
    fn eq(&self, o: &Self) -> bool {
        self.cmp(o).is_eq()
    }
}
impl Eq for Item {}
impl PartialOrd for Item {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Item {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        o.0.total_cmp(&self.0).then(o.1.cmp(&self.1))
    }
}

/// Dijkstra over any node type mapped to i64, cost = metres / speed × (1.5 on steps).
pub fn dijkstra_generic(
    from: i64,
    speed: f64,
    edges: impl Fn(i64) -> Vec<(i64, f64, bool)>,
) -> BTreeMap<i64, f64> {
    let mut dist: BTreeMap<i64, f64> = BTreeMap::new();
    let mut heap = BinaryHeap::new();
    dist.insert(from, 0.0);
    heap.push(Item(0.0, from));
    while let Some(Item(d, n)) = heap.pop() {
        if d > *dist.get(&n).unwrap_or(&f64::INFINITY) {
            continue;
        }
        for (t, m, st) in edges(n) {
            let c = d + m / speed * if st { 1.5 } else { 1.0 };
            if c < *dist.get(&t).unwrap_or(&f64::INFINITY) {
                dist.insert(t, c);
                heap.push(Item(c, t));
            }
        }
    }
    dist
}

/// xorshift64*, as in the M3 tests.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    pub fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Temp dir removed on drop, also when an assertion fails.
pub struct TempDir(pub std::path::PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> Self {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let p =
            std::env::temp_dir().join(format!("rustline_m4_{}_{}_{n}", std::process::id(), tag));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
    pub fn file(&self, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = self.0.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
