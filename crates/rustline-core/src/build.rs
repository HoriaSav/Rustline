//! Adding the walking network to a pack at build time (M4): the street
//! graph, which stops it reaches, and the footpaths between stops.

use rustline_model::LatLon;
use rustline_pack::{TimetableData, SEG_ONEWAY, SEG_STEPS};

use crate::plan::{DETOUR_FACTOR, MAX_TRANSFER_M};
use crate::street::{haversine_m, Streets};

/// A stop farther than this from every street is "not linked": walks to it
/// are straight lines.
pub const LINK_MAX_M: f64 = 100.0;
/// Footpath times in the pack are at this speed.
const PACK_SPEED_MPS: f64 = 1.3;

/// A street graph as plain arrays (what `rustline_osm::FootGraph` holds).
#[derive(Debug, Clone, Copy)]
pub struct StreetInput<'a> {
    /// Node positions.
    pub nodes: &'a [LatLon],
    /// First node of each segment (a one-way segment runs a → b).
    pub seg_a: &'a [u32],
    /// Last node of each segment.
    pub seg_b: &'a [u32],
    /// Segment length in metres.
    pub seg_len_m: &'a [f32],
    /// True for `highway=steps`.
    pub seg_steps: &'a [bool],
    /// True for one-way (for walkers) segments.
    pub seg_oneway: &'a [bool],
    /// Segment s's shape points are `shape[shape_start[s]..shape_start[s + 1]]`.
    pub shape_start: &'a [u32],
    /// Shape points between the two nodes, segment by segment.
    pub shape: &'a [LatLon],
}

/// What [`add_walking`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WalkingReport {
    /// Street graph as given: nodes, segments, connected components.
    pub nodes_in: usize,
    pub segments_in: usize,
    pub components: usize,
    /// Components kept (those with at least one linked stop) and dropped.
    pub components_kept: usize,
    pub components_dropped: usize,
    /// Street graph stored in the pack.
    pub nodes: usize,
    pub segments: usize,
    /// Directed edges (two per segment, one for one-way).
    pub edges: usize,
    /// Stops with coordinates linked to a street (≤ 100 m) and not linked.
    pub stops_linked: usize,
    pub stops_unlinked: Vec<String>,
    /// Footpaths stored (each direction counts).
    pub footpaths: usize,
}

