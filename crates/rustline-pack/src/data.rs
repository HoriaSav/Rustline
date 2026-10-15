//! The timetable as flat arrays, ready to be saved.
//!
//! "CSR" (compressed sparse row) is used for every one-to-many link: one big
//! array plus a `start` array, so the items of `p` are
//! `items[start[p]..start[p + 1]]`. No pointers, no `HashMap`s, only fixed-size
//! unsigned numbers (`u8`/`u16`/`u32`/`u64`, never `usize`, which differs between
//! 32- and 64-bit phones) and strings.

use rkyv::{Archive, Deserialize, Serialize};

/// Marks a stop without coordinates in `stop_lat`/`stop_lon` (bits of `f64::NAN`).
pub const NO_COORD: u64 = 0x7FF8_0000_0000_0000;

/// [`TimetableData::seg_flags`] bit: `highway=steps` (walking costs ×1.5).
pub const SEG_STEPS: u8 = 1;
/// [`TimetableData::seg_flags`] bit: walkable from a to b only (`oneway:foot=yes`).
pub const SEG_ONEWAY: u8 = 2;
/// Longest straight piece of a street segment (node or shape point to the
/// next one). Real streets have pieces of a few km at most; the bound keeps
/// what a loader derives from the geometry (the snapping grid) small.
pub const STREET_MAX_PIECE_M: f64 = 10_000.0;
/// Street nodes and shape points must lie within ±85° of latitude (near the
/// poles a degree of longitude shrinks to nothing).
pub const STREET_MAX_LAT: f64 = 85.0;
/// `seg_len_cm` must be within this fraction (plus 1 m) of the length of the
/// segment's geometry.
pub const SEG_LEN_TOLERANCE: f64 = 0.01;

/// Great-circle distance in metres between two points in 1e-7 degrees.
fn haversine_e7(a: (i32, i32), b: (i32, i32)) -> f64 {
    let d = |v: i32| f64::from(v) * 1e-7;
    let (la1, la2) = (d(a.0).to_radians(), d(b.0).to_radians());
    let h = ((la2 - la1) / 2.0).sin().powi(2)
        + la1.cos() * la2.cos() * ((d(b.1) - d(a.1)).to_radians() / 2.0).sin().powi(2);
    2.0 * 6_371_008.8 * h.sqrt().min(1.0).asin()
}

