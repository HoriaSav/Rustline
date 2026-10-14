//! Errors that stop a feed from being loaded at all.
//!
//! Referential-integrity and CSV-shape problems are fatal (they come from the
//! parser); trip-level timing problems go into the
//! [`ValidationReport`](crate::ValidationReport).

use std::path::PathBuf;

/// A fatal problem with the feed as a whole.
#[derive(Debug, thiserror::Error)]
pub enum GtfsError {
    /// The parser could not read the zip/directory (missing file, bad CSV, …).
    #[error("could not read GTFS feed {path}")]
    Read {
        /// The feed path that was given.
        path: PathBuf,
        /// The parser's own error.
        #[source]
        source: gtfs_structures::Error,
    },
    /// The parser panicked (e.g. an hour value so large it overflows).
    #[error("the GTFS parser crashed while reading {path} (bad value in the feed?): {message}")]
    ParserPanic {
        /// The feed path that was given.
        path: PathBuf,
        /// The panic message.
        message: String,
    },
    /// The timetable does not fit the pack's `u32` offsets.
    #[error("the timetable is too large for a pack ({0} do not fit in u32)")]
    TooLargeForPack(String),
    /// `agency.txt` is empty, so there is no timezone.
    #[error("agency.txt has no agencies, so the feed timezone is unknown")]
    NoAgency,
    /// GTFS requires all agencies of a feed to share one timezone.
    #[error("agencies use different timezones: {0:?}")]
    MixedTimezones(Vec<String>),
    /// Neither calendar.txt nor calendar_dates.txt give any date.
    #[error("the feed has no service dates (calendar.txt and calendar_dates.txt are empty)")]
    NoServiceDates,
}
