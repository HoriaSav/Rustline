//! Walking on the pack's street graph (M4): snapping a point to the nearest
//! street, and shortest walks (Dijkstra / A*).
//!
//! Costs are "effective metres": metres walked, with steps counting ×1.5.
//! Dividing by the walking speed gives seconds. Limits (`max_m`) are on the
//! real metres walked.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use rustline_model::LatLon;
use rustline_pack::{TimetableData, SEG_ONEWAY, SEG_STEPS};

/// Mean Earth radius (IUGG), as in rustline-osm.
const EARTH_RADIUS_M: f64 = 6_371_008.8;
/// Walking on steps costs this much more.
pub(crate) const STEPS_FACTOR: f64 = 1.5;
/// Side of one grid cell of the snapping index.
const CELL_M: f64 = 200.0;
/// No street search goes further than this, whatever the request says.
pub const SEARCH_CAP_M: f64 = 10_000.0;
/// And settles at most this many nodes.
const MAX_SETTLED: usize = 500_000;
/// The snapping grid has at most this many cells (its cells grow instead).
const MAX_GRID_CELLS: usize = 1 << 20;
/// Grid entries allowed per segment piece, on average. Real streets need
/// about 1.2 (measured on two city packs: 316 354 entries for 269 694
/// pieces, 485 794 for 399 843): most pieces are much shorter than a cell.
const MAX_GRID_ENTRIES_PER_PIECE: usize = 32;
/// Most entries in one grid cell, else the pack is corrupt. Real packs: at
/// most 1 331 (measured on four city packs: 1 331, 844, 310, 280).
const MAX_CELL_ENTRIES: u32 = 8_192;
/// Most grid entries one snap may look at. A snap that needs more fails closed,
/// never a partial best: at load the pack is corrupt, at query time the point
/// is refused. A snap of up to 300 m covers at most 5 × 5 cells, so with the
/// cell cap an honest pack cannot reach it (worst stop on two city packs:
/// 2 854 entries within 100 m, 6 521 within 300 m).
const MAX_SNAP_SCAN: usize = 25 * MAX_CELL_ENTRIES as usize;
// `check_installable` relies on these (Reviewer MF1): a stop's link window is
// at most one cell wide, so it spans at most 3 × 3 cells, under the budget.
const _: () = assert!(2.0 * crate::LINK_MAX_M <= CELL_M);
const _: () = assert!(9 * MAX_CELL_ENTRIES as usize <= MAX_SNAP_SCAN);

/// Great-circle distance in metres.
pub(crate) fn haversine_m(a: LatLon, b: LatLon) -> f64 {
    let (la1, la2) = (a.lat.to_radians(), b.lat.to_radians());
    let h = ((la2 - la1) / 2.0).sin().powi(2)
        + la1.cos() * la2.cos() * ((b.lon - a.lon).to_radians() / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_M * h.sqrt().min(1.0).asin()
}

fn e7(v: i32) -> f64 {
    f64::from(v) * 1e-7
}

/// Where a point meets the street graph.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Snap {
    /// Segment.
    pub seg: u32,
    /// Metres from the segment's first node to the snapped point, along it.
    pub along: f64,
    /// Straight metres from the point to the street (walked too).
    pub dist: f64,
    /// The point on the street.
    pub point: LatLon,
}

/// A walk found on the graph.
#[derive(Debug, Clone)]
pub(crate) struct Walk {
    /// Effective metres (steps ×1.5); seconds = cost / speed.
    pub cost: f64,
    /// Real metres.
    pub metres: f64,
    /// The line walked, from the start point to the end point.
    pub points: Vec<LatLon>,
}

/// The street graph of a pack, with the indexes queries need.
pub(crate) struct Streets {
    node: Vec<LatLon>,
    seg_a: Vec<u32>,
    seg_b: Vec<u32>,
    seg_len: Vec<f64>,
    seg_flags: Vec<u8>,
    shape_start: Vec<u32>,
    shape: Vec<LatLon>,
    /// Edges leaving / reaching each node: (segment, runs a → b).
    out_start: Vec<u32>,
    out: Vec<(u32, bool)>,
    in_start: Vec<u32>,
    inc: Vec<(u32, bool)>,
    /// Grid of segment pieces (segment, piece index): cell (x, y) is
    /// `cells[cell_start[y * nx + x]..]`.
    grid: Grid,
    cell_start: Vec<u32>,
    cells: Vec<(u32, u32)>,
    /// Snap of each stop within the link distance (`None`: not linked).
    stop_snap: Vec<Option<Snap>>,
    /// Linked stops on each segment (CSR).
    seg_stops_start: Vec<u32>,
    seg_stops: Vec<u32>,
}