/// Everything the router needs: the timetable and (M4) the walking network.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct TimetableData {
    // ---- stops (all locations from stops.txt, sorted by GTFS id) ----
    /// GTFS `stop_id`.
    pub stop_ids: Vec<String>,
    /// `stop_name`.
    pub stop_names: Vec<String>,
    /// Latitude as `f64::to_bits`, [`NO_COORD`] when the feed has none.
    pub stop_lat: Vec<u64>,
    /// Longitude as `f64::to_bits`, [`NO_COORD`] when the feed has none.
    pub stop_lon: Vec<u64>,
    /// 0 = stop/platform, 1 = station, 2 = other.
    pub stop_kind: Vec<u8>,
    /// Parent station index, `u32::MAX` when there is none.
    pub stop_parent: Vec<u32>,

    // ---- routes ----
    /// GTFS `route_id`.
    pub route_ids: Vec<String>,
    /// `route_short_name`.
    pub route_short_names: Vec<String>,
    /// `route_long_name`.
    pub route_long_names: Vec<String>,
    /// GTFS `route_type` (always ≥ 0 in GTFS).
    pub route_types: Vec<u16>,

    // ---- trips ----
    /// GTFS `trip_id`.
    pub trip_ids: Vec<String>,
    /// Route index of each trip.
    pub trip_route: Vec<u32>,
    /// Service index of each trip.
    pub trip_service: Vec<u32>,
    /// `trip_headsign`.
    pub trip_headsigns: Vec<String>,
    /// 1 for trips made from a headway-only frequency, else 0.
    pub trip_approximate: Vec<u8>,
    /// Pattern index of each trip.
    pub trip_pattern: Vec<u32>,
    /// Where the trip's times start in `stop_times`.
    pub trip_times_start: Vec<u32>,
    /// GTFS `block_id` of each trip ("" = none). Empty array = no blocks.
    /// Stored for later; the router does not use blocks yet.
    pub trip_block_ids: Vec<String>,

    // ---- patterns (CSR) ----
    /// Pattern p's stops are `pattern_stops[pattern_stops_start[p]..[p + 1]]`.
    pub pattern_stops_start: Vec<u32>,
    /// Stop indices.
    pub pattern_stops: Vec<u32>,
    /// Pattern p's trips (earliest first) are `pattern_trips[pattern_trips_start[p]..[p + 1]]`.
    pub pattern_trips_start: Vec<u32>,
    /// Trip indices.
    pub pattern_trips: Vec<u32>,
    /// Arrival, departure, arrival, departure, … per stop, in seconds.
    /// Ordered pattern by pattern, trip by trip, so one pattern's times are
    /// next to each other in memory (what RAPTOR scans).
    pub stop_times: Vec<u32>,
    /// `pickup_type` (0–3) of each stop time, in `stop_times` order (entry
    /// `i` belongs to `stop_times[2 * i]`). Empty array = all regular.
    pub stop_pickup: Vec<u8>,
    /// `drop_off_type` (0–3), like `stop_pickup`.
    pub stop_drop_off: Vec<u8>,

    // ---- which patterns serve each stop (CSR) ----
    /// Stop s's patterns are `stop_patterns[stop_patterns_start[s]..[s + 1]]`.
    pub stop_patterns_start: Vec<u32>,
    /// Pattern indices.
    pub stop_patterns: Vec<u32>,

    // ---- transfers.txt (CSR by from-stop, sorted by to-stop) ----
    /// Stop s's transfers are `transfer_to[transfer_start[s]..[s + 1]]`.
    /// Empty array = no transfers.
    pub transfer_start: Vec<u32>,
    /// Destination stop.
    pub transfer_to: Vec<u32>,
    /// `transfer_type`: 0 recommended, 1 timed, 2 minimum time, 3 not possible.
    pub transfer_type: Vec<u8>,
    /// `min_transfer_time` in seconds (0 when not given).
    pub transfer_secs: Vec<u32>,

    // ---- footpaths (M4; CSR by from-stop, sorted by to-stop) ----
    /// Stop s's footpaths are `footpath_to[footpath_start[s]..[s + 1]]`:
    /// walks to nearby stops, precomputed when the pack is built (on the
    /// street graph, or straight lines without one). Empty array = none.
    pub footpath_start: Vec<u32>,
    /// Destination stop.
    pub footpath_to: Vec<u32>,
    /// Walking time at 1.3 m/s (steps count ×1.5), in seconds.
    pub footpath_secs: Vec<u32>,
    /// Walking distance in metres.
    pub footpath_m: Vec<u32>,

    // ---- street graph for walking (M4). All empty = no street graph ----
    /// Node latitude in 1e-7 degrees (OSM's own precision).
    pub street_lat: Vec<i32>,
    /// Node longitude in 1e-7 degrees.
    pub street_lon: Vec<i32>,
    /// First node of each street segment (a one-way segment runs a → b).
    pub seg_a: Vec<u32>,
    /// Last node of each segment.
    pub seg_b: Vec<u32>,
    /// Segment length in centimetres, along its shape.
    pub seg_len_cm: Vec<u32>,
    /// [`SEG_STEPS`] | [`SEG_ONEWAY`].
    pub seg_flags: Vec<u8>,
    /// Shape points between a and b of segment s (ends not included) are
    /// `shape_lat/lon[shape_start[s]..[s + 1]]`.
    pub shape_start: Vec<u32>,
    /// Shape point latitude in 1e-7 degrees.
    pub shape_lat: Vec<i32>,
    /// Shape point longitude in 1e-7 degrees.
    pub shape_lon: Vec<i32>,

    // ---- services ----
    /// GTFS `service_id`.
    pub service_ids: Vec<String>,
    /// How many `u64` words each service uses in `service_bits`.
    pub service_words: u32,
    /// Bit d of service s (word `s * service_words + d / 64`) is set when it
    /// runs on day `valid_from + d`.
    pub service_bits: Vec<u64>,
}

