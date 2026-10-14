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

`rustline build-pack` turns a GTFS feed into a routing pack: `rustline-{feedId}-{sha12}-v2.rlp` (format version 2)
and the same file compressed as `.rlp.zst` (the loader accepts both). The header carries the
SHA-256 of the feed zip, so an app can compare it with the backend's `current.sha256`.
`--feed-id` defaults to the feed's file name. `--built-at <unix seconds>` (or `SOURCE_DATE_EPOCH`)
fixes the build time stored in the header, so the same feed always gives a byte-identical pack.

```bash
cargo run --locked --release -p rustline-cli -- build-pack --gtfs fixtures/feeds/brasov-manifest.zip --out out/ --feed-id myfeed --built-at 1700000000
cargo run --locked --release -p rustline-cli -- inspect out/rustline-myfeed-*.rlp
cargo test --locked -p rustline-pack                     # round trip + bad-file tests on the fixture
RUSTLINE_MANIFEST_FEED=fixtures/feeds/brasov-manifest.zip \
cargo test --locked --release -p rustline-pack --test round_trip -- --ignored --nocapture   # real feed, all trips
RUSTLINE_MANIFEST_FEED=fixtures/feeds/brasov-manifest.zip \
cargo test --locked --release -p rustline-core --test engine -- --ignored --nocapture       # load time, median of 10
```

Test fixtures live in `data/fixtures/` (`tiny`: a clean feed; `broken`: one of
each validation error).

## Known gaps (M1, re-targeted in M2)
Gaps #1, #3, #4 and #5 have an `#[ignore = "Known gap (M1) #N, see README: ..."]` test in
`crates/rustline-gtfs/tests/edge_cases.rs`. #2 has a test that runs in debug builds only.
#6 has `duplicate_stop_sequence_is_reported` (passes; it pins today's behaviour). M2 (the pack) did not touch GTFS reading, so #1, #2, #3, #5 and #6 move
to **M5**, where Rustline gets its own lenient `stop_times`/`trips` reader (spec §7.2).

1. **Time range check** – ingest uses gtfs-structures' time parser, so `24:60:00`
   loads as `25:00:00`. Fix: own reader (M5).
2. **Huge hours** – `9999999:00:00` overflows in the parser. Debug builds give a
   `ParserPanic` error (via `catch_unwind`, which does not help with `panic = "abort"`);
   release builds wrap silently. Fix: own reader with `checked_*` (M5).
3. **Duplicate trip_id** – duplicate rows in trips.txt are silently collapsed. Fix: own reader (M5).
4. **IANA timezone** – `agency_timezone` is not checked against IANA names. Fix: chrono-tz (M5).
5. **Short CSV rows / strict loading** – rows with fewer fields than the header, dangling ids
   or a missing required file fail the whole feed. Google's `sample-feed-1.zip` fails
   here for `inspect` and `build-pack`. Fix: own lenient reader (M5).
6. **Duplicate stop_sequence** – not reported; the trip may show up as decreasing times.
   Fix: own reader (M5).

## Toolchain
- Rust stable via rustup, pinned in `rust-toolchain.toml` (Android targets `aarch64-linux-android`, `x86_64-linux-android`).
- `cargo-ndk` 4.1.2, Android NDK 26.1.10909125.
- flutter_rust_bridge **2.12.0** (codegen and Dart package must match). 2.13.x requires Dart >= 3.9.2; RoTransit is on Flutter 3.27 (Dart 3.6).

## Data attribution
Map data © OpenStreetMap contributors, available under the [Open Database License (ODbL)](https://opendatacommons.org/licenses/odbl/). Timetable data from RATBV Brașov; any derived databases are subject to the ODbL where applicable.

## License
Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at your option.