/// Heap entry ordered by smallest key first.
#[derive(PartialEq)]
struct Item(f64, u32);
impl Eq for Item {}
impl PartialOrd for Item {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Item {
    fn cmp(&self, o: &Self) -> Ordering {
        o.0.total_cmp(&self.0).then(o.1.cmp(&self.1))
    }
}

/// A shortest-path tree from (or, backwards, to) a few seed nodes.
pub(crate) struct Tree {
    /// node → (cost, metres, previous node, segment from it); `u32::MAX` for a seed.
    pub at: HashMap<u32, (f64, f64, u32, u32)>,
    /// Settled nodes in the order they were settled.
    pub settled: Vec<u32>,
}

impl Streets {
    /// Builds the indexes; `Ok(None)` when the pack has no street graph.
    /// `tt` passed `check()`, so every index in it is in range and every
    /// piece is at most 10 km; an index that would still be too big is an error.
    pub(crate) fn new(tt: &TimetableData, link_m: f64) -> Result<Option<Streets>, String> {
        if tt.seg_a.is_empty() {
            return Ok(None);
        }
        let node: Vec<LatLon> = tt
            .street_lat
            .iter()
            .zip(&tt.street_lon)
            .map(|(&a, &o)| LatLon::new(e7(a), e7(o)))
            .collect();
        let shape: Vec<LatLon> = tt
            .shape_lat
            .iter()
            .zip(&tt.shape_lon)
            .map(|(&a, &o)| LatLon::new(e7(a), e7(o)))
            .collect();
        let n = node.len();
        let segs = tt.seg_a.len();
        let mut s = Streets {
            node,
            seg_a: tt.seg_a.clone(),
            seg_b: tt.seg_b.clone(),
            seg_len: tt
                .seg_len_cm
                .iter()
                .map(|&c| f64::from(c) / 100.0)
                .collect(),
            seg_flags: tt.seg_flags.clone(),
            shape_start: tt.shape_start.clone(),
            shape,
            out_start: Vec::new(),
            out: Vec::new(),
            in_start: Vec::new(),
            inc: Vec::new(),
            grid: Grid::for_box([0.0; 4]),
            cell_start: Vec::new(),
            cells: Vec::new(),
            stop_snap: Vec::new(),
            seg_stops_start: Vec::new(),
            seg_stops: Vec::new(),
        };
        // Adjacency, both directions.
        let mut outs: Vec<Vec<(u32, bool)>> = vec![Vec::new(); n];
        let mut ins: Vec<Vec<(u32, bool)>> = vec![Vec::new(); n];
        for g in 0..segs {
            let (a, b) = (s.seg_a[g] as usize, s.seg_b[g] as usize);
            outs[a].push((g as u32, true));
            ins[b].push((g as u32, true));
            if !s.oneway(g as u32) {
                outs[b].push((g as u32, false));
                ins[a].push((g as u32, false));
            }
        }
        (s.out_start, s.out) = flatten(outs);
        (s.in_start, s.inc) = flatten(ins);
        s.build_grid()?;
        s.link_stops(tt, link_m, MAX_SNAP_SCAN)?;
        let mut on: Vec<Vec<u32>> = vec![Vec::new(); segs];
        for (i, sn) in s.stop_snap.iter().enumerate() {
            if let Some(sn) = sn {
                on[sn.seg as usize].push(i as u32);
            }
        }
        (s.seg_stops_start, s.seg_stops) = flatten(on);
        Ok(Some(s))
    }

    pub(crate) fn node_count(&self) -> usize {
        self.node.len()
    }

    pub(crate) fn seg_count(&self) -> usize {
        self.seg_a.len()
    }

    pub(crate) fn seg_ends(&self, g: u32) -> (u32, u32) {
        (self.seg_a[g as usize], self.seg_b[g as usize])
    }

    fn oneway(&self, g: u32) -> bool {
        self.seg_flags[g as usize] & SEG_ONEWAY != 0
    }

    fn factor(&self, g: u32) -> f64 {
        if self.seg_flags[g as usize] & SEG_STEPS != 0 {
            STEPS_FACTOR
        } else {
            1.0
        }
    }

    /// The stop's snap, if it is linked to the streets.
    pub(crate) fn stop_snap(&self, stop: usize) -> Option<Snap> {
        self.stop_snap.get(stop).copied().flatten()
    }

    /// All points of segment g, from a to b.
    fn seg_points(&self, g: u32) -> Vec<LatLon> {
        let g = g as usize;
        let r = self.shape_start[g] as usize..self.shape_start[g + 1] as usize;
        let mut p = Vec::with_capacity(r.len() + 2);
        p.push(self.node[self.seg_a[g] as usize]);
        p.extend_from_slice(&self.shape[r]);
        p.push(self.node[self.seg_b[g] as usize]);
        p
    }

