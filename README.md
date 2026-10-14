# Rustline

Rustline answers one question: *"I am at point A at time T and want to get to point B — what should I do?"* The answer is a short list of itineraries, each a sequence of legs (walk, ride a bus, transfer, walk). It reads standard **GTFS** timetables and an **OpenStreetMap** street graph for walking. It is a standalone Rust library first; the RoTransit Flutter app is its first user.

**Status:** M1 (GTFS ingest).

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
cargo ndk -t arm64-v8a build -p rustline-core
```

## Inspecting a GTFS feed

`rustline inspect` loads any GTFS feed (zip or directory), prints row counts, the
timezone and validity window (taken from the feed), the patterns it built, and a
validation report. Add `--json` for machine-readable output.

```bash
cargo run --locked --release -p rustline-cli -- inspect data/fixtures/tiny
cargo run --locked --release -p rustline-cli -- inspect data/raw/feed.zip --json
```

Real feeds go in `data/raw/` (git-ignored). Any MobilityDatabase feed works:
`curl -L -o data/raw/feed.zip https://files.mobilitydatabase.org/<mdb-id>/latest.zip`
(the ids RoTransit uses are in its `V2__seed_feeds.sql`).

To check a feed against known counts (`stops,routes,trips,stop_times,services,exceptions`):

```bash
RUSTLINE_MANIFEST_FEED=data/raw/brasov-1771800663.zip \
RUSTLINE_MANIFEST_COUNTS=811,84,8075,118534,5,110 \
cargo test --locked -p rustline-gtfs --test manifest_feed -- --ignored
```

(That Brașov feed is the one RoTransit's companion manifest was built from. The
manifest's `departure_rows` = 157 698 counts one row per stop time *per day kind*;
the GTFS stop_times count is 118 534.)

Test fixtures live in `data/fixtures/` (`tiny`: a clean feed; `broken`: one of
each validation error).

## Known gaps (M1)
Each gap has an `#[ignore = "Known gap (M1) #N: ..."]` test in
`crates/rustline-gtfs/tests/edge_cases.rs`.

1. **Time range check** – ingest uses gtfs-structures' time parser, so `24:60:00`
   loads as `25:00:00`. Fix: own `stop_times.txt` reader (spec §7.2).
2. **Huge hours** – `9999999:00:00` overflows in the parser. Debug builds now give a
   `ParserPanic` error instead of a panic; release builds still wrap silently. Fix: own reader.
3. **Duplicate trip_id** – duplicate rows in trips.txt are silently collapsed. Fix: own reader (M1.x/M2).
4. **IANA timezone** – `agency_timezone` is not checked against IANA names. Fix: chrono-tz in M5.
5. **Short CSV rows / strict loading** – rows with fewer fields than the header, dangling ids
   or a missing required file fail the whole feed. Fix: own lenient reader (M1.x/M2).
6. **Duplicate stop_sequence** – not reported; the trip may show up as decreasing times. (No test yet.)

## Toolchain
- Rust stable via rustup, pinned in `rust-toolchain.toml` (Android targets `aarch64-linux-android`, `x86_64-linux-android`).
- `cargo-ndk` 4.1.2, Android NDK 26.1.10909125.
- flutter_rust_bridge **2.12.0** (codegen and Dart package must match). 2.13.x requires Dart >= 3.9.2; RoTransit is on Flutter 3.27 (Dart 3.6).

## Data attribution
Map data © OpenStreetMap contributors, available under the [Open Database License (ODbL)](https://opendatacommons.org/licenses/odbl/). Timetable data from RATBV Brașov; any derived databases are subject to the ODbL where applicable.

## License
Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at your option.