/// Stores the street graph in `tt` (keeping only the parts that reach a
/// stop) and computes footpaths of up to 500 m between stops: along the
/// streets between linked stops, straight lines × 1.3 otherwise (and for
/// every stop when `streets` is `None` or empty). Returns what it did, or
/// the text of what is wrong with the input.
pub fn add_walking(
    tt: &mut TimetableData,
    streets: Option<StreetInput<'_>>,
) -> Result<WalkingReport, String> {
    let mut r = WalkingReport::default();
    clear_streets(tt);
    if let Some(input) = streets.filter(|s| !s.seg_a.is_empty()) {
        check_input(&input)?;
        r.nodes_in = input.nodes.len();
        r.segments_in = input.seg_a.len();
        store(tt, &input, None);
        let index = Streets::new(tt, LINK_MAX_M)?.ok_or("street graph vanished")?;
        // Components (union-find over segments).
        let mut parent: Vec<u32> = (0..input.nodes.len() as u32).collect();
        fn root(p: &mut [u32], mut v: u32) -> u32 {
            while p[v as usize] != v {
                p[v as usize] = p[p[v as usize] as usize];
                v = p[v as usize];
            }
            v
        }
        for g in 0..input.seg_a.len() {
            let (a, b) = (
                root(&mut parent, input.seg_a[g]),
                root(&mut parent, input.seg_b[g]),
            );
            if a != b {
                parent[a.max(b) as usize] = a.min(b);
            }
        }
        let used: Vec<bool> = {
            let mut u = vec![false; input.nodes.len()];
            for g in 0..input.seg_a.len() {
                u[input.seg_a[g] as usize] = true;
                u[input.seg_b[g] as usize] = true;
            }
            u
        };
        let mut keep_root = vec![false; input.nodes.len()];
        for s in 0..tt.stop_count() {
            if let Some(sn) = index.stop_snap(s) {
                let a = input.seg_a[sn.seg as usize];
                let ra = root(&mut parent, a);
                keep_root[ra as usize] = true;
            }
        }
        let mut roots = 0;
        let mut kept = 0;
        for v in 0..input.nodes.len() as u32 {
            if used[v as usize] && root(&mut parent, v) == v {
                roots += 1;
                if keep_root[v as usize] {
                    kept += 1;
                }
            }
        }
        r.components = roots;
        r.components_kept = kept;
        r.components_dropped = roots - kept;
        let keep_seg: Vec<bool> = (0..input.seg_a.len())
            .map(|g| keep_root[root(&mut parent, input.seg_a[g]) as usize])
            .collect();
        drop(index);
        clear_streets(tt);
        store(tt, &input, Some(&keep_seg));
        if tt.seg_a.is_empty() {
            clear_streets(tt);
        }
    }
    let index = Streets::new(tt, LINK_MAX_M)?;
    if let Some(ix) = &index {
        r.nodes = ix.node_count();
        r.segments = ix.seg_count();
        r.edges = tt
            .seg_flags
            .iter()
            .map(|&f| if f & SEG_ONEWAY != 0 { 1 } else { 2 })
            .sum();
    }
    // Which stops are linked.
    let n = tt.stop_count();
    let pos: Vec<Option<LatLon>> = (0..n)
        .map(|s| {
            tt.stop_pos(s)
                .map(|(a, o)| LatLon::new(a, o))
                .filter(|p| p.is_valid())
        })
        .collect();
    let linked: Vec<bool> = (0..n)
        .map(|s| index.as_ref().is_some_and(|ix| ix.stop_snap(s).is_some()))
        .collect();
    for s in 0..n {
        if pos[s].is_some() && index.is_some() {
            if linked[s] {
                r.stops_linked += 1;
            } else {
                r.stops_unlinked.push(tt.stop_ids[s].clone());
            }
        }
    }
    // Footpaths: (from, to, cost in effective metres, metres).
    let max_cost = f64::from(MAX_TRANSFER_M);
    let mut paths: Vec<Vec<(u32, f64, f64)>> = vec![Vec::new(); n];
    if let Some(ix) = &index {
        for (s, list) in paths.iter_mut().enumerate() {
            let Some(sn) = ix.stop_snap(s) else { continue };
            let tree = ix.tree(&ix.leave(&sn), false, max_cost);
            for (y, c, m) in ix.stops_in(&tree, &sn, false, max_cost) {
                if y as usize != s && c <= max_cost {
                    list.push((y, c, m));
                }
            }
        }
    }
    // Straight lines for every pair with an unlinked stop (all pairs
    // without a street graph), swept in latitude order.
    let mut order: Vec<usize> = (0..n).filter(|&s| pos[s].is_some()).collect();
    order.sort_by(|&a, &b| {
        let (pa, pb) = (pos[a].map_or(0.0, |p| p.lat), pos[b].map_or(0.0, |p| p.lat));
        pa.total_cmp(&pb).then(a.cmp(&b))
    });
    let reach_deg = (max_cost / DETOUR_FACTOR / 6_371_008.8f64).to_degrees();
    for (i, &a) in order.iter().enumerate() {
        let pa = pos[a].unwrap_or(LatLon::new(0.0, 0.0));
        for &b in &order[i + 1..] {
            let pb = pos[b].unwrap_or(LatLon::new(0.0, 0.0));
            if pb.lat - pa.lat > reach_deg {
                break;
            }
            if linked[a] && linked[b] {
                continue;
            }
            let m = haversine_m(pa, pb) * DETOUR_FACTOR;
            if m <= max_cost {
                paths[a].push((b as u32, m, m));
                paths[b].push((a as u32, m, m));
            }
        }
    }
    tt.footpath_start = vec![0];
    for list in &mut paths {
        list.sort_by_key(|p| p.0);
        list.dedup_by_key(|p| p.0);
        for &(y, c, m) in list.iter() {
            tt.footpath_to.push(y);
            tt.footpath_secs.push((c / PACK_SPEED_MPS).round() as u32);
            tt.footpath_m.push(m.round() as u32);
        }
        tt.footpath_start.push(tt.footpath_to.len() as u32);
    }
    r.footpaths = tt.footpath_to.len();
    tt.check()?;
    Ok(r)
}

