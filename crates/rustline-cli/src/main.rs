//! `rustline` command-line tool: the debugging window into the engine.
//!
//! ```text
//! rustline inspect <feed.zip|dir|pack.rlp|pack.rlp.zst> [--json]
//! rustline bbox --gtfs <feed.zip|dir> [--margin-km <km>]
//! rustline build-pack --gtfs <feed.zip|dir> --out <dir> [--osm <extract.osm.pbf> [--bbox <min_lon,min_lat,max_lon,max_lat>]]
//!                     [--feed-id <id>] [--built-at <unix>]
//! rustline plan --pack <pack> --from <lat,lon> --to <lat,lon> (--at | --arrive-by) <YYYY-MM-DDTHH:MM[:SS][±HH:MM|Z]> [--geojson <file>]
//! rustline plan --pack <pack> --from-stop <stop_id> --to-stop <stop_id> (--at | --arrive-by) <YYYY-MM-DDTHH:MM[:SS]>
//! ```

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use anyhow::{Context, Result};
use clap::{ArgGroup, Parser, Subcommand};
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
    /// Print the area of a feed's stops plus a margin, as
    /// `min_lon,min_lat,max_lon,max_lat` (the order `osmium extract -b` and
    /// `build-pack --bbox` take).
    Bbox {
        /// A GTFS .zip file or an unpacked feed directory.
        #[arg(long)]
        gtfs: PathBuf,
        /// Margin around the stops, in km (0 to 100).
        #[arg(long, default_value = "2")]
        margin_km: String,
    },
    /// Build a routing pack (.rlp and .rlp.zst) from a GTFS feed.
    BuildPack {
        /// A GTFS .zip file or an unpacked feed directory.
        #[arg(long)]
        gtfs: PathBuf,
        /// OpenStreetMap extract (.osm.pbf) for walking along streets.
        /// Without it, walks are straight lines × 1.3.
        #[arg(long)]
        osm: Option<PathBuf>,
        /// Only use the OSM data inside `min_lon,min_lat,max_lon,max_lat`
        /// (see `rustline bbox`).
        #[arg(long, requires = "osm", allow_hyphen_values = true)]
        bbox: Option<String>,
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
    /// Plan door to door (`--from`/`--to`) or between two stops
    /// (`--from-stop`/`--to-stop`). Exactly one of `--at` / `--arrive-by`.
    #[command(group(ArgGroup::new("time").required(true).args(["at", "arrive_by"])))]
    Plan {
        /// A .rlp or .rlp.zst pack.
        #[arg(long)]
        pack: PathBuf,
        /// Start point `lat,lon`.
        #[arg(
            long,
            allow_hyphen_values = true,
            conflicts_with = "from_stop",
            required_unless_present = "from_stop",
            allow_hyphen_values = true
        )]
        from: Option<String>,
        /// End point `lat,lon`.
        #[arg(
            long,
            allow_hyphen_values = true,
            conflicts_with = "to_stop",
            required_unless_present = "to_stop",
            allow_hyphen_values = true
        )]
        to: Option<String>,
        /// GTFS stop_id to leave from.
        #[arg(long, requires = "to_stop")]
        from_stop: Option<String>,
        /// GTFS stop_id to go to.
        #[arg(long, requires = "from_stop")]
        to_stop: Option<String>,
        /// Write the itineraries as GeoJSON (a FeatureCollection of LineStrings).
        #[arg(long)]
        geojson: Option<PathBuf>,
        /// Departure, e.g. 2030-01-02T08:00. Door to door: local time in the
        /// feed's zone, or with an offset (2030-01-02T08:00+02:00, …Z); an
        /// ambiguous local time takes the earlier instant, one in a DST gap
        /// moves forward past it. Stop to stop: service day and time.
        #[arg(long)]
        at: Option<String>,
        /// Arrive by this time instead (same formats as --at), leaving as
        /// late as possible.
        #[arg(long)]
        arrive_by: Option<String>,
        /// Most transfers to use (capped at 8; stop to stop defaults to 8,
        /// door to door to 3).
        #[arg(long)]
        max_transfers: Option<u8>,
        /// Departure window (arrive-by: arrival window), door to door: plain
        /// seconds, or 30m / 2h; at most 2 h.
        #[arg(long, value_parser = parse_window)]
        window: Option<u32>,
        /// Most itineraries to show (1–20, default 5), door to door.
        #[arg(long)]
        max_results: Option<u8>,
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
        Command::Bbox { gtfs, margin_km } => bbox(&gtfs, &margin_km)?,
        Command::BuildPack {
            gtfs,
            osm,
            bbox,
            out,
            feed_id,
            built_at,
        } => {
            let osm = osm.map(|p| (p, bbox));
            build_pack(&gtfs, osm, &out, feed_id, built_at)?
        }
        Command::Plan {
            pack,
            from,
            to,
            from_stop,
            to_stop,
            geojson,
            at,
            arrive_by,
            max_transfers,
            window,
            max_results,
        } => {
            // clap makes exactly one of the two present.
            let (when, arrive) = match (at, arrive_by) {
                (_, Some(t)) => (t, true),
                (Some(t), None) => (t, false),
                (None, None) => anyhow::bail!("give one of --at or --arrive-by"),
            };
            let door = from.is_some() && to.is_some();
            if !door && (window.is_some() || max_results.is_some()) {
                anyhow::bail!("--window and --max-results need --from and --to");
            }
            match (from_stop, to_stop, from, to) {
                (Some(a), Some(b), _, _) if arrive => {
                    stops_arrive_by(&pack, &a, &b, &when, max_transfers)?
                }
                (Some(a), Some(b), _, _) => plan(&pack, &a, &b, &when, max_transfers.unwrap_or(8))?,
                (_, _, Some(a), Some(b)) => door_to_door(
                    &pack,
                    &a,
                    &b,
                    &when,
                    arrive,
                    false,
                    max_transfers,
                    (window.unwrap_or(0), max_results),
                    geojson.as_deref(),
                )?,
                _ => anyhow::bail!("give --from and --to, or --from-stop and --to-stop"),
            }
        }
    }
    Ok(())
}

