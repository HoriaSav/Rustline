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
        check_arrays(self)
    }
}

/// Lists every array of [`TimetableData`] once, in declaration order (the
/// order rkyv writes and checks them), with its element type. Generates the
/// [`A`] names, the [`Arrays`] view of an in-memory `TimetableData`, and where
/// each array sits in the archived root (for the on-disk check, M7).
macro_rules! arrays {
    ($($v:ident => $f:ident: $t:ty,)*) => {
        /// An array of [`TimetableData`] (`SegA` is "segment a", not the enum).
        #[derive(Clone, Copy, Debug)]
        #[allow(clippy::enum_variant_names)]
        pub(crate) enum A { $($v,)* }

        impl A {
            /// All arrays, in declaration order.
            pub(crate) const ALL: &'static [A] = &[$(A::$v,)*];

            /// Offset of the field in the archived root, and the size and
            /// alignment of one archived element.
            pub(crate) fn layout(self) -> (usize, usize, usize) {
                match self {
                    $(A::$v => (
                        std::mem::offset_of!(ArchivedTimetableData, $f),
                        std::mem::size_of::<rkyv::Archived<$t>>(),
                        std::mem::align_of::<rkyv::Archived<$t>>(),
                    ),)*
                }
            }

            /// True for the `Vec<String>` arrays.
            pub(crate) fn is_strings(self) -> bool {
                match self {
                    $(A::$v => std::any::TypeId::of::<$t>() == std::any::TypeId::of::<String>(),)*
                }
            }
        }

        impl Arrays for TimetableData {
            fn len(&self, a: A) -> usize {
                match a { $(A::$v => self.$f.len(),)* }
            }
            fn get(&self, a: A, i: usize) -> i64 {
                match a { $(A::$v => Num::num(&self.$f[i]),)* }
            }
            fn service_words(&self) -> u32 {
                self.service_words
            }
        }
    };
}

/// An element as i64 for [`Arrays::get`]; strings have no number (0).
trait Num {
    fn num(&self) -> i64;
}

impl Num for String {
    fn num(&self) -> i64 {
        0
    }
}

macro_rules! num {
    ($($t:ty),*) => { $(impl Num for $t { fn num(&self) -> i64 { *self as i64 } })* };
}
num!(u8, u16, u32, u64, i32);

/// Where `service_words` sits in the archived root.
pub(crate) const SERVICE_WORDS_AT: usize =
    std::mem::offset_of!(ArchivedTimetableData, service_words);

arrays! {
    StopIds => stop_ids: String,
    StopNames => stop_names: String,
    StopLat => stop_lat: u64,
    StopLon => stop_lon: u64,
    StopKind => stop_kind: u8,
    StopParent => stop_parent: u32,
    RouteIds => route_ids: String,
    RouteShortNames => route_short_names: String,
    RouteLongNames => route_long_names: String,
    RouteTypes => route_types: u16,
    TripIds => trip_ids: String,
    TripRoute => trip_route: u32,
    TripService => trip_service: u32,
    TripHeadsigns => trip_headsigns: String,
    TripApproximate => trip_approximate: u8,
    TripPattern => trip_pattern: u32,
    TripTimesStart => trip_times_start: u32,
    TripBlockIds => trip_block_ids: String,
    PatternStopsStart => pattern_stops_start: u32,
    PatternStops => pattern_stops: u32,
    PatternTripsStart => pattern_trips_start: u32,
    PatternTrips => pattern_trips: u32,
    StopTimes => stop_times: u32,
    StopPickup => stop_pickup: u8,
    StopDropOff => stop_drop_off: u8,
    StopPatternsStart => stop_patterns_start: u32,
    StopPatterns => stop_patterns: u32,
    TransferStart => transfer_start: u32,
    TransferTo => transfer_to: u32,
    TransferType => transfer_type: u8,
    TransferSecs => transfer_secs: u32,
    FootpathStart => footpath_start: u32,
    FootpathTo => footpath_to: u32,
    FootpathSecs => footpath_secs: u32,
    FootpathM => footpath_m: u32,
    StreetLat => street_lat: i32,
    StreetLon => street_lon: i32,
    SegA => seg_a: u32,
    SegB => seg_b: u32,
    SegLenCm => seg_len_cm: u32,
    SegFlags => seg_flags: u8,
    ShapeStart => shape_start: u32,
    ShapeLat => shape_lat: i32,
    ShapeLon => shape_lon: i32,
    ServiceIds => service_ids: String,
    ServiceBits => service_bits: u64,
}

