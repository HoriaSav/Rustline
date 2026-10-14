# Rustline

Rustline answers one question: *"I am at point A at time T and want to get to point B — what should I do?"* The answer is a short list of itineraries, each a sequence of legs (walk, ride a bus, transfer, walk). It reads standard **GTFS** timetables and an **OpenStreetMap** street graph for walking. It is a standalone Rust library first; the RoTransit Flutter app is its first user.

**Status:** M0 (setup).

## Toolchain
- Rust stable via rustup, pinned in `rust-toolchain.toml` (Android targets `aarch64-linux-android`, `x86_64-linux-android`).
- `cargo-ndk` 4.1.2, Android NDK 26.1.10909125.
- flutter_rust_bridge **2.12.0** (codegen and Dart package must match). 2.13.x requires Dart >= 3.9.2; RoTransit is on Flutter 3.27 (Dart 3.6).

## Data attribution
Map data © OpenStreetMap contributors, available under the [Open Database License (ODbL)](https://opendatacommons.org/licenses/odbl/). Timetable data from RATBV Brașov; any derived databases are subject to the ODbL where applicable.

## License
Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at your option.
