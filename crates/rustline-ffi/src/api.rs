//! The Dart-facing API (plan M6 step 2, spec §11.3). flutter_rust_bridge reads
//! this file; keep every public type plain data (String, numbers, Vec, Option,
//! enums) and every public function returning `Result`.
//!
//! Times cross as epoch milliseconds (UTC) plus the offset in seconds. The
//! answer is always in the feed's offset, whatever the device's zone is.
//!
//! Errors are [`RustlineException`] with a stable `code`: `invalid_request`,
//! `outside_coverage`, `no_route`, `date_outside_feed`, `pack_corrupt`,
//! `pack_version`, `pack_missing`, `pack_io`, `pack_too_large`, `no_space` and
//! `internal` (a panic, caught here).

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::Arc;

use chrono::{DateTime, FixedOffset, NaiveDate};
use flutter_rust_bridge::frb;
use rustline_core::{Engine, Leg, PlanError, PlanRequest, StopRef, TimeSpec, Warning};
use rustline_model::LatLon;
use rustline_pack::{PackError, DEFAULT_MAX_BODY_BYTES};

/// Largest UTC offset accepted from the device (ISO 8601 / tz database range).
const MAX_OFFSET_SECS: i32 = 18 * 3600;
/// Room for the header and checksum on top of the body limit.
const HEADER_SLACK: u64 = 64 * 1024;

/// An error with a stable code; thrown as `RustlineException` in Dart.
#[frb(dart_code = "
  /// Maps a `code: text` message to a typed exception; an unknown code is
  /// `internal`. Fallback only: Rust errors already arrive typed.
  factory RustlineException.fromMessage(String message) {
    final i = message.indexOf(':');
    final c = (i < 0 ? message : message.substring(0, i)).trim();
    return RustlineException(
        code: knownCodes.contains(c) ? c : 'internal', message: message);
  }

  /// Every stable code.
  static const knownCodes = {
    'invalid_request', 'outside_coverage', 'no_route', 'date_outside_feed',
    'pack_corrupt', 'pack_version', 'pack_missing', 'pack_io',
    'pack_too_large', 'no_space', 'internal',
  };

  @override
  String toString() => 'RustlineException($code): $message';
")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustlineException {
    pub code: String,
    pub message: String,
}