/// Read access to the arrays of a timetable: the in-memory [`TimetableData`],
/// or a pack body on disk (the streaming install check, M7). [`check_arrays`]
/// works on both, so there is one set of rules.
pub(crate) trait Arrays {
    /// Length of an array.
    fn len(&self, a: A) -> usize;
    /// Element `i` (below `len(a)`) as i64 (u64 values may wrap; none is
    /// compared; strings give 0).
    fn get(&self, a: A, i: usize) -> i64;
    /// [`TimetableData::service_words`].
    fn service_words(&self) -> u32;
}

/// The first index of `a` whose value matches `bad`, with that value.
fn find(t: &impl Arrays, a: A, bad: impl Fn(i64) -> bool) -> Option<i64> {
    (0..t.len(a)).map(|i| t.get(a, i)).find(|&v| bad(v))
}

/// [`TimetableData::check`] on any [`Arrays`].
pub(crate) fn check_arrays(t: &impl Arrays) -> Result<(), String> {
    let stops = t.len(A::StopIds);
    let trips = t.len(A::TripIds);
    let patterns = t.len(A::PatternStopsStart).saturating_sub(1);
    let same = |name: &str, len: usize, want: usize| {
        if len == want {
            Ok(())
        } else {
            Err(format!("{name} has {len} entries, expected {want}"))
        }
    };
    same("stop_names", t.len(A::StopNames), stops)?;
    same("stop_lat", t.len(A::StopLat), stops)?;
    same("stop_lon", t.len(A::StopLon), stops)?;
    same("stop_kind", t.len(A::StopKind), stops)?;
    same("stop_parent", t.len(A::StopParent), stops)?;
    let routes = t.len(A::RouteIds);
    same("route_short_names", t.len(A::RouteShortNames), routes)?;
    same("route_long_names", t.len(A::RouteLongNames), routes)?;
    same("route_types", t.len(A::RouteTypes), routes)?;
    same("trip_route", t.len(A::TripRoute), trips)?;
    same("trip_service", t.len(A::TripService), trips)?;
    same("trip_headsigns", t.len(A::TripHeadsigns), trips)?;
    same("trip_approximate", t.len(A::TripApproximate), trips)?;
    same("trip_pattern", t.len(A::TripPattern), trips)?;
    same("trip_times_start", t.len(A::TripTimesStart), trips)?;
    same(
        "pattern_trips_start",
        t.len(A::PatternTripsStart),
        patterns + 1,
    )?;
    same(
        "stop_patterns_start",
        t.len(A::StopPatternsStart),
        stops + 1,
    )?;
    let services = t.len(A::ServiceIds);
    same(
        "service_bits",
        t.len(A::ServiceBits),
        services * t.service_words() as usize,
    )?;
    check_csr(
        "pattern_stops",
        t,
        A::PatternStopsStart,
        A::PatternStops,
        stops,
    )?;
    check_csr(
        "pattern_trips",
        t,
        A::PatternTripsStart,
        A::PatternTrips,
        trips,
    )?;
    check_csr(
        "stop_patterns",
        t,
        A::StopPatternsStart,
        A::StopPatterns,
        patterns,
    )?;
    all_below("stop_parent", t, A::StopParent, stops, true)?;
    all_below("trip_route", t, A::TripRoute, routes, false)?;
    all_below("trip_service", t, A::TripService, services, false)?;
    all_below("trip_pattern", t, A::TripPattern, patterns, false)?;
    // Optional arrays (added in format 3): empty, or one entry per item.
    let optional = |name: &str, len: usize, want: usize| {
        if len == 0 {
            Ok(())
        } else {
            same(name, len, want)
        }
    };
    optional("trip_block_ids", t.len(A::TripBlockIds), trips)?;
    let times = t.len(A::StopTimes) / 2;
    optional("stop_pickup", t.len(A::StopPickup), times)?;
    optional("stop_drop_off", t.len(A::StopDropOff), times)?;
    for (name, a) in [
        ("stop_pickup", A::StopPickup),
        ("stop_drop_off", A::StopDropOff),
    ] {
        if let Some(x) = find(t, a, |x| x > 3) {
            return Err(format!("{name}: unknown value {x}"));
        }
    }
    if t.len(A::TransferStart) == 0 {
        if t.len(A::TransferTo) != 0 {
            return Err("transfer_to without transfer_start".to_string());
        }
    } else {
        same("transfer_start", t.len(A::TransferStart), stops + 1)?;
        check_csr("transfer_to", t, A::TransferStart, A::TransferTo, stops)?;
    }
    same(
        "transfer_type",
        t.len(A::TransferType),
        t.len(A::TransferTo),
    )?;
    same(
        "transfer_secs",
        t.len(A::TransferSecs),
        t.len(A::TransferTo),
    )?;
    if let Some(k) = find(t, A::TransferType, |k| k > 3) {
        return Err(format!("transfer_type: unknown value {k}"));
    }
    check_footpaths(t, stops)?;
    check_streets(t)?;
    if let Some(k) = find(t, A::StopKind, |k| k > 2) {
        return Err(format!("stop_kind: unknown value {k}"));
    }
    if let Some(a) = find(t, A::TripApproximate, |a| a > 1) {
        return Err(format!("trip_approximate: value {a} is not 0 or 1"));
    }
    // Every trip is in exactly the pattern that trip_pattern names.
    if t.len(A::PatternTrips) != trips {
        return Err("pattern_trips must list every trip once".to_string());
    }
    let at = |a: A, i: usize| t.get(a, i) as usize;
    let mut seen = vec![false; trips];
    for p in 0..patterns {
        for k in at(A::PatternTripsStart, p)..at(A::PatternTripsStart, p + 1) {
            let tr = at(A::PatternTrips, k);
            if seen[tr] || at(A::TripPattern, tr) != p {
                return Err(format!("trip {tr} is listed wrongly in pattern {p}"));
            }
            seen[tr] = true;
        }
    }
    // Each trip's times fit in stop_times and don't overlap another trip's.
    let mut ranges = Vec::with_capacity(trips);
    for tr in 0..trips {
        let p = at(A::TripPattern, tr);
        let n = at(A::PatternStopsStart, p + 1) - at(A::PatternStopsStart, p);
        let start = at(A::TripTimesStart, tr);
        if start % 2 != 0 {
            return Err(format!("times of trip {tr} start at an odd offset"));
        }
        let end = start + 2 * n;
        if end > t.len(A::StopTimes) {
            return Err(format!("times of trip {tr} go past the end"));
        }
        ranges.push((start, end));
    }
    ranges.sort_unstable();
    if ranges.windows(2).any(|w| w[1].0 < w[0].1) {
        return Err("the times of two trips overlap".to_string());
    }
    Ok(())
}