/// `--window`: plain seconds, or minutes / hours as `30m` / `2h`.
fn parse_window(s: &str) -> Result<u32, String> {
    let (n, unit) = match s.strip_suffix('m') {
        Some(n) => (n, 60),
        None => match s.strip_suffix('h') {
            Some(n) => (n, 3600),
            None => (s, 1),
        },
    };
    let secs = n
        .parse::<u32>()
        .ok()
        .and_then(|n| n.checked_mul(unit))
        .ok_or_else(|| format!("window {s:?}: expected seconds, 30m or 2h"))?;
    if secs > rustline_core::MAX_WINDOW_SECS {
        return Err(format!(
            "window {s:?} is over the {} s cap",
            rustline_core::MAX_WINDOW_SECS
        ));
    }
    Ok(secs)
}

/// `" [fastest, leaves_latest]"`, or nothing.
fn tags(tags: &[rustline_core::Tag]) -> String {
    if tags.is_empty() {
        return String::new();
    }
    let names: Vec<&str> = tags
        .iter()
        .map(|t| match t {
            rustline_core::Tag::Fastest => "fastest",
            rustline_core::Tag::FewestTransfers => "fewest_transfers",
            rustline_core::Tag::LeavesLatest => "leaves_latest",
        })
        .collect();
    format!(" [{}]", names.join(", "))
}

fn is_pack(path: &Path) -> bool {
    let name = path.to_string_lossy();
    name.ends_with(".rlp") || name.ends_with(".rlp.zst")
}

