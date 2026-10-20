//! The journey planner engine. M2: opening a pack; M3: RAPTOR stop to
//! stop; M4: door to door with walking ([`Engine::plan`]); M5: time zones,
//! DST and after-midnight trips (D − 1 / D / D + 1).

use std::path::Path;

mod back;
mod build;
#[cfg(feature = "oracle")]
pub mod oracle;
mod plan;
mod raptor;
mod street;
mod time;

pub use build::{add_walking, StreetInput, WalkingReport, LINK_MAX_M};
pub use plan::{
    Itinerary, Leg, PlanError, PlanRequest, PlanResponse, Polyline, StopRef, TimeSpec, Warning,
    DETOUR_FACTOR, EXPIRES_SOON_DAYS, MAX_ACCESS_M, MAX_TRANSFER_M, MAX_WALK_SPEED_MPS, SNAP_MAX_M,
    WALK_BONUS_SECS, WALK_ONLY_MAX_M,
};
pub use raptor::{
    raptor_pareto, raptor_stop_to_stop, QueryError, MAX_TRANSFERS, MORE_RIDES_MIN_GAIN_SECS,
};
pub use street::SEARCH_CAP_M;
pub use time::{local_instant, to_service_time};

use rustline_pack::{read_pack, read_pack_with_limit, Pack, PackError};

/// The feed timezone as an IANA name, or [`PackError::Corrupt`]. The one rule
/// for [`Engine::new`] and for install ([`check_installable`]).
pub fn check_timezone(name: &str) -> Result<chrono_tz::Tz, PackError> {
    name.parse()
        .map_err(|_| PackError::Corrupt(format!("timezone {name:?} is not an IANA timezone name")))
}

/// What [`Engine::new`] checks beyond `read_pack`, on a pack file checked on
/// disk (M7 install, Reviewer MF1): the timezone and the street-grid size
/// rules, with the same code. The stop-snapping scan budget needs no check of
/// its own: a stop links within [`LINK_MAX_M`] (100 m), less than one 200 m
/// cell, so its snap looks at no more than 3 × 3 cells, and a grid that passes
/// the per-cell cap holds at most 9 × `MAX_CELL_ENTRIES` there, below the
/// budget of 25 × `MAX_CELL_ENTRIES`.
pub fn check_installable(file: &rustline_pack::CheckedFile) -> Result<(), PackError> {
    check_timezone(&file.meta.timezone)?;
    street::check_disk_grid(&file.streets).map_err(PackError::Corrupt)
}

/// The entry point the app (and later the FFI layer) talks to.
pub struct Engine {
    pack: Pack,
    streets: Option<street::Streets>,
    /// Walking links by destination stop, for arrive-by (M8).
    incoming: back::Incoming,
    /// The feed's timezone, parsed once (M5 step 1).
    tz: chrono_tz::Tz,
}

/// What a loaded pack contains.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackInfo {
    /// Feed id from the header.
    pub feed_id: String,
    /// SHA-256 of the GTFS zip, as 64 hex characters.
    pub feed_sha256: String,
    /// Pack format version.
    pub format_version: u16,
    /// First service day.
    pub valid_from: chrono::NaiveDate,
    /// Last service day.
    pub valid_to: chrono::NaiveDate,
    /// IANA timezone of the feed.
    pub timezone: String,
    /// Build time, unix seconds.
    pub built_at_unix: i64,
    /// Number of locations (stops, stations, …).
    pub stops: usize,
    /// Number of routes.
    pub routes: usize,
    /// Number of trips.
    pub trips: usize,
    /// Number of stop times.
    pub stop_times: usize,
    /// Number of stop patterns.
    pub patterns: usize,
    /// Number of services (calendars).
    pub services: usize,
    /// True when the pack has a street graph; false: walks are straight
    /// lines × 1.3 (built without OSM data, or no streets were found).
    pub street_graph: bool,
    /// SHA-256 of the OSM extract, 64 hex characters (zeros without one).
    pub osm_sha256: String,
    /// Street graph nodes and segments (0 without a street graph).
    pub street_nodes: usize,
    pub street_segments: usize,
    /// Precomputed footpaths between stops.
    pub footpaths: usize,
}

impl Engine {
    /// Opens a `.rlp` or `.rlp.zst` file.
    pub fn open(path: &Path) -> Result<Engine, PackError> {
        let bytes = std::fs::read(path)?;
        Engine::from_bytes(&bytes)
    }

    /// Opens a pack that is already in memory.
    pub fn from_bytes(bytes: &[u8]) -> Result<Engine, PackError> {
        Engine::new(read_pack(bytes)?)
    }

    /// Wraps a checked pack (builds the street indexes). A street graph whose
    /// index would be too big, or a timezone that is not an IANA name, is
    /// [`PackError::Corrupt`].
    pub fn new(pack: Pack) -> Result<Engine, PackError> {
        let tz = check_timezone(&pack.meta().timezone)?;
        let streets =
            street::Streets::new(pack.timetable(), LINK_MAX_M).map_err(PackError::Corrupt)?;
        let incoming = back::Incoming::new(pack.timetable());
        Ok(Engine {
            pack,
            streets,
            incoming,
            tz,
        })
    }

    /// The feed's timezone.
    pub fn timezone(&self) -> chrono_tz::Tz {
        self.tz
    }

    /// Opens a pack in memory whose body may be up to `max_body_bytes`
    /// (the default limit is [`rustline_pack::DEFAULT_MAX_BODY_BYTES`]).
    pub fn from_bytes_with_limit(bytes: &[u8], max_body_bytes: u64) -> Result<Engine, PackError> {
        Engine::new(read_pack_with_limit(bytes, max_body_bytes)?)
    }

    /// The loaded pack.
    pub fn pack(&self) -> &Pack {
        &self.pack
    }

    /// Journeys between two stops; see [`raptor_stop_to_stop`].
    pub fn plan_stop_to_stop(
        &self,
        from: rustline_model::StopIdx,
        to: rustline_model::StopIdx,
        date: chrono::NaiveDate,
        dep: rustline_model::Seconds,
        max_transfers: u8,
    ) -> Result<Vec<rustline_model::Journey>, QueryError> {
        raptor_stop_to_stop(&self.pack, from, to, date, dep, max_transfers)
    }

    /// Door-to-door itineraries; see [`PlanRequest`] and the `plan` module docs.
    pub fn plan(&self, req: &PlanRequest) -> Result<PlanResponse, PlanError> {
        plan::plan(
            &self.pack,
            self.streets.as_ref(),
            &self.incoming,
            self.tz,
            req,
        )
    }

    /// Counts, sha and validity of the loaded pack.
    pub fn info(&self) -> PackInfo {
        let m = &self.pack.meta();
        let t = &self.pack.timetable();
        PackInfo {
            feed_id: m.feed_id.clone(),
            feed_sha256: m.feed_sha256.iter().map(|b| format!("{b:02x}")).collect(),
            format_version: m.format_version,
            valid_from: m.valid_from,
            valid_to: m.valid_to,
            timezone: m.timezone.clone(),
            built_at_unix: m.built_at_unix,
            stops: t.stop_count(),
            routes: t.route_ids.len(),
            trips: t.trip_count(),
            stop_times: t.stop_time_count(),
            patterns: t.pattern_count(),
            services: t.service_ids.len(),
            street_graph: self.streets.is_some(),
            osm_sha256: m.osm_sha256.iter().map(|b| format!("{b:02x}")).collect(),
            street_nodes: t.street_lat.len(),
            street_segments: t.seg_a.len(),
            footpaths: t.footpath_to.len(),
        }
    }
}
