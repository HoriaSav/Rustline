//! OpenStreetMap ingest: the walking street graph (milestone M4).
//!
//! [`build_foot_graph`] reads an `.osm.pbf` extract (optionally cut to a
//! [`BBox`]), keeps the ways people can walk on (spec §7.3), and returns a
//! [`FootGraph`]: nodes, and directed edges with their length in metres.
//! Chains of nodes with only two neighbours are merged into one edge that
//! keeps the street's shape, so the graph only has junctions and dead ends.
//!
//! Which ways are walkable:
//! - `highway` = footway, path, pedestrian, steps, living_street, residential,
//!   service, unclassified, tertiary, secondary, primary, track, crossing,
//!   platform, corridor, primary_link, secondary_link, tertiary_link
//!   (motorway, motorway_link, trunk, trunk_link need a `foot` tag);
//! - `foot=yes|designated|permissive` makes any `highway` walkable (also a
//!   motorway or a cycleway); `foot=no` excludes it;
//! - `access=private|no` excludes it unless `foot` allows it;
//! - `oneway:foot=yes` is one-way for walkers; plain `oneway=yes` is not;
//! - `highway=steps` edges are marked ([`EDGE_KIND_STEPS`]); `incline` is ignored.
//!
//! A way that references a node missing from the file (or outside the bbox)
//! is cut there: its pieces with at least two nodes are kept. A way is also
//! cut at a piece longer than [`MAX_PIECE_M`], and nodes beyond ±85° of
//! latitude are ignored (packs refuse both).

mod graph;
mod pbf;

pub use graph::{FootGraph, EDGE_KIND_STEPS, EDGE_KIND_WALK, MAX_PIECE_M};

use std::path::Path;

/// A rectangle in degrees (WGS84). Nodes outside it are ignored.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BBox {
    /// Southern edge.
    pub min_lat: f64,
    /// Western edge.
    pub min_lon: f64,
    /// Northern edge.
    pub max_lat: f64,
    /// Eastern edge.
    pub max_lon: f64,
}

impl BBox {
    /// True when `lat`/`lon` is inside (edges included).
    pub fn contains(&self, lat: f64, lon: f64) -> bool {
        (self.min_lat..=self.max_lat).contains(&lat) && (self.min_lon..=self.max_lon).contains(&lon)
    }
}

/// Caps on what one extract may make us hold in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsmLimits {
    /// Largest blob, packed or unpacked (the PBF spec's own maximum is 32 MiB).
    /// Also caps every list in a block (packed values, string table, a way's
    /// refs) at `max_blob_bytes / 64` items (at least 4 096).
    pub max_blob_bytes: u64,
    /// Most nodes kept (inside the bbox, with valid coordinates).
    pub max_nodes: u64,
    /// Most walkable ways kept.
    pub max_ways: u64,
}

impl Default for OsmLimits {
    /// 32 MiB blobs, 8 million nodes, 1 million walkable ways: a large
    /// city with margin (a 2 km bbox around București has about 2 million
    /// nodes) in a few hundred MB. A whole country hits the node cap.
    fn default() -> Self {
        OsmLimits {
            max_blob_bytes: 32 << 20,
            max_nodes: 8_000_000,
            max_ways: 1_000_000,
        }
    }
}

/// What the build read and dropped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OsmReport {
    /// Nodes in the file.
    pub nodes_read: u64,
    /// Nodes kept (inside the bbox, valid coordinates).
    pub nodes_kept: u64,
    /// Ways in the file.
    pub ways_read: u64,
    /// Walkable ways that gave at least one edge.
    pub ways_walkable: u64,
    /// Walkable ways skipped: fewer than two usable nodes.
    pub ways_skipped: u64,
    /// Walkable ways with no node inside the bbox (not counted as skipped).
    pub ways_outside_bbox: u64,
    /// Pieces between two consecutive nodes of a way longer than
    /// [`MAX_PIECE_M`] (dropped; the way is cut there).
    pub long_pieces: u64,
    /// References from walkable ways to nodes missing from the file (or
    /// outside the bbox, for ways that cross its edge).
    pub missing_node_refs: u64,
}

/// Why an extract could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OsmError {
    /// The file could not be opened or read.
    #[error("cannot read the OSM file: {0}")]
    Io(String),
    /// Not an `.osm.pbf` file, or a damaged one.
    #[error("not a valid OSM .osm.pbf file: {0}")]
    Corrupt(String),
    /// Valid PBF using something this reader does not support.
    #[error("unsupported OSM .osm.pbf feature: {0}")]
    Unsupported(String),
    /// The extract is bigger than [`OsmLimits`] allows.
    #[error("OSM extract is over the limit: {0} (use a smaller bbox or raise OsmLimits)")]
    Limit(String),
}

/// [`build_foot_graph_with_limits`] with the default [`OsmLimits`].
pub fn build_foot_graph(
    pbf: &Path,
    bbox: Option<BBox>,
) -> Result<(FootGraph, OsmReport), OsmError> {
    build_foot_graph_with_limits(pbf, bbox, &OsmLimits::default())
}

/// Reads `pbf` and builds the walking graph of the ways inside `bbox`
/// (all of the file when `None`).
///
/// An extract without walkable ways (or a bbox without streets) gives an
/// empty graph, not an error; callers decide what that means.
pub fn build_foot_graph_with_limits(
    pbf: &Path,
    bbox: Option<BBox>,
    limits: &OsmLimits,
) -> Result<(FootGraph, OsmReport), OsmError> {
    let mut b = graph::Builder::new(bbox, limits.clone());
    pbf::read(pbf, limits, &mut b)?;
    Ok(b.finish())
}
