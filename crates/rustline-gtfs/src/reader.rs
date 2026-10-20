//! Own lenient reader for trips.txt and stop_times.txt (M7, spec §7.2).
//!
//! The file is read row by row with `csv-core` into one fixed buffer of
//! [`ROW_CAP`] bytes, so no row (not even a quote that never closes) can make
//! memory grow. Columns are found by their header name, in any order; unknown
//! columns are ignored. A bad row is dropped (or padded) and counted in the
//! report, never fatal; a missing required column is.

use std::borrow::Cow;
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read};

use gtfs_structures::{PickupDropOffType, RawStopTime, RawTrip};

use crate::report::ReportBuilder;
use crate::{GtfsError, IssueKind};

/// Longest row (or header) in bytes; a longer row is dropped as `RowTooLong`.
pub const ROW_CAP: usize = 64 * 1024;
/// Most fields in one row; more is also `RowTooLong`.
const FIELD_CAP: usize = 4096;
/// Times must be below this many hours (47:59:59 loads, 48:00:00 does not).
pub const MAX_HOURS: u32 = 48;

/// The rows of one CSV file, each as its (trimmed) values for `columns`.
struct Rows<R> {
    csv: csv_core::Reader,
    src: BufReader<R>,
    out: Vec<u8>,
    ends: Vec<usize>,
    file: &'static str,
    /// Position of each wanted column in the header (`None`: no such column).
    cols: Vec<Option<usize>>,
    header_len: usize,
    /// CSV records read so far (the header is record 1).
    record: u64,
}

impl<R: Read> Rows<R> {
    /// Reads the header; every name in `required` must be there.
    fn new(
        src: R,
        file: &'static str,
        required: &[&str],
        optional: &[&str],
    ) -> Result<Self, GtfsError> {
        let mut rows = Rows {
            csv: csv_core::Reader::new(),
            src: BufReader::new(src),
            out: vec![0; ROW_CAP],
            ends: vec![0; FIELD_CAP],
            file,
            cols: Vec::new(),
            header_len: 0,
            record: 1,
        };
        let bad = |message: String| GtfsError::Csv { file, message };
        let n = match rows.record().map_err(|e| bad(e.to_string()))? {
            Some(Some(n)) => n,
            Some(None) => return Err(bad(format!("header longer than {ROW_CAP} bytes"))),
            None => return Err(bad("empty file, no header".into())),
        };
        let header: Vec<String> = (0..n)
            .map(|i| {
                let name = String::from_utf8_lossy(rows.field(i));
                name.trim().trim_start_matches('\u{feff}').to_string()
            })
            .collect();
        for name in required.iter().chain(optional) {
            let at = header.iter().position(|h| h == name);
            if at.is_none() && required.contains(name) {
                return Err(bad(format!("no {name} column")));
            }
            rows.cols.push(at);
        }
        rows.header_len = n;
        Ok(rows)
    }

    /// The next record: `Some(Some(fields))`, `Some(None)` for a row over the
    /// caps (skipped), `None` at the end of the file.
    fn record(&mut self) -> std::io::Result<Option<Option<usize>>> {
        use csv_core::ReadRecordResult::*;
        let (mut nout, mut nend, mut long) = (0, 0, false);
        loop {
            let input = self.src.fill_buf()?;
            let (res, nin, o, e) =
                self.csv
                    .read_record(input, &mut self.out[nout..], &mut self.ends[nend..]);
            self.src.consume(nin);
            // Field ends are counted from the start of the row, across calls.
            nout += o;
            nend += e;
            match res {
                InputEmpty => {}
                // Too long: forget what we have and skip to the end of the row.
                OutputFull | OutputEndsFull => {
                    long = true;
                    nout = 0;
                    nend = 0;
                }
                Record => return Ok(Some((!long).then_some(nend))),
                End => return Ok(None),
            }
        }
    }

    fn field(&self, i: usize) -> &[u8] {
        let start = if i == 0 { 0 } else { self.ends[i - 1] };
        &self.out[start..self.ends[i]]
    }

    /// Calls `f(values, row, report)` for every usable row; the values are in the
    /// order of `required` then `optional`, "" when the row is short.
    fn each(
        mut self,
        report: &mut ReportBuilder,
        mut f: impl FnMut(&[Cow<str>], Row, &mut ReportBuilder),
    ) -> Result<(), GtfsError> {
        let file = self.file;
        loop {
            let rec = self.record().map_err(|e| GtfsError::Csv {
                file,
                message: e.to_string(),
            })?;
            self.record += 1;
            let row = Row {
                file,
                record: self.record,
            };
            let n = match rec {
                None => return Ok(()),
                Some(None) => {
                    report.add(IssueKind::RowTooLong, 1, row);
                    continue;
                }
                Some(Some(n)) => n,
            };
            // A blank line.
            if n == 1 && self.field(0).iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            if n < self.header_len {
                report.add(IssueKind::ShortRow, 1, row);
            }
            let mut utf8 = true;
            let values: Vec<Cow<str>> = self
                .cols
                .iter()
                .map(|c| match c.filter(|&c| c < n) {
                    Some(c) => {
                        let v = String::from_utf8_lossy(self.field(c));
                        utf8 &= matches!(v, Cow::Borrowed(_));
                        match v {
                            Cow::Borrowed(s) => Cow::Borrowed(s.trim()),
                            Cow::Owned(s) => Cow::Owned(s.trim().to_string()),
                        }
                    }
                    None => Cow::Borrowed(""),
                })
                .collect();
            if !utf8 {
                report.add(IssueKind::InvalidUtf8, 1, row);
            }
            f(&values, row, report);
        }
    }
}

