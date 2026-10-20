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
    /// trips.txt or stop_times.txt can't be read at all: no header, a
    /// missing required column, or an I/O error (M7 own reader).
    #[error("{file}: {message}")]
    Csv {
        /// The file inside the feed.
        file: &'static str,
        /// What is wrong.
        message: String,
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
    /// `agency_timezone` is not an IANA timezone name.
    #[error("agency_timezone {0:?} is not an IANA timezone name (e.g. America/New_York)")]
    InvalidTimezone(String),
    /// transfers.txt could not be read (the file itself, not a bad row).
    #[error("could not read transfers.txt: {0}")]
    Transfers(String),
    /// frequencies.txt would generate more trips or stop times than allowed
    /// (spec §7.2 caps), e.g. a 1 s headway over 48 h.
    #[error("frequencies.txt expands to too many trips: {0}")]
    TooManyFrequencyTrips(String),
    /// Neither calendar.txt nor calendar_dates.txt give any date.
    #[error("the feed has no service dates (calendar.txt and calendar_dates.txt are empty)")]
    NoServiceDates,
}