    /// Indexes every segment piece in the grid cells its line crosses.
    /// `check()` bounds pieces to 10 km, so a piece crosses a bounded number
    /// of cells; the total is capped anyway ([`MAX_GRID_ENTRIES_PER_PIECE`]).
    /// Snaps every stop; a stop whose snap would scan more than `budget`
    /// grid entries makes the pack corrupt.
    fn link_stops(&mut self, tt: &TimetableData, link_m: f64, budget: usize) -> Result<(), String> {
        let mut snaps = Vec::with_capacity(tt.stop_count());
        for i in 0..tt.stop_count() {
            let sn = match tt.stop_pos(i) {
                Some((lat, lon)) => self.try_snap(LatLon::new(lat, lon), link_m, budget).map_err(|n| {
                    format!("snapping stop {i} would scan {n} street grid entries (more than {budget}): streets piled up near it")
                })?,
                None => None,
            };
            snaps.push(sn);
        }
        self.stop_snap = snaps;
        Ok(())
    }

    fn build_grid(&mut self) -> Result<(), String> {
        let (mut lo_lat, mut hi_lat, mut lo_lon, mut hi_lon) =
            (90.0f64, -90.0f64, 180.0f64, -180.0f64);
        for p in self.node.iter().chain(&self.shape) {
            lo_lat = lo_lat.min(p.lat);
            hi_lat = hi_lat.max(p.lat);
            lo_lon = lo_lon.min(p.lon);
            hi_lon = hi_lon.max(p.lon);
        }
        self.grid = Grid::for_box([lo_lat, hi_lat, lo_lon, hi_lon]);
        let mut counter = Counter::new(&self.grid, self.shape.len() + self.seg_a.len());
        'all: for g in 0..self.seg_a.len() as u32 {
            for i in 0..self.piece_count(g) {
                let (a, b) = self.piece(g, i);
                if !counter.add(a, b) {
                    break 'all;
                }
            }
        }
        let (mut count, total) = counter.finish()?;
        let cells = self.grid.nx * self.grid.ny;
        for c in 0..cells {
            count[c + 1] += count[c];
        }
        let mut fill = count.clone();
        let mut entries = vec![(0u32, 0u32); total];
        for g in 0..self.seg_a.len() as u32 {
            for i in 0..self.piece_count(g) {
                let (a, b) = self.piece(g, i);
                self.grid.cells_on_line(a, b, |c| {
                    entries[fill[c] as usize] = (g, i);
                    fill[c] += 1;
                });
            }
        }
        self.cell_start = count;
        self.cells = entries;
        Ok(())
    }

    /// Number of straight pieces of segment g.
    fn piece_count(&self, g: u32) -> u32 {
        self.shape_start[g as usize + 1] - self.shape_start[g as usize] + 1
    }

    /// Piece i of segment g: its two end points.
    fn piece(&self, g: u32, i: u32) -> (LatLon, LatLon) {
        let gi = g as usize;
        let s = self.shape_start[gi] as usize;
        let n = self.piece_count(g);
        let at = |j: u32| {
            if j == 0 {
                self.node[self.seg_a[gi] as usize]
            } else if j == n {
                self.node[self.seg_b[gi] as usize]
            } else {
                self.shape[s + j as usize - 1]
            }
        };
        (at(i), at(i + 1))
    }

    /// The nearest street point within `max_m` (inclusive). Ties go to the
    /// lower segment index, so the answer is always the same.
    /// A snap that would scan more than `MAX_SNAP_SCAN` grid entries is
    /// refused (`None`, with a warning on stderr), never answered partly.
    pub(crate) fn snap(&self, p: LatLon, max_m: f64) -> Option<Snap> {
        match self.try_snap(p, max_m, MAX_SNAP_SCAN) {
            Ok(sn) => sn,
            Err(n) => {
                eprintln!(
                    "warning: refusing the point ({:.6}, {:.6}): snapping it would scan {n} street grid entries (more than {MAX_SNAP_SCAN}); the pack's streets are piled up there",
                    p.lat, p.lon
                );
                None
            }
        }
    }