impl std::fmt::Display for RustlineException {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for RustlineException {}

fn fail(code: &str, message: impl Into<String>) -> RustlineException {
    RustlineException {
        code: code.to_string(),
        message: message.into(),
    }
}

/// A trip request. `depart_offset_secs` is the device's offset at that
/// instant (only used to read the instant; the answer uses the feed's zone).
#[derive(Debug, Clone, PartialEq)]
pub struct FfiPlanRequest {
    pub from_lat: f64,
    pub from_lon: f64,
    pub to_lat: f64,
    pub to_lon: f64,
    pub depart_at_ms: i64,
    pub depart_offset_secs: i32,
    pub max_results: u8,
    pub max_access_m: Option<u32>,
    pub walk_speed_mps: Option<f64>,
    pub max_transfers: Option<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FfiPlanResponse {
    pub itineraries: Vec<FfiItinerary>,
    /// `code` or `code: detail`, e.g. `feed_expires_soon: 2027-05-23`.
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FfiItinerary {
    pub departure_ms: i64,
    pub departure_offset_secs: i32,
    pub arrival_ms: i64,
    pub arrival_offset_secs: i32,
    pub transfers: u8,
    pub walk_m: u32,
    pub legs: Vec<FfiLeg>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FfiLeg {
    Walk {
        start_ms: i64,
        start_offset_secs: i32,
        end_ms: i64,
        end_offset_secs: i32,
        distance_m: u32,
        from_stop: Option<FfiStop>,
        to_stop: Option<FfiStop>,
        geometry: Vec<FfiLatLon>,
    },
    Transit {
        trip_id: String,
        route_id: String,
        route_name: String,
        headsign: String,
        from_stop: FfiStop,
        to_stop: FfiStop,
        board_ms: i64,
        board_offset_secs: i32,
        alight_ms: i64,
        alight_offset_secs: i32,
        geometry: Vec<FfiLatLon>,
    },
}

/// A GTFS stop; `lat`/`lon` are None for a stop without coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct FfiStop {
    pub id: String,
    pub name: String,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FfiLatLon {
    pub lat: f64,
    pub lon: f64,
}

/// What a pack contains. Dates are days since 1970-01-01.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfiPackInfo {
    pub feed_id: String,
    pub feed_sha256: String,
    pub osm_sha256: String,
    pub format_version: u16,
    pub valid_from_days: i32,
    pub valid_to_days: i32,
    pub timezone: String,
    pub built_at_unix: i64,
    pub stops: u32,
    pub routes: u32,
    pub trips: u32,
    pub stop_times: u32,
    pub street_graph: bool,
    pub footpaths: u32,
}

/// A loaded pack. Shared by all calls; a new pack means a new engine (the old
/// one lives on until its running queries finish).
#[frb(opaque)]
pub struct RustlineEngine {
    inner: Arc<Engine>,
}

impl RustlineEngine {
    /// Opens a `.rlp` (or `.rlp.zst`) with the default 256 MiB body cap.
    pub fn open(path: String) -> Result<RustlineEngine, RustlineException> {
        RustlineEngine::open_with_limit(path, DEFAULT_MAX_BODY_BYTES)
    }

    /// Opens a pack whose body may be at most `max_body_bytes` (a lower cap
    /// for phones with little memory). A bigger file is refused before reading.
    pub fn open_with_limit(
        path: String,
        max_body_bytes: u64,
    ) -> Result<RustlineEngine, RustlineException> {
        guard(|| {
            let file = File::open(&path).map_err(io_error)?;
            if file.metadata().map_err(io_error)?.is_dir() {
                return Err(fail("pack_io", "the pack path is a directory"));
            }
            // Read through the same handle, at most one byte over the limit, so a
            // file that grows after the check cannot get past it.
            let limit = max_body_bytes.saturating_add(HEADER_SLACK);
            let mut bytes = Vec::new();
            file.take(limit.saturating_add(1))
                .read_to_end(&mut bytes)
                .map_err(io_error)?;
            if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
                return Err(fail("pack_too_large", "pack file is over the limit"));
            }
            let engine =
                Engine::from_bytes_with_limit(&bytes, max_body_bytes).map_err(pack_error)?;
            Ok(RustlineEngine {
                inner: Arc::new(engine),
            })
        })
    }

    /// Door-to-door itineraries.
    pub fn plan(&self, req: FfiPlanRequest) -> Result<FfiPlanResponse, RustlineException> {
        guard(|| {
            let r = self.inner.plan(&core_request(&req)?).map_err(plan_error)?;
            Ok(FfiPlanResponse {
                itineraries: r.itineraries.iter().map(itinerary).collect(),
                warnings: r.warnings.iter().map(warning).collect(),
            })
        })
    }

    /// Feed id, sha, validity and counts of the loaded pack.
    pub fn info(&self) -> Result<FfiPackInfo, RustlineException> {
        guard(|| Ok(pack_info(&self.inner)))
    }
}

/// Installs a downloaded `.rlp.zst` at `dest`. Holds an exclusive lock on
/// `{dest}.lock` for the whole install (a second install of the same `dest`
/// waits), removes temp files a killed install left behind, decompresses and
/// checks the pack in memory, checks the free space, writes its own
/// `{name}.{pid}.{n}.tmp` (create_new), syncs it and renames it over `dest`.
/// A bad download never touches an existing pack. The `.lock` file stays.
pub fn install_pack(src_zst: String, dest: String) -> Result<FfiPackInfo, RustlineException> {
    guard(|| install(&src_zst, &dest, None))
}

/// Debug builds only (tests): `install_pack` with a pretend free space.
#[cfg(debug_assertions)]
#[frb(ignore)]
pub fn install_pack_with_free_space(
    src_zst: String,
    dest: String,
    free_bytes: u64,
) -> Result<FfiPackInfo, RustlineException> {
    guard(|| install(&src_zst, &dest, Some(free_bytes)))
}

/// Test hook: in debug builds a panic through the same guard as `plan` (so it
/// comes back as `internal`); in profile/release builds it does nothing and
/// returns `invalid_request`. Visible to Dart for the integration test.
pub fn debug_inject_panic() -> Result<(), RustlineException> {
    guard(|| {
        if cfg!(debug_assertions) {
            panic!("injected panic (debug hook)");
        }
        Err(fail(
            "invalid_request",
            "debug_inject_panic works in debug builds only",
        ))
    })
}

/// Called once by `RustLib.init()`: replaces the default panic hook, which
/// would print the panic message with file:line to stderr / logcat.
#[frb(init)]
pub fn init_rustline() -> Result<(), RustlineException> {
    quiet_panics();
    Ok(())
}

/// Installs the quiet panic hook (once per process).
fn quiet_panics() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| std::panic::set_hook(Box::new(|_| eprintln!("rustline: panic caught"))));
}

