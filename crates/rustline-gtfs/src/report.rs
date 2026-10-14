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
    /// A trip with fewer than two stop times can't be ridden.
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
    /// A frequencies.txt row with headway 0 or end ≤ start.
    InvalidFrequency,
    /// A stop/station/entrance without valid coordinates.
    StopWithoutCoordinates,
    /// Stop times without arrival and departure, filled by linear interpolation.
    InterpolatedTimes,
    /// A pattern split because a later trip overtook an earlier one.
    OvertakingSplit,
    /// Trips generated from frequencies.txt with `exact_times=0` (headway only).
    ApproximateFrequencyTrips,
}

impl IssueKind {
    /// Errors drop data; warnings don't.
    pub fn severity(self) -> Severity {
        use IssueKind::*;
        match self {
            TooFewStopTimes | DecreasingTimes | MissingBoundaryTime | UnknownService
            | UnknownRoute | UnknownStop | InvalidFrequency => Severity::Error,
            StopWithoutCoordinates
            | InterpolatedTimes
            | OvertakingSplit
            | ApproximateFrequencyTrips => Severity::Warning,
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
#[derive(Default)]
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