fn build_pack(
    gtfs: &Path,
    osm: Option<(PathBuf, Option<String>)>,
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
    let mut data = rustline_gtfs::try_timetable_data(&tt)?;
    let osm_sha256 = match &osm {
        Some((pbf, bbox)) => {
            let sha = file_sha256(pbf)
                .with_context(|| format!("reading OSM extract {}", pbf.display()))?;
            add_streets(&mut data, pbf, bbox.as_deref())?;
            sha
        }
        None => {
            let r = rustline_core::add_walking(&mut data, None).map_err(anyhow::Error::msg)?;
            println!(
                "no OSM extract: walks are straight lines × 1.3 ({} footpaths)",
                r.footpaths
            );
            [0; 32]
        }
    };

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
    let mut meta = rustline_gtfs::pack_meta(&tt, feed_sha256, &feed_id, built_at);
    meta.osm_sha256 = osm_sha256;

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
    let dup = report.count(rustline_gtfs::IssueKind::DuplicateTransfer);
    if dup > 0 {
        println!(
            "warning: {dup} {}",
            rustline_gtfs::IssueKind::DuplicateTransfer.describe()
        );
    }
    println!("build {} ms", t0.elapsed().as_millis());
    Ok(())
}

/// Reads the OSM extract, adds the street graph and footpaths to `data`
/// and prints what it found.
fn add_streets(
    data: &mut rustline_pack::TimetableData,
    pbf: &Path,
    bbox: Option<&str>,
) -> Result<()> {
    let bbox = match bbox {
        None => None,
        Some(b) => {
            let v: Vec<f64> = b
                .split(',')
                .map(|x| x.trim().parse::<f64>())
                .collect::<Result<_, _>>()
                .ok()
                .filter(|v: &Vec<f64>| v.len() == 4 && v.iter().all(|x| x.is_finite()))
                .with_context(|| {
                    format!("--bbox {b:?}: expected min_lon,min_lat,max_lon,max_lat")
                })?;
            let bb = rustline_osm::BBox {
                min_lon: v[0],
                min_lat: v[1],
                max_lon: v[2],
                max_lat: v[3],
            };
            if !(bb.min_lat <= bb.max_lat && bb.min_lon <= bb.max_lon) {
                anyhow::bail!("--bbox {b:?}: min is above max");
            }
            Some(bb)
        }
    };
    let t0 = Instant::now();
    let (g, r) = rustline_osm::build_foot_graph(pbf, bbox)
        .with_context(|| format!("reading OSM extract {}", pbf.display()))?;
    println!(
        "OSM: {} nodes read ({} kept), {} ways read, {} walkable, {} skipped, {} outside the bbox, {} references to missing nodes; {} ms",
        r.nodes_read,
        r.nodes_kept,
        r.ways_read,
        r.ways_walkable,
        r.ways_skipped,
        r.ways_outside_bbox,
        r.missing_node_refs,
        t0.elapsed().as_millis()
    );
    if r.long_pieces > 0 {
        println!(
            "OSM: {} way pieces longer than {} m dropped",
            r.long_pieces,
            rustline_osm::MAX_PIECE_M
        );
    }
    if g.seg_count() == 0 {
        eprintln!("warning: no walkable streets in the OSM extract (or bbox): walks are straight lines × 1.3");
        rustline_core::add_walking(data, None).map_err(anyhow::Error::msg)?;
        return Ok(());
    }
    let steps: Vec<bool> = g
        .seg_kind()
        .iter()
        .map(|&k| k == rustline_osm::EDGE_KIND_STEPS)
        .collect();
    let input = rustline_core::StreetInput {
        nodes: g.node_coords(),
        seg_a: g.seg_a(),
        seg_b: g.seg_b(),
        seg_len_m: g.seg_len_m(),
        seg_steps: &steps,
        seg_oneway: g.seg_oneway(),
        shape_start: g.shape_start(),
        shape: g.shape(),
    };
    let w = rustline_core::add_walking(data, Some(input)).map_err(anyhow::Error::msg)?;
    println!(
        "foot graph: {} nodes, {} edges ({} segments); {} components, {} kept (with stops), {} dropped",
        w.nodes, w.edges, w.segments, w.components, w.components_kept, w.components_dropped
    );
    let n = w.stops_unlinked.len();
    let shown: Vec<&str> = w
        .stops_unlinked
        .iter()
        .take(20)
        .map(String::as_str)
        .collect();
    println!(
        "stops: {} linked to a street, {n} unlinked (more than 100 m away){}{}",
        w.stops_linked,
        if n > 0 { ": " } else { "" },
        shown.join(", ") + if n > 20 { ", …" } else { "" }
    );
    if w.nodes == 0 {
        eprintln!("warning: no street reaches a stop: walks are straight lines × 1.3");
    }
    println!("{} footpaths between stops", w.footpaths);
    Ok(())
}

