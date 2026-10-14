//! `rustline` command-line tool: the debugging window into the engine.
//!
//! ```text
//! rustline inspect <feed.zip|dir> [--json]
//! ```

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use anyhow::Result;
use clap::{Parser, Subcommand};
use rustline_gtfs::{FeedCounts, ValidationReport};
use serde::Serialize;

/// Rustline: offline public-transport journey planning.
#[derive(Parser)]
#[command(name = "rustline", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Load a GTFS feed, print counts and the validation report.
    Inspect {
        /// A GTFS .zip file or an unpacked feed directory.
        feed: PathBuf,
        /// Print machine-readable JSON instead of text.
        #[arg(long)]
        json: bool,
    },
}

/// Everything `inspect` prints.
#[derive(Serialize)]
struct Inspection {
    feed: String,
    timezone: String,
    valid_from: String,
    valid_to: String,
    /// Raw rows in the feed files.
    counts: FeedCounts,
    /// What made it into the timetable.
    timetable: TimetableStats,
    report: ValidationReport,
    load_ms: u128,
    build_ms: u128,
}

#[derive(Serialize)]
struct TimetableStats {
    trips: usize,
    stop_times: usize,
    patterns: usize,
    overtaking_splits: usize,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Inspect { feed, json } => {
            let i = inspect(feed)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&i)?);
            } else {
                print_text(&i);
            }
        }
    }
    Ok(())
}

fn inspect(path: PathBuf) -> Result<Inspection> {
    let t0 = Instant::now();
    let feed = rustline_gtfs::load(&path)?;
    let load_ms = t0.elapsed().as_millis();
    let t1 = Instant::now();
    let (tt, report) = rustline_gtfs::build_timetable(&feed)?;
    let build_ms = t1.elapsed().as_millis();
    Ok(Inspection {
        feed: path.display().to_string(),
        timezone: tt.timezone.clone(),
        valid_from: tt.valid_from.to_string(),
        valid_to: tt.valid_to.to_string(),
        counts: feed.counts(),
        timetable: TimetableStats {
            trips: tt.trips.len(),
            stop_times: tt.stop_times.len(),
            patterns: tt.patterns.len(),
            overtaking_splits: report.count(rustline_gtfs::IssueKind::OvertakingSplit),
        },
        report,
        load_ms,
        build_ms,
    })
}

fn print_text(i: &Inspection) {
    let c = &i.counts;
    println!("feed        {}", i.feed);
    println!("timezone    {}", i.timezone);
    println!("valid       {} → {}", i.valid_from, i.valid_to);
    println!();
    println!("feed rows");
    println!(
        "  stops               {:>8}   (+{} stations, {} other)",
        c.stops, c.stations, c.other_locations
    );
    println!("  routes              {:>8}", c.routes);
    println!("  trips               {:>8}", c.trips);
    println!("  stop_times          {:>8}", c.stop_times);
    println!("  services            {:>8}", c.services);
    println!("  service exceptions  {:>8}", c.service_exceptions);
    println!("  frequencies         {:>8}", c.frequencies);
    println!();
    let t = &i.timetable;
    println!("timetable");
    println!("  trips               {:>8}", t.trips);
    println!("  stop_times          {:>8}", t.stop_times);
    println!(
        "  patterns            {:>8}   ({} from overtaking splits)",
        t.patterns, t.overtaking_splits
    );
    println!();
    print!("{}", i.report);
    println!();
    println!("load {} ms, build {} ms", i.load_ms, i.build_ms);
}
