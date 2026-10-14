//! Reading the feed with the `gtfs-structures` crate.

use std::collections::BTreeSet;
use std::path::Path;

use gtfs_structures::{Gtfs, LocationType};
use serde::Serialize;

use crate::GtfsError;

/// The feed exactly as the parser read it, before normalisation.
///
/// Kept as a thin wrapper so the rest of Rustline never depends on the
/// parser's types and the parser can be swapped later (spec §7.2).
pub struct RawFeed {
    pub(crate) gtfs: Gtfs,
}

/// Loads a GTFS feed from a `.zip` file or an unpacked directory.
pub fn load(path: &Path) -> Result<RawFeed, GtfsError> {
    // gtfs-structures can panic on bad values (e.g. huge hours), so turn that
    // into an error instead of crashing the caller.
    let result = std::panic::catch_unwind(|| Gtfs::from_path(path)).map_err(|payload| {
        // A panic payload is usually a &str or a String; keep its text.
        let message = if let Some(s) = payload.downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic".to_string()
        };
        GtfsError::ParserPanic {
            path: path.to_owned(),
            message,
        }
    })?;
    let gtfs = result.map_err(|source| GtfsError::Read {
        path: path.to_owned(),
        source,
    })?;
    Ok(RawFeed { gtfs })
}

/// Row counts of the raw feed, as a GTFS validator or the RoTransit
/// companion manifest would count them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FeedCounts {
    /// Boarding stops/platforms (`location_type` 0 or empty).
    pub stops: usize,
    /// Stations (`location_type` 1); not counted in `stops`.
    pub stations: usize,
    /// Entrances, generic nodes and boarding areas (`location_type` 2–4).
    pub other_locations: usize,
    /// Rows of routes.txt.
    pub routes: usize,
    /// Distinct trip_ids in trips.txt (duplicate rows collapse into one).
    pub trips: usize,
    /// Rows of stop_times.txt.
    pub stop_times: usize,
    /// Distinct service ids in calendar.txt and calendar_dates.txt.
    pub services: usize,
    /// Rows of calendar_dates.txt (added or removed days).
    pub service_exceptions: usize,
    /// Rows of frequencies.txt.
    pub frequencies: usize,
}

impl RawFeed {
    /// Counts the raw rows of each file.
    pub fn counts(&self) -> FeedCounts {
        let g = &self.gtfs;
        let (mut stops, mut stations, mut other) = (0, 0, 0);
        for s in g.stops.values() {
            match s.location_type {
                LocationType::StopPoint => stops += 1,
                LocationType::StopArea => stations += 1,
                _ => other += 1,
            }
        }
        let services: BTreeSet<&str> = g
            .calendar
            .keys()
            .chain(g.calendar_dates.keys())
            .map(String::as_str)
            .collect();
        FeedCounts {
            stops,
            stations,
            other_locations: other,
            routes: g.routes.len(),
            trips: g.trips.len(),
            stop_times: g.trips.values().map(|t| t.stop_times.len()).sum(),
            services: services.len(),
            service_exceptions: g.calendar_dates.values().map(Vec::len).sum(),
            frequencies: g.trips.values().map(|t| t.frequencies.len()).sum(),
        }
    }

    /// The timezone from `agency.txt` (`agency_timezone`).
    pub fn timezone(&self) -> Result<String, GtfsError> {
        let mut zones: Vec<String> = self
            .gtfs
            .agencies
            .iter()
            .map(|a| a.timezone.clone())
            .collect();
        zones.sort();
        zones.dedup();
        match zones.len() {
            0 => Err(GtfsError::NoAgency),
            1 => Ok(zones.remove(0)),
            _ => Err(GtfsError::MixedTimezones(zones)),
        }
    }
}