impl TimetableData {
    /// Number of locations.
    pub fn stop_count(&self) -> usize {
        self.stop_ids.len()
    }

    /// Coordinates of a stop as `(lat, lon)`, `None` if the feed had none
    /// (or `stop` is out of range).
    pub fn stop_pos(&self, stop: usize) -> Option<(f64, f64)> {
        let (&lat, &lon) = (self.stop_lat.get(stop)?, self.stop_lon.get(stop)?);
        if lat == NO_COORD || lon == NO_COORD {
            return None;
        }
        Some((f64::from_bits(lat), f64::from_bits(lon)))
    }

    /// Number of trips.
    pub fn trip_count(&self) -> usize {
        self.trip_ids.len()
    }

    /// Number of patterns.
    pub fn pattern_count(&self) -> usize {
        self.pattern_stops_start.len().saturating_sub(1)
    }

    /// Number of stop times (one per trip and stop).
    pub fn stop_time_count(&self) -> usize {
        self.stop_times.len() / 2
    }

    /// The stops of one pattern (empty if `pattern` is out of range).
    pub fn pattern_stops(&self, pattern: usize) -> &[u32] {
        let start = self.pattern_stops_start.get(pattern);
        let end = self.pattern_stops_start.get(pattern.wrapping_add(1));
        match (start, end) {
            (Some(&s), Some(&e)) => self
                .pattern_stops
                .get(s as usize..e as usize)
                .unwrap_or(&[]),
            _ => &[],
        }
    }

    /// `(stop, arrival, departure)` of one trip, in stop order (empty if
    /// `trip` is out of range).
    pub fn trip_times(&self, trip: usize) -> Vec<(u32, u32, u32)> {
        let (Some(&pattern), Some(&start)) =
            (self.trip_pattern.get(trip), self.trip_times_start.get(trip))
        else {
            return Vec::new();
        };
        let stops = self.pattern_stops(pattern as usize);
        let start = start as usize;
        let mut times = Vec::with_capacity(stops.len());
        for (i, &s) in stops.iter().enumerate() {
            let k = start + 2 * i;
            match (self.stop_times.get(k), self.stop_times.get(k + 1)) {
                (Some(&a), Some(&d)) => times.push((s, a, d)),
                _ => return Vec::new(),
            }
        }
        times
    }

    /// `(pickup_type, drop_off_type)` of stop time `i` (`stop_times[2 * i]`);
    /// `(0, 0)` when the pack has none or `i` is out of range.
    pub fn pickup_drop_off(&self, i: usize) -> (u8, u8) {
        (
            self.stop_pickup.get(i).copied().unwrap_or(0),
            self.stop_drop_off.get(i).copied().unwrap_or(0),
        )
    }

    /// Range of `stop`'s transfers in `transfer_to`/`_type`/`_secs`
    /// (empty if there are none or `stop` is out of range).
    pub fn transfers(&self, stop: usize) -> std::ops::Range<usize> {
        match (
            self.transfer_start.get(stop),
            self.transfer_start.get(stop.wrapping_add(1)),
        ) {
            (Some(&s), Some(&e)) if s <= e => s as usize..e as usize,
            _ => 0..0,
        }
    }

    /// Range of `stop`'s footpaths in `footpath_to`/`_secs`/`_m`
    /// (empty if there are none or `stop` is out of range).
    pub fn footpaths(&self, stop: usize) -> std::ops::Range<usize> {
        match (
            self.footpath_start.get(stop),
            self.footpath_start.get(stop.wrapping_add(1)),
        ) {
            (Some(&s), Some(&e)) if s <= e => s as usize..e as usize,
            _ => 0..0,
        }
    }

