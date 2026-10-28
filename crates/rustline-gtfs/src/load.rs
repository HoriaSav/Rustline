//! Reading the feed: `gtfs-structures` for most files, Rustline's own lenient
//! reader for trips.txt and stop_times.txt (M7).

use std::collections::{BTreeSet, HashSet};
use std::io::Read;
use std::path::Path;

use gtfs_structures::{Gtfs, GtfsReader, LocationType};
use serde::Serialize;

use crate::report::ReportBuilder;
use crate::{reader, GtfsError, IssueKind};

/// The feed exactly as the parser read it, before normalisation.
///
/// Kept as a thin wrapper so the rest of Rustline never depends on the
/// parser's types and the parser can be swapped later (spec §7.2).
pub struct RawFeed {
    pub(crate) gtfs: Gtfs,
    /// transfers.txt as text (M5): Rustline reads it itself, tolerantly,
    /// because the parser fails the whole feed on one unknown
    /// `transfer_type` and drops the trip/route columns.
    pub(crate) transfers: Option<String>,
    /// Rows dropped or padded while reading trips.txt and stop_times.txt (M7).
    pub(crate) issues: ReportBuilder,
}

/// Loads a GTFS feed from a `.zip` file or an unpacked directory.
///
/// A missing file or required column fails the feed; a bad row in trips.txt
/// or stop_times.txt is dropped (or padded) and reported (M7).
pub fn load(path: &Path) -> Result<RawFeed, GtfsError> {
    let mut raw = no_panic(path, || {
        GtfsReader::default()
            .read_stop_times(false)
            .raw()
            .read_from_path(path)
    })?
    .map_err(|source| GtfsError::Read {
        path: path.to_owned(),
        source,
    })?;
    raw.transfers = None;
    // levels.txt is unused, but gtfs-structures (since 0.52) fails the whole
    // feed in `Gtfs::try_from` on one bad row there: drop it.
    raw.levels = None;

    let mut issues = ReportBuilder::default();
    let trips = member(path, "trips.txt", |r| reader::trips(r, &mut issues))?;
    let stops: HashSet<&str> = match &raw.stops {
        Ok(s) => s.iter().map(|s| s.id.as_str()).collect(),
        Err(_) => HashSet::new(),
    };
    let ids: HashSet<String> = trips.iter().map(|t| t.id.clone()).collect();
    let (stop_times, unknown_stop) = member(path, "stop_times.txt", |r| {
        reader::stop_times(r, &ids, &stops, &mut issues)
    })?;
    // A trip naming an unknown stop is dropped as a whole (sorted: same report every run).
    let mut unknown: Vec<&String> = unknown_stop.iter().collect();
    unknown.sort();
    for t in unknown {
        issues.add(IssueKind::UnknownStop, 1, t.as_str());
    }
    let keep = |id: &str| ids.contains(id) && !unknown_stop.contains(id);
    let stop_times = stop_times
        .into_iter()
        .filter(|s| keep(&s.trip_id))
        .collect();
    if let Some(Ok(f)) = &mut raw.frequencies {
        f.retain(|f| keep(&f.trip_id));
    }
    let trips = trips.into_iter().filter(|t| keep(&t.id)).collect();
    raw.trips = Ok(trips);
    raw.stop_times = Ok(stop_times);

    let gtfs = no_panic(path, || Gtfs::try_from(raw))?.map_err(|source| GtfsError::Read {
        path: path.to_owned(),
        source,
    })?;
    let transfers = transfers_text(path).map_err(|e| GtfsError::Transfers(e.to_string()))?;
    Ok(RawFeed {
        gtfs,
        transfers,
        issues,
    })
}

/// Runs the parser; a panic in it becomes an error instead of crashing the caller.
fn no_panic<T>(path: &Path, f: impl FnOnce() -> T) -> Result<T, GtfsError> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).map_err(|payload| {
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
    })
}

/// Runs `f` on file `name` of a feed directory or zip (in a zip it may sit in
/// a folder, as the parser allows). A missing file is an error.
fn member<T>(
    path: &Path,
    name: &'static str,
    f: impl FnOnce(&mut dyn Read) -> Result<T, GtfsError>,
) -> Result<T, GtfsError> {
    let bad = |e: &dyn std::fmt::Display| GtfsError::Csv {
        file: name,
        message: e.to_string(),
    };
    let missing = || GtfsError::Read {
        path: path.to_owned(),
        source: gtfs_structures::Error::MissingFile(name.to_string()),
    };
    if path.is_dir() {
        let p = path.join(name);
        if !p.is_file() {
            return Err(missing());
        }
        return f(&mut std::fs::File::open(p).map_err(|e| bad(&e))?);
    }
    let file = std::fs::File::open(path).map_err(|e| bad(&e))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| bad(&e))?;
    // Last match wins, as in gtfs-structures (later entries overwrite its map).
    let found = (0..zip.len()).rev().find(|&i| {
        zip.name_for_index(i)
            .and_then(Result::ok)
            .is_some_and(|n| Path::new(n.as_ref()).file_name() == Some(std::ffi::OsStr::new(name)))
    });
    let Some(found) = found else {
        return Err(missing());
    };
    let mut entry = zip.by_index(found).map_err(|e| bad(&e))?;
    f(&mut entry)
}

/// The text of transfers.txt in a feed directory or zip (`None`: no such file).
/// In a zip the file may sit in a folder, as the parser allows.
fn transfers_text(path: &Path) -> Result<Option<String>, Box<dyn std::error::Error>> {
    use std::io::Read;
    let bytes = if path.is_dir() {
        let p = path.join("transfers.txt");
        if !p.is_file() {
            return Ok(None);
        }
        std::fs::read(p)?
    } else {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(path)?)?;
        // Last match wins, as in gtfs-structures (later entries overwrite its map).
        let index = (0..zip.len()).rev().find(|&i| {
            zip.name_for_index(i).and_then(Result::ok).is_some_and(|n| {
                Path::new(n.as_ref()).file_name() == Some(std::ffi::OsStr::new("transfers.txt"))
            })
        });
        let Some(index) = index else { return Ok(None) };
        let mut f = zip.by_index(index)?;
        // A real transfers.txt is a few MB at most; refuse a zip bomb.
        const MAX: u64 = 256 << 20;
        let mut v = Vec::new();
        f.by_ref().take(MAX + 1).read_to_end(&mut v)?;
        if v.len() as u64 > MAX {
            return Err("transfers.txt is larger than 256 MiB".into());
        }
        v
    };
    Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
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

    /// The timezone from `agency.txt` (`agency_timezone`); it must be an
    /// IANA name (e.g. `America/New_York`).
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
            1 => {
                let z = zones.remove(0);
                // An IANA name (M1 gap #4): what the engine converts times with.
                match z.parse::<chrono_tz::Tz>() {
                    Ok(_) => Ok(z),
                    Err(_) => Err(GtfsError::InvalidTimezone(z)),
                }
            }
            _ => Err(GtfsError::MixedTimezones(zones)),
        }
    }
}