fn clear_streets(tt: &mut TimetableData) {
    for v in [
        &mut tt.street_lat,
        &mut tt.street_lon,
        &mut tt.shape_lat,
        &mut tt.shape_lon,
    ] {
        v.clear();
    }
    for v in [
        &mut tt.seg_a,
        &mut tt.seg_b,
        &mut tt.seg_len_cm,
        &mut tt.shape_start,
    ] {
        v.clear();
    }
    tt.seg_flags.clear();
    tt.footpath_start.clear();
    tt.footpath_to.clear();
    tt.footpath_secs.clear();
    tt.footpath_m.clear();
}

fn check_input(s: &StreetInput<'_>) -> Result<(), String> {
    let segs = s.seg_a.len();
    if [
        s.seg_b.len(),
        s.seg_len_m.len(),
        s.seg_steps.len(),
        s.seg_oneway.len(),
    ]
    .iter()
    .any(|&l| l != segs)
        || s.shape_start.len() != segs + 1
        || s.shape_start.first() != Some(&0)
        || s.shape_start.windows(2).any(|w| w[0] > w[1])
        || s.shape_start.last().map(|&e| e as usize) != Some(s.shape.len())
        || s.seg_a
            .iter()
            .chain(s.seg_b)
            .any(|&v| v as usize >= s.nodes.len())
        || s.nodes.iter().chain(s.shape).any(|p| !p.is_valid())
        || s.seg_len_m.iter().any(|l| !(l.is_finite() && *l >= 0.0))
    {
        return Err("street graph arrays do not fit together".to_string());
    }
    Ok(())
}

fn e7(v: f64) -> i32 {
    (v * 1e7).round() as i32
}

/// Writes the segments (`keep`: only those) and the nodes they use.
fn store(tt: &mut TimetableData, s: &StreetInput<'_>, keep: Option<&[bool]>) {
    let mut id = vec![u32::MAX; s.nodes.len()];
    let kept = |g: usize| keep.is_none_or(|k| k[g]);
    for g in (0..s.seg_a.len()).filter(|&g| kept(g)) {
        for v in [s.seg_a[g], s.seg_b[g]] {
            id[v as usize] = 0;
        }
    }
    for (v, i) in id.iter_mut().enumerate() {
        if *i == 0 {
            *i = tt.street_lat.len() as u32;
            tt.street_lat.push(e7(s.nodes[v].lat));
            tt.street_lon.push(e7(s.nodes[v].lon));
        }
    }
    tt.shape_start.push(0);
    for g in (0..s.seg_a.len()).filter(|&g| kept(g)) {
        tt.seg_a.push(id[s.seg_a[g] as usize]);
        tt.seg_b.push(id[s.seg_b[g] as usize]);
        tt.seg_len_cm
            .push((f64::from(s.seg_len_m[g]) * 100.0).round() as u32);
        let mut f = 0;
        if s.seg_steps[g] {
            f |= SEG_STEPS;
        }
        if s.seg_oneway[g] {
            f |= SEG_ONEWAY;
        }
        tt.seg_flags.push(f);
        for p in &s.shape[s.shape_start[g] as usize..s.shape_start[g + 1] as usize] {
            tt.shape_lat.push(e7(p.lat));
            tt.shape_lon.push(e7(p.lon));
        }
        tt.shape_start.push(tt.shape_lat.len() as u32);
    }
}