    /// True when the pack has a street graph (built with OSM data).
    pub fn has_streets(&self) -> bool {
        !self.street_lat.is_empty()
    }

    /// True when service `service` runs on day number `day` (0 = `valid_from`).
    pub fn service_runs(&self, service: usize, day: u32) -> bool {
        if day / 64 >= self.service_words {
            return false;
        }
        let word = service
            .checked_mul(self.service_words as usize)
            .and_then(|w| w.checked_add((day / 64) as usize));
        word.and_then(|w| self.service_bits.get(w))
            .is_some_and(|w| w & (1 << (day % 64)) != 0)
    }

    /// Checks that all arrays fit together, so later indexing can't go out
    /// of bounds. Returns what is wrong as text.
    pub fn check(&self) -> Result<(), String> {
        let stops = self.stop_ids.len();
        let trips = self.trip_ids.len();
        let patterns = self.pattern_count();
        let same = |name: &str, len: usize, want: usize| {
            if len == want {
                Ok(())
            } else {
                Err(format!("{name} has {len} entries, expected {want}"))
            }
        };
        same("stop_names", self.stop_names.len(), stops)?;
        same("stop_lat", self.stop_lat.len(), stops)?;
        same("stop_lon", self.stop_lon.len(), stops)?;
        same("stop_kind", self.stop_kind.len(), stops)?;
        same("stop_parent", self.stop_parent.len(), stops)?;
        let routes = self.route_ids.len();
        same("route_short_names", self.route_short_names.len(), routes)?;
        same("route_long_names", self.route_long_names.len(), routes)?;
        same("route_types", self.route_types.len(), routes)?;
        same("trip_route", self.trip_route.len(), trips)?;
        same("trip_service", self.trip_service.len(), trips)?;
        same("trip_headsigns", self.trip_headsigns.len(), trips)?;
        same("trip_approximate", self.trip_approximate.len(), trips)?;
        same("trip_pattern", self.trip_pattern.len(), trips)?;
        same("trip_times_start", self.trip_times_start.len(), trips)?;
        same(
            "pattern_trips_start",
            self.pattern_trips_start.len(),
            patterns + 1,
        )?;
        same(
            "stop_patterns_start",
            self.stop_patterns_start.len(),
            stops + 1,
        )?;
        same(
            "service_bits",
            self.service_bits.len(),
            self.service_ids.len() * self.service_words as usize,
        )?;
        check_csr(
            "pattern_stops",
            &self.pattern_stops_start,
            &self.pattern_stops,
            stops,
        )?;
        check_csr(
            "pattern_trips",
            &self.pattern_trips_start,
            &self.pattern_trips,
            trips,
        )?;
        check_csr(
            "stop_patterns",
            &self.stop_patterns_start,
            &self.stop_patterns,
            patterns,
        )?;
        all_below("stop_parent", &self.stop_parent, stops, true)?;
        all_below("trip_route", &self.trip_route, routes, false)?;
        all_below(
            "trip_service",
            &self.trip_service,
            self.service_ids.len(),
            false,
        )?;
        all_below("trip_pattern", &self.trip_pattern, patterns, false)?;
        // Optional arrays (added in format 3): empty, or one entry per item.
        let optional = |name: &str, len: usize, want: usize| {
            if len == 0 {
                Ok(())
            } else {
                same(name, len, want)
            }
        };
        optional("trip_block_ids", self.trip_block_ids.len(), trips)?;
        let times = self.stop_times.len() / 2;
        optional("stop_pickup", self.stop_pickup.len(), times)?;
        optional("stop_drop_off", self.stop_drop_off.len(), times)?;
        for (name, v) in [
            ("stop_pickup", &self.stop_pickup),
            ("stop_drop_off", &self.stop_drop_off),
        ] {
            if let Some(x) = v.iter().find(|&&x| x > 3) {
                return Err(format!("{name}: unknown value {x}"));
            }
        }
        if self.transfer_start.is_empty() {
            if !self.transfer_to.is_empty() {
                return Err("transfer_to without transfer_start".to_string());
            }
        } else {
            same("transfer_start", self.transfer_start.len(), stops + 1)?;
            check_csr(
                "transfer_to",
                &self.transfer_start,
                &self.transfer_to,
                stops,
            )?;
        }
        same(
            "transfer_type",
            self.transfer_type.len(),
            self.transfer_to.len(),
        )?;
        same(
            "transfer_secs",
            self.transfer_secs.len(),
            self.transfer_to.len(),
        )?;
        if let Some(k) = self.transfer_type.iter().find(|&&k| k > 3) {
            return Err(format!("transfer_type: unknown value {k}"));
        }
        self.check_footpaths()?;
        self.check_streets()?;
        if let Some(k) = self.stop_kind.iter().find(|&&k| k > 2) {
            return Err(format!("stop_kind: unknown value {k}"));
        }
        if let Some(a) = self.trip_approximate.iter().find(|&&a| a > 1) {
            return Err(format!("trip_approximate: value {a} is not 0 or 1"));
        }
        // Every trip is in exactly the pattern that trip_pattern names.
        if self.pattern_trips.len() != trips {
            return Err("pattern_trips must list every trip once".to_string());
        }
        let mut seen = vec![false; trips];
        for p in 0..patterns {
            let start = self.pattern_trips_start[p] as usize;
            let end = self.pattern_trips_start[p + 1] as usize;
            for &t in &self.pattern_trips[start..end] {
                let t = t as usize;
                if seen[t] || self.trip_pattern[t] as usize != p {
                    return Err(format!("trip {t} is listed wrongly in pattern {p}"));
                }
                seen[t] = true;
            }
        }
        // Each trip's times fit in stop_times and don't overlap another trip's.
        let mut ranges = Vec::with_capacity(trips);
        for t in 0..trips {
            let n = self.pattern_stops(self.trip_pattern[t] as usize).len();
            let start = self.trip_times_start[t] as usize;
            if start % 2 != 0 {
                return Err(format!("times of trip {t} start at an odd offset"));
            }
            let end = start + 2 * n;
            if end > self.stop_times.len() {
                return Err(format!("times of trip {t} go past the end"));
            }
            ranges.push((start, end));
        }
        ranges.sort_unstable();
        if ranges.windows(2).any(|w| w[1].0 < w[0].1) {
            return Err("the times of two trips overlap".to_string());
        }
        Ok(())
    }
}

