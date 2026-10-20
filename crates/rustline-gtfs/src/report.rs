//! The validation report: data problems found while building the timetable.
//!
//! Problems are grouped by kind with a count and a few example ids, so a feed
//! with 3 000 interpolated times prints one line, not 3 000.

use std::collections::BTreeMap;
use std::fmt;

use serde::Serialize;

/// How bad an issue is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// The affected trip was dropped from the timetable.
    Error,
    /// Kept, but worth knowing about.
    Warning,
}

/// The kinds of problems Rustline checks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueKind {
    /// A trip with fewer than two stop times can't be ridden: it is dropped,
    /// the feed is kept (warning since M7, Leader).
    TooFewStopTimes,
    /// Times go backwards along a trip.
    DecreasingTimes,
    /// The first or last stop of a trip has no time, so it can't be interpolated.
    MissingBoundaryTime,
    /// The trip's `service_id` is in neither calendar file.
    UnknownService,
    /// The trip's `route_id` is not in routes.txt.
    UnknownRoute,
    /// A stop time points at a `stop_id` that is not in stops.txt.
    UnknownStop,
    /// A frequencies.txt row with headway 0, end ≤ start, or shifted times
    /// that do not fit in u32: the row is dropped, the feed is kept (M5).
    InvalidFrequency,
    /// A stop/station/entrance without valid coordinates.
    StopWithoutCoordinates,
    /// Stop times without arrival and departure, filled by linear interpolation.
    InterpolatedTimes,
    /// A pattern split because a later trip overtook an earlier one.
    OvertakingSplit,
    /// Trips generated from frequencies.txt with `exact_times=0` (headway only).
    ApproximateFrequencyTrips,
    /// A transfers.txt row for a (from, to) pair that already has one (skipped).
    DuplicateTransfer,
    /// A transfers.txt row Rustline does not use: `transfer_type` 4/5 (in-seat),
    /// an unknown or unreadable type, or trip/route columns (skipped).
    UnsupportedTransfer,
    /// A transfers.txt row naming a stop that is not in stops.txt (skipped).
    UnknownTransferStop,
    /// A type 2 transfers.txt row without `min_transfer_time`: the footpath
    /// time is used instead.
    MissingMinTransferTime,
    /// calendar_dates.txt adds and removes a service on the same day: it does not run.
    ConflictingCalendarDates,
    /// A calendar.txt row with `start_date` after `end_date`: no weekday runs.
    EmptyDateRange,
    /// A stop_times.txt time that is not `H:MM:SS` with minutes and seconds
    /// below 60 and hours below 48: the row is dropped (M7).
    InvalidTime,
    /// A trips.txt row whose `trip_id` was already seen: the first row wins (M7).
    DuplicateTripId,
    /// A trip with two stop times of the same `stop_sequence`: the trip is dropped (M7).
    DuplicateStopSequence,
    /// A row with fewer fields than the header (padded with empty values), or
    /// without a required id or with a bad `stop_sequence` (dropped) (M7).
    ShortRow,
    /// A row longer than 64 KiB (or 4096 fields): dropped (M7).
    RowTooLong,
    /// A row with bytes that are not UTF-8: replaced by U+FFFD, row kept (M7).
    InvalidUtf8,
    /// A stop_times.txt row whose `trip_id` is not in trips.txt: dropped (M7).
    UnknownTrip,
}

impl IssueKind {
    /// Errors drop trips; warnings don't (an `InvalidFrequency` row is only
    /// one frequencies.txt row, dropped with a warning: Leader, M5).
    pub fn severity(self) -> Severity {
        use IssueKind::*;
        match self {
            DecreasingTimes | MissingBoundaryTime | UnknownService | UnknownRoute | UnknownStop => {
                Severity::Error
            }
            TooFewStopTimes
            | InvalidFrequency
            | StopWithoutCoordinates
            | InterpolatedTimes
            | OvertakingSplit
            | ApproximateFrequencyTrips
            | DuplicateTransfer
            | UnsupportedTransfer
            | UnknownTransferStop
            | MissingMinTransferTime
            | ConflictingCalendarDates
            | EmptyDateRange
            | InvalidTime
            | DuplicateTripId
            | DuplicateStopSequence
            | ShortRow
            | RowTooLong
            | InvalidUtf8
            | UnknownTrip => Severity::Warning,
        }
    }

