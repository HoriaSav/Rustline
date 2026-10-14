//! GTFS ingest: read a feed, normalise it into Rustline's own [`Timetable`],
//! build stop patterns and report data problems.
//!
//! Typical use:
//! ```no_run
//! let feed = rustline_gtfs::load(std::path::Path::new("feed.zip"))?;
//! let counts = feed.counts();
//! let (timetable, report) = rustline_gtfs::build_timetable(&feed)?;
//! println!("{} stops, {} patterns, {} errors", counts.stops, timetable.patterns.len(), report.errors.len());
//! # Ok::<(), rustline_gtfs::GtfsError>(())
//! ```
//!
//! Nothing here is specific to one city: the timezone comes from
//! `agency.txt`, the validity window from `calendar.txt`/`calendar_dates.txt`.

mod build;
mod error;
mod load;
mod pack;
mod report;
mod timetable;

pub use build::build_timetable;
pub use error::GtfsError;
pub use load::{load, FeedCounts, RawFeed};
pub use pack::{pack_meta, try_timetable_data};
pub use report::{Issue, IssueKind, Severity, ValidationReport};
pub use timetable::{Pattern, Route, Service, Stop, StopKind, StopTime, Timetable, Trip};