impl TimetableData {
    fn check_footpaths(&self) -> Result<(), String> {
        let stops = self.stop_ids.len();
        if self.footpath_start.is_empty() {
            if !self.footpath_to.is_empty() {
                return Err("footpath_to without footpath_start".to_string());
            }
        } else {
            if self.footpath_start.len() != stops + 1 {
                return Err(format!(
                    "footpath_start has {} entries, expected {}",
                    self.footpath_start.len(),
                    stops + 1
                ));
            }
            check_csr(
                "footpath_to",
                &self.footpath_start,
                &self.footpath_to,
                stops,
            )?;
        }
        let n = self.footpath_to.len();
        if self.footpath_secs.len() != n || self.footpath_m.len() != n {
            return Err("footpath_secs/footpath_m do not match footpath_to".to_string());
        }
        Ok(())
    }

    fn check_streets(&self) -> Result<(), String> {
        let nodes = self.street_lat.len();
        let segs = self.seg_a.len();
        if self.street_lon.len() != nodes {
            return Err("street_lon does not match street_lat".to_string());
        }
        if [
            self.seg_b.len(),
            self.seg_len_cm.len(),
            self.seg_flags.len(),
        ]
        .iter()
        .any(|&l| l != segs)
        {
            return Err("street segment arrays differ in length".to_string());
        }
        all_below("seg_a", &self.seg_a, nodes, false)?;
        all_below("seg_b", &self.seg_b, nodes, false)?;
        if let Some(f) = self.seg_flags.iter().find(|&&f| f > SEG_STEPS | SEG_ONEWAY) {
            return Err(format!("seg_flags: unknown value {f}"));
        }
        if self.shape_lon.len() != self.shape_lat.len() {
            return Err("shape_lon does not match shape_lat".to_string());
        }
        if segs == 0 && self.shape_start.is_empty() {
            if !self.shape_lat.is_empty() {
                return Err("shape points without segments".to_string());
            }
        } else {
            if self.shape_start.len() != segs + 1 {
                return Err(format!(
                    "shape_start has {} entries, expected {}",
                    self.shape_start.len(),
                    segs + 1
                ));
            }
            let ok = self.shape_start.first() == Some(&0)
                && self.shape_start.windows(2).all(|w| w[0] <= w[1])
                && self.shape_start.last().map(|&e| e as usize) == Some(self.shape_lat.len());
            if !ok {
                return Err("shape_start: bad start offsets".to_string());
            }
        }
        let max_lat = (STREET_MAX_LAT * 1e7) as i32;
        let lat_ok = |v: &[i32]| v.iter().all(|&x| (-max_lat..=max_lat).contains(&x));
        let lon_ok = |v: &[i32]| {
            v.iter()
                .all(|&x| (-1_800_000_000..=1_800_000_000).contains(&x))
        };
        if !lat_ok(&self.street_lat)
            || !lat_ok(&self.shape_lat)
            || !lon_ok(&self.street_lon)
            || !lon_ok(&self.shape_lon)
        {
            return Err(format!(
                "street coordinate out of range (latitude beyond ±{STREET_MAX_LAT}° or longitude beyond ±180°)"
            ));
        }
        // Bound the geometry, not just the ranges: every piece is short and
        // the stored length matches the shape.
        for g in 0..segs {
            let node = |n: u32| (self.street_lat[n as usize], self.street_lon[n as usize]);
            let r = self.shape_start[g] as usize..self.shape_start[g + 1] as usize;
            let mid = r.map(|i| (self.shape_lat[i], self.shape_lon[i]));
            let mut prev = node(self.seg_a[g]);
            let mut len = 0.0;
            for p in mid.chain(std::iter::once(node(self.seg_b[g]))) {
                let d = haversine_e7(prev, p);
                if d > STREET_MAX_PIECE_M {
                    return Err(format!(
                        "street segment {g}: a piece is longer than {STREET_MAX_PIECE_M} m ({d:.0} m)"
                    ));
                }
                len += d;
                prev = p;
            }
            let stored = f64::from(self.seg_len_cm[g]) / 100.0;
            if (stored - len).abs() > SEG_LEN_TOLERANCE * len + 1.0 {
                return Err(format!(
                    "street segment {g}: seg_len_cm says {stored:.2} m, the geometry {len:.2} m"
                ));
            }
        }
        Ok(())
    }
}

/// `start` must begin at 0, never decrease, end at `items.len()`, and every
/// item must be below `max`.
fn check_csr(name: &str, start: &[u32], items: &[u32], max: usize) -> Result<(), String> {
    let ok = start.first() == Some(&0)
        && start.windows(2).all(|w| w[0] <= w[1])
        && start.last().map(|&e| e as usize) == Some(items.len());
    if !ok {
        return Err(format!("{name}: bad start offsets"));
    }
    all_below(name, items, max, false)
}

fn all_below(name: &str, items: &[u32], max: usize, allow_none: bool) -> Result<(), String> {
    match items
        .iter()
        .find(|&&i| (i as usize) >= max && !(allow_none && i == u32::MAX))
    {
        Some(i) => Err(format!("{name}: index {i} out of range (max {max})")),
        None => Ok(()),
    }
}