    /// One-line human description.
    pub fn describe(self) -> &'static str {
        use IssueKind::*;
        match self {
            TooFewStopTimes => "trips with fewer than 2 stop times (dropped)",
            DecreasingTimes => "trips whose times decrease (dropped)",
            MissingBoundaryTime => "trips without a time at the first/last stop (dropped)",
            UnknownService => "trips with an unknown service_id (dropped)",
            UnknownRoute => "trips with an unknown route_id (dropped)",
            UnknownStop => "trips with an unknown stop_id (dropped)",
            InvalidFrequency => "invalid frequencies.txt rows (skipped)",
            StopWithoutCoordinates => "stops without valid coordinates",
            InterpolatedTimes => "stop times interpolated (no arrival/departure given)",
            OvertakingSplit => "extra patterns created by overtaking splits",
            ApproximateFrequencyTrips => "trips expanded from headway-only frequencies",
            DuplicateTransfer => {
                "duplicate transfers.txt rows (kept the longest min_transfer_time, then the higher transfer_type)"
            }
            UnsupportedTransfer => {
                "transfers.txt rows not used (type 4/5 or unknown, or trip/route-specific; skipped)"
            }
            UnknownTransferStop => "transfers.txt rows with an unknown stop_id (skipped)",
            MissingMinTransferTime => {
                "type 2 transfers.txt rows without min_transfer_time (footpath time used)"
            }
            ConflictingCalendarDates => {
                "services added and removed on the same day in calendar_dates.txt (not running)"
            }
            EmptyDateRange => "calendar.txt rows with start_date after end_date (never run)",
            InvalidTime => "stop_times.txt rows with an invalid time (dropped)",
            DuplicateTripId => "duplicate trip_id rows in trips.txt (first kept)",
            DuplicateStopSequence => "trips with a repeated stop_sequence (dropped)",
            ShortRow => "rows shorter than the header (padded) or without a required value (dropped)",
            RowTooLong => "rows longer than 64 KiB (dropped)",
            InvalidUtf8 => "rows with non-UTF-8 bytes (kept, bytes replaced)",
            UnknownTrip => "stop_times.txt rows with a trip_id not in trips.txt (dropped)",
        }
    }
}

/// One kind of problem, how often it happened, and a few examples.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Issue {
    /// What went wrong.
    pub kind: IssueKind,
    /// How many times.
    pub count: usize,
    /// Up to [`ValidationReport::MAX_EXAMPLES`] ids (trip, stop, pattern…).
    pub examples: Vec<String>,
}

/// All problems found in a feed, split into errors and warnings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ValidationReport {
    /// Problems that caused data to be dropped.
    pub errors: Vec<Issue>,
    /// Problems that were tolerated.
    pub warnings: Vec<Issue>,
}

impl ValidationReport {
    /// How many example ids are kept per issue kind.
    pub const MAX_EXAMPLES: usize = 5;

    /// The count for one kind (0 if it never happened).
    pub fn count(&self, kind: IssueKind) -> usize {
        self.errors
            .iter()
            .chain(&self.warnings)
            .find(|i| i.kind == kind)
            .map_or(0, |i| i.count)
    }

    /// True when nothing was dropped.
    pub fn is_clean(&self) -> bool {
        self.errors.is_empty()
    }
}

impl fmt::Display for ValidationReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (title, list) in [("errors", &self.errors), ("warnings", &self.warnings)] {
            if list.is_empty() {
                writeln!(f, "{title}: none")?;
            } else {
                writeln!(f, "{title}:")?;
            }
            for i in list {
                writeln!(f, "  {:>7}  {}", i.count, i.kind.describe())?;
                if !i.examples.is_empty() {
                    writeln!(f, "           e.g. {}", i.examples.join(", "))?;
                }
            }
        }
        Ok(())
    }
}

/// Collects issues while building; turned into a report at the end.
#[derive(Default, Clone)]
pub(crate) struct ReportBuilder {
    issues: BTreeMap<IssueKind, Issue>,
}

impl ReportBuilder {
    /// Records `n` occurrences of `kind`, with `example` as an id to show.
    pub(crate) fn add(&mut self, kind: IssueKind, n: usize, example: impl Into<String>) {
        let issue = self.issues.entry(kind).or_insert(Issue {
            kind,
            count: 0,
            examples: Vec::new(),
        });
        issue.count += n;
        if issue.examples.len() < ValidationReport::MAX_EXAMPLES {
            issue.examples.push(example.into());
        }
    }

    pub(crate) fn finish(self) -> ValidationReport {
        let mut report = ValidationReport::default();
        for issue in self.issues.into_values() {
            match issue.kind.severity() {
                Severity::Error => report.errors.push(issue),
                Severity::Warning => report.warnings.push(issue),
            }
        }
        report
    }
}