fn check_footpaths(t: &impl Arrays, stops: usize) -> Result<(), String> {
    if t.len(A::FootpathStart) == 0 {
        if t.len(A::FootpathTo) != 0 {
            return Err("footpath_to without footpath_start".to_string());
        }
    } else {
        if t.len(A::FootpathStart) != stops + 1 {
            return Err(format!(
                "footpath_start has {} entries, expected {}",
                t.len(A::FootpathStart),
                stops + 1
            ));
        }
        check_csr("footpath_to", t, A::FootpathStart, A::FootpathTo, stops)?;
    }
    let n = t.len(A::FootpathTo);
    if t.len(A::FootpathSecs) != n || t.len(A::FootpathM) != n {
        return Err("footpath_secs/footpath_m do not match footpath_to".to_string());
    }
    Ok(())
}

fn check_streets(t: &impl Arrays) -> Result<(), String> {
    let nodes = t.len(A::StreetLat);
    let segs = t.len(A::SegA);
    if t.len(A::StreetLon) != nodes {
        return Err("street_lon does not match street_lat".to_string());
    }
    if [t.len(A::SegB), t.len(A::SegLenCm), t.len(A::SegFlags)]
        .iter()
        .any(|&l| l != segs)
    {
        return Err("street segment arrays differ in length".to_string());
    }
    all_below("seg_a", t, A::SegA, nodes, false)?;
    all_below("seg_b", t, A::SegB, nodes, false)?;
    if let Some(f) = find(t, A::SegFlags, |f| f > i64::from(SEG_STEPS | SEG_ONEWAY)) {
        return Err(format!("seg_flags: unknown value {f}"));
    }
    if t.len(A::ShapeLon) != t.len(A::ShapeLat) {
        return Err("shape_lon does not match shape_lat".to_string());
    }
    if segs == 0 && t.len(A::ShapeStart) == 0 {
        if t.len(A::ShapeLat) != 0 {
            return Err("shape points without segments".to_string());
        }
    } else {
        if t.len(A::ShapeStart) != segs + 1 {
            return Err(format!(
                "shape_start has {} entries, expected {}",
                t.len(A::ShapeStart),
                segs + 1
            ));
        }
        if !starts_ok(t, A::ShapeStart, t.len(A::ShapeLat)) {
            return Err("shape_start: bad start offsets".to_string());
        }
    }
    let max_lat = i64::from((STREET_MAX_LAT * 1e7) as i32);
    let out = |a: A, max: i64| find(t, a, |x| !(-max..=max).contains(&x)).is_some();
    if out(A::StreetLat, max_lat)
        || out(A::ShapeLat, max_lat)
        || out(A::StreetLon, 1_800_000_000)
        || out(A::ShapeLon, 1_800_000_000)
    {
        return Err(format!(
            "street coordinate out of range (latitude beyond ±{STREET_MAX_LAT}° or longitude beyond ±180°)"
        ));
    }
    // Bound the geometry, not just the ranges: every piece is short and
    // the stored length matches the shape.
    let at = |a: A, i: usize| t.get(a, i);
    for g in 0..segs {
        let node = |n: i64| {
            let n = n as usize;
            (at(A::StreetLat, n) as i32, at(A::StreetLon, n) as i32)
        };
        let r = at(A::ShapeStart, g) as usize..at(A::ShapeStart, g + 1) as usize;
        let mid = r.map(|i| (at(A::ShapeLat, i) as i32, at(A::ShapeLon, i) as i32));
        let mut prev = node(at(A::SegA, g));
        let mut len = 0.0;
        for p in mid.chain(std::iter::once(node(at(A::SegB, g)))) {
            let d = haversine_e7(prev, p);
            if d > STREET_MAX_PIECE_M {
                return Err(format!(
                    "street segment {g}: a piece is longer than {STREET_MAX_PIECE_M} m ({d:.0} m)"
                ));
            }
            len += d;
            prev = p;
        }
        let stored = at(A::SegLenCm, g) as f64 / 100.0;
        if (stored - len).abs() > SEG_LEN_TOLERANCE * len + 1.0 {
            return Err(format!(
                "street segment {g}: seg_len_cm says {stored:.2} m, the geometry {len:.2} m"
            ));
        }
    }
    Ok(())
}

/// `start` begins at 0, never decreases and ends at `total`.
fn starts_ok(t: &impl Arrays, start: A, total: usize) -> bool {
    let n = t.len(start);
    n > 0
        && t.get(start, 0) == 0
        && (1..n).all(|i| t.get(start, i - 1) <= t.get(start, i))
        && t.get(start, n - 1) as usize == total
}

/// `start` must begin at 0, never decrease, end at `items.len()`, and every
/// item must be below `max`.
fn check_csr(name: &str, t: &impl Arrays, start: A, items: A, max: usize) -> Result<(), String> {
    if !starts_ok(t, start, t.len(items)) {
        return Err(format!("{name}: bad start offsets"));
    }
    all_below(name, t, items, max, false)
}

fn all_below(
    name: &str,
    t: &impl Arrays,
    items: A,
    max: usize,
    allow_none: bool,
) -> Result<(), String> {
    match find(t, items, |i| {
        (i as usize) >= max && !(allow_none && i == i64::from(u32::MAX))
    }) {
        Some(i) => Err(format!("{name}: index {i} out of range (max {max})")),
        None => Ok(()),
    }
}