    /// `snap`, or `Err(entries)` when the cells within `max_m` hold more than
    /// `budget` entries (counted before scanning any of them).
    fn try_snap(&self, p: LatLon, max_m: f64, budget: usize) -> Result<Option<Snap>, usize> {
        if !p.is_valid() {
            return Ok(None);
        }
        let r_lat = (max_m / EARTH_RADIUS_M).to_degrees();
        let r_lon = r_lat / p.lat.to_radians().cos().max(1e-6);
        let fx = |lon: f64| ((lon - self.grid.lon0) / self.grid.dlon).floor();
        let fy = |lat: f64| ((lat - self.grid.lat0) / self.grid.dlat).floor();
        let (x0, x1) = (fx(p.lon - r_lon).max(0.0), fx(p.lon + r_lon));
        let (y0, y1) = (fy(p.lat - r_lat).max(0.0), fy(p.lat + r_lat));
        if x1 < 0.0 || y1 < 0.0 || x0 >= self.grid.nx as f64 || y0 >= self.grid.ny as f64 {
            return Ok(None);
        }
        let (x0, y0) = (x0 as usize, y0 as usize);
        let x1 = (x1 as usize).min(self.grid.nx - 1);
        let y1 = (y1 as usize).min(self.grid.ny - 1);
        let mut best: Option<(f64, u32, u32, f64, LatLon)> = None;
        let k = p.lat.to_radians().cos();
        let xy = |q: LatLon| {
            (
                (q.lon - p.lon).to_radians() * k * EARTH_RADIUS_M,
                (q.lat - p.lat).to_radians() * EARTH_RADIUS_M,
            )
        };
        let rows = (y0..=y1).map(|y| {
            let c = y * self.grid.nx;
            self.cell_start[c + x0] as usize..self.cell_start[c + x1 + 1] as usize
        });
        // Saturating: a tampered cell_start cannot wrap the count below the budget.
        let entries = rows.clone().fold(0usize, |n, r| n.saturating_add(r.len()));
        if entries > budget {
            return Err(entries);
        }
        for &(g, i) in rows.flat_map(|r| &self.cells[r]) {
            let (a, b) = self.piece(g, i);
            let ((ax, ay), (bx, by)) = (xy(a), xy(b));
            let (dx, dy) = (bx - ax, by - ay);
            let l2 = dx * dx + dy * dy;
            let t = if l2 > 0.0 {
                (-(ax * dx + ay * dy) / l2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let (cx, cy) = (ax + t * dx, ay + t * dy);
            let d = (cx * cx + cy * cy).sqrt();
            if d > max_m {
                continue;
            }
            let key = (d, g, i);
            if best.is_none_or(|b| key < (b.0, b.1, b.2)) {
                let q = LatLon::new(a.lat + t * (b.lat - a.lat), a.lon + t * (b.lon - a.lon));
                best = Some((d, g, i, t, q));
            }
        }
        let Some((dist, seg, piece, _, point)) = best else {
            return Ok(None);
        };
        let pts = self.seg_points(seg);
        let mut along: f64 = pts
            .windows(2)
            .take(piece as usize)
            .map(|w| haversine_m(w[0], w[1]))
            .sum();
        along += haversine_m(pts[piece as usize], point);
        let along = along.clamp(0.0, self.seg_len[seg as usize]);
        Ok(Some(Snap {
            seg,
            along,
            dist,
            point,
        }))
    }

    /// Walking from the snapped point onto the graph: (node, cost, metres).
    pub(crate) fn leave(&self, s: &Snap) -> Vec<(u32, f64, f64)> {
        let (a, b) = self.seg_ends(s.seg);
        let (f, len) = (self.factor(s.seg), self.seg_len[s.seg as usize]);
        let mut v = vec![(b, s.dist + (len - s.along) * f, s.dist + len - s.along)];
        if !self.oneway(s.seg) {
            v.push((a, s.dist + s.along * f, s.dist + s.along));
        }
        v
    }

    /// Walking from the graph to the snapped point: (node, cost, metres).
    pub(crate) fn reach(&self, s: &Snap) -> Vec<(u32, f64, f64)> {
        let (a, b) = self.seg_ends(s.seg);
        let (f, len) = (self.factor(s.seg), self.seg_len[s.seg as usize]);
        let mut v = vec![(a, s.dist + s.along * f, s.dist + s.along)];
        if !self.oneway(s.seg) {
            v.push((b, s.dist + (len - s.along) * f, s.dist + len - s.along));
        }
        v
    }

    /// Walking from `o` to `t` along their common segment, if they share one.
    fn direct(&self, o: &Snap, t: &Snap) -> Option<(f64, f64)> {
        if o.seg != t.seg || (t.along < o.along && self.oneway(o.seg)) {
            return None;
        }
        let m = (t.along - o.along).abs();
        Some((
            o.dist + t.dist + m * self.factor(o.seg),
            o.dist + t.dist + m,
        ))
    }

    /// Dijkstra from `seeds` (forward) or to them (`backward`), up to
    /// `max_m` real metres.
    pub(crate) fn tree(&self, seeds: &[(u32, f64, f64)], backward: bool, max_m: f64) -> Tree {
        let max_m = max_m.min(SEARCH_CAP_M);
        let mut t = Tree {
            at: HashMap::new(),
            settled: Vec::new(),
        };
        let mut heap = BinaryHeap::new();
        for &(v, c, m) in seeds {
            if m <= max_m && t.at.get(&v).is_none_or(|x| c < x.0) {
                t.at.insert(v, (c, m, u32::MAX, u32::MAX));
                heap.push(Item(c, v));
            }
        }
        let mut done = std::collections::HashSet::new();
        while let Some(Item(c, u)) = heap.pop() {
            if !done.insert(u) {
                continue;
            }
            t.settled.push(u);
            if t.settled.len() >= MAX_SETTLED {
                break;
            }
            let m = t.at[&u].1;
            let (start, list) = if backward {
                (&self.in_start, &self.inc)
            } else {
                (&self.out_start, &self.out)
            };
            for &(g, fwd) in &list[start[u as usize] as usize..start[u as usize + 1] as usize] {
                let (a, b) = self.seg_ends(g);
                // Forward edge u → v; backward: v → u reaches u.
                let v = if fwd != backward { b } else { a };
                let len = self.seg_len[g as usize];
                let (c2, m2) = (c + len * self.factor(g), m + len);
                if m2 <= max_m && t.at.get(&v).is_none_or(|x| c2 < x.0) {
                    t.at.insert(v, (c2, m2, u, g));
                    heap.push(Item(c2, v));
                }
            }
        }
        t
    }

    /// Linked stops reachable in a forward tree from `from` (or, for a
    /// backward tree to `from`, stops that reach it): (stop, cost, metres),
    /// best per stop, up to `max_m`.
    pub(crate) fn stops_in(
        &self,
        t: &Tree,
        from: &Snap,
        backward: bool,
        max_m: f64,
    ) -> Vec<(u32, f64, f64)> {
        let mut best: HashMap<u32, (f64, f64)> = HashMap::new();
        let mut order = Vec::new();
        let mut offer = |s: u32, c: f64, m: f64| {
            if m > max_m {
                return;
            }
            match best.get_mut(&s) {
                Some(x) if c < x.0 => *x = (c, m),
                Some(_) => {}
                None => {
                    best.insert(s, (c, m));
                    order.push(s);
                }
            }
        };
        let on = |g: u32| {
            &self.seg_stops[self.seg_stops_start[g as usize] as usize
                ..self.seg_stops_start[g as usize + 1] as usize]
        };
        // Stops on the start segment itself.
        for &s in on(from.seg) {
            let sn = self.stop_snap[s as usize].expect("linked stop");
            let d = if backward {
                self.direct(&sn, from)
            } else {
                self.direct(from, &sn)
            };
            if let Some((c, m)) = d {
                offer(s, c, m);
            }
        }
        for &u in &t.settled {
            let (c0, m0, ..) = t.at[&u];
            let (start, list) = if backward {
                (&self.in_start, &self.inc)
            } else {
                (&self.out_start, &self.out)
            };
            for &(g, _) in &list[start[u as usize] as usize..start[u as usize + 1] as usize] {
                for &s in on(g) {
                    let sn = self.stop_snap[s as usize].expect("linked stop");
                    let ends = if backward {
                        self.leave(&sn)
                    } else {
                        self.reach(&sn)
                    };
                    for (v, c, m) in ends {
                        if v == u {
                            offer(s, c0 + c, m0 + m);
                        }
                    }
                }
            }
        }
        order.sort_unstable();
        order
            .into_iter()
            .map(|s| (s, best[&s].0, best[&s].1))
            .collect()
    }

    /// Shortest walk from `o` (at point `op`) to `t` (at `tp`) with at most
    /// `max_m` real metres (A* with the straight-line distance).
    pub(crate) fn walk(
        &self,
        op: LatLon,
        o: &Snap,
        tp: LatLon,
        t: &Snap,
        max_m: f64,
    ) -> Option<Walk> {
        let max_m = max_m.min(SEARCH_CAP_M);
        let ends = self.reach(t);
        // (cost, metres, last node before the end; None: along one segment)
        let mut best: Option<(f64, f64, Option<u32>)> = self
            .direct(o, t)
            .filter(|d| d.1 <= max_m)
            .map(|(c, m)| (c, m, None));
        let mut at: HashMap<u32, (f64, f64, u32, u32)> = HashMap::new();
        let mut heap = BinaryHeap::new();
        let h = |v: u32| haversine_m(self.node[v as usize], t.point);
        for (v, c, m) in self.leave(o) {
            if m <= max_m && at.get(&v).is_none_or(|x| c < x.0) {
                at.insert(v, (c, m, u32::MAX, u32::MAX));
                heap.push(Item(c + h(v), v));
            }
        }
        let mut done = std::collections::HashSet::new();
        while let Some(Item(f, u)) = heap.pop() {
            // The heuristic never overestimates: nothing left can beat `best`.
            if best.is_some_and(|b| f >= b.0) || done.len() >= MAX_SETTLED {
                break;
            }
            if !done.insert(u) {
                continue;
            }
            let (c, m, ..) = at[&u];
            for &(v, ec, em) in &ends {
                if v == u && m + em <= max_m && best.is_none_or(|b| c + ec < b.0) {
                    best = Some((c + ec, m + em, Some(u)));
                }
            }
            for &(g, fwd) in &self.out
                [self.out_start[u as usize] as usize..self.out_start[u as usize + 1] as usize]
            {
                let (a, b) = self.seg_ends(g);
                let v = if fwd { b } else { a };
                let len = self.seg_len[g as usize];
                let (c2, m2) = (c + len * self.factor(g), m + len);
                if m2 <= max_m && at.get(&v).is_none_or(|x| c2 < x.0) {
                    at.insert(v, (c2, m2, u, g));
                    heap.push(Item(c2 + h(v), v));
                }
            }
        }
        let (cost, metres, last) = best?;
        // The line: start point, along the street, end point.
        let mut points = vec![op, o.point];
        match last {
            None => points.extend(self.cut(o.seg, o.along, t.along)),
            Some(end) => {
                let mut chain = Vec::new(); // (node, segment into it)
                let mut v = end;
                loop {
                    let (_, _, prev, g) = at[&v];
                    if prev == u32::MAX {
                        break;
                    }
                    chain.push((v, prev, g));
                    v = prev;
                }
                // v is the first node: walk from o.point to it.
                let (a, _) = self.seg_ends(o.seg);
                let to_first = if v == a {
                    0.0
                } else {
                    self.seg_len[o.seg as usize]
                };
                points.extend(self.cut(o.seg, o.along, to_first));
                for &(to, from, g) in chain.iter().rev() {
                    let (a, _) = self.seg_ends(g);
                    let len = self.seg_len[g as usize];
                    let (x, y) = if from == a && to != a {
                        (0.0, len)
                    } else {
                        (len, 0.0)
                    };
                    points.extend(self.cut(g, x, y));
                }
                let (a, _) = self.seg_ends(t.seg);
                let from_last = if end == a {
                    0.0
                } else {
                    self.seg_len[t.seg as usize]
                };
                points.extend(self.cut(t.seg, from_last, t.along));
            }
        }
        points.push(t.point);
        points.push(tp);
        points.dedup();
        Some(Walk {
            cost,
            metres,
            points,
        })
    }

    /// Points of segment g from position `x` to position `y` (metres from a),
    /// in that direction, both ends included.
    fn cut(&self, g: u32, x: f64, y: f64) -> Vec<LatLon> {
        let pts = self.seg_points(g);
        let mut cum = vec![0.0];
        for w in pts.windows(2) {
            cum.push(cum.last().copied().unwrap_or(0.0) + haversine_m(w[0], w[1]));
        }
        // Positions use the stored length; scale the shape's own length to it.
        let total = cum.last().copied().unwrap_or(0.0);
        let len = self.seg_len[g as usize];
        let k = if len > 0.0 { total / len } else { 0.0 };
        let at = |d: f64| {
            let d = (d * k).clamp(0.0, total);
            let i = cum.partition_point(|&c| c <= d).clamp(1, pts.len() - 1);
            let span = cum[i] - cum[i - 1];
            let t = if span > 0.0 {
                (d - cum[i - 1]) / span
            } else {
                0.0
            };
            let (a, b) = (pts[i - 1], pts[i]);
            LatLon::new(a.lat + t * (b.lat - a.lat), a.lon + t * (b.lon - a.lon))
        };
        let (lo, hi) = (x.min(y) * k, x.max(y) * k);
        let mut v = vec![at(x.min(y))];
        for (i, &c) in cum.iter().enumerate() {
            if c > lo && c < hi {
                v.push(pts[i]);
            }
        }
        v.push(at(x.max(y)));
        if y < x {
            v.reverse();
        }
        v
    }
}

/// The snapping grid's place and size. One rule for [`Streets::new`] and for
/// install ([`check_street_grid`]): 200 m cells, grown until the grid has at
/// most [`MAX_GRID_CELLS`] cells.
pub(crate) struct Grid {
    lat0: f64,
    lon0: f64,
    dlat: f64,
    dlon: f64,
    nx: usize,
    ny: usize,
}

impl Grid {
    /// The grid over `[lo_lat, hi_lat, lo_lon, hi_lon]` (degrees).
    pub(crate) fn for_box([lo_lat, hi_lat, lo_lon, hi_lon]: [f64; 4]) -> Grid {
        let k = ((lo_lat.abs().max(hi_lat.abs())).min(89.0))
            .to_radians()
            .cos();
        let mut cell = CELL_M;
        // Keep the grid at most MAX_GRID_CELLS cells, whatever the extent.
        loop {
            let dlat = (cell / EARTH_RADIUS_M).to_degrees();
            let dlon = dlat / k;
            // range: both spans are ≥ 0 and finite (coordinates are checked), the casts saturate.
            let nx = ((hi_lon - lo_lon) / dlon) as usize + 1;
            let ny = ((hi_lat - lo_lat) / dlat) as usize + 1;
            if nx.saturating_mul(ny) <= MAX_GRID_CELLS {
                return Grid {
                    lat0: lo_lat,
                    lon0: lo_lon,
                    dlat,
                    dlon,
                    nx,
                    ny,
                };
            }
            cell *= 2.0;
        }
    }

    /// Calls `f` with every grid cell the straight line a–b passes through
    /// (a grid walk in cell units; a line through a corner also gets one of
    /// the two side cells, which is harmless).
    fn cells_on_line(&self, a: LatLon, b: LatLon, mut f: impl FnMut(usize)) {
        let u = |p: LatLon| (p.lon - self.lon0) / self.dlon;
        let v = |p: LatLon| (p.lat - self.lat0) / self.dlat;
        let (u0, v0, u1, v1) = (u(a), v(a), u(b), v(b));
        let (xa, ya) = self.cell_of(a);
        let (xb, yb) = self.cell_of(b);
        let (mut x, mut y) = (xa, ya);
        let (du, dv) = (u1 - u0, v1 - v0);
        let next = |c: usize, d: f64, p0: f64| {
            if d > 0.0 {
                ((c + 1) as f64 - p0) / d
            } else if d < 0.0 {
                (c as f64 - p0) / d
            } else {
                f64::INFINITY
            }
        };
        let (mut tx, mut ty) = (next(x, du, u0), next(y, dv, v0));
        let (sx, sy) = (1.0 / du.abs(), 1.0 / dv.abs());
        // At most one step per cell boundary crossed: the walk always ends.
        for _ in 0..xa.abs_diff(xb) + ya.abs_diff(yb) {
            f(y * self.nx + x);
            if (tx < ty && x != xb) || y == yb {
                x = if xb > x { x + 1 } else { x - 1 };
                tx += sx;
            } else {
                y = if yb > y { y + 1 } else { y - 1 };
                ty += sy;
            }
        }
        f(yb * self.nx + xb);
    }

    fn cell_of(&self, p: LatLon) -> (usize, usize) {
        let x = ((p.lon - self.lon0) / self.dlon).max(0.0) as usize;
        let y = ((p.lat - self.lat0) / self.dlat).max(0.0) as usize;
        (x.min(self.nx - 1), y.min(self.ny - 1))
    }
}

/// Counts grid entries cell by cell, piece after piece, and applies the size
/// rules: at most [`MAX_GRID_ENTRIES_PER_PIECE`] per piece on average (at
/// least 4096 in all) and [`MAX_CELL_ENTRIES`] per cell.
pub(crate) struct Counter<'g> {
    grid: &'g Grid,
    cap: usize,
    total: usize,
    count: Vec<u32>,
    over: bool,
}

impl<'g> Counter<'g> {
    /// A counter for `pieces` segment pieces on `grid`.
    pub(crate) fn new(grid: &'g Grid, pieces: usize) -> Counter<'g> {
        Counter {
            grid,
            cap: (MAX_GRID_ENTRIES_PER_PIECE * pieces).max(4096),
            total: 0,
            count: vec![0u32; grid.nx * grid.ny + 1],
            over: false,
        }
    }

    /// Adds the piece a–b; false once the entry cap is passed (stop then).
    pub(crate) fn add(&mut self, a: LatLon, b: LatLon) -> bool {
        let (total, cap, over, count) =
            (&mut self.total, self.cap, &mut self.over, &mut self.count);
        self.grid.cells_on_line(a, b, |c| {
            *total += 1;
            *over |= *total > cap;
            if !*over {
                count[c + 1] += 1;
            }
        });
        !self.over
    }

    /// The per-cell counts (cell c at c + 1) and the total, or why the grid is too big.
    pub(crate) fn finish(self) -> Result<(Vec<u32>, usize), String> {
        if self.over {
            return Err(format!(
                "street grid needs more than {} entries ({MAX_GRID_ENTRIES_PER_PIECE} per segment piece)",
                self.cap
            ));
        }
        if let Some(n) = self.count.iter().copied().find(|&n| n > MAX_CELL_ENTRIES) {
            return Err(format!(
                "street grid cell has {n} entries (more than {MAX_CELL_ENTRIES}): streets piled up in one spot"
            ));
        }
        Ok((self.count, self.total))
    }
}

/// The street-grid rules of [`Streets::new`] for a pack checked on disk: the
/// same box, grid and counts, read through the disk page cache.
pub(crate) fn check_disk_grid(s: &rustline_pack::DiskStreets) -> Result<(), String> {
    if s.segments() == 0 {
        return Ok(());
    }
    let b = s.bbox_e7().map_err(|e| e.to_string())?;
    // e7 is increasing, so the min/max of e7 values is e7 of the min/max.
    let grid = Grid::for_box([e7(b[0]), e7(b[1]), e7(b[2]), e7(b[3])]);
    let mut counter = Counter::new(&grid, s.pieces());
    let pt = |(lat, lon): (i32, i32)| LatLon::new(e7(lat), e7(lon));
    s.each_piece(|a, b| counter.add(pt(a), pt(b)))
        .map_err(|e| e.to_string())?;
    counter.finish().map(|_| ())
}

/// Lists → (CSR start, items).
fn flatten<T: Copy>(lists: Vec<Vec<T>>) -> (Vec<u32>, Vec<T>) {
    let mut start = Vec::with_capacity(lists.len() + 1);
    let mut items = Vec::new();
    start.push(0);
    for l in lists {
        items.extend(l);
        start.push(items.len() as u32);
    }
    (start, items)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reviewer MF3: 5 × 5 cells of `MAX_CELL_ENTRIES` short streets each, all
    /// within 400 m of `p`; the only street through `p` is in the middle cell.
    /// Two tiny anchor streets fix the grid to 7 × 7 cells. Returns the data,
    /// `p` and the index of the street through `p`.
    fn full_window() -> (TimetableData, LatLon, u32) {
        let dlat = (CELL_M / EARTH_RADIUS_M).to_degrees();
        let (lat0, lon0) = (45.0, 25.0);
        let hi = lat0 + 6.5 * dlat;
        let dlon = dlat / hi.to_radians().cos();
        let at = |y: f64, x: f64| LatLon::new(lat0 + y * dlat, lon0 + x * dlon);
        let mut tt = TimetableData::default();
        let street = |tt: &mut TimetableData, a: LatLon, b: LatLon, n: u32| {
            let i = tt.street_lat.len() as u32;
            for q in [a, b] {
                tt.street_lat.push((q.lat * 1e7).round() as i32);
                tt.street_lon.push((q.lon * 1e7).round() as i32);
            }
            for _ in 0..n {
                tt.seg_a.push(i);
                tt.seg_b.push(i + 1);
                tt.seg_len_cm
                    .push((haversine_m(a, b) * 100.0).round() as u32);
                tt.seg_flags.push(0);
            }
        };
        street(&mut tt, at(0.0, 0.0), at(0.01, 0.0), 1);
        street(&mut tt, at(6.5, 6.5), at(6.49, 6.5), 1);
        let n = MAX_CELL_ENTRIES;
        let mut through_p = 0;
        for y in 1..=5 {
            for x in 1..=5 {
                let (y, x) = (f64::from(y), f64::from(x));
                if (y, x) == (3.0, 3.0) {
                    street(&mut tt, at(3.2, 3.2), at(3.21, 3.2), n - 1);
                    through_p = tt.seg_a.len() as u32;
                    street(&mut tt, at(3.49, 3.5), at(3.51, 3.5), 1);
                } else {
                    street(&mut tt, at(y + 0.5, x + 0.5), at(y + 0.51, x + 0.5), n);
                }
            }
        }
        tt.shape_start = vec![0; tt.seg_a.len() + 1];
        (tt, at(3.5, 3.5), through_p)
    }

    #[test]
    fn a_full_5x5_window_snaps_exactly_and_never_partly() {
        let (tt, p, through_p) = full_window();
        let s = Streets::new(&tt, 100.0).unwrap().unwrap();
        // The whole window fits the scan budget: the true street, 0 m away
        // (before the fix the scan stopped early and answered a far street).
        let sn = s.snap(p, 400.0).unwrap();
        assert_eq!(sn.seg, through_p);
        assert!(sn.dist < 0.01, "{}", sn.dist);
        let all = 25 * MAX_CELL_ENTRIES as usize;
        assert_eq!(all, MAX_SNAP_SCAN);
        // One entry less of budget: refused, not answered from part of it.
        assert_eq!(s.try_snap(p, 400.0, MAX_SNAP_SCAN - 1).err(), Some(all));
        // 600 m also covers the two anchor cells: over the budget, refused.
        assert!(s.snap(p, 600.0).is_none());
    }

    #[test]
    fn a_stop_whose_snap_is_over_the_budget_makes_the_pack_corrupt() {
        let (mut tt, p, through_p) = full_window();
        tt.stop_ids = vec!["s".into()];
        tt.stop_lat = vec![p.lat.to_bits()];
        tt.stop_lon = vec![p.lon.to_bits()];
        let mut s = Streets::new(&tt, 100.0).unwrap().unwrap();
        assert_eq!(s.stop_snap(0).unwrap().seg, through_p);
        let e = s.link_stops(&tt, 100.0, 1_000).unwrap_err();
        assert!(e.contains("snapping stop 0 would scan"), "{e}");
    }
}
