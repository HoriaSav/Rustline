//! Building the foot graph from the nodes and ways the reader hands over.

use rustline_model::LatLon;

use crate::pbf::{Sink, Tags};
use crate::{BBox, OsmError, OsmLimits, OsmReport};

/// [`FootGraph::edge_kind`] of an ordinary walkable edge.
pub const EDGE_KIND_WALK: u8 = 0;
/// [`FootGraph::edge_kind`] of `highway=steps` (walking costs ×1.5 there).
pub const EDGE_KIND_STEPS: u8 = 1;

/// Mean Earth radius (IUGG) for haversine distances.
const EARTH_RADIUS_M: f64 = 6_371_008.8;

/// Great-circle distance in metres.
pub(crate) fn haversine_m(a: LatLon, b: LatLon) -> f64 {
    let (la1, la2) = (a.lat.to_radians(), b.lat.to_radians());
    let h = ((la2 - la1) / 2.0).sin().powi(2)
        + la1.cos() * la2.cos() * ((b.lon - a.lon).to_radians() / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_M * h.sqrt().min(1.0).asin()
}

/// The walking graph.
///
/// Nodes are junctions and dead ends, in OSM id order. Every street piece
/// between two of them is one *segment* (with its shape points); a segment
/// gives a directed edge each way, or one edge for `oneway:foot=yes`.
/// Edges of node `n` are `edges_start()[n]..edges_start()[n + 1]` in the
/// `edge_*` arrays. Built only by [`crate::build_foot_graph`], so the arrays
/// always fit together.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FootGraph {
    coords: Vec<LatLon>,
    edges_start: Vec<u32>,
    edge_to: Vec<u32>,
    edge_len_m: Vec<f32>,
    edge_kind: Vec<u8>,
    edge_seg: Vec<u32>,
    seg_a: Vec<u32>,
    seg_b: Vec<u32>,
    seg_len_m: Vec<f32>,
    seg_kind: Vec<u8>,
    seg_oneway: Vec<bool>,
    shape_start: Vec<u32>,
    shape: Vec<LatLon>,
}

impl FootGraph {
    /// Position of every node.
    pub fn node_coords(&self) -> &[LatLon] {
        &self.coords
    }

    /// CSR offsets: node n's edges are `edges_start[n]..edges_start[n + 1]`
    /// (one entry more than there are nodes).
    pub fn edges_start(&self) -> &[u32] {
        &self.edges_start
    }

    /// Target node of each edge.
    pub fn edge_to(&self) -> &[u32] {
        &self.edge_to
    }

    /// Length of each edge in metres (along the street's shape).
    pub fn edge_len_m(&self) -> &[f32] {
        &self.edge_len_m
    }

    /// [`EDGE_KIND_WALK`] or [`EDGE_KIND_STEPS`] for each edge.
    pub fn edge_kind(&self) -> &[u8] {
        &self.edge_kind
    }

    /// The segment each edge runs along.
    pub fn edge_seg(&self) -> &[u32] {
        &self.edge_seg
    }

    /// Number of segments.
    pub fn seg_count(&self) -> usize {
        self.seg_a.len()
    }

    /// First node of each segment (a one-way segment runs from a to b).
    pub fn seg_a(&self) -> &[u32] {
        &self.seg_a
    }

    /// Last node of each segment.
    pub fn seg_b(&self) -> &[u32] {
        &self.seg_b
    }

    /// Length of each segment in metres.
    pub fn seg_len_m(&self) -> &[f32] {
        &self.seg_len_m
    }

    /// Kind of each segment ([`EDGE_KIND_WALK`] / [`EDGE_KIND_STEPS`]).
    pub fn seg_kind(&self) -> &[u8] {
        &self.seg_kind
    }

    /// True for `oneway:foot=yes` segments (walkable from a to b only).
    pub fn seg_oneway(&self) -> &[bool] {
        &self.seg_oneway
    }

    /// Shape points between a and b of segment s (ends not included):
    /// `shape()[shape_start()[s]..shape_start()[s + 1]]`.
    pub fn shape_start(&self) -> &[u32] {
        &self.shape_start
    }

    /// All shape points, segment by segment.
    pub fn shape(&self) -> &[LatLon] {
        &self.shape
    }
}