/// Runs `f`; a panic becomes the fixed error `internal: internal error` (no
/// payload, no location, no backtrace).
fn guard<T>(f: impl FnOnce() -> Result<T, RustlineException>) -> Result<T, RustlineException> {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| Err(fail("internal", "internal error")))
}

fn io_error(e: io::Error) -> RustlineException {
    match e.kind() {
        io::ErrorKind::NotFound => fail("pack_missing", "pack file not found"),
        _ if e.raw_os_error() == Some(libc::ENOSPC) => fail("no_space", "no space left on device"),
        _ => fail("pack_io", format!("could not read or write the pack: {e}")),
    }
}

fn pack_error(e: PackError) -> RustlineException {
    match e {
        PackError::Io(e) => io_error(e),
        PackError::UnsupportedVersion { .. } => fail("pack_version", e.to_string()),
        PackError::BadMagic | PackError::Corrupt(_) => fail("pack_corrupt", e.to_string()),
    }
}

fn plan_error(e: PlanError) -> RustlineException {
    let code = match e {
        PlanError::InvalidRequest(_) => "invalid_request",
        PlanError::OutsideCoverage => "outside_coverage",
        PlanError::NoRoute => "no_route",
        PlanError::DateOutsideFeed => "date_outside_feed",
        PlanError::Internal(_) => "internal",
    };
    fail(code, e.to_string())
}

fn core_request(r: &FfiPlanRequest) -> Result<PlanRequest, RustlineException> {
    let bad = |what: &str| fail("invalid_request", format!("invalid request: {what}"));
    for (name, v, max) in [
        ("from_lat", r.from_lat, 90.0),
        ("from_lon", r.from_lon, 180.0),
        ("to_lat", r.to_lat, 90.0),
        ("to_lon", r.to_lon, 180.0),
    ] {
        if !v.is_finite() || v.abs() > max {
            return Err(bad(name));
        }
    }
    if r.depart_offset_secs.abs() > MAX_OFFSET_SECS {
        return Err(bad("depart_offset_secs"));
    }
    let offset = FixedOffset::east_opt(r.depart_offset_secs).ok_or_else(|| bad("offset"))?;
    let t = DateTime::from_timestamp_millis(r.depart_at_ms)
        .ok_or_else(|| bad("depart_at_ms"))?
        .with_timezone(&offset);
    let mut req = PlanRequest {
        from: LatLon::new(r.from_lat, r.from_lon),
        to: LatLon::new(r.to_lat, r.to_lon),
        time: TimeSpec::DepartAt(t),
        max_results: r.max_results,
        ..PlanRequest::default()
    };
    if let Some(m) = r.max_access_m {
        req.max_access_m = m;
    }
    if let Some(s) = r.walk_speed_mps {
        if !s.is_finite() || s <= 0.0 {
            return Err(bad("walk_speed_mps"));
        }
        req.walk_speed_mps = s;
    }
    if let Some(n) = r.max_transfers {
        req.max_transfers = n;
    }
    Ok(req)
}