fn file_sha256(path: &Path) -> Result<[u8; 32]> {
    use sha2::{Digest, Sha256};
    let mut f = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    // Through `Digest::update`, not `io::copy`: sha2 0.11 hashers are not
    // `io::Write` (Dependabot sha2 0.11, plan M10 step 7).
    let mut buf = vec![0u8; 64 << 10];
    loop {
        let n = match std::io::Read::read(&mut f, &mut buf) {
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        };
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().into())
}

/// `rustline bbox`: stop extent plus a margin.
fn bbox(gtfs: &Path, margin_km: &str) -> Result<()> {
    let km: f64 = margin_km
        .trim()
        .parse()
        .ok()
        .filter(|k: &f64| k.is_finite() && (0.0..=100.0).contains(k))
        .with_context(|| format!("--margin-km {margin_km:?}: expected a number from 0 to 100"))?;
    let feed = rustline_gtfs::load(gtfs)?;
    let (tt, _) = rustline_gtfs::build_timetable(&feed)?;
    let pts: Vec<_> = tt
        .stops
        .iter()
        .filter_map(|s| s.pos)
        .filter(|p| p.is_valid())
        .collect();
    if pts.is_empty() {
        anyhow::bail!("{}: no stop has coordinates", gtfs.display());
    }
    let (mut lo, mut hi) = (pts[0], pts[0]);
    for p in &pts {
        lo.lat = lo.lat.min(p.lat);
        lo.lon = lo.lon.min(p.lon);
        hi.lat = hi.lat.max(p.lat);
        hi.lon = hi.lon.max(p.lon);
    }
    let m = km * 1000.0;
    let dlat = (m / 6_371_008.8f64).to_degrees();
    // Degrees of longitude at the latitude farthest from the equator (widest).
    let k = lo.lat.abs().max(hi.lat.abs()).min(89.0).to_radians().cos();
    let dlon = dlat / k;
    println!(
        "{:.6},{:.6},{:.6},{:.6}",
        (lo.lon - dlon).max(-180.0),
        (lo.lat - dlat).max(-90.0),
        (hi.lon + dlon).min(180.0),
        (hi.lat + dlat).min(90.0)
    );
    Ok(())
}

/// `plan --from-stop --to-stop --arrive-by`: door to door between the two
/// stops' own points with a 1 m access walk, up to 8 transfers by default
/// (as `--at`). The time is read as for `--from/--to`.
fn stops_arrive_by(
    path: &Path,
    from: &str,
    to: &str,
    at: &str,
    max_transfers: Option<u8>,
) -> Result<()> {
    let engine =
        rustline_core::Engine::open(path).with_context(|| format!("opening {}", path.display()))?;
    let tt = engine.pack().timetable();
    let point = |id: &str| {
        let s = tt
            .stop_ids
            .iter()
            .position(|s| s == id)
            .with_context(|| format!("unknown stop id {id:?}"))?;
        let (lat, lon) = tt
            .stop_pos(s)
            .with_context(|| format!("stop {id:?} has no coordinates"))?;
        Ok::<String, anyhow::Error>(format!("{lat},{lon}"))
    };
    let (a, b) = (point(from)?, point(to)?);
    door_to_door(
        path,
        &a,
        &b,
        at,
        true,
        true,
        Some(max_transfers.unwrap_or(8)),
        (0, None),
        None,
    )
}

