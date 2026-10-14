//! `rustline` command-line tool: the debugging window into the engine.
//!
//! ```text
//! rustline inspect <feed.zip|dir|pack.rlp|pack.rlp.zst> [--json]
//! rustline build-pack --gtfs <feed.zip|dir> --out <dir> [--feed-id <id>] [--built-at <unix>]
//! ```

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use anyhow::{Context, Result};
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
    /// Load a GTFS feed (or a pack), print counts and the validation report.
    Inspect {
        /// A GTFS .zip file, an unpacked feed directory, or a .rlp/.rlp.zst pack.
        feed: PathBuf,
        /// Print machine-readable JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Build a routing pack (.rlp and .rlp.zst) from a GTFS feed.
    BuildPack {
        /// A GTFS .zip file or an unpacked feed directory.
        #[arg(long)]
        gtfs: PathBuf,
        /// Output directory (created if missing).
        #[arg(long)]
        out: PathBuf,
        /// Feed id used in the file name; defaults to the feed's file name.
        #[arg(long)]
        feed_id: Option<String>,
        /// Build time to store (unix seconds). Defaults to `SOURCE_DATE_EPOCH`,
        /// then to now. Fix it to get byte-identical packs.
        #[arg(long)]
        built_at: Option<i64>,
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
        Command::Inspect { feed, json } if is_pack(&feed) => inspect_pack(&feed, json)?,
        Command::Inspect { feed, json } => {
            let i = inspect(feed)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&i)?);
            } else {
                print_text(&i);
            }
        }
        Command::BuildPack {
            gtfs,
            out,
            feed_id,
            built_at,
        } => build_pack(&gtfs, &out, feed_id, built_at)?,
    }
    Ok(())
}

fn is_pack(path: &Path) -> bool {
    let name = path.to_string_lossy();
    name.ends_with(".rlp") || name.ends_with(".rlp.zst")
}

fn build_pack(
    gtfs: &Path,
    out: &Path,
    feed_id: Option<String>,
    built_at: Option<i64>,
) -> Result<()> {
    let t0 = Instant::now();
    let feed = rustline_gtfs::load(gtfs)?;
    let (tt, report) = rustline_gtfs::build_timetable(&feed)?;
    if tt.trips.is_empty() {
        anyhow::bail!(
            "no valid trips left after validation in {}, so there is nothing to pack ({} errors in the report)",
            gtfs.display(),
            report.errors.len()
        );
    }
    let data = rustline_gtfs::try_timetable_data(&tt)?;

    let feed_id = feed_id.unwrap_or_else(|| feed_name(gtfs));
    let id_ok = |c: char| c.is_ascii_alphanumeric() || "-_.".contains(c);
    if feed_id.is_empty() || !feed_id.chars().all(id_ok) {
        anyhow::bail!("feed id {feed_id:?} may only use letters, digits, '-', '_' and '.'");
    }
    let feed_sha256 = feed_sha256(gtfs)?;
    let built_at = match built_at {
        Some(t) => t,
        None => match std::env::var("SOURCE_DATE_EPOCH") {
            Ok(v) => v
                .trim()
                .parse()
                .context("SOURCE_DATE_EPOCH is not a number")?,
            Err(_) => {
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?;
                i64::try_from(now.as_secs())?
            }
        },
    };
    let meta = rustline_gtfs::pack_meta(&tt, feed_sha256, &feed_id, built_at);

    let mut bytes = Vec::new();
    rustline_pack::write_pack(&data, &meta, &mut bytes)?;
    // The frame carries the uncompressed size (so the loader can refuse a
    // zstd bomb early) and zstd's own checksum of the content.
    let mut compressor = zstd::bulk::Compressor::new(19)?;
    compressor.set_parameter(zstd::zstd_safe::CParameter::ChecksumFlag(true))?;
    let compressed = compressor.compress(&bytes)?;

    fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;
    let sha12: String = hex(&feed_sha256)[..12].to_string();
    let name = format!(
        "rustline-{feed_id}-{sha12}-v{}.rlp",
        rustline_pack::FORMAT_VERSION
    );
    let path = out.join(&name);
    let zst_path = out.join(format!("{name}.zst"));
    fs::write(&path, &bytes).with_context(|| format!("writing {}", path.display()))?;
    fs::write(&zst_path, &compressed).with_context(|| format!("writing {}", zst_path.display()))?;

    println!("wrote {} ({} bytes)", path.display(), bytes.len());
    println!("wrote {} ({} bytes)", zst_path.display(), compressed.len());
    println!(
        "{} stops, {} trips, {} stop times, {} patterns; {} errors in report",
        data.stop_count(),
        data.trip_count(),
        data.stop_time_count(),
        data.pattern_count(),
        report.errors.len()
    );
    println!("build {} ms", t0.elapsed().as_millis());
    Ok(())
}

/// File name without `.zip`, used as the default feed id.
fn feed_name(path: &Path) -> String {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string());
    let name = name.unwrap_or_else(|| "feed".to_string());
    name.trim_end_matches(".zip").to_string()
}

/// SHA-256 of the zip bytes. For a directory: of every file, sorted by name.
fn feed_sha256(path: &Path) -> Result<[u8; 32]> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    if path.is_dir() {
        let mut files: Vec<PathBuf> = fs::read_dir(path)?
            .map(|e| e.map(|e| e.path()))
            .collect::<Result<_, _>>()?;
        files.sort();
        for f in files.iter().filter(|f| f.is_file()) {
            hasher.update(f.file_name().unwrap_or_default().as_encoded_bytes());
            hasher.update(fs::read(f)?);
        }
    } else {
        hasher.update(fs::read(path)?);
    }
    Ok(hasher.finalize().into())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn inspect_pack(path: &Path, json: bool) -> Result<()> {
    let t0 = Instant::now();
    let engine =
        rustline_core::Engine::open(path).with_context(|| format!("opening {}", path.display()))?;
    let load_ms = t0.elapsed().as_millis();
    let i = engine.info();
    if json {
        let value = serde_json::json!({
            "pack": path.display().to_string(),
            "feed_id": i.feed_id,
            "feed_sha256": i.feed_sha256,
            "timezone": i.timezone,
            "format_version": i.format_version,
            "valid_from": i.valid_from.to_string(),
            "valid_to": i.valid_to.to_string(),
            "built_at_unix": i.built_at_unix,
            "stops": i.stops,
            "routes": i.routes,
            "trips": i.trips,
            "stop_times": i.stop_times,
            "patterns": i.patterns,
            "services": i.services,
            "load_ms": load_ms,
        });
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }
    println!("pack        {}", path.display());
    println!("feed id     {}", i.feed_id);
    println!("feed sha256 {}", i.feed_sha256);
    println!("timezone    {}", i.timezone);
    println!("valid       {} → {}", i.valid_from, i.valid_to);
    println!("built at    {} (unix)", i.built_at_unix);
    println!("format      v{}", i.format_version);
    println!();
    println!("  stops               {:>8}", i.stops);
    println!("  routes              {:>8}", i.routes);
    println!("  trips               {:>8}", i.trips);
    println!("  stop_times          {:>8}", i.stop_times);
    println!("  patterns            {:>8}", i.patterns);
    println!("  services            {:>8}", i.services);
    println!();
    println!("load {load_ms} ms");
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
