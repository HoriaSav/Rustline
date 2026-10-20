# Rustline

Rustline answers one question: *"I am at point A at time T and want to get to point B — what should I do?"* The answer is a short list of itineraries, each a sequence of legs (walk, ride a bus, transfer, walk). It reads standard **GTFS** timetables and an **OpenStreetMap** street graph for walking. It is a standalone Rust library first; the RoTransit Flutter app is its first user.

**Status:** M2 (Pack v1, timetable only).

## Building

Prerequisites: [rustup](https://rustup.rs) (the toolchain in `rust-toolchain.toml` is installed automatically on first use).

```bash
cargo build --locked --workspace                          # build everything
cargo test --locked --workspace                           # run all tests
cargo fmt --all --check                                   # formatting
cargo clippy --locked --workspace --all-targets -- -D warnings   # lints
```

Minimum supported Rust version: 1.85 (`rust-version` in `Cargo.toml`).

### Android cross-compile

Needs `cargo install cargo-ndk` and an Android NDK (e.g. `~/Android/Sdk/ndk/26.1.10909125`):

```bash
export ANDROID_NDK_HOME=$HOME/Android/Sdk/ndk/26.1.10909125
cargo ndk -t arm64-v8a build --locked -p rustline-core
```

## Inspecting a GTFS feed

`rustline inspect` loads any GTFS feed (zip or directory), prints row counts, the
timezone and validity window (taken from the feed), the patterns it built, and a
validation report. Add `--json` for machine-readable output.

```bash
cargo run --locked --release -p rustline-cli -- inspect data/fixtures/tiny
cargo run --locked --release -p rustline-cli -- inspect fixtures/feeds/brasov-manifest.zip --json
```

Shared test feeds live in `fixtures/feeds/` (git-ignored); `scripts/fetch-fixtures.sh`
downloads and verifies them against `fixtures/manifest.tsv`. Any other GTFS feed works too,
e.g. `curl -L -o data/raw/feed.zip https://files.mobilitydatabase.org/<mdb-id>/latest.zip`.

To check a feed against known counts (`stops,routes,trips,stop_times,services,exceptions`):

```bash
RUSTLINE_MANIFEST_FEED=fixtures/feeds/brasov-manifest.zip \
RUSTLINE_MANIFEST_COUNTS=811,84,8075,118534,5,110 \
cargo test --locked -p rustline-gtfs --test manifest_feed -- --ignored
```

(That Brașov feed is the one RoTransit's companion manifest was built from. The
manifest's `departure_rows` = 157 698 counts one row per stop time *per day kind*;
the GTFS stop_times count is 118 534.)

## Building a pack

`rustline build-pack` turns a GTFS feed into a routing pack: `rustline-{feedId}-{sha12}-v4.rlp` (format version 4)
and the same file compressed as `.rlp.zst` (the loader accepts both). The header carries the
SHA-256 of the feed zip, so an app can compare it with the backend's `current.sha256`.
`--feed-id` defaults to the feed's file name. `--built-at <unix seconds>` (or `SOURCE_DATE_EPOCH`)
fixes the build time stored in the header, so the same feed always gives a byte-identical pack.

```bash
cargo run --locked --release -p rustline-cli -- build-pack --gtfs fixtures/feeds/brasov-manifest.zip --out out/ --feed-id myfeed --built-at 1700000000
cargo run --locked --release -p rustline-cli -- inspect out/rustline-myfeed-627559bac2a3-v4.rlp
cargo test --locked -p rustline-pack                     # round trip + bad-file tests on the fixture
RUSTLINE_MANIFEST_FEED=fixtures/feeds/brasov-manifest.zip \
cargo test --locked --release -p rustline-pack --test round_trip -- --ignored --nocapture   # real feed, all trips
RUSTLINE_MANIFEST_FEED=fixtures/feeds/brasov-manifest.zip \
cargo test --locked --release -p rustline-core --test engine -- --ignored --nocapture       # load time, median of 10
```

## Planning a journey (M3)

`rustline plan --from-stop/--to-stop` runs RAPTOR between two GTFS stop ids of a pack.
Since M4 it also walks between nearby stops (the pack's footpaths, see below).

A `transfers.txt` row straight from the origin stop to the destination stop gives a
0-ride journey (just that walk), listed like any other journey.

```bash
cargo run --locked --release -p rustline-cli -- plan --pack out/rustline-myfeed-627559bac2a3-v4.rlp --from-stop node/3652436629 --to-stop node/3708889570 --at 2026-10-15T08:00
cargo test --locked --release -p rustline-core --test m3_raptor -- --nocapture      # fixture and edge-case tests
```

The slow tests are `#[ignore = "slow: ..."]` (they need `fixtures/feeds/`). Run them with:

```bash
cargo test --locked --release -p rustline-core --test m3_raptor --test m3_budget -- --ignored --nocapture   # RAPTOR == CSA (1 000 queries × Brașov, Cluj, Timișoara) + MBTA time/memory
```

```bash
cargo test --locked -p rustline-core --features oracle --test oracle_feature
```

## Door to door (M4)

Walking uses an OpenStreetMap extract (`fixtures/feeds/romania-260101.osm.pbf`, see
`fixtures/manifest.tsv`). The area comes from the feed, never from the code:

```bash
BBOX=$(cargo run -q --locked --release -p rustline-cli -- bbox --gtfs fixtures/feeds/brasov-latest.zip --margin-km 2)
cargo run --locked --release -p rustline-cli -- build-pack --gtfs fixtures/feeds/brasov-latest.zip --osm fixtures/feeds/romania-260101.osm.pbf --bbox "$BBOX" --out out/ --feed-id brasov --built-at 1700000000
cargo run --locked --release -p rustline-cli -- plan --pack out/rustline-brasov-ad0d22ca80b0-v4.rlp --from 45.6427,25.5887 --to 45.6579,25.6012 --at 2026-10-15T08:00 --geojson /tmp/j.geojson
```

`bbox` prints `min_lon,min_lat,max_lon,max_lat` (the order `osmium extract -b` takes).
`--geojson` writes a FeatureCollection of LineStrings (one per leg) for geojson.io.
`build-pack --osm` prints the OSM and foot-graph counts, the stops more than 100 m from
any street, and the footpath count. Without `--osm` (or when the extract has no streets
in the bbox: a warning) every walk is a straight line × 1.3, and `PackInfo::street_graph`
is false.

What is walkable (`rustline-osm`, spec §7.3 list): ways with `highway` = footway, path,
pedestrian, steps, living_street, residential, service, unclassified, tertiary,
secondary, primary, track, crossing, platform, corridor, primary_link, secondary_link,
tertiary_link; any other `highway` (motorway, motorway_link, trunk, trunk_link,
cycleway, …) only with `foot` = yes/designated/permissive. `foot=no` always excludes; `access=private`
or `access=no` excludes unless `foot` allows it. Pedestrians walk both ways; only
`oneway:foot=yes` makes a way one-way (plain `oneway=yes` does not). `highway=steps`
costs × 1.5; `incline`/elevation is ignored. Street islands are dropped unless a stop is
linked to them.

API (`rustline-core`, FFI-friendly: plain structs and enums, no pub fields on the graph):
`Engine::plan(&PlanRequest) -> Result<PlanResponse, PlanError>`.
- `PlanRequest { from, to, time: TimeSpec::DepartAt(..), walk_speed_mps, max_access_m,
  max_transfer_m, max_transfers, max_results }`; `Default` = 1.3 m/s, 1 000 m, 500 m,
  3 transfers, 5 results.
- Limits (else `PlanError::InvalidRequest`): valid lat/lon; speed in (0, 3] m/s;
  `max_access_m` ≤ 3 000; `max_transfer_m` ≤ 500 (the pack's footpaths stop there);
  `max_results` 1–255. `max_transfers` is capped at 8.
- A point more than 300 m from every street → `OutsideCoverage` (exactly 300 m still
  snaps; the walk to the street counts). A date outside the feed → `DateOutsideFeed`;
  nothing usable → `NoRoute`.
- Walking only is offered up to 3 km and kept when it arrives at most 3 min after the
  best ride. Street searches stop at 10 km and 500 000 settled nodes.
- `PlanResponse { itineraries: Vec<Itinerary>, warnings: Vec<Warning> }`;
  `Itinerary { legs, departure, arrival, transfers, walk_m }`;
  `Leg::Walk { start, end, distance_m, from_stop, to_stop, geometry }` and
  `Leg::Transit { trip_id, route_id, route_name, headsign, from_stop, to_stop, board,
  alight, geometry }`; `Warning::StopNotLinkedToStreets(stop_id)`, `Warning::NoStreetGraph`.
- `rustline_osm::build_foot_graph(path, Option<BBox>)` and
  `build_foot_graph_with_limits(path, Option<BBox>, &OsmLimits)` (defaults: blobs ≤ 32 MiB,
  ≤ 8 M nodes kept, ≤ 1 M walkable ways; the whole Romania file without a bbox is over
  the node limit and fails with `OsmError::Limit`). An empty extract or a bbox with no
  streets gives an empty graph, not an error.

Slow M4 tests (need `fixtures/feeds/` with the OSM extract):

```bash
cargo test --locked --release -p rustline-cli --test m4_real --test m4_budget -- --ignored --nocapture --test-threads 1
cargo test --locked --release -p rustline-osm --test m4_osm -- --ignored --nocapture   # whole-country extract
```

Test fixtures live in `data/fixtures/` (`tiny`: a clean feed; `broken`: one of
each validation error).

## Known gaps (M1, re-targeted in M2 and M5)
Gaps #1, #3 and #5 have an `#[ignore = "Known gap (M1) #N, see README: ..."]` test in
`crates/rustline-gtfs/tests/edge_cases.rs`. #2 has a test that runs in debug builds only
(as does `m5_resources::times_near_u32_max_build_or_fail_cleanly_and_never_panic`).
#6 has `duplicate_stop_sequence_is_reported` (passes; it pins today's behaviour). #4 is
fixed in M5 (its test `invalid_timezone_is_rejected` now runs). M5 added an own reader for
`transfers.txt` only, so #1, #2, #3, #5 and #6 (own lenient `stop_times`/`trips` reader,
spec §7.2) are proposed for **M8**.

1. **Time range check** – ingest uses gtfs-structures' time parser, so `24:60:00`
   loads as `25:00:00`. Fix: own reader (moved to M8, Savin OK 2026-10-18).
2. **Huge hours** – `9999999:00:00` overflows in the parser. Debug builds give a
   `ParserPanic` error (via `catch_unwind`, which does not help with `panic = "abort"`);
   release builds wrap silently. Fix: own reader with `checked_*` (moved to M8, Savin OK 2026-10-18).
3. **Duplicate trip_id** – duplicate rows in trips.txt are silently collapsed. Fix: own reader (moved to M8, Savin OK 2026-10-18).
4. ~~**IANA timezone**~~ – fixed in M5 (chrono-tz): a bad `agency_timezone` fails the
   build (`InvalidTimezone`) and a pack with one fails `Engine::open`.
5. **Short CSV rows / strict loading** – rows with fewer fields than the header, dangling ids
   or a missing required file fail the whole feed. Google's `sample-feed-1.zip` fails
   here for `inspect` and `build-pack`. Fix: own lenient reader (moved to M8, Savin OK 2026-10-18). (A
   `transfers.txt` row with an unknown stop no longer fails the feed since M5.)
6. **Duplicate stop_sequence** – not reported; the trip may show up as decreasing times.
   Fix: own reader (moved to M8, Savin OK 2026-10-18).

## Known gaps (M3)
7. **block_id** – stored in the pack but not used: staying seated from one trip to the
   next of the same block counts as a transfer (2 rides). Test:
   `block_id_is_treated_as_a_normal_transfer`.
8. **transfers.txt** – partly fixed in M5 (own tolerant reader): rows with trip or route
   ids, types 4/5 (in-seat) and unknown types are skipped with an `UnsupportedTransfer`
   warning, rows with an unknown stop with `UnknownTransferStop`. Still open: trip/route
   transfers are not applied (only stop-to-stop rows are).
9. **Sibling platforms** – platforms of one station are linked only by transfers.txt
   (Leader, M3 decision 8) until M4 footpaths. (A query from/to a parent station uses all
   its platforms with 0 s access.)
(#10, "one service day", is fixed in M5: RAPTOR searches service days D‑1, D and D+1,
shifted by the real day length around DST.)

(#11, "a self-transfer hides a walk", was fixed in the M3 review round by keeping ride and
walk arrivals apart; its test `self_transfer_time_does_not_hide_a_faster_walk` now runs.)

(#12, "no max_results", is closed in M4: `PlanRequest::max_results`, 0 → `InvalidRequest`.
`plan --from-stop/--to-stop` still lists the whole Pareto set.)

13. **Duplicate transfers.txt rows** – for the same (from, to) only one row is kept: the
    longest `min_transfer_time` (ties: the higher `transfer_type`, so 3 "not possible"
    wins), so Rustline never promises a quicker change than the agency. The others are counted
    as a `DuplicateTransfer` warning in the validation report, and `build-pack` prints
    `warning: N duplicate transfers.txt rows …`. Test:
    `duplicate_transfer_rows_are_counted_and_one_is_kept`.

## Known gaps (M5)
22. **RAPTOR vs CSA across midnight** – the CSA oracle (`oracle_feature.rs`) is single-day,
    so multi-day answers are checked only by the M5 hand tests and properties, not against
    the oracle.
23. **Frequency caps** – a feed over the caps of spec §7.2 (10 000 trips per row, 200 000
    trips, 5 000 000 stop times) fails to build instead of being cut down. Tests:
    `m5_resources::frequency_expansion_over_the_cap_is_a_build_error`,
    `m5_variants::frequency_row_over_the_cap_is_too_many_frequency_trips`; a feed exactly
    at the caps builds (`m5_resources::frequency_expansion_at_the_caps_stays_under_512_mib`).

## Known gaps (M4)
14. **Departure only** – `TimeSpec` has only `DepartAt` (no arrive-by).
15. **Tag list** – `cycleway` (and any value not listed above) is walkable only with an
    explicit `foot` tag, so a cycle path without one is not used even where people walk
    on it in practice (Leader, M4 answers; spec §7.3 says the list is to be tuned).
16. **OSM order** – nodes must come before ways (the usual PBF order); a file with nodes
    after ways gives `OsmError::Unsupported`. Relations (multipolygon squares) are ignored.
17. **Bbox edge** – a way leaving the bbox is cut at its last node inside; walks along
    the edge of the bbox may be longer than in reality.
18. **Transit geometry** – the ride line is drawn stop to stop (shapes.txt is not used).
19. **0-ride RAPTOR results** – a pure transfers.txt/footpath result is dropped; the
    direct street walk stands in for it.
20. **Rounding** – footpath times are whole seconds; one-to-many walks are capped by real
    metres while steps are costed × 1.5.
21. **Unlinked stops** – a stop more than 100 m from any street is reached by a straight
    line × 1.3 and gives `Warning::StopNotLinkedToStreets` (Brașov: 2 stops).

## Known gaps (M6)
24. **Emulator only** – the integration tests run on an x86_64 android-34 emulator
    (AVD `rustline_x64`; pack pushed to `/data/local/tmp`), not on a phone yet. The A54
    numbers (spec §12) are for Savin.
25. **RoTransit not wired** – plan M6 steps 4–6 (bundling, freshness banner, planner
    screen, attribution in the app) wait for the RoTransit work (Savin, 2026-10-14).
26. **Cancellation** – `RustlinePlanner` drops a superseded answer; the Rust call itself
    is not interrupted (it is bounded instead: ≤ 1 s, worst 2.7 ms measured on the PC).
27. **ABIs** – only arm64-v8a and x86_64 are built (`abiFilters` in the example app);
    armeabi-v7a is not supported.
28. **Host library path** – the generated `frb_generated.dart` looks for the desktop
    library in `../crates/rustline-ffi/target/release/` (FRB derives it from
    `rust_root`); the workspace builds into `target/`, so host runs pass
    `RustLib.init(externalLibrary: ExternalLibrary.open(...))`.

## Flutter plugin (M6)

`rustline_flutter/` is the plugin (Dart API generated by flutter_rust_bridge 2.12.0 from
`crates/rustline-ffi/src/api.rs`; Android build via Cargokit) and `rustline_flutter/example`
its test app.

```bash
cd rustline_flutter && flutter_rust_bridge_codegen generate        # after changing api.rs
cd example && flutter build apk --release --target-platform android-arm64,android-x64
cargo ndk -t arm64-v8a build --locked --release -p rustline-ffi   # what CI (android.yml) runs
```

## Toolchain
- Rust stable via rustup, pinned in `rust-toolchain.toml` (Android targets `aarch64-linux-android`, `x86_64-linux-android`).
- `cargo-ndk` 4.1.2, Android NDK 26.1.10909125.
- flutter_rust_bridge **2.12.0** (codegen and Dart package must match). 2.13.x requires Dart >= 3.9.2; RoTransit is on Flutter 3.27 (Dart 3.6).

## Data attribution
Map data © OpenStreetMap contributors, available under the [Open Database License (ODbL)](https://opendatacommons.org/licenses/odbl/). Timetable data from RATBV Brașov; any derived databases are subject to the ODbL where applicable.

## License
Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at your option.