/// `plan --from --to`: door-to-door itineraries (`arrive`: arrive by `at`).
#[allow(clippy::too_many_arguments)]
fn door_to_door(
    path: &Path,
    from: &str,
    to: &str,
    at: &str,
    arrive: bool,
    stop_to_stop: bool,
    max_transfers: Option<u8>,
    (window, max_results): (u32, Option<u8>),
    geojson: Option<&Path>,
) -> Result<()> {
    let point = |s: &str, what: &str| -> Result<rustline_model::LatLon> {
        let v: Vec<f64> = s
            .split(',')
            .map(|x| x.trim().parse::<f64>())
            .collect::<Result<_, _>>()
            .ok()
            .filter(|v: &Vec<f64>| v.len() == 2)
            .with_context(|| format!("{what} {s:?}: expected lat,lon"))?;
        let p = rustline_model::LatLon::new(v[0], v[1]);
        if !p.is_valid() {
            anyhow::bail!("{what} {s:?}: not a valid coordinate");
        }
        Ok(p)
    };
    let (a, b) = (point(from, "--from")?, point(to, "--to")?);
    let offset_at = ["%Y-%m-%dT%H:%M:%S%:z", "%Y-%m-%dT%H:%M%:z"]
        .iter()
        .find_map(|f| chrono::DateTime::parse_from_str(&at.replace('Z', "+00:00"), f).ok());
    let local_at = if offset_at.is_none() {
        Some(parse_at(at)?)
    } else {
        None
    };
    let engine =
        rustline_core::Engine::open(path).with_context(|| format!("opening {}", path.display()))?;
    let at = match offset_at {
        Some(t) => t,
        None => rustline_core::local_instant(engine.timezone(), local_at.unwrap_or_default()),
    };
    let mut req = rustline_core::PlanRequest {
        from: a,
        to: b,
        time: if arrive {
            rustline_core::TimeSpec::ArriveBy(at)
        } else {
            rustline_core::TimeSpec::DepartAt(at)
        },
        ..Default::default()
    };
    if stop_to_stop {
        req.max_access_m = 1;
    }
    if let Some(m) = max_transfers {
        req.max_transfers = m;
    }
    if let Some(m) = max_results {
        req.max_results = m;
    }
    req.window_secs = window;
    let r = engine.plan(&req)?;
    println!(
        "{},{} → {},{}, {}",
        a.lat,
        a.lon,
        b.lat,
        b.lon,
        at.with_timezone(&engine.timezone()).to_rfc3339()
    );
    for w in &r.warnings {
        println!("warning: {w:?}");
    }
    if r.itineraries.is_empty() {
        println!("no journey found");
    }
    // Local time in the feed's zone with its offset, so the two 03:30 of a
    // DST fall-back night can be told apart.
    let hm = |t: chrono::DateTime<chrono::FixedOffset>| t.format("%H:%M:%S%:z").to_string();
    let stop = |s: &rustline_core::StopRef| {
        if s.name.is_empty() || s.name == s.id {
            s.id.clone()
        } else {
            format!("{} [{}]", s.name, s.id)
        }
    };
    let mut features = Vec::new();
    for (i, it) in r.itineraries.iter().enumerate() {
        println!();
        println!(
            "{}. {} → {}, {} transfer(s), {} m walking{}",
            i + 1,
            hm(it.departure),
            hm(it.arrival),
            it.transfers,
            it.walk_m,
            tags(&it.tags)
        );
        for leg in &it.legs {
            let (kind, line, label) = match leg {
                rustline_core::Leg::Walk {
                    start,
                    end,
                    distance_m,
                    from_stop,
                    to_stop,
                    geometry,
                } => {
                    let place = |s: &Option<rustline_core::StopRef>, end: &str| {
                        s.as_ref().map_or(end.to_string(), stop)
                    };
                    let text = format!(
                        "walk {distance_m} m, {} → {}",
                        place(from_stop, "start"),
                        place(to_stop, "destination")
                    );
                    println!("   {} → {}  {text}", hm(*start), hm(*end));
                    ("walk", geometry, text)
                }
                rustline_core::Leg::Transit {
                    trip_id,
                    route_id,
                    route_name,
                    from_stop,
                    to_stop,
                    board,
                    alight,
                    geometry,
                    ..
                } => {
                    let r = if route_name.is_empty() {
                        route_id
                    } else {
                        route_name
                    };
                    let text = format!(
                        "line {r}, {} → {} (trip {trip_id})",
                        stop(from_stop),
                        stop(to_stop)
                    );
                    println!("   {} → {}  {text}", hm(*board), hm(*alight));
                    ("transit", geometry, text)
                }
            };
            if let Some(g) = line {
                let coords: Vec<[f64; 2]> = g.points().iter().map(|p| [p.lon, p.lat]).collect();
                features.push(serde_json::json!({
                    "type": "Feature",
                    "properties": {"itinerary": i + 1, "kind": kind, "label": label},
                    "geometry": {"type": "LineString", "coordinates": coords},
                }));
            }
        }
    }
    if let Some(p) = geojson {
        let fc = serde_json::json!({"type": "FeatureCollection", "features": features});
        fs::write(p, serde_json::to_string_pretty(&fc)?)
            .with_context(|| format!("writing {}", p.display()))?;
    }
    Ok(())
}

