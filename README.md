<p align="center">
  <img src="Documentation/images/rustline-logo.svg" alt="Rustline logo" width="600">
</p>

# Rustline: offline GTFS journey planner (RAPTOR) in Rust + Flutter

Rustline is an offline public transport trip planner and transit routing library in Rust: RAPTOR over GTFS + OpenStreetMap, with multiple journey options and a Flutter plugin.

> An offline, on-device public-transport journey planner in Rust: it answers "I am at A at time T and want to get to B — what should I do?" from GTFS timetables and OpenStreetMap streets, without a server.

[![CI](https://github.com/HoriaSav/Rustline/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/HoriaSav/Rustline/actions/workflows/ci.yml)
![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)
![Status](https://img.shields.io/badge/status-in%20development-yellow)
![Rust](https://img.shields.io/badge/rust-1.88%2B-orange)

## Table of Contents

- [Overview](#overview)
- [Features](#features)
- [Preview](#preview)
- [Tech Stack](#tech-stack)
- [Getting Started](#getting-started)
- [Usage](#usage)
- [Project Structure](#project-structure)
- [Configuration](#configuration)
- [Testing](#testing)
- [Continuous integration](#continuous-integration)
- [Roadmap](#roadmap)
- [Similar projects](#similar-projects)
- [Contributing](#contributing)
- [License](#license)
- [Contact](#contact)

## Overview

**Rustline** is a journey-planning library (with a CLI and a Flutter plugin) designed to plan door-to-door public-transport trips fully offline, on the phone.

It solves "how do I get from A to B by bus?" by turning a standard **GTFS** feed plus an **OpenStreetMap** extract into a compact routing *pack* (`.rlp` / `.rlp.zst`), then running the **RAPTOR** algorithm on it. The answer is a short list of itineraries, each a sequence of legs (walk, ride, transfer, walk).

It is a standalone Rust library first; the RoTransit Flutter app (Brașov and other Romanian cities) is its first user.

## Features

- **GTFS ingest** — loads any GTFS feed (zip or directory) and prints counts, timezone, validity window, patterns and a validation report (`rustline inspect`, `--json` available).
- **Reproducible routing packs** — `rustline build-pack` writes `rustline-{feedId}-{sha12}-v4.rlp` and `.rlp.zst`; `--built-at` / `SOURCE_DATE_EPOCH` gives byte-identical packs. Bad files return errors, never panic.
- **RAPTOR stop-to-stop and door-to-door** — Pareto set on (arrival, rides), checked against a CSA oracle on 1 000 random queries per city.
- **Multiple journey options (rRAPTOR)** — a departure window (depart-at) or arrival window (arrive-by) of up to 2 h returns up to 20 options, each tagged `Fastest`, `FewestTransfers` or `LeavesLatest`.
- **Walking on real streets** — OSM foot graph, snapping (≤ 300 m), footpaths between stops (≤ 500 m), walk-only answers up to 3 km.
- **Calendar and time edge cases** — service days D-1/D/D+1 across midnight, DST-aware (chrono-tz), `calendar_dates`, `frequencies.txt`, `transfers.txt` types 0–3, feed-expiry warnings.
- **Flutter plugin** — `rustline_flutter` (flutter_rust_bridge 2.13.0) with stable error codes, safe atomic pack install, and a "newest search wins" planner; Android arm64-v8a and x86_64.

## Preview

Real output of `rustline plan` on the Brașov pack:

```text
45.6427,25.5887 → 45.6579,25.6012, 2026-10-15T08:00:00+03:00

1. 08:00:27+03:00 → 08:22:51+03:00, 0 transfer(s), 966 m walking
   08:00:27+03:00 → 08:06:00+03:00  walk 432 m, start → Livada Poștei [node/2375041371]
   08:06:00+03:00 → 08:16:00+03:00  line 36, Livada Poștei [node/2375041371] → Mircea cel Bătrân [node/3652479574] (trip 36_Mo-Fr_LivadaPostei-Independentei_15)
   08:16:00+03:00 → 08:22:51+03:00  walk 534 m, Mircea cel Bătrân [node/3652479574] → destination
```

`plan --geojson /tmp/j.geojson` writes the legs as LineStrings you can view on [geojson.io](https://geojson.io).

## Tech Stack

| Category | Technology |
|----------|------------|
| Language | Rust (stable, pinned in `rust-toolchain.toml`; MSRV 1.88, `rust-version` in `Cargo.toml`); Dart for the plugin |
| Framework | flutter_rust_bridge **2.13.0** + Cargokit (Flutter plugin) |
| Data | GTFS feeds, OpenStreetMap `.osm.pbf` extracts, own binary pack format v4 (zstd-compressed) |
| Tools | Cargo workspace, criterion benchmarks, chrono / chrono-tz, `cargo-ndk` 4.1.2, Android NDK 26.1.10909125, GitHub Actions |

Rustline uses flutter_rust_bridge 2.13.0 (Rust crate, Dart package and codegen; the three versions must match) and is built and tested with Flutter 3.47.8. An app using the plugin needs Flutter 3.35.3 or newer (Dart ≥ 3.9.2), as set in `rustline_flutter/pubspec.yaml`; RoTransit (Flutter 3.27, Dart 3.6) has to upgrade Flutter before it can use this version.

## Getting Started

### Prerequisites

- [rustup](https://rustup.rs) (the toolchain in `rust-toolchain.toml`, including the Android targets `aarch64-linux-android` and `x86_64-linux-android`, installs itself on first use)
- For Android: `cargo install cargo-ndk` and Android NDK 26.1.10909125
- For the Flutter plugin: Flutter 3.47.8 and `flutter_rust_bridge_codegen` 2.13.0

### Installation

1. Clone the repository:

   ```bash
   git clone https://github.com/HoriaSav/Rustline.git
   ```

2. Navigate to the project directory:

   ```bash
   cd Rustline
   ```

3. Build the workspace (and, optionally, fetch the shared test feeds into `fixtures/feeds/`):

   ```bash
   cargo build --locked --release --workspace
   scripts/fetch-fixtures.sh      # downloads and checks feeds listed in fixtures/manifest.tsv
   ```

4. Run the CLI:

   ```bash
   cargo run --locked --release -p rustline-cli -- inspect data/fixtures/tiny
   ```

Android cross-compile:

```bash
export ANDROID_NDK_HOME=$HOME/Android/Sdk/ndk/26.1.10909125
cargo ndk -t arm64-v8a build --locked --release -p rustline-ffi   # what CI (android.yml) runs
```

## Usage

### CLI

Inspect a feed (`--json` for machine-readable output):

```text
cargo run --locked --release -p rustline-cli -- inspect data/fixtures/tiny
```

Expected output (start):

```text
feed        data/fixtures/tiny
timezone    America/Toronto
valid       2030-01-01 → 2030-12-31

feed rows
  stops                      5   (+0 stations, 0 other)
  routes                     3
  trips                     10
  stop_times                34
  ...
timetable
  patterns                   4   (1 from overtaking splits)

errors: none
```

Build a door-to-door pack (the area comes from the feed, never from the code) and plan:

```bash
BBOX=$(cargo run -q --locked --release -p rustline-cli -- bbox --gtfs fixtures/feeds/brasov-latest.zip --margin-km 2)
cargo run --locked --release -p rustline-cli -- build-pack --gtfs fixtures/feeds/brasov-latest.zip \
  --osm fixtures/feeds/romania-260101.osm.pbf --bbox "$BBOX" --out out/ --feed-id brasov --built-at 1700000000
cargo run --locked --release -p rustline-cli -- plan --pack out/rustline-brasov-ad0d22ca80b0-v4.rlp \
  --from 45.6427,25.5887 --to 45.6579,25.6012 --at 2026-10-15T08:00 --geojson /tmp/j.geojson
```

The output is shown under [Preview](#preview). Other commands:

- `plan --from-stop <id> --to-stop <id>` — stop-to-stop RAPTOR; lists the whole Pareto set.
- `inspect <pack>.rlp` — prints `PackInfo` (counts, sha, validity, timezone, format version).
- `bbox` prints `min_lon,min_lat,max_lon,max_lat` (the order `osmium extract -b` takes).
- Without `--osm` every walk is a straight line × 1.3 and `PackInfo::street_graph` is false.
- `--arrive-by <time>` instead of `--at` (exactly one of the two): arrive by then, leaving as late as possible.
- `--window <w>` (door-to-door only): plain seconds, or `30m` / `2h`; at most 2 h (7 200 s). With `--at` it searches every departure in `[at, at + w]`; with `--arrive-by` every arrival in `[deadline − w, deadline]`. Tags print after each itinerary, e.g. ` [fastest, leaves_latest]`.
- `--max-results <n>` (door-to-door only): 1–20, default 5.
- `--window` or `--max-results` with `--from-stop` / `--to-stop` is an error.
- `--at`: an ambiguous local time (DST fall-back) takes the earlier instant; a time in the DST gap moves forward.

### Rust API (`rustline-core`)

```rust
use chrono::DateTime;
use rustline_core::{Engine, Leg, PlanRequest, TimeSpec};
use rustline_model::LatLon;
use std::path::Path;

let engine = Engine::open(Path::new("out/rustline-brasov-ad0d22ca80b0-v4.rlp")).expect("pack");
let req = PlanRequest {
    from: LatLon { lat: 45.6427, lon: 25.5887 },
    to: LatLon { lat: 45.6579, lon: 25.6012 },
    time: TimeSpec::DepartAt(DateTime::parse_from_rfc3339("2026-10-15T08:00:00+03:00").unwrap()),
    ..PlanRequest::default() // 1.3 m/s, 1 000 m access, 500 m transfer, 3 transfers, 5 results
};
let resp = engine.plan(&req).expect("plan");
for it in &resp.itineraries {
    println!("{} → {}, {} transfer(s)", it.departure, it.arrival, it.transfers);
    for leg in &it.legs {
        match leg {
            Leg::Walk { distance_m, .. } => println!("  walk {distance_m} m"),
            Leg::Transit { route_name, .. } => println!("  line {route_name}"),
        }
    }
}
```

API rules:

- `PlanRequest { from, to, time: TimeSpec::DepartAt(..) | TimeSpec::ArriveBy(..), walk_speed_mps, max_access_m, max_transfer_m, max_transfers, max_results, window_secs }`.
- Limits (else `PlanError::InvalidRequest`): valid lat/lon; speed in (0, 3] m/s; `max_access_m` ≤ 3 000; `max_transfer_m` ≤ 500; `max_results` 1–20 (`MAX_RESULTS`, default 5); `max_transfers` ≤ 8 (`MAX_TRANSFERS`, default 3); `window_secs` ≤ 7 200 (`MAX_WINDOW_SECS`, default 0).
- Breaking change (M9): `max_transfers` above 8 is now `InvalidRequest` (`invalid_request` over FFI); before M9 it was silently capped at 8.
- `window_secs` (M9): 0 is a single instant (M8 behaviour). Depart-at returns journeys that leave the door in `[at, at + window_secs]`; arrive-by returns journeys that arrive in `[deadline − window_secs, deadline]`. If the window finds nothing, the answer for the instant itself is returned.
- Range limit: one range search considers at most the 256 departures nearest the requested time (`MAX_RANGE_DEPARTURES`); arrive-by runs at most 256 searches stepping back from the deadline.
- Results are the Pareto front on (arrival, rides). Order: with a window (`window_secs` > 0), sorted by door departure, then arrival; without one, depart-at is sorted by arrival, then fewer rides, and arrive-by by latest departure first, then fewer rides, less walking, earlier arrival. Over `max_results`, the fastest, the fewest-transfers and the leaves-latest itinerary are always kept (picked in the order fastest, fewest-transfers, leaves-latest for depart-at and leaves-latest, fewest-transfers, fastest for arrive-by, while `max_results` allows; so `max_results` 1 matches M8), then the rest fill the remaining slots: in order for depart-at, latest departure first for arrive-by. The kept itineraries keep that order.
- `Itinerary::tags: Vec<Tag>` (ties get the tag too): `Tag::Fastest` (earliest arrival, in both depart-at and arrive-by), `Tag::FewestTransfers` (fewest transfers; walking only counts as 0), `Tag::LeavesLatest` (latest departure).
- A point more than 300 m from every street → `OutsideCoverage`; a date outside the feed → `DateOutsideFeed`; nothing usable → `NoRoute`.
- `PlanResponse { itineraries, warnings }`; `Itinerary { legs, departure, arrival, transfers, walk_m, tags }` with times as `DateTime<FixedOffset>` in the feed's zone. Warnings include `StopNotLinkedToStreets`, `NoStreetGraph`, `FeedExpiresSoon` (≤ 14 days) and `ApproximateFrequencyTrip`.
- `Engine::from_bytes`, `Engine::info() -> PackInfo` and `Engine::plan_stop_to_stop` are also available.
- `rustline_osm::build_foot_graph(path, Option<BBox>)` / `build_foot_graph_with_limits(..., &OsmLimits)` (defaults: blobs ≤ 32 MiB, ≤ 8 M nodes, ≤ 1 M walkable ways).

### Flutter / Dart API (`rustline_flutter`)

```dart
import 'package:rustline_flutter/rustline_flutter.dart';

await RustLib.init();
final info = await installPack(srcZst: downloadedZstPath, dest: '$appDir/brasov.rlp'); // atomic, streamed, checks free space; header fields only (counts 0)
final engine = await RustlineEngine.open(path: '$appDir/brasov.rlp');
final planner = RustlinePlanner(engine: engine);

final now = DateTime.now();
final resp = await planner.plan(FfiPlanRequest(
  fromLat: 45.6427, fromLon: 25.5887,
  toLat: 45.6579, toLon: 25.6012,
  timeKind: FfiTimeKind.departAt, // or FfiTimeKind.arriveBy: timeMs is then the deadline
  timeMs: now.millisecondsSinceEpoch,
  timeOffsetSecs: now.timeZoneOffset.inSeconds,
  maxResults: 5, // 1–20
  windowSecs: 1800, // optional (M9): null or 0 = one instant, at most 7200
));
if (resp == null) return; // superseded by a newer search
for (final it in resp.itineraries) {
  print('${it.transfers} transfer(s), ${it.walkM} m walking, ${it.legs.length} legs, ${it.tags}');
}
planner.cancel(); // e.g. when leaving the screen
```

- Every Rust error arrives as `RustlineException { code, message }` with a stable `code`: `invalid_request`, `outside_coverage`, `no_route`, `date_outside_feed`, `pack_corrupt`, `pack_version`, `pack_missing`, `pack_io`, `pack_too_large`, `no_space`, `internal`.
- `FfiPlanRequest.windowSecs` (`int?`) is the departure window (arrive-by: arrival window); `maxResults` is 1–20; `maxTransfers` above 8 is `invalid_request`.
- `FfiItinerary.tags` is a `List<FfiTag>`: `FfiTag.fastest`, `FfiTag.fewestTransfers`, `FfiTag.leavesLatest` (same meaning as `Tag` in Rust; `fastest` is the earliest arrival in both modes).
- `RustlinePlanner`: the newest search wins; a superseded one resolves to `null`. Each Rust call is bounded (≤ 1 s).
- Host (desktop) runs load the library explicitly: `RustLib.init(externalLibrary: ExternalLibrary.open('target/release/librustline_ffi.so'))`.

The Dart API is generated from `crates/rustline-ffi/src/api.rs`:

```bash
cd rustline_flutter && flutter_rust_bridge_codegen generate        # after changing api.rs
cd example && flutter build apk --release --target-platform android-arm64,android-x64
```

## Project Structure

```text
Rustline/
├── crates/
│   ├── rustline-model/   # Shared types: LatLon, ids, GTFS time parsing, Journey/Leg
│   ├── rustline-gtfs/    # GTFS reader, validation report, timetable/pattern builder
│   ├── rustline-osm/     # OSM .pbf reader → walkable foot graph
│   ├── rustline-pack/    # Pack format v4 (.rlp / .rlp.zst): write, read, validate
│   ├── rustline-core/    # Engine: RAPTOR, street search, PlanRequest/PlanResponse
│   ├── rustline-ffi/     # flutter_rust_bridge API (plain-data types, install_pack)
│   └── rustline-cli/     # `rustline` CLI: inspect, bbox, build-pack, plan
├── rustline_flutter/     # Flutter plugin (Cargokit) + example/ test app
├── benches/              # criterion benchmark (plan)
├── tests/golden/         # Golden Brașov journeys
├── data/fixtures/        # Small committed feeds: tiny (clean), broken (one of each error)
├── fixtures/             # manifest.tsv; feeds/ is git-ignored, filled by scripts/
├── scripts/              # fetch-fixtures.sh
├── Documentation/Plan/   # Rustline-Spec.md, Rustline-Implementation-Plan.md
├── .github/workflows/    # ci.yml, android.yml, bench.yml
├── rust-toolchain.toml   # Pinned Rust toolchain and Android targets
├── LICENSE-APACHE
├── LICENSE-MIT
└── README.md
```

## Configuration

Rustline needs no config file or secrets. These environment variables are used by the CLI and tests:

| Variable | Description | Required |
|----------|-------------|----------|
| `SOURCE_DATE_EPOCH` | Fixed build time stored in the pack header (same as `--built-at`), for byte-identical packs | No |
| `ANDROID_NDK_HOME` | Android NDK path for `cargo ndk` builds | Android only |
| `RUSTLINE_MANIFEST_FEED` | Feed used by the ignored real-feed tests | No |
| `RUSTLINE_MANIFEST_COUNTS` | Expected `stops,routes,trips,stop_times,services,exceptions` for that feed | No |

**Security:** Never commit real passwords, API keys, tokens, or other secrets to the repository.

## Testing

```bash
cargo test --locked --workspace                                  # all tests
cargo fmt --all --check                                          # formatting
cargo clippy --locked --workspace --all-targets -- -D warnings   # lints
cargo test --locked -p rustline-core --features oracle --test oracle_feature
cargo bench -p rustline-core                                     # criterion, fixed seeds
```

Slow tests are `#[ignore = "slow: ..."]` and need `fixtures/feeds/`:

```bash
RUSTLINE_MANIFEST_FEED=fixtures/feeds/brasov-manifest.zip \
RUSTLINE_MANIFEST_COUNTS=811,84,8075,118534,5,110 \
cargo test --locked -p rustline-gtfs --test manifest_feed -- --ignored
cargo test --locked --release -p rustline-core --test m3_raptor --test m3_budget -- --ignored --nocapture   # RAPTOR == CSA, MBTA time/memory
cargo test --locked --release -p rustline-cli --test m4_real --test m4_budget -- --ignored --nocapture --test-threads 1
cargo test --locked --release -p rustline-osm --test m4_osm -- --ignored --nocapture
```

(The Brașov manifest's `departure_rows` = 157 698 counts one row per stop time *per day kind*; the GTFS stop_times count is 118 534.)

The Flutter plugin's integration tests run on an x86_64 android-34 emulator (AVD `rustline_x64`) locally, or in CI with `android-emulator.yml`. See [Continuous integration](#continuous-integration).

## Continuous integration

GitHub Actions workflows in `.github/workflows/`:

| Workflow | Runs on | Jobs |
|---|---|---|
| `ci.yml` (CI) | push to `main`, every pull request, manual | `rust`: `cargo fmt --check`, `clippy -D warnings`, tests debug + release (stable); `msrv`: tests on Rust 1.88; `flutter`: `flutter analyze` (plugin + example), plugin `flutter test`, and a check that `flutter_rust_bridge_codegen generate` (2.13.0) leaves no diff |
| `android.yml` (Android) | push to `main`, every pull request, manual | `ffi`: `cargo ndk` release build of `rustline-ffi` for arm64-v8a and x86_64 on Rust 1.88, `.so` size in the job summary; `apk`: example `flutter build apk --release`, APK and `.so` sizes in the summary |
| `android-emulator.yml` | manual only | example `integration_test` on an x86_64 API 34 emulator; pack-based tests print `skip:` (no pack) |
| `real-feeds.yml` | manual, and Mondays 04:00 UTC | fetches and verifies real feeds, then `cargo test --workspace --release` with them present |
| `bench.yml` | manual only | `cargo bench -p rustline-core --bench plan` (spot checks; shared runners are noisy) |

- Pull requests use the read-only `GITHUB_TOKEN` and no secrets (`pull_request`, not `pull_request_target`). Failed runs are not retried automatically.
- `fixtures/feeds/` is git-ignored, so in `ci.yml` the real-feed tests skip. `scripts/fetch-fixtures.sh` downloads missing files listed in `fixtures/manifest.tsv` and checks each against its pinned sha256. A changed upstream file is saved as `<file>.new` and fails (`CHANGED`). `--verify` only checks; `--skip-manual` reports files without a public URL as `SKIP` (used by `real-feeds.yml` and `bench.yml`). `real-feeds.yml` caches the feeds keyed by the manifest hash.
- Dependabot (`.github/dependabot.yml`) opens one grouped PR per week each for Cargo and GitHub Actions (at most 5 open). `gtfs-structures` is excluded from the Cargo group and gets its own PR, because its 0.x bumps change the API (it is pinned with `=`). Ignored: `flutter_rust_bridge` (upgraded by hand with the codegen, the Dart package and the regenerated bindings) and `dtolnay/rust-toolchain` (pinned to a commit).

## Roadmap

- [x] M0 — Setup: toolchain, workspace, CI, hello-world FRB on Android
- [x] M1 — GTFS ingest: `inspect` counts match the manifest
- [x] M2 — Pack v1: timetable saved to `.rlp` and loaded back
- [x] M3 — RAPTOR stop-to-stop, checked by CSA
- [x] M4 — Streets: OSM graph, snapping, access/egress, walk-only
- [x] M5 — Calendar & edge cases: midnight, DST, holidays, frequencies, transfers.txt, benchmarks
- [x] M6 — Flutter integration: plugin + Android build (engine ready, not yet wired into RoTransit)
- [ ] M7 — Cleanup: M1 Known gaps #1–3, #5, #6 + M6 Reviewer leftovers *(built, in review)*
- [x] M8 — Arrive-by: "be there by 09:00" (reverse RAPTOR)
- [x] M9 — rRAPTOR options: Pareto (arrival vs transfers) + departure-window range queries
- [x] M10 — GitHub CI: fmt, clippy, tests debug+release, Flutter tests, Android build
- [ ] M11 — iOS build: xcframework + Cargokit iOS on GitHub macOS runners
- [ ] M12 — Server (optional): same engine behind `POST /v1/plan`
- [ ] M13 — RoTransit integration
- [ ] M14 — Future: backend-served packs, realtime, multi-city, McRAPTOR walking

### Known gaps

**M1** – all closed in M7: trips.txt and stop_times.txt are read by Rustline's own lenient reader (`crates/rustline-gtfs/src/reader.rs`; streaming, columns by name, rows capped at 64 KiB). A missing file or required column still fails the feed; a bad row is dropped (or padded) with a warning. The new kinds `InvalidTime`, `DuplicateTripId`, `DuplicateStopSequence`, `ShortRow`, `RowTooLong` and `InvalidUtf8` are warnings (Leader, M7). Tests: `edge_cases.rs`, `m7_reader.rs`.

1. ~~**Time range check**~~ – fixed in M7: `24:60:00`, `-1:00:00`, `08:00` … are an `InvalidTime` row (dropped).
2. ~~**Huge hours**~~ – fixed in M7: hours must be below 48 (`47:59:59` loads, `48:00:00` is `InvalidTime`), same in debug and release.
3. ~~**Duplicate trip_id**~~ – fixed in M7: the first row wins, the others are `DuplicateTripId`.
4. ~~**IANA timezone**~~ – fixed in M5: a bad `agency_timezone` fails the build (`InvalidTimezone`) and `Engine::open`.
5. ~~**Short CSV rows / strict loading**~~ – fixed in M7: short rows are padded (`ShortRow`); a stop_times row with an unknown stop drops its trip (`UnknownStop`); Google's `sample-feed-1.zip` passes `inspect` and `build-pack`. Rows whose trip is not in trips.txt are dropped and reported (`UnknownTrip`).
6. ~~**Duplicate stop_sequence**~~ – fixed in M7: the trip is dropped with `DuplicateStopSequence`.

**M3**

7. **block_id** – stored but not used: staying seated counts as a transfer. Test: `block_id_is_treated_as_a_normal_transfer`.
8. **transfers.txt** – rows with trip/route ids, types 4/5 and unknown types are skipped with `UnsupportedTransfer`; unknown stops give `UnknownTransferStop`. Trip/route transfers are not applied.
9. **Sibling platforms** – linked only by transfers.txt and footpaths; a parent-station query uses all platforms with 0 s access.
10. ~~One service day~~ – fixed in M5 (D-1, D, D+1, DST-aware).
11. ~~A self-transfer hides a walk~~ – fixed in the M3 review round.
12. ~~No max_results~~ – closed in M4 (`plan --from-stop/--to-stop` still lists the whole Pareto set).
13. **Duplicate transfers.txt rows** – one row kept per (from, to): the longest `min_transfer_time` (ties: higher `transfer_type`); others counted as `DuplicateTransfer`.

**M4**

14. **Arrive-by** – looks back over the same service days D − 1, D and D + 1 of the deadline's date (M8).
15. **Tag list** – `cycleway` (and unlisted values) is walkable only with an explicit `foot` tag.
16. **OSM order** – nodes must come before ways, else `OsmError::Unsupported`; relations are ignored.
17. **Bbox edge** – a way leaving the bbox is cut at its last node inside.
18. **Transit geometry** – rides are drawn stop to stop (shapes.txt unused).
19. **0-ride RAPTOR results** – dropped; the direct street walk stands in.
20. **Rounding** – footpath times are whole seconds; one-to-many walks capped by real metres while steps cost × 1.5.
21. **Unlinked stops** – a stop > 100 m from any street uses a straight line × 1.3 and `StopNotLinkedToStreets` (Brașov: 2 stops).

**M5**

22. **RAPTOR vs CSA across midnight** – the CSA oracle is single-day; multi-day answers are checked by hand tests and properties only.
23. **Frequency caps** – a feed over 10 000 trips per row / 200 000 trips / 5 000 000 stop times fails to build instead of being cut down.

**M6**

24. **Emulator only** – integration tests run on an x86_64 android-34 emulator, not on a phone yet (A54 numbers pending).
25. **RoTransit not wired** – bundling, freshness banner, planner screen and attribution wait for M13.
26. **Cancellation** – a superseded answer is dropped; the Rust call itself is not interrupted (bounded ≤ 1 s, worst 2.7 ms measured on the PC).
27. **ABIs** – only arm64-v8a and x86_64; armeabi-v7a is not supported.
28. **Host library path** – generated `frb_generated.dart` looks in `../crates/rustline-ffi/target/release/`; host runs pass `RustLib.init(externalLibrary: ...)`.

**M7**

29. **Zip decompressed size** – `build-pack` streams trips.txt and stop_times.txt out of the zip without a cap on decompressed bytes or rows, so a zip bomb can grow memory (as before M7; desktop tool only).
30. **trips.txt read twice** – `gtfs-structures` 0.52 always parses trips.txt in raw mode before Rustline's reader replaces it; its API has no switch to skip it.

**gtfs-structures 0.52**

31. **Uncapped feed files** – `gtfs-structures` 0.52 caps none of the files it reads: agency.txt, routes.txt, stops.txt, trips.txt, calendar.txt, calendar_dates.txt, shapes.txt, fare_attributes.txt, fare_rules.txt, frequencies.txt, transfers.txt, pathways.txt, feed_info.txt, translations.txt, levels.txt (new in 0.52), attributions.txt (since 0.50) and licenses.txt (since 0.51), plus the newer fare and ticketing files (fare_products.txt, fare_media.txt, rider_categories.txt, ticketing_*.txt), are each parsed whole, with no cap on bytes or rows, so a huge or zip-bombed file can grow memory (desktop tool only). stop_times.txt is turned off there (Rustline streams it itself; see #29). Rustline's own cap covers only transfers.txt (256 MiB, `load.rs`). levels.txt is still read and parsed, then dropped (`raw.levels = None` in `rustline-gtfs` `load.rs`): Rustline does not use it, and a bad row there no longer fails the build.

## Similar projects

- [OpenTripPlanner](https://www.opentripplanner.org/), a multimodal trip planner in Java that runs as a server.
- [MOTIS](https://github.com/motis-project/motis), an intermodal routing and timetable platform in C++ that runs as a server.
- [Navitia](https://github.com/hove-io/navitia), an open-source journey planning and transit data API that runs as a hosted service.
- [r5](https://github.com/conveyal/r5), Conveyal's RAPTOR-based routing engine in Java for transport network analysis.

These engines run on servers. Rustline is a library that runs on the device: it plans trips offline from a pack on the phone and plugs into mobile apps through its Flutter plugin.

## Contributing

Contributions, suggestions, and bug reports are welcome.

1. Fork the repository.
2. Create a branch for your changes.
3. Make your changes and add tests where appropriate (`cargo fmt`, `cargo clippy -D warnings` and `cargo test` must pass).
4. Commit your changes with a descriptive message.
5. Open a pull request explaining what you changed and why.

The design lives in [Documentation/Plan/Rustline-Spec.md](Documentation/Plan/Rustline-Spec.md) and the milestones in [Documentation/Plan/Rustline-Implementation-Plan.md](Documentation/Plan/Rustline-Implementation-Plan.md).

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at your option.

**Data attribution:** Map data © OpenStreetMap contributors, available under the [Open Database License (ODbL)](https://opendatacommons.org/licenses/odbl/). Timetable data from RATBV Brașov; any derived databases are subject to the ODbL where applicable.

## Contact

**Author:** [Horia Savin](https://github.com/HoriaSav)

- GitHub: [@HoriaSav](https://github.com/HoriaSav)
- Issues: [Report a bug or request a feature](https://github.com/HoriaSav/Rustline/issues)

For questions or feedback, open an issue in the repository.
