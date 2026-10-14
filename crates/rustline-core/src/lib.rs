//! The journey planner engine. M2: opening a pack; RAPTOR and walking
//! follow in M3 onward.

use std::path::Path;

use rustline_pack::{read_pack, read_pack_with_limit, Pack, PackError};

/// The entry point the app (and later the FFI layer) talks to.
pub struct Engine {
    pack: Pack,
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
}

impl Engine {
    /// Opens a `.rlp` or `.rlp.zst` file.
    pub fn open(path: &Path) -> Result<Engine, PackError> {
        let bytes = std::fs::read(path)?;
        Engine::from_bytes(&bytes)
    }

    /// Opens a pack that is already in memory.
    pub fn from_bytes(bytes: &[u8]) -> Result<Engine, PackError> {
        Ok(Engine {
            pack: read_pack(bytes)?,
        })
    }

    /// Opens a pack in memory whose body may be up to `max_body_bytes`
    /// (the default limit is [`rustline_pack::DEFAULT_MAX_BODY_BYTES`]).
    pub fn from_bytes_with_limit(bytes: &[u8], max_body_bytes: u64) -> Result<Engine, PackError> {
        Ok(Engine {
            pack: read_pack_with_limit(bytes, max_body_bytes)?,
        })
    }

    /// The loaded pack.
    pub fn pack(&self) -> &Pack {
        &self.pack
    }

    /// Counts, sha and validity of the loaded pack.
    pub fn info(&self) -> PackInfo {
        let m = &self.pack.meta;
        let t = &self.pack.timetable;
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
        }
    }
}