fn parse_at(at: &str) -> Result<chrono::NaiveDateTime> {
    ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M"]
        .iter()
        .find_map(|f| chrono::NaiveDateTime::parse_from_str(at, f).ok())
        .with_context(|| format!("--at {at:?}: expected YYYY-MM-DDTHH:MM[:SS]"))
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
            "street_graph": i.street_graph,
            "osm_sha256": i.osm_sha256,
            "street_nodes": i.street_nodes,
            "street_segments": i.street_segments,
            "footpaths": i.footpaths,
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
    println!("  street nodes        {:>8}", i.street_nodes);
    println!("  street segments     {:>8}", i.street_segments);
    println!("  footpaths           {:>8}", i.footpaths);
    if i.street_graph {
        println!("streets     OSM {}", i.osm_sha256);
    } else {
        println!("streets     none (walks are straight lines × 1.3)");
    }
    println!();
    println!("load {load_ms} ms");
    Ok(())
}

fn plan(path: &Path, from: &str, to: &str, at: &str, max_transfers: u8) -> Result<()> {
    let at = parse_at(at)?;
    let engine =
        rustline_core::Engine::open(path).with_context(|| format!("opening {}", path.display()))?;
    let tt = &engine.pack().timetable();
    let stop = |id: &str| {
        tt.stop_ids
            .iter()
            .position(|s| s == id)
            .map(rustline_model::StopIdx::from_usize)
            .with_context(|| format!("unknown stop id {id:?}"))
    };
    let (a, b) = (stop(from)?, stop(to)?);
    let secs = chrono::Timelike::num_seconds_from_midnight(&at.time());
    let dep = rustline_model::Seconds(secs);
    let journeys = engine.plan_stop_to_stop(a, b, at.date(), dep, max_transfers)?;
    let name = |s: rustline_model::StopIdx| {
        let id = &tt.stop_ids[s.index()];
        let n = &tt.stop_names[s.index()];
        if n.is_empty() || n == id {
            id.clone()
        } else {
            format!("{n} [{id}]")
        }
    };
    println!("{} → {}, {} from {}", name(a), name(b), at.date(), dep);
    if journeys.is_empty() {
        println!("no journey found");
    }
    for (i, j) in journeys.iter().enumerate() {
        println!();
        println!("{}. {} → {}, {} ride(s)", i + 1, j.dep, j.arr, j.rides);
        for leg in &j.legs {
            match *leg {
                rustline_model::Leg::Transit {
                    route,
                    trip,
                    from,
                    to,
                    dep,
                    arr,
                } => {
                    let r = &tt.route_short_names[route.index()];
                    let r = if r.is_empty() {
                        &tt.route_ids[route.index()]
                    } else {
                        r
                    };
                    println!(
                        "   {dep} {} → {arr} {}  line {r} (trip {})",
                        name(from),
                        name(to),
                        tt.trip_ids[trip.index()]
                    );
                }
                rustline_model::Leg::Walk { from, to, secs } => {
                    println!("   change {} → {} ({secs} s)", name(from), name(to));
                }
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