fn ms(t: &DateTime<FixedOffset>) -> (i64, i32) {
    (t.timestamp_millis(), t.offset().local_minus_utc())
}

fn stop(s: &StopRef) -> FfiStop {
    FfiStop {
        id: s.id.clone(),
        name: s.name.clone(),
        lat: s.pos.map(|p| p.lat),
        lon: s.pos.map(|p| p.lon),
    }
}

fn line(g: &Option<rustline_core::Polyline>) -> Vec<FfiLatLon> {
    g.as_ref()
        .map(|p| {
            p.points()
                .iter()
                .map(|q| FfiLatLon {
                    lat: q.lat,
                    lon: q.lon,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn itinerary(i: &rustline_core::Itinerary) -> FfiItinerary {
    let (departure_ms, departure_offset_secs) = ms(&i.departure);
    let (arrival_ms, arrival_offset_secs) = ms(&i.arrival);
    let legs = i
        .legs
        .iter()
        .map(|l| match l {
            Leg::Walk {
                start,
                end,
                distance_m,
                from_stop,
                to_stop,
                geometry,
            } => {
                let ((start_ms, start_offset_secs), (end_ms, end_offset_secs)) =
                    (ms(start), ms(end));
                FfiLeg::Walk {
                    start_ms,
                    start_offset_secs,
                    end_ms,
                    end_offset_secs,
                    distance_m: *distance_m,
                    from_stop: from_stop.as_ref().map(stop),
                    to_stop: to_stop.as_ref().map(stop),
                    geometry: line(geometry),
                }
            }
            Leg::Transit {
                trip_id,
                route_id,
                route_name,
                headsign,
                from_stop,
                to_stop,
                board,
                alight,
                geometry,
            } => {
                let ((board_ms, board_offset_secs), (alight_ms, alight_offset_secs)) =
                    (ms(board), ms(alight));
                FfiLeg::Transit {
                    trip_id: trip_id.clone(),
                    route_id: route_id.clone(),
                    route_name: route_name.clone(),
                    headsign: headsign.clone(),
                    from_stop: stop(from_stop),
                    to_stop: stop(to_stop),
                    board_ms,
                    board_offset_secs,
                    alight_ms,
                    alight_offset_secs,
                    geometry: line(geometry),
                }
            }
        })
        .collect();
    FfiItinerary {
        departure_ms,
        departure_offset_secs,
        arrival_ms,
        arrival_offset_secs,
        transfers: i.transfers,
        walk_m: i.walk_m,
        legs,
    }
}

fn warning(w: &Warning) -> String {
    match w {
        Warning::StopNotLinkedToStreets(id) => format!("stop_not_linked_to_streets: {id}"),
        Warning::NoStreetGraph => "no_street_graph".to_string(),
        Warning::FeedExpiresSoon { valid_to } => format!("feed_expires_soon: {valid_to}"),
        Warning::ApproximateFrequencyTrip => "approximate_frequency_trip".to_string(),
    }
}

fn days(d: NaiveDate) -> i32 {
    let epoch = NaiveDate::from_ymd_opt(1970, 1, 1).unwrap_or_default();
    i32::try_from((d - epoch).num_days()).unwrap_or(i32::MAX)
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn pack_info(e: &Engine) -> FfiPackInfo {
    let i = e.info();
    FfiPackInfo {
        feed_id: i.feed_id,
        feed_sha256: i.feed_sha256,
        osm_sha256: i.osm_sha256,
        format_version: i.format_version,
        valid_from_days: days(i.valid_from),
        valid_to_days: days(i.valid_to),
        timezone: i.timezone,
        built_at_unix: i.built_at_unix,
        stops: count(i.stops),
        routes: count(i.routes),
        trips: count(i.trips),
        stop_times: count(i.stop_times),
        street_graph: i.street_graph,
        footpaths: count(i.footpaths),
    }
}

fn install(src: &str, dest: &str, free: Option<u64>) -> Result<FfiPackInfo, RustlineException> {
    let dest = Path::new(dest);
    let name = dest
        .file_name()
        .ok_or_else(|| fail("invalid_request", "invalid request: dest has no file name"))?
        .to_string_lossy()
        .into_owned();
    let dir = dest
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    // Held until this function returns; another install of `dest` waits here.
    let _lock = lock(&dir.join(format!("{name}.lock")))?;
    // Temp files of an install that was killed (none can be running: we hold
    // the lock). `{name}.tmp` is the name used before M6 review MF1.
    if let Ok(entries) = fs::read_dir(dir) {
        let prefix = format!("{name}.");
        for e in entries.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with(&prefix) && n.ends_with(".tmp") {
                let _ = fs::remove_file(e.path());
            }
        }
    }
    let limit = DEFAULT_MAX_BODY_BYTES + HEADER_SLACK;
    if fs::metadata(src).map_err(io_error)?.len() > limit {
        return Err(fail("pack_too_large", "download is over the pack limit"));
    }
    let compressed = fs::read(src).map_err(io_error)?;
    let mut bytes = Vec::new();
    zstd::Decoder::new(&compressed[..])
        .and_then(|d| d.take(limit + 1).read_to_end(&mut bytes))
        .map_err(|e| fail("pack_corrupt", format!("not a valid .rlp.zst: {e}")))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Err(fail(
            "pack_too_large",
            "pack unpacks to more than the limit",
        ));
    }
    let engine = Engine::from_bytes(&bytes).map_err(pack_error)?;
    let need = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let free = match free {
        Some(f) => f,
        None => free_space(dir)?,
    };
    if need > free {
        return Err(fail(
            "no_space",
            format!("the pack needs {need} bytes, {free} are free"),
        ));
    }
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = dir.join(format!("{name}.{}.{n}.tmp", std::process::id()));
    let written = write_synced(&tmp, &bytes).and_then(|()| fs::rename(&tmp, dest));
    if let Err(e) = written {
        let _ = fs::remove_file(&tmp);
        return Err(io_error(e));
    }
    // Make the rename durable (best effort; not every platform allows it).
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(pack_info(&engine))
}

fn write_synced(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    f.write_all(bytes)?;
    f.sync_all()
}

/// Bytes free for this app in `dir`.
#[cfg(unix)]
fn free_space(dir: &Path) -> Result<u64, RustlineException> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(dir.as_os_str().as_bytes())
        .map_err(|_| fail("invalid_request", "invalid request: NUL in dest"))?;
    // SAFETY: statvfs is plain data, so all zeros is a valid value.
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is a NUL-terminated path and `s` a valid out pointer.
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return Err(io_error(io::Error::last_os_error()));
    }
    // u64 on 64-bit targets, narrower on some 32-bit ones.
    #[allow(clippy::useless_conversion)]
    let block = if s.f_frsize == 0 {
        u64::from(s.f_bsize)
    } else {
        u64::from(s.f_frsize)
    };
    #[allow(clippy::useless_conversion)]
    let free = u64::from(s.f_bavail).saturating_mul(block);
    Ok(free)
}

/// Not unix (not a target today): the free-space check is off.
#[cfg(not(unix))]
fn free_space(_dir: &Path) -> Result<u64, RustlineException> {
    Ok(u64::MAX)
}

/// An exclusive lock on `path` (created if missing), released when the file
/// is dropped.
#[cfg(unix)]
fn lock(path: &Path) -> Result<File, RustlineException> {
    use std::os::unix::io::AsRawFd;
    let f = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(io_error)?;
    // SAFETY: the fd belongs to `f`, which outlives the call.
    if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(io_error(io::Error::last_os_error()));
    }
    Ok(f)
}

/// Not unix (not a target today): no lock.
#[cfg(not(unix))]
fn lock(path: &Path) -> Result<File, RustlineException> {
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(io_error)
}