/// Where a row is, for report examples: `{file} row {record}`, counting CSV
/// records from the header (a quoted newline does not start a new one). The
/// text is only made when the report keeps the example.
#[derive(Clone, Copy)]
pub(crate) struct Row {
    file: &'static str,
    /// CSV records read so far (the header is record 1).
    record: u64,
}

impl From<Row> for String {
    fn from(r: Row) -> String {
        format!("{} row {}", r.file, r.record)
    }
}

fn some(v: &str) -> Option<String> {
    (!v.is_empty()).then(|| v.to_string())
}

/// trips.txt: a duplicate `trip_id` keeps the first row (`DuplicateTripId`),
/// a row without trip_id/route_id/service_id is dropped (`ShortRow`).
pub(crate) fn trips(src: impl Read, report: &mut ReportBuilder) -> Result<Vec<RawTrip>, GtfsError> {
    let rows = Rows::new(
        src,
        "trips.txt",
        &["trip_id", "route_id", "service_id"],
        &["trip_headsign", "trip_short_name", "shape_id", "block_id"],
    )?;
    let mut seen = HashSet::new();
    let mut trips = Vec::new();
    rows.each(report, |v, row, report| {
        if v[..3].iter().any(|x| x.is_empty()) {
            report.add(IssueKind::ShortRow, 1, row);
        } else if !seen.insert(v[0].to_string()) {
            report.add(IssueKind::DuplicateTripId, 1, v[0].as_ref());
        } else {
            trips.push(RawTrip {
                id: v[0].to_string(),
                route_id: v[1].to_string(),
                service_id: v[2].to_string(),
                trip_headsign: some(&v[3]),
                trip_short_name: some(&v[4]),
                shape_id: some(&v[5]),
                block_id: some(&v[6]),
                ..Default::default()
            });
        }
    })?;
    Ok(trips)
}

/// A GTFS time below [`MAX_HOURS`]; `Ok(None)` when empty.
fn time(v: &str) -> Result<Option<u32>, ()> {
    if v.is_empty() {
        return Ok(None);
    }
    match rustline_model::parse_gtfs_time(v) {
        Ok(t) if t.0 < MAX_HOURS * 3600 => Ok(Some(t.0)),
        _ => Err(()),
    }
}

/// GTFS pickup/drop-off value; anything unknown is regular, as before.
fn pickup(v: &str) -> PickupDropOffType {
    match v {
        "1" => PickupDropOffType::NotAvailable,
        "2" => PickupDropOffType::ArrangeByPhone,
        "3" => PickupDropOffType::CoordinateWithDriver,
        _ => PickupDropOffType::Regular,
    }
}

/// stop_times.txt for the trips in `trips`. A row with a bad time is dropped
/// (`InvalidTime`), as is a row without trip_id/stop_id or with a bad
/// stop_sequence (`ShortRow`) and a row whose trip is not in trips.txt
/// (`UnknownTrip`).
/// Returns the rows and the trips that name a stop not in `stops`.
pub(crate) fn stop_times(
    src: impl Read,
    trips: &HashSet<String>,
    stops: &HashSet<&str>,
    report: &mut ReportBuilder,
) -> Result<(Vec<RawStopTime>, HashSet<String>), GtfsError> {
    let rows = Rows::new(
        src,
        "stop_times.txt",
        &[
            "trip_id",
            "arrival_time",
            "departure_time",
            "stop_id",
            "stop_sequence",
        ],
        &["pickup_type", "drop_off_type"],
    )?;
    let mut out = Vec::new();
    let mut unknown_stop = HashSet::new();
    rows.each(report, |v, row, report| {
        let seq = v[4].parse::<u32>();
        if v[0].is_empty() || v[3].is_empty() || seq.is_err() {
            report.add(IssueKind::ShortRow, 1, row);
            return;
        }
        let (Ok(arrival), Ok(departure)) = (time(&v[1]), time(&v[2])) else {
            report.add(IssueKind::InvalidTime, 1, row);
            return;
        };
        if !trips.contains(v[0].as_ref()) {
            report.add(IssueKind::UnknownTrip, 1, v[0].as_ref());
            return;
        }
        if !stops.contains(v[3].as_ref()) {
            unknown_stop.insert(v[0].to_string());
            return;
        }
        out.push(RawStopTime {
            trip_id: v[0].to_string(),
            arrival_time: arrival,
            departure_time: departure,
            stop_id: v[3].to_string(),
            stop_sequence: seq.unwrap_or_default(),
            pickup_type: pickup(&v[5]),
            drop_off_type: pickup(&v[6]),
            ..Default::default()
        });
    })?;
    Ok((out, unknown_stop))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hands out one byte per read, so every row and field crosses a buffer edge.
    struct OneByte<'a>(&'a [u8]);

    impl Read for OneByte<'_> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let Some((&b, rest)) = self.0.split_first() else {
                return Ok(0);
            };
            buf[0] = b;
            self.0 = rest;
            Ok(1)
        }
    }

    #[test]
    fn fields_are_right_when_rows_arrive_byte_by_byte() {
        let csv = b"\xef\xbb\xbftrip_id,route_id,service_id,trip_headsign\r\nT1,R,S,\"a, b\"\r\nT2,R2,S2,c\n";
        let mut report = ReportBuilder::default();
        let trips = trips(OneByte(csv), &mut report).unwrap();
        let got: Vec<_> = trips
            .iter()
            .map(|t| {
                (
                    t.id.as_str(),
                    t.route_id.as_str(),
                    t.service_id.as_str(),
                    t.trip_headsign.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            got,
            [
                ("T1", "R", "S", Some("a, b")),
                ("T2", "R2", "S2", Some("c"))
            ]
        );
        assert!(report.finish().warnings.is_empty());
    }
}