/// True when people may walk on a way with these tags (rules in the crate docs).
fn walkable(tags: &Tags<'_>) -> bool {
    let Some(highway) = tags.get("highway") else {
        return false;
    };
    let foot = tags.get("foot");
    if foot == Some("no") {
        return false;
    }
    if matches!(foot, Some("yes" | "designated" | "permissive")) {
        return true;
    }
    if matches!(tags.get("access"), Some("private" | "no")) {
        return false;
    }
    matches!(
        highway,
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

const STEPS: u8 = 1;
const ONEWAY: u8 = 2;
/// Nodes beyond this latitude are ignored, and a way is cut at a piece longer
/// than [`MAX_PIECE_M`]: packs refuse both (rustline-pack `STREET_MAX_LAT`,
/// `STREET_MAX_PIECE_M`).
const MAX_LAT: f64 = 85.0;
/// Longest straight piece kept between two nodes of a way.
pub const MAX_PIECE_M: f64 = 10_000.0;

/// Collects nodes, then the walkable ways' pieces, then builds the graph.
pub(crate) struct Builder {
    bbox: Option<BBox>,
    limits: OsmLimits,
    /// Kept nodes; sorted by id (duplicates removed) when the first way arrives.
    ids: Vec<i64>,
    pos: Vec<LatLon>,
    sorted: bool,
    /// (node a, node b, STEPS | ONEWAY) for every piece of a walkable way.
    pieces: Vec<(u32, u32, u8)>,
    report: OsmReport,
}

impl Builder {
    pub(crate) fn new(bbox: Option<BBox>, limits: OsmLimits) -> Self {
        Builder {
            bbox,
            limits,
            ids: Vec::new(),
            pos: Vec::new(),
            sorted: false,
            pieces: Vec::new(),
            report: OsmReport::default(),
        }
    }

    fn sort_nodes(&mut self) {
        let mut order: Vec<u32> = (0..self.ids.len() as u32).collect();
        order.sort_by_key(|&i| self.ids[i as usize]); // stable: the first copy wins
        order.dedup_by_key(|i| self.ids[*i as usize]);
        self.ids = order.iter().map(|&i| self.ids[i as usize]).collect();
        self.pos = order.iter().map(|&i| self.pos[i as usize]).collect();
        self.sorted = true;
    }

    pub(crate) fn finish(mut self) -> (FootGraph, OsmReport) {
        let pieces = std::mem::take(&mut self.pieces);
        // Only nodes that some piece uses, renumbered in id order.
        let mut new_id = vec![u32::MAX; self.ids.len()];
        for &(a, b, _) in &pieces {
            new_id[a as usize] = 0;
            new_id[b as usize] = 0;
        }
        let mut coords = Vec::new();
        for (i, n) in new_id.iter_mut().enumerate() {
            if *n == 0 {
                *n = coords.len() as u32;
                coords.push(self.pos[i]);
            }
        }
        let pieces: Vec<(u32, u32, u8)> = pieces
            .iter()
            .map(|&(a, b, f)| (new_id[a as usize], new_id[b as usize], f))
            .collect();
        (contract(&coords, &pieces), self.report)
    }
}

impl Sink for Builder {
    fn node(&mut self, id: i64, lat: f64, lon: f64) -> Result<(), OsmError> {
        self.report.nodes_read += 1;
        if self.sorted {
            return Err(OsmError::Unsupported(
                "nodes after ways (sort the file, e.g. with `osmium sort`)".to_string(),
            ));
        }
        let p = LatLon::new(lat, lon);
        if !p.is_valid() || lat.abs() > MAX_LAT || self.bbox.is_some_and(|b| !b.contains(lat, lon))
        {
            return Ok(());
        }
        if self.ids.len() as u64 >= self.limits.max_nodes {
            return Err(OsmError::Limit(format!(
                "more than {} nodes",
                self.limits.max_nodes
            )));
        }
        self.ids.push(id);
        self.pos.push(p);
        self.report.nodes_kept += 1;
        Ok(())
    }

    fn way(&mut self, tags: &Tags<'_>, refs: &[i64]) -> Result<(), OsmError> {
        self.report.ways_read += 1;
        if !self.sorted {
            self.sort_nodes();
        }
        if !walkable(tags) {
            return Ok(());
        }
        let mut flags = 0;
        if tags.get("highway") == Some("steps") {
            flags |= STEPS;
        }
        if tags.get("oneway:foot") == Some("yes") {
            flags |= ONEWAY;
        }
        let mut prev: Option<u32> = None;
        let (mut made, mut missing) = (false, 0);
        for r in refs {
            match self.ids.binary_search(r) {
                Ok(i) => {
                    let i = i as u32;
                    if let Some(p) = prev.filter(|&p| p != i) {
                        if haversine_m(self.pos[p as usize], self.pos[i as usize]) > MAX_PIECE_M {
                            self.report.long_pieces += 1;
                        } else {
                            self.pieces.push((p, i, flags));
                            made = true;
                        }
                    }
                    prev = Some(i);
                }
                Err(_) => {
                    missing += 1;
                    prev = None;
                }
            }
        }
        if self.bbox.is_some() && missing == refs.len() && missing > 0 {
            self.report.ways_outside_bbox += 1;
            return Ok(());
        }
        self.report.missing_node_refs += missing as u64;
        if !made {
            self.report.ways_skipped += 1;
            return Ok(());
        }
        self.report.ways_walkable += 1;
        if self.report.ways_walkable > self.limits.max_ways {
            return Err(OsmError::Limit(format!(
                "more than {} walkable ways",
                self.limits.max_ways
            )));
        }
        Ok(())
    }
}

/// Merges chains through nodes with exactly two pieces of the same kind
/// (both two-way) into segments, and builds the edge arrays.
fn contract(coords: &[LatLon], pieces: &[(u32, u32, u8)]) -> FootGraph {
    let n = coords.len();
    // Pieces at each node (CSR).
    let mut start = vec![0u32; n + 1];
    for &(a, b, _) in pieces {
        start[a as usize + 1] += 1;
        start[b as usize + 1] += 1;
    }
    for i in 0..n {
        start[i + 1] += start[i];
    }
    let mut fill = start.clone();
    let mut at = vec![0u32; 2 * pieces.len()];
    for (p, &(a, b, _)) in pieces.iter().enumerate() {
        for v in [a, b] {
            at[fill[v as usize] as usize] = p as u32;
            fill[v as usize] += 1;
        }
    }
    let incident = |v: usize| &at[start[v] as usize..start[v + 1] as usize];
    let mut keep: Vec<bool> = (0..n)
        .map(|v| {
            let inc = incident(v);
            inc.len() != 2
                || pieces[inc[0] as usize].2 != pieces[inc[1] as usize].2
                || pieces[inc[0] as usize].2 & ONEWAY != 0
        })
        .collect();

    let mut used = vec![false; pieces.len()];
    let mut segs: Vec<(u32, u32, f64, u8, Vec<LatLon>)> = Vec::new();
    let walk = |from: u32, first: u32, keep: &[bool], used: &mut [bool]| {
        let (mut v, mut p) = (from, first);
        let mut len = 0.0;
        let mut shape = Vec::new();
        let flags = pieces[p as usize].2;
        loop {
            used[p as usize] = true;
            let (a, b, _) = pieces[p as usize];
            let next = if a == v { b } else { a };
            len += haversine_m(coords[v as usize], coords[next as usize]);
            v = next;
            if keep[v as usize] {
                break;
            }
            shape.push(coords[v as usize]);
            match incident(v as usize).iter().find(|&&q| !used[q as usize]) {
                Some(&q) => p = q,
                None => break, // only reached on a loop back to `from`
            }
        }
        // A one-way segment is stored in its walking direction.
        let (a, b, _) = pieces[first as usize];
        if flags & ONEWAY != 0 && from == b && a != b {
            shape.reverse();
            (v, from, len, flags, shape)
        } else {
            (from, v, len, flags, shape)
        }
    };
    for v in 0..n as u32 {
        if !keep[v as usize] {
            continue;
        }
        for &p in incident(v as usize) {
            if !used[p as usize] {
                segs.push(walk(v, p, &keep, &mut used));
            }
        }
    }
    // Closed loops without any junction: cut them at their first node.
    for p in 0..pieces.len() {
        if !used[p] {
            let v = pieces[p].0;
            keep[v as usize] = true;
            segs.push(walk(v, p as u32, &keep, &mut used));
        }
    }

    // Keep only junctions and dead ends, renumbered in order.
    let mut new_id = vec![u32::MAX; n];
    let mut g = FootGraph::default();
    for v in 0..n {
        if keep[v] {
            new_id[v] = g.coords.len() as u32;
            g.coords.push(coords[v]);
        }
    }
    g.shape_start.push(0);
    for (a, b, len, flags, shape) in segs {
        g.seg_a.push(new_id[a as usize]);
        g.seg_b.push(new_id[b as usize]);
        g.seg_len_m.push(len as f32);
        g.seg_kind.push(if flags & STEPS != 0 {
            EDGE_KIND_STEPS
        } else {
            EDGE_KIND_WALK
        });
        g.seg_oneway.push(flags & ONEWAY != 0);
        g.shape.extend(shape);
        g.shape_start.push(g.shape.len() as u32);
    }
    // Directed edges, node by node, in segment order.
    let m = g.coords.len();
    let mut out: Vec<Vec<(u32, u32)>> = vec![Vec::new(); m];
    for s in 0..g.seg_a.len() {
        let (a, b) = (g.seg_a[s], g.seg_b[s]);
        out[a as usize].push((b, s as u32));
        if !g.seg_oneway[s] {
            out[b as usize].push((a, s as u32));
        }
    }
    g.edges_start.push(0);
    for list in out {
        for (to, s) in list {
            g.edge_to.push(to);
            g.edge_len_m.push(g.seg_len_m[s as usize]);
            g.edge_kind.push(g.seg_kind[s as usize]);
            g.edge_seg.push(s);
        }
        g.edges_start.push(g.edge_to.len() as u32);
    }
    g
}
