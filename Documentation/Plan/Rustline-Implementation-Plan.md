# Rustline — Implementation Plan (step by step)

> Companion to [Rustline-Spec.md](Rustline-Spec.md). The spec says **what** Rustline is; this file says **in what order to build it, how, and why**.
> Decision confirmed: **the engine runs on the phone** (Flutter → `flutter_rust_bridge` → Rust). The server stays optional ([spec §5](Rustline-Spec.md#5-deployment-decision)).
> You write the code. This plan gives commands, file names, type names and *small* snippets only — enough to know where to go, not the finished code.
> Times marked **(est.)** are rough guesses for evenings of focused work while learning Rust, not measurements.
> Unknown words? See the glossary in [spec §2](Rustline-Spec.md#2-glossary).

---

## 0. How to use this plan

- Work **one milestone at a time**, top to bottom. Each one ends in something you can run and see.
- Each milestone has: **Goal · Prerequisites · Steps (What / How / Why) · How to check it works · Done when · Pitfalls · Time (est.)**.
- Each milestone = one branch + one PR (`horia/m1-gtfs-ingest`, …), same flow as RoTransit ([spec §16](Rustline-Spec.md#16-tooling-and-ci)). Tick the checkboxes in the PR description.
- After every step: `cargo fmt --all && cargo clippy --locked --workspace --all-targets -- -D warnings && cargo test --locked --workspace`. Small green steps beat big red ones.
- Before handing a milestone to Tester, go through the **Builder checklist** below and the milestone's **Definition of done (strict)**.
- When stuck on Rust itself: *The Rust Book* (`rustup doc --book`, works offline) and *Rust by Example*.

### Defaults assumed (from the spec's open questions, [§19](Rustline-Spec.md#19-open-questions-for-horia))

| Open question | Default used in this plan | Where it matters |
|---|---|---|
| Q1 On-device or server first | **On-device** (confirmed) | M6 before M7 |
| Q2 Public open-source? | Public repo, **MIT OR Apache-2.0**, not published to crates.io yet | M0 licence |
| Q3 Brașov only or 13 feeds | **Brașov only** | Size/perf targets in M2, M5, M6 |
| Q4 iOS? | **Android first**, iOS skipped until after M6 | M0 targets, M6 |
| Q5 Who builds packs | **Your PC** with `rustline build-pack` | M2, M4; backend later in M9 |
| Q6 Defaults (walk speed etc.) | Spec defaults: 1.3 m/s, 1 000 m access, 500 m transfer, 3 transfers | `PlanRequest::default()` |
| Q7 Arrive-by in v1? | **No** — M8 | M3 API only has `DepartAt` |
| Q8 RATBV realtime | Assume none | M9 |
| Q9 Walk-only always shown? | Shown **only when competitive** (spec §9 rule) | M4 ranking |
| Q10 Who writes the core | **You**, with review | Whole plan |

If you answer a question differently later, the "Where it matters" column tells you what to revisit.

### Loop process (Leader, Builder, Tester, Reviewer)

The bots work each milestone in this order. M2 was already in progress when this was added, so steps 1 and 4 start with M3.

**1. Tests first (Tester, before Builder starts)**
- Tester turns the milestone's **Definition of done (strict)** into tests *before* Builder writes code.
- Tests that can't pass yet are marked `#[ignore = "Mx DoD: <item>"]` (or simply fail). Each one names the DoD item it checks.
- Builder works until all of them pass and removes the `#[ignore]`. Builder may not weaken or delete these tests. If one is wrong, Builder says so in the hand-over report and Tester/Reviewer decide.
- *Why:* misunderstandings of the plan show up on day one, not after the whole milestone is built.

**2. Check in parallel (Tester + Reviewer, after Builder hands over)**
- Tester and Reviewer check the same hand-over at the same time. Neither one changes engine code.
- All findings go back to Builder in one list. After Builder's fixes, **both** re-check.
- The milestone is approved only when Reviewer says APPROVE and Tester reports no open failures. Then Leader commits (as Horia Savin, no co-authors). Pushing still needs Savin's "push".

**3. Shared test feeds (everyone)**
- One fixed set of GTFS feeds lives in `fixtures/feeds/` (gitignored). The committed `fixtures/manifest.tsv` lists each feed's name, URL, sha256, size and download date.
- Get or check the set with:
```bash
scripts/fetch-fixtures.sh            # download missing files, verify all sha256s
scripts/fetch-fixtures.sh --verify   # verify only, no network
```
- Every bot uses these files for the multi-feed check and real-feed tests (e.g. `RUSTLINE_MANIFEST_FEED=fixtures/feeds/brasov-manifest.zip`). Never download a feed ad hoc into `/tmp` or `data/raw/`.
- Changing the set (new feed, newer version) is a deliberate change: update the manifest (sha256, bytes, date) and mention it in the hand-over report.

**4. Progress note (Leader, after each approved milestone)**
- Leader adds a 2–3 line entry to the Obsidian note `Projects/Rustline/Progress.md`: what was done (with the commit hash), issues found in testing/review, and what's next.

### Builder checklist (applies to every milestone)

Learned from the M0/M1 reviews. Every item is checked by Tester/Reviewer; skipping one costs a review round.

**1. Generic engine**
- No city, agency, timezone, bbox, feed id or coordinate literal in `crates/*/src`. Everything comes from the feed, the OSM file or CLI/env arguments.
- Real coordinates only in fixtures and ignored real-feed tests, never in engine code. Prefer neutral coordinates in unit tests.
- Check: `rg -n -i 'brasov|brașov|ratbv|bucharest|europe/|45\.6|25\.5' crates/*/src` returns nothing.

**2. Toolchain**
- MSRV is **1.85** (`rust-version` in the workspace). Before adding or bumping a dependency, check its own `rust-version`; if a newer version needs more, pin the last compatible one with a comment and a Dependabot `ignore` for the incompatible range.
- Every cargo command in code, docs and CI uses `--locked` (except `cargo fmt`, which has no such flag). `Cargo.lock` is committed.
- Check: `cargo +1.85 test --locked --workspace` passes.

**3. No panics on bad input**
- Anything read from disk or the user (GTFS, OSM, `.rlp`, CLI args, FFI calls) returns `Result<_, SpecificError>` (`thiserror`). No `unwrap`/`expect`/`panic!`/indexing that can go out of bounds on input data. `unwrap` only on invariants you create yourself, with a comment saying why.
- Arithmetic on input-derived numbers uses `checked_*` (error) or `saturating_*` (documented). Casts use `u32::try_from`, not `as`, unless the range is proven. Remember: overflow panics in debug but **silently wraps in release**.
- Do not rely on `catch_unwind`; release builds may use `panic = "abort"` (spec §12).
- **Report vs fail:** one bad row/trip/stop goes into the validation report (with counts and up to 5 examples) and is skipped; only a feed that is unusable as a whole (not a zip, no stops, no trips) is a hard error. Every report kind must have a fixture case.

**4. Lint and format**
- `cargo fmt --all --check` and `cargo clippy --locked --workspace --all-targets -- -D warnings` are clean. No `#[allow]` without a comment.

**5. Tests written with the code**
- Each new function that reads input gets edge-case tests in the same change: empty, one element, maximum/overflow values, malformed, duplicates, out-of-order. The milestone's "Edge cases to test" list is the minimum.
- Tests are deterministic: fixed seeds, no `Local` time, no network, no dependency on file order or `HashMap` order.

**6. Docs match reality**
- Every command in README and in this plan's "How to check it works" is **copied and run verbatim** before handing over, from a clean shell in the repo root. If it fails or needs a tweak, fix the doc, not the command you typed.
- Every number written into docs (counts, sizes, timings) is pasted from a command you just ran, with the command next to it. If the plan's number is wrong, correct the plan with an explanation (as with 157,698 vs 118,534 in M1) and flag it to Savin.

**7. Known gaps are tracked**
- Anything not fixed goes into README "Known gaps (Mx)" with a number, a one-line cause and the milestone that fixes it.
- Each gap has a test marked `#[ignore = "Known gap (Mx) #N, see README: ..."]`. Ignored tests must not break documented commands (run them by `--test <name>`).
- Gaps from earlier milestones that this milestone was meant to fix are removed from the list and their tests un-ignored.

**8. Multi-feed check** (release build, `rustline inspect` and, from M2, `build-pack`)

| Feed | Why |
|---|---|
| Brașov (manifest feed + latest) | Main target, numbers in the plan |
| Cluj (mdb-2121) | Medium city, no calendar_dates |
| Timișoara (mdb-2868) | Many services/exceptions, frequencies |
| București (mdb-2098) | Largest (1.45 M stop_times), metro interpolation |
| MBTA (Boston) | Non-Romanian timezone, 3 M+ stop_times |
| Google `sample-feed-1.zip` | Minimal official feed with short CSV rows |

All six come from the shared set in `fixtures/feeds/` (see **Loop process** step 3); `fixtures/manifest.tsv` maps each row to its file. All must finish with no panic. Record counts, time and peak RAM in a table in the report. A feed that fails must be a listed Known gap.

**9. Hand-over report format** (message to Tester/Reviewer)
1. Status line: done / partly done, nothing committed, no sudo.
2. Definition-of-done checklist with PASS/FAIL per item and the command that proves it.
3. Multi-feed table.
4. Deviations from the plan, and numbers in the plan that turned out wrong.
5. Known gaps added/removed.
6. `git status` summary (files added/modified/deleted).
7. Open questions for Savin.

**10. Scope**
- Do only this milestone. Stage nothing, commit nothing; Savin commits after review. Don't edit unrelated files.

### Milestone overview

| # | Name | One line | Time (est.) |
|---|---|---|---|
| M0 | Setup | Toolchain, workspace skeleton, CI, hello-world FRB on Android | 4–6 evenings |
| M1 | GTFS ingest | `rustline inspect brasov.zip` prints counts that match the manifest | 5–7 evenings |
| M2 | Pack v1 | Timetable saved to a `.rlp` file and loaded back | 6–8 evenings |
| M3 | RAPTOR stop-to-stop | First real journeys between two stops, checked by CSA | 10–14 evenings |
| M4 | Streets | Walking: OSM graph, snapping, access/egress, walk-only | 10–14 evenings |
| M5 | Calendar & edge cases | Midnight, DST, holidays, frequencies, transfers.txt, benchmarks | 6–9 evenings |
| M6 | Flutter integration | Rustline running offline on your A54 inside RoTransit | 10–14 evenings |
| M7 | Server (optional) | Same engine behind `POST /v1/plan` | 3–5 evenings |
| M8 | Quality+ | Walking as a criterion, arrive-by, range queries, thesis charts | 10–15 evenings |
| M9 | Future | Backend-served packs, realtime, multi-city (own spec) | — |

Total to a working phone prototype (M0–M6): roughly **50–70 evenings (est.)**.

---

## M0 — Setup

**Goal:** an empty-but-correct Rust workspace that builds, tests and lints locally and in GitHub Actions, plus proof that a Rust function can be called from Flutter on Android.
**Prerequisites:** Arch/Omarchy PC, Flutter + Android Studio already working (they are, from RoTransit), the `Rustline` repo cloned at `~/Projects/Rustline`.
**Spec:** [§6.2 workspace](Rustline-Spec.md#62-cargo-workspace-layout), [§16 tooling](Rustline-Spec.md#16-tooling-and-ci).

### Steps

**1. Install Rust with rustup**
- *What:* install the Rust toolchain manager.
- *How:*
```bash
sudo pacman -S --needed rustup base-devel clang
rustup default stable
rustup component add rustfmt clippy rust-src
rustc --version && cargo --version
```
- *Why:* `rustup` (not the `rust` pacman package) lets you add Android targets and pin versions per project. Don't install both — `pacman -S rust` conflicts with `rustup`. `clang` is needed by some crates and by FRB.

**2. Editor support**
- *How:* in Android Studio / IntelliJ install the **RustRover**-style "Rust" plugin, or use VS Code / Neovim with `rust-analyzer` (`rustup component add rust-analyzer`).
- *Why:* inline type hints and errors make learning Rust much faster.

**3. Pin the toolchain**
- *How:* create `rust-toolchain.toml` at the repo root:
```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
targets = ["aarch64-linux-android", "x86_64-linux-android"]
```
- *Why:* CI and your PC then use the same compiler and targets automatically.

**4. Create the workspace**
- *How:*
```bash
cd ~/Projects/Rustline
mkdir crates && cd crates
cargo new --lib rustline-model
cargo new --lib rustline-gtfs
cargo new --lib rustline-osm
cargo new --lib rustline-pack
cargo new --lib rustline-core
cargo new --bin rustline-cli
cd .. && mkdir -p tests/golden data/fixtures benches
```
  Root `Cargo.toml`:
```toml
[workspace]
resolver = "2"
members = ["crates/*"]

[workspace.package]
edition = "2021"
license = "MIT OR Apache-2.0"

[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
strip = true
```
  Leave out `rustline-ffi` and `rustline-server` for now (M0 step 9 and M7).
- *Why:* a workspace builds all crates together with one `Cargo.lock`. Separate crates keep the heavy parsers (`-gtfs`, `-osm`) off the phone ([spec §6.2 rule](Rustline-Spec.md#62-cargo-workspace-layout)). The release profile is the one from [spec §12](Rustline-Spec.md#12-performance-targets-and-budgets-all-est-to-be-validated-in-m5) (`panic = "abort"` is added later, carefully, because FFI needs to catch panics).

**5. Wire crate dependencies (empty for now)**
- *How:* in each `Cargo.toml` add path deps, e.g. `rustline-core`:
```toml
[dependencies]
rustline-model = { path = "../rustline-model" }
rustline-pack  = { path = "../rustline-pack" }
```
  Graph: `model` ← everyone; `gtfs`, `osm` → `model`; `pack` → `model`; `core` → `model`, `pack`; `cli` → all.
- *Why:* writing the dependency graph now prevents accidental cycles later (e.g. `core` must **never** depend on `gtfs`).

**6. README, licence, .gitignore**
- *How:* add `LICENSE-MIT`, `LICENSE-APACHE`, a short `README.md` (one paragraph from spec §1 + "status: M0"), `.gitignore` with `/target`, `*.rlp`, `*.osm.pbf`, `data/raw/`.
- *Why:* big data files must never end up in git; licence decided early is easier than later. Mention ODbL attribution for OSM/RATBV data in the README.

**7. CI**
- *How:* `.github/workflows/ci.yml` running on push/PR: `actions/checkout`, `dtolnay/rust-toolchain@stable`, `Swatinem/rust-cache`, then `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. Add `.github/dependabot.yml` for `cargo` and `github-actions` (copy RoTransit's).
- *Why:* CI catches "works on my machine" issues and keeps style consistent.

**8. Android build tools**
- *How:*
```bash
rustup target add aarch64-linux-android x86_64-linux-android   # armv7 only if needed
cargo install cargo-ndk
# NDK: Android Studio → SDK Manager → SDK Tools → NDK (Side by side)
export ANDROID_NDK_HOME=$HOME/Android/Sdk/ndk/<version>          # add to ~/.bashrc
```
- *Why:* phones run ARM, your PC runs x86. `cargo-ndk` sets the NDK linker for you. `x86_64` is for the emulator, `aarch64` for the A54.

**9. flutter_rust_bridge "hello world" (risk spike)**
- *How:*
```bash
cargo install flutter_rust_bridge_codegen   # version 2.x, match the Dart package
cd ~/Projects/Rustline
flutter_rust_bridge_codegen create rustline_hello   # throwaway, delete later
cd rustline_hello && flutter run    # pick the A54 or the emulator
```
  You should see "Hello, Tom" coming from Rust. Then note the exact FRB version in the README.
- *Why:* the spec lists FRB/NDK build pain as the biggest risk ([§18](Rustline-Spec.md#18-risks)). Proving it works on day one means M6 has no surprise. Delete the throwaway folder afterwards (don't commit it), or keep it on a separate branch.

**10. A first real test**
- *How:* in `rustline-model/src/lib.rs` add `pub struct LatLon { pub lat: f64, pub lon: f64 }` and a `#[cfg(test)] mod tests` with a trivial test.
- *Why:* proves `cargo test` runs in CI.

### How to check it works
```bash
cargo build --workspace
cargo test --workspace
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings
cargo ndk -t arm64-v8a build -p rustline-core     # cross-compile check
```
Push the branch, open a PR, see CI green.

### Done when
- [ ] `rustc`, `cargo`, `cargo-ndk`, `flutter_rust_bridge_codegen` installed
- [ ] Workspace with 6 crates builds; dependency graph as in step 5
- [ ] `rust-toolchain.toml`, README, licences, `.gitignore` committed
- [ ] CI green on the PR (fmt, clippy, test)
- [ ] FRB hello-world ran on the A54 or emulator
- [ ] `rustline-core` cross-compiles for `arm64-v8a`

### Common pitfalls
- Installing `rust` from pacman *and* `rustup` → conflicts. Use only `rustup`.
- `cargo-ndk` "NDK not found" → `ANDROID_NDK_HOME` not exported in the shell you use.
- FRB codegen version ≠ `flutter_rust_bridge` version in `pubspec.yaml` → runtime "content hash" error. Keep them identical.
- `cargo install` binaries live in `~/.cargo/bin` — make sure it is in `PATH`.

**Time (est.):** 4–6 evenings.

---

## M1 — GTFS ingest

**Goal:** `rustline inspect brasov.zip` prints counts and a validation report that match the companion manifest (811 stops, 84 routes, 8 075 trips, 118 534 stop_times (manifest `departure_rows` = 157 698 counts per day-kind), 5 services, 110 exceptions).
**Prerequisites:** M0. The Brașov GTFS zip (download from your backend: `curl -o data/raw/brasov.zip https://api.horiasavin.me/api/feeds/<id>/file`).
**Spec:** [§7.1–7.2](Rustline-Spec.md#7-data-pipeline), [§10](Rustline-Spec.md#10-time-calendars-timezone-dst-after-midnight).

### Steps

**1. Core model types (`rustline-model`)**
- *What:* the shared vocabulary.
- *How:* create modules `ids.rs`, `time.rs`, `geo.rs`. Key types:
```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StopIdx(pub u32);     // also RouteIdx, TripIdx, PatternIdx, ServiceIdx
pub struct Seconds(pub u32);     // seconds since service-day "noon minus 12h"
pub struct LatLon { pub lat: f64, pub lon: f64 }
```
  Add `serde` derives (`serde = { version = "1", features = ["derive"] }`).
- *Why:* small number "index" types (`StopIdx`) instead of `String` ids make arrays fast and stop you mixing a stop index with a trip index — the compiler will catch it. GTFS string ids are kept once in a lookup table.

**2. GTFS time parsing**
- *How:* `fn parse_gtfs_time(s: &str) -> Result<Seconds, TimeError>` accepting `"8:05:00"`, `"08:05:00"`, `"25:10:00"`. Unit tests for each, plus garbage input.
- *Why:* GTFS times go past 24:00 for after-midnight trips ([spec §10](Rustline-Spec.md#10-time-calendars-timezone-dst-after-midnight)); `chrono::NaiveTime` can't hold 25:10, so use plain seconds.

**3. Read the feed (`rustline-gtfs`)**
- *How:* add `gtfs-structures` and `anyhow`/`thiserror`. Write `pub fn load(path: &Path) -> Result<RawFeed, GtfsError>` that wraps `gtfs_structures::Gtfs::from_path`.
- *Why:* don't write a CSV parser yourself first; the crate handles quoting, BOMs, optional files. If memory becomes a problem, swap for `csv`+`serde` later (spec §7.2).

**4. Normalise into your own `Timetable`**
- *How:* `pub struct Timetable { stops: Vec<Stop>, routes: Vec<Route>, trips: Vec<Trip>, stop_times: Vec<StopTime>, services: Vec<Service>, timezone: String, valid_from: NaiveDate, valid_to: NaiveDate }`, plus `HashMap<String, StopIdx>` lookups used only during build.
- *Why:* the rest of Rustline should depend on **your** types, not on the parser crate. That keeps the parser swappable and the pack format stable.

**5. Stop patterns**
- *How:* group trips by their exact stop sequence (`HashMap<Vec<StopIdx>, PatternIdx>`), sort trips in each pattern by first departure.
- *Why:* RAPTOR scans patterns, not GTFS routes ([glossary](Rustline-Spec.md#2-glossary)). One "Line 4" may have several patterns (short-turns, directions).

**6. Overtaking check + split**
- *How:* within a pattern, after sorting by first departure, trip *i+1* must not arrive at any stop before trip *i*. If it does, move it to a new pattern.
- *Why:* RAPTOR's "first catchable trip" trick assumes trips never pass each other ([spec §18](Rustline-Spec.md#18-risks)).

**7. Validation report**
- *How:* `pub struct ValidationReport { errors: Vec<Issue>, warnings: Vec<Issue> }`. Checks: trip with < 2 stop times, decreasing times, unknown service id, stop without coordinates, number of overtaking splits.
- *Why:* the RATBV feed comes from osm2gtfs and may have quirks; you want to see them, not have them silently break routing.

**8. CLI `inspect`**
- *How:* in `rustline-cli` add `clap` (derive). Subcommand `inspect <feed.zip>` prints counts and the report. Optional `--json`.
- *Why:* the CLI is your debugging window and thesis screenshot tool (UC7).

**9. Tiny fixture feed**
- *How:* `data/fixtures/tiny/` with hand-written `agency.txt, stops.txt, routes.txt, trips.txt, stop_times.txt, calendar.txt, calendar_dates.txt`: 5 stops, 3 routes, ~10 trips, one 24:10 trip, one holiday exception.
- *Why:* tests on a feed you can read in one screen are far easier to debug than Brașov. You'll reuse it in M3–M5.

### How to check it works
```bash
cargo test -p rustline-model -p rustline-gtfs
cargo run -p rustline-cli -- inspect data/raw/brasov.zip
```
Compare with `RoTransit/frontend/assets/data/brasov_companion.manifest.json`.

### Done when
- [ ] Time parser handles `>24:00` with tests
- [ ] `Timetable` built from Brașov; patterns built and sorted
- [ ] Overtaking trips split, count shown in report
- [ ] `inspect` counts equal the manifest (811 / 84 / 8 075 / 118 534 / 5 / 110; manifest `departure_rows` = 157 698 counts per day-kind)
- [ ] Fixture feed committed with tests that load it

### Common pitfalls
- Using `f32` for coordinates → precision loss. Use `f64` in the model (pack can compress later).
- Fighting the borrow checker with references between structs → use indices (`StopIdx`) instead of `&Stop`.
- Counting stops including `location_type=1` stations — check what the manifest counts.

**Time (est.):** 5–7 evenings.

---

## M2 — Pack v1 (timetable only)

**Goal:** `rustline build-pack` writes a `.rlp` file; `Engine::open` loads it; a round-trip test proves nothing is lost. Size and load time measured.
**Prerequisites:** M1.
**Spec:** [§7.5 pack format](Rustline-Spec.md#75-pack-format-rlp--rustline-pack), [§11.2 errors](Rustline-Spec.md#112-errors).

### Steps

**1. Decide the in-memory layout first (flat arrays)**
- *How:* in `rustline-pack` define `TimetableData`:
```rust
pub struct TimetableData {
    pub pattern_stops_start: Vec<u32>,   // CSR: pattern p's stops are pattern_stops[start[p]..start[p+1]]
    pub pattern_stops: Vec<StopIdx>,
    pub pattern_trips_start: Vec<u32>,
    pub stop_times: Vec<u32>,            // arrival/departure seconds, trip-major, flat
    pub stop_patterns_start: Vec<u32>,   // which patterns serve each stop
    pub stop_patterns: Vec<PatternIdx>,
    pub service_bits: Vec<u64>,          // one bit per (service, day in validity window)
    // + strings: stop names, route short names, colours, GTFS ids
}
```
- *Why:* "CSR" (compressed sparse row: one big array + start offsets) is what makes RAPTOR fast — no pointers, cache friendly, and trivially saved to disk.

**2. Header**
- *How:* `magic b"RLPK"`, `format_version: u16 = 1`, `feed_sha256: [u8;32]`, `osm_sha256: [u8;32]` (zeros for now), `built_at`, `valid_from/to`, `timezone`, section table. Compute the sha with the `sha2` crate over the zip bytes.
- *Why:* the app decides "is my pack current?" by comparing `feed_sha256` with the backend's `current.sha256` ([spec §13](Rustline-Spec.md#13-integration-with-rotransit)).

**3. Format benchmark: `rkyv` vs `bytemuck` flat arrays**
- *How:* implement the simplest one first (suggest **`rkyv`** with `#[derive(Archive, Serialize, Deserialize)]` on `TimetableData`), measure file size and load time; if you have time, try `bytemuck` for the number arrays. Write the result into the spec §7.5.
- *Why:* the spec leaves this decision to M2. Zero-copy loading means the phone can open the pack in milliseconds instead of parsing it.

**4. Writer + reader**
- *How:* `pub fn write_pack(tt: &TimetableData, meta: &PackMeta, out: &mut impl Write) -> io::Result<()>` and `pub fn read_pack(bytes: &[u8]) -> Result<Pack, PackError>`. `PackError` variants exactly as spec §11.2 (use `thiserror`). Check magic first, then version.
- *Why:* loader must reject foreign or newer files with a clear error instead of crashing.

**5. Compression for transport**
- *How:* `zstd` crate; `build-pack` writes `brasov.rlp.zst`; the loader accepts both (detect zstd magic `28 B5 2F FD`).
- *Why:* smaller download/APK; decompressed once on device like `brasov_companion.sqlite.gz`.

**6. `Engine::open` skeleton (`rustline-core`)**
- *How:* `Engine { pack: Pack }` with `open(path)`, `from_bytes(bytes)`, `info() -> PackInfo` (counts, sha, validity).
- *Why:* fixes the public entry point now so the FFI layer in M6 can wrap it unchanged.

**7. CLI**
- *How:* `rustline build-pack --gtfs data/raw/brasov.zip --out out/` naming the file `rustline-{feedId}-{sha12}-v1.rlp` ([spec §7.5 versioning](Rustline-Spec.md#75-pack-format-rlp--rustline-pack)); `rustline inspect out/….rlp` prints `PackInfo`.

**8. Robustness**
- *How:* tests: wrong magic → `BadMagic`; version 99 → `UnsupportedVersion`; truncated file → `Corrupt`. Optional: `cargo install cargo-fuzz` and a fuzz target for `read_pack` (needs nightly, can wait until M5).
- *Why:* the phone will load whatever is on disk; it must never crash on a half-downloaded file.

### How to check it works
```bash
cargo run --locked --release -p rustline-cli -- build-pack --gtfs fixtures/feeds/brasov-manifest.zip --out out/
cargo run --locked --release -p rustline-cli -- inspect out/rustline-*.rlp
cargo test --locked -p rustline-pack       # round-trip: build → write → read → same counts + 20 sample trips identical
/usr/bin/ls -lh out/
```

### Done when
- [ ] `.rlp` and `.rlp.zst` written for Brașov
- [ ] Round-trip test passes (counts + sampled trips equal)
- [ ] Bad magic / version / truncation return errors, never panic
- [ ] Format decision (rkyv vs flat) + measured size and load time written into spec §7.5
- [ ] Pack build < 60 s on PC ([spec §12](Rustline-Spec.md#12-performance-targets-and-budgets-all-est-to-be-validated-in-m5))

### Definition of done (strict)

**Interfaces (names may change only with a note in the report)**
```rust
// rustline-pack
pub struct PackMeta { pub format_version: u16, pub feed_sha256: [u8; 32], pub osm_sha256: [u8; 32],
    pub built_at_unix: i64, pub valid_from: NaiveDate, pub valid_to: NaiveDate, pub timezone: String }
pub fn write_pack(tt: &TimetableData, meta: &PackMeta, out: &mut impl Write) -> Result<(), PackError>;
pub fn read_pack(bytes: &[u8]) -> Result<Pack, PackError>;   // accepts plain and zstd
#[derive(thiserror::Error, Debug)]
pub enum PackError { BadMagic, UnsupportedVersion { found: u16, supported: u16 }, Corrupt(String), Io(#[from] std::io::Error) }
// rustline-core
impl Engine { pub fn open(path: &Path) -> Result<Engine, PackError>; pub fn from_bytes(b: &[u8]) -> Result<Engine, PackError>; pub fn info(&self) -> PackInfo; }
```
- `TimetableData` uses only `u32`/`u64`/`u16`/`u8` and sorted `Vec`s (no `usize`, no `HashMap`).
- Timezone is stored as the feed's string; `valid_from/to` come from calendar + calendar_dates (no hard-coded dates).

**Acceptance criteria**
- [ ] `build-pack` writes both `.rlp` and `.rlp.zst` for every feed in the Builder checklist table (Google sample may be a Known gap only if M1 gap #5 still stands).
- [ ] Round-trip: for Brașov, every count (stops, routes, trips, patterns, stop_times, services) equals M1's timetable, and **all** trips (not a sample) have identical stop sequence and times. Same check on the tiny fixture.
- [ ] **Reproducible:** building the same feed twice gives byte-identical `.rlp` (only `built_at` may differ; put it in a fixed place or allow `--built-at` / `SOURCE_DATE_EPOCH`). Test compares the sha256 of two builds.
- [ ] `read_pack` validates every CSR offset (monotonic, last == array length, every index < its table length) and returns `Corrupt` instead of indexing out of bounds.
- [ ] Brașov: pack build < 60 s, `.rlp` size, `.zst` size and `Engine::from_bytes` time (median of 10, release) written into spec §7.5 with the command used.
- [ ] Format decision (rkyv vs bytemuck) written into spec §7.5 with numbers. The chosen crates satisfy MSRV 1.85.
- [ ] `rustline inspect` on a `.rlp` prints `PackInfo` (counts, sha, validity, timezone, format version); `--json` works.
- [ ] M1 Known gaps #1, #2, #3, #5, #6 (own lenient reader) either fixed here or explicitly re-targeted in README.

**Edge cases to test**
- Empty file, 1 byte, header only, truncated at every section boundary, truncated zstd stream → `Corrupt`, never panic.
- Wrong magic → `BadMagic`; version 0 and 99 → `UnsupportedVersion`.
- A CSR offset pointing past the end; a `StopIdx` ≥ stop count; section table with overlapping/out-of-range sections → `Corrupt`.
- Unaligned input buffer (slice starting at offset 1) still loads or gives a clear error.
- Feed with one stop and one trip; feed with zero valid trips after validation (hard error from `build-pack`, clear message).
- Strings with non-ASCII (ș, ț, emoji) round-trip.
- Fuzz-style test: 10 000 random byte mutations of a valid fixture pack (fixed seed) → only `Err`, no panic.

**Tester will check**
- All "How to check it works" commands verbatim; byte-identical rebuild; full round-trip on Brașov.
- Corruption/mutation tests in debug **and** `--release`.
- Multi-feed `build-pack` + `inspect` table; sizes and times match spec §7.5.
- No `usize`/`HashMap` in the on-disk types; MSRV build; genericity `rg`.

### Common pitfalls
- Saving `HashMap`s in the pack → order isn't stable, file isn't reproducible. Use sorted `Vec`s.
- `usize` in the file format → differs between 32/64-bit. Use `u32`/`u64` explicitly.
- `rkyv` alignment: read into an aligned buffer (`rkyv::util::AlignedVec`) or validation fails.

**Time (est.):** 6–8 evenings.

---

## M3 — RAPTOR stop-to-stop

**Goal:** `rustline plan --from-stop X --to-stop Y --at 2026-10-15T08:00` returns correct journeys; a second algorithm (CSA) agrees on 1 000 random queries.
**Prerequisites:** M2.
**Spec:** [§8 algorithms](Rustline-Spec.md#8-algorithms-and-why), [§8.1 step 4](Rustline-Spec.md#81-query-algorithm-in-detail), [§9](Rustline-Spec.md#9-multi-criteria).

### Steps

**1. Read the RAPTOR paper (sections 1–3) once**
- *Why:* 10 pages, very readable; everything below maps to it. Keep `vulture`'s source open as a reference, but write your own.

**2. Active services for a date (`transit/calendar.rs`)**
- *How:* `fn is_active(&self, service: ServiceIdx, date: NaiveDate) -> bool` using the bitset from M2. Simple version now; edge cases in M5.

**3. Time axis**
- *How:* for M3 only search one service day: absolute time = seconds since that day's noon-minus-12h. (M5 adds the previous day for after-midnight.)
- *Why:* get the algorithm right on the easy case first.

**4. RAPTOR data structures (`transit/raptor.rs`)**
- *How:*
```rust
struct RaptorState {
    best: Vec<u32>,                  // best arrival per stop, any round
    rounds: Vec<Vec<u32>>,           // arrival per stop per round k
    back: Vec<Vec<Option<Label>>>,   // how we got there (for reconstruction)
    marked: FixedBitSet,             // stops improved in the last round
}
enum Label { Ride { trip: TripIdx, board_stop: StopIdx, board_pos: u16 }, Walk { from: StopIdx, secs: u32 }, Start }
```
  Use `u32::MAX` as "unreachable". Crate `fixedbitset`.
- *Why:* plain vectors indexed by `StopIdx` — no heap/priority queue — is exactly why RAPTOR is fast.

**5. The round loop**
- *How:* round 0: origin stop(s) get departure time. For k = 1..=max_transfers+1:
  1. collect patterns serving marked stops (remember earliest marked position per pattern);
  2. for each pattern walk along its stops: if you're "on" a trip, update arrival at this stop if better; check whether an earlier trip can be caught here (`earliest_trip(pattern, pos, time)` — binary search on departures, skip inactive services);
  3. relax footpaths (in M3: only `transfers.txt` or none; real footpaths in M4);
  4. stop when nothing is marked.
- *Why:* round k = journeys with k rides, so you get the arrival × transfers Pareto set for free.

**6. Journey reconstruction**
- *How:* for each round k where the target improved, follow `back` pointers from target to origin, building `Leg::Transit` / `Leg::Walk` (types from spec §11.1, in `rustline-model`).
- *Why:* the algorithm only stores times; you need the trail to show the user the legs.

**7. Ranking (first version)**
- *How:* keep per-round results, drop one with an extra ride unless ≥ 5 min earlier (`transfer_penalty`), cap `max_results`.

**8. CSA oracle (`tests/` or `transit/csa.rs` under `#[cfg(test)]` / feature `oracle`)**
- *How:* sort all connections (one trip hop) by departure, scan once, earliest arrival only. ~60 lines.
- *Why:* two different algorithms agreeing on 1 000 random queries is strong evidence both are right — great for the thesis.

**9. CLI + golden tests**
- *How:* `rustline plan --pack … --from-stop <gtfs_stop_id> --to-stop … --at …` pretty-prints legs. Add `insta` and 10 golden stop-to-stop queries in `tests/golden/` (check each once by hand against the RATBV timetable).

### How to check it works
```bash
cargo test --locked -p rustline-core                       # fixture tests: direct ride, one transfer, unreachable
cargo test --locked -p rustline-core --features oracle     # RAPTOR == CSA on 1 000 random queries
cargo insta review                                # approve golden outputs once
cargo run --locked --release -p rustline-cli -- plan --pack out/*.rlp --from-stop … --to-stop … --at 2026-10-15T08:00
```

### Done when
- [ ] Fixture tests: direct ride, forced transfer, no route
- [ ] 10 golden Brașov stop-to-stop queries checked by hand and saved
- [ ] RAPTOR earliest arrival == CSA on 1 000 random queries
- [ ] Results show different ride counts when they trade off with arrival time

### Definition of done (strict)

**Interfaces**
```rust
// rustline-model (spec §11.1)
pub enum Leg { Transit { route: RouteIdx, trip: TripIdx, from: StopIdx, to: StopIdx, dep: Seconds, arr: Seconds }, Walk { from: StopIdx, to: StopIdx, secs: u32 } }
pub struct Journey { pub legs: Vec<Leg>, pub dep: Seconds, pub arr: Seconds, pub rides: u8 }
// rustline-core
pub fn raptor_stop_to_stop(tt: &Timetable, from: StopIdx, to: StopIdx, date: NaiveDate, dep: Seconds, max_transfers: u8) -> Vec<Journey>;
```
- Times inside the engine are `u32` seconds from service-day start; `u32::MAX` = unreachable; all additions `saturating_add` or checked.
- CSA oracle lives behind `#[cfg(test)]` or feature `oracle`, never in the default build.

**Acceptance criteria**
- [ ] Fixture tests: direct ride, forced transfer, no route (empty result, not error), origin == destination (zero legs), inactive service on that date skipped, `pickup_type=1`/`drop_off_type=1` respected.
- [ ] Returned journeys form a Pareto set on (arrival, rides): no journey is dominated by another.
- [ ] Every journey is valid: legs continuous (each leg starts where the previous ended), times non-decreasing, each transit leg's times equal the timetable.
- [ ] RAPTOR earliest arrival == CSA on **1 000** random queries (fixed seed, printed) on Brașov **and** on Cluj and Timișoara; 0 mismatches. Mismatch output prints the query so it can be replayed.
- [ ] 10 golden Brașov queries checked by hand against the RATBV timetable; who/when checked is noted in the test file. Uses `insta`; snapshot files committed.
- [ ] Plan CLI: `rustline plan --pack <rlp> --from-stop <gtfs_stop_id> --to-stop <id> --at <ISO>`; unknown stop id → clear error, exit code ≠ 0, no panic.
- [ ] Release timing for the 1 000 queries (median/p95) printed and noted in the report (target comes in M5).

**Edge cases to test**
- Unknown stop id; stop with no patterns; departure time after the last trip of the day (empty result).
- Trip that visits the same stop twice (loop route).
- Two trips with equal departure; `max_transfers = 0`.
- Boarding at the stop you just alighted from in the same round.

**Tester will check**
- Commands verbatim; oracle on 3 feeds with a different seed than Builder's; golden snapshots stable on re-run.
- Random fuzz of query parameters (random stop pairs/times incl. 0 and 30:00:00) → no panic.
- Pareto and continuity invariants on every returned journey.

### Common pitfalls
- Off-by-one in rounds (round 0 = no ride). Write it on paper with the fixture first.
- Boarding a trip at the stop you just alighted from in the same round → compare with *previous* round's arrival when boarding.
- Forgetting `pickup_type=1` / `drop_off_type=1` (no boarding/alighting).
- `u32::MAX + walk` overflows → use `saturating_add`.

**Time (est.):** 10–14 evenings.

---

## M4 — Streets (walking)

**Goal:** door-to-door from coordinates: walk to a stop, ride, transfer on foot, walk to B — and "just walk" when that's better.
**Prerequisites:** M3. Brașov OSM extract.
**Spec:** [§7.3–7.4](Rustline-Spec.md#73-osm-ingest-rustline-osm), [§8.1 steps 1–3, 6](Rustline-Spec.md#81-query-algorithm-in-detail), [§9 walk-only rule](Rustline-Spec.md#9-multi-criteria).

### Steps

**1. Get the OSM extract**
- *How:*
```bash
sudo pacman -S osmium-tool
curl -o data/raw/romania.osm.pbf https://download.geofabrik.de/europe/romania-latest.osm.pbf
osmium extract -b 25.45,45.58,25.72,45.72 data/raw/romania.osm.pbf -o data/raw/brasov.osm.pbf
```
  (The numbers above are a placeholder. Add `rustline bbox --gtfs <zip> --margin-km 2` that prints the bbox from the feed's stops, and use its output; never hard-code a bbox in the engine.)
- *Why:* the whole country is ~250 MB; Brașov alone is a few MB and builds in seconds.

**2. Parse walkable ways (`rustline-osm`)**
- *How:* `osmpbf` crate, two passes: ways with a walkable `highway` tag (list in spec §7.3) and not `foot=no`/`access=private`; then the nodes they use. Output `FootGraph { node_coords: Vec<LatLon>, edges_start: Vec<u32>, edge_to: Vec<u32>, edge_len_m: Vec<f32>, edge_kind: Vec<u8> }` (CSR, both directions).
- *Why:* pedestrians walk both ways on every street, so edges are added twice. CSR again for speed and easy saving.

**3. Clean the graph**
- *How:* keep the largest connected component (+ components containing stops); contract degree-2 nodes but keep their coordinates as edge geometry.
- *Why:* fewer nodes = smaller pack and faster Dijkstra; geometry is still there for drawing the line on the map.

**4. Snapping with an R-tree (`street/snap.rs`)**
- *How:* crate `rstar`; index edge segments; `fn snap(&self, p: LatLon) -> Option<Snap { edge, offset_m, dist_m }>`; > 300 m → `PlanError::OutsideCoverage`.
- *Why:* the user's position is rarely exactly on a node.

**5. Link stops**
- *How:* snap every GTFS stop at build time (≤ 100 m); list failures in the build report and as `Warning::StopNotLinkedToStreets`.

**6. Dijkstra / A\* (`street/search.rs`)**
- *How:* `BinaryHeap<Reverse<(u32 secs, u32 node)>>`; one-to-many with a max cost (access/egress), A\* with straight-line heuristic for direct A→B. Cost = length / 1.3 m/s, steps ×1.5.
- *Why:* classic, correct, and fast enough at city size; no contraction hierarchies needed (spec §8).

**7. Precompute footpaths (pack v2)**
- *How:* at build time Dijkstra from each stop up to 500 m → `footpaths` CSR section. Add the street graph + R-tree + footpaths to the pack, bump `format_version` to 2, fill `osm_sha256`. Fallback without OSM: straight line × 1.3.
- *Why:* transfers on foot are the same every query, so compute them once.

**8. Wire into RAPTOR**
- *How:* round 0 = all access stops with `t_dep + walk`; after each round relax footpaths; target = min over egress stops of `arrival + egress walk`.

**9. `plan.rs` orchestration**
- *How:* `Engine::plan(&PlanRequest) -> Result<PlanResponse, PlanError>` exactly following spec §6.4: snap → access → egress → direct walk → RAPTOR → build itineraries → add walk-only if `walk_arrival ≤ best_transit + 3 min` → Pareto filter → sort.
- *Why:* this is the function the app will call; everything else is a helper.

**10. CLI**
- *How:* `rustline plan --from 45.64,25.59 --to 45.66,25.61 --at 2026-10-15T08:00` (UC7). Optional `--geojson out.geojson` to view the result on geojson.io.

### How to check it works
```bash
cargo run --locked --release -p rustline-cli -- build-pack --gtfs data/raw/brasov.zip --osm data/raw/brasov.osm.pbf --out out/
cargo run --locked --release -p rustline-cli -- plan --pack out/*.rlp --from 45.6427,25.5887 --to 45.6579,25.6012 --at 2026-10-15T08:00 --geojson /tmp/j.geojson
cargo test --locked --workspace
```
Open the GeoJSON in geojson.io and check the walking lines follow real streets.

### Done when
- [ ] Brașov foot graph built; stats (nodes, edges, size) printed
- [ ] All stops linked or listed in the report
- [ ] Door-to-door golden tests pass, including "walk is better" (UC3) and "point outside the city" (`OutsideCoverage`)
- [ ] Pack v2 loads; old v1 packs rejected with `UnsupportedVersion`

### Definition of done (strict)

**Interfaces**
```rust
// rustline-osm
pub fn build_foot_graph(pbf: &Path, bbox: Option<BBox>) -> Result<(FootGraph, OsmReport), OsmError>;
pub struct FootGraph { /* CSR, u32 indices, f32 metres */ }
// rustline-core
pub struct PlanRequest { pub from: LatLon, pub to: LatLon, pub time: TimeSpec, pub walk_speed_mps: f32, pub max_access_m: u32, pub max_transfer_m: u32, pub max_transfers: u8, pub max_results: u8 }
impl Default for PlanRequest { /* spec defaults: 1.3, 1000, 500, 3 */ }
impl Engine { pub fn plan(&self, req: &PlanRequest) -> Result<PlanResponse, PlanError>; }
pub enum PlanError { InvalidRequest(String), OutsideCoverage, NoRoute, DateOutsideFeed }
```

**Acceptance criteria**
- [ ] Foot graph stats (nodes, edges, components kept, pack size delta) printed by `build-pack` for Brașov and Cluj; numbers in the report.
- [ ] Every stop linked ≤ 100 m or listed in the build report; Brașov count of unlinked stops recorded.
- [ ] `PlanRequest` validation: NaN/inf/out-of-range lat/lon, speed ≤ 0, absurd limits → `InvalidRequest`, never panic.
- [ ] Door-to-door golden tests (≥ 8): normal trip, "walk is better" (UC3), origin == destination, point > 300 m from any street → `OutsideCoverage`, both points near the same stop.
- [ ] Never worse than walking: for 1 000 random coordinate pairs inside the coverage, best result arrival ≤ walk-only arrival (property test, fixed seed).
- [ ] Footpaths are symmetric within 1 s on an undirected graph (test).
- [ ] Pack v2 loads; v1 packs return `UnsupportedVersion`; pack without OSM uses the straight-line fallback and says so in `PackInfo`.
- [ ] Release timing for 1 000 door-to-door queries on Brașov noted in the report.

**Edge cases to test**
- Zero-length edges, duplicate nodes, ways with one node, ways referencing missing nodes (skip + report, no panic).
- `foot=no`, `access=private`, motorways excluded; `highway=steps` cost ×1.5.
- Bbox that contains no streets; OSM file that is not a PBF → `OsmError`.
- Points exactly on a node; on the antimeridian/poles in input → `InvalidRequest` or `OutsideCoverage`, no panic.

**Tester will check**
- Commands verbatim; GeoJSON of 3 queries opened and walking lines follow streets.
- Property test with a different seed; random coordinates incl. garbage → no panic.
- `build-pack` with and without `--osm` on 2 cities; genericity `rg` (no Brașov bbox in src).

### Common pitfalls
- Using degrees as metres. Use haversine (or an equirectangular approximation) for distances.
- Forgetting reverse edges → one-way walking.
- Snapping to a motorway or a private road → filter tags *before* building the R-tree.
- Pack size jumps → measure, drop geometry if needed (spec §18).

**Time (est.):** 10–14 evenings.

---

## M5 — Calendar & edge cases

**Goal:** correct answers on the hard days: after midnight, DST switches, holidays, frequency-based trips, transfers.txt. Benchmarks meet the PC targets.
**Prerequisites:** M4.
**Spec:** [§10](Rustline-Spec.md#10-time-calendars-timezone-dst-after-midnight), [§7.2](Rustline-Spec.md#72-gtfs-ingest-rustline-gtfs), [§12](Rustline-Spec.md#12-performance-targets-and-budgets-all-est-to-be-validated-in-m5), [§15](Rustline-Spec.md#15-testing-strategy).

### Steps

**1. Time zone at the edges only**
- *How:* `chrono` + `chrono-tz`, parsing the **feed's** `agency_timezone` (stored in the pack) into a `Tz` once in `Engine::open`; invalid name → `PackError`/build error (fixes M1 gap #4). Use it only in `plan()` input/output. Convert `DateTime<FixedOffset>` → `(service_date, secs)`; convert results back with the right offset (`+02:00` / `+03:00`).
- *Why:* inside the engine everything stays simple integers; mistakes concentrate in one tested place.

**2. "Noon minus 12h"**
- *How:* service-day reference = local noon of that date minus 12 h (as an absolute instant). On 2026‑10‑25 that is 23:00 the day before in wall-clock terms. Test both DST dates: **2026‑10‑25** and **2027‑03‑28**.
- *Why:* it is the official GTFS definition and keeps 03:00 unambiguous on switch days.

**3. After midnight**
- *How:* when querying at time t on day D, RAPTOR considers trips active on D‑1 (times −86 400 s), D, and D+1 (+86 400 s) for long searches. Implement in `earliest_trip` by checking the shifted day.
- *Why:* UC4: a 23:50 query must find the 24:10 bus; a 00:20 query must also find it.

**4. Holidays (calendar_dates)**
- *How:* make sure type 1 adds, type 2 removes; test 1 December (UC5) on the fixture.

**5. frequencies.txt**
- *How:* already expanded in M1; here add tests that RAPTOR finds the generated trips and that `exact_times=0` trips carry the warning through to the result; `exact_times=0` → mark trips approximate and emit `Warning::ApproximateFrequencyTrip`.

**6. transfers.txt**
- *How:* type 2 = min time overrides footpath; type 3 = forbidden; type 1 = timed transfer (treat as 0 min).

**7. Feed validity**
- *How:* date outside `valid_from..=valid_to` → `DateOutsideFeed`; within 14 days of expiry → `Warning::FeedExpiresSoon` (UC6).

**8. Property tests + benchmarks**
- *How:* `proptest` with the invariants from spec §15 (arrival ≥ departure, legs continuous, never worse than walk-only, RAPTOR == CSA). `criterion` bench in `benches/plan.rs` with 1 000 fixed random queries (fixed seed). Add `bench.yml` (manual trigger).
- *Why:* properties find bugs you didn't think to test; benches catch slowdowns.

### How to check it works
```bash
cargo test --locked --workspace                 # DST, midnight, holiday, frequency, transfer tests
cargo test --locked -p rustline-core --test properties -- --ignored    # slow property tests on Brașov
cargo bench --locked -p rustline-core
```

### Done when
- [ ] Tests for 23:50 → 24:10, 00:20 → 24:10, both DST dates, 1 December, frequencies, forbidden transfer
- [ ] Output times carry correct offsets (+02:00 winter, +03:00 summer)
- [ ] Property tests pass on Brașov
- [ ] PC p95 < 20 ms ([spec §12](Rustline-Spec.md#12-performance-targets-and-budgets-all-est-to-be-validated-in-m5)); numbers written into the spec

### Definition of done (strict)

**Interfaces**
```rust
pub enum TimeSpec { DepartAt(DateTime<FixedOffset>) }       // ArriveBy comes in M8
pub enum Warning { FeedExpiresSoon { valid_to: NaiveDate }, ApproximateFrequencyTrip, StopNotLinkedToStreets(StopIdx) }
fn to_service_time(tz: Tz, t: DateTime<FixedOffset>) -> Vec<(NaiveDate, Seconds)>;  // D-1, D (and D+1)
```

**Acceptance criteria**
- [ ] UC4: 23:50 query finds the 24:10 trip; 00:20 query on the next day finds the same trip via D-1.
- [ ] DST tests on **2026-10-25** and **2027-03-28** in Europe/Bucharest and on the matching dates of a second zone (America/Toronto fixture): output offsets correct (+02:00/+03:00 and -05:00/-04:00); 03:00 on switch day handled.
- [ ] Holidays: calendar_dates type 1 adds and type 2 removes (fixture 1 December + a Brașov date with an exception).
- [ ] transfers.txt types 0–3 implemented and tested; unknown type → report warning, not failure.
- [ ] `DateOutsideFeed` and `FeedExpiresSoon` (≤ 14 days) tested at the exact boundary days.
- [ ] Invalid IANA timezone rejected at build time (M1 gap #4 test un-ignored).
- [ ] Property tests (spec §15) pass on Brașov with 1 000 cases and fixed seed; run via a documented `--test` name.
- [ ] `criterion` bench: Brașov p50/p95 for 1 000 fixed queries; **PC p95 < 20 ms**; numbers + CPU model written into spec §12.
- [ ] Every M1 Known gap is now fixed or explicitly moved to M8 with Savin's OK.

**Edge cases to test**
- Times ≥ 48:00:00; trip spanning midnight across a DST switch; a service active only on D-1.
- Query exactly at 00:00:00 and 23:59:59; feed with a single service day.
- Feed whose validity ends today / started tomorrow.

**Tester will check**
- All tests run with `TZ=UTC` and `TZ=Pacific/Auckland` set in the shell (result must not change).
- DST and midnight tests on two timezones; boundary dates; bench numbers reproduced within ±20 %.
- README Known gaps list matches the ignored tests exactly.

### Common pitfalls
- Using `Local` time (your PC's zone) anywhere → tests break in CI (UTC). Always use the pack's timezone; tests set it explicitly (Europe/Bucharest fixture **and** a non-Romanian one, e.g. America/Toronto).
- Treating a day as always 86 400 s in *wall-clock* terms; it isn't on DST days — that's exactly why the noon-minus-12h rule exists.

**Time (est.):** 6–9 evenings.

---

## M6 — Flutter integration (on the phone)

**Goal:** RoTransit plans a journey offline on your Samsung A54 using Rustline, shown in a prototype planner screen.
**Prerequisites:** M5, FRB hello-world from M0.
**Spec:** [§11.3 Dart API](Rustline-Spec.md#113-dart-via-frb-rustline-ffi), [§13 integration](Rustline-Spec.md#13-integration-with-rotransit), [§12 phone targets](Rustline-Spec.md#12-performance-targets-and-budgets-all-est-to-be-validated-in-m5).

### Steps

**1. Plugin package**
- *How:* in the Rustline repo: `flutter_rust_bridge_codegen create rustline_flutter --template plugin` (check `--help` for the exact flag in your FRB version), then point its Rust side at a new crate `crates/rustline-ffi` (add it to the workspace).
- *Why:* the plugin (Dart package + native build via CargoKit) is what RoTransit depends on; `rustline-ffi` keeps all FRB-specific code in one thin crate.

**2. Thin FFI API (`rustline-ffi/src/api.rs`)**
- *How:* FRB-friendly mirror types (`FfiPlanRequest`, `FfiItinerary`, `FfiLeg` as an enum) and:
```rust
pub struct RustlineEngine { inner: Arc<rustline_core::Engine> }   // #[frb(opaque)]
impl RustlineEngine {
    pub fn open(path: String) -> anyhow::Result<RustlineEngine> { … }
    pub fn plan(&self, req: FfiPlanRequest) -> anyhow::Result<FfiPlanResponse> { … }
    pub fn info(&self) -> FfiPackInfo { … }
}
```
  Times cross as ISO strings or epoch millis + offset. Run codegen: `flutter_rust_bridge_codegen generate`.
- *Why:* keeping FFI types separate means the core stays free of FRB attributes and the server can reuse it. FRB turns `Result::Err` into Dart exceptions and runs calls off the UI thread.

**3. Android build**
- *How:* `cd rustline_flutter/example && flutter run` on the emulator, then the A54. Check the `.so` size: `/usr/bin/ls -lh build/app/intermediates/merged_native_libs/release/*/lib/arm64-v8a/`. Only ship `arm64-v8a` (+ `x86_64` for emulator debug).
- *Why:* target < 4 MB stripped per ABI (spec §12).

**4. Add to RoTransit**
- *How:* new branch in RoTransit; in `frontend/pubspec.yaml`:
```yaml
rustline_flutter:
  path: ../../Rustline/rustline_flutter    # or git: url + ref later
```
  Bundle `assets/data/brasov.rlp.zst`; on first start decompress to app documents dir (same idea as the companion pack), then `RustlineEngine.open`.
- *Why:* path dependency = fast iteration while both repos change.

**5. Pack freshness**
- *How:* compare `engine.info().feedSha256` with `/api/feeds` `current.sha256` (logic already exists for the companion pack). Mismatch → "timetable may be outdated" banner.

**6. Riverpod + prototype screen**
- *How:* `features/journey/`: `journey_engine_provider.dart` (FutureProvider opening the engine once), `journey_planner_screen.dart` (from/to picker using existing map + "my location"), `journey_results_list.dart`, polyline on flutter_map from leg geometry. Link transit legs to existing stop boards via GTFS `stop_id`.
- *Why:* same patterns as the rest of the app; the engine opens once and is reused.

**7. Measure on the phone**
- *How:* in debug UI or an integration test run 100 fixed queries in **release/profile** mode, log p50/p95 and pack load time.
- *Why:* debug builds of Rust are 10–50× slower; only release numbers count.

**8. Integration test + CI**
- *How:* `integration_test/rustline_smoke_test.dart` opens the pack and plans one journey. Add `android.yml` in Rustline: `cargo ndk -t arm64-v8a build --release -p rustline-ffi`.

### How to check it works
```bash
cd ~/Projects/Rustline/rustline_flutter/example && flutter run --release
cd ~/StudioProjects/RoTransit/frontend && flutter test integration_test/rustline_smoke_test.dart -d <A54 id>
```
Airplane mode on → plan a journey → it still works.

### Done when
- [ ] `rustline-ffi` + `rustline_flutter` plugin build for Android
- [ ] RoTransit opens the bundled pack and shows itineraries offline on the A54
- [ ] Phone p95 and load time measured and written into spec §12 (targets: < 150 ms, < 500 ms)
- [ ] Native lib size per ABI measured (< 4 MB target)
- [ ] Smoke integration test passes; Android CI job green
- [ ] OSM/RATBV ODbL attribution shown in the app

### Definition of done (strict)

**Interfaces**
- `rustline-ffi` depends on `flutter_rust_bridge = "=2.12.0"`; Dart package and codegen also 2.12.0 (RoTransit is on Dart 3.6).
- FFI types are plain data (`String`, `f64`, `i64`, `Vec`, enums); no `rustline_core` type crosses the bridge. Times cross as epoch millis + offset seconds.
- Errors cross as a Dart exception with a stable code (`invalid_request`, `outside_coverage`, `no_route`, `date_outside_feed`, `pack_corrupt`, `pack_version`).

**Acceptance criteria**
- [ ] `cargo ndk -t arm64-v8a build --locked --release -p rustline-ffi` succeeds; stripped `.so` size per ABI recorded (< 4 MB target).
- [ ] Example app and RoTransit build with `flutter build apk --release`; APK size delta vs RoTransit `main` recorded.
- [ ] Opening a corrupt/truncated pack and a v1 pack shows an error in the UI; the app does not crash (tested in the integration test).
- [ ] No panic path in `rustline-ffi`: every public fn returns `Result`; garbage input from Dart (NaN coords, empty strings) → error.
- [ ] On the A54, airplane mode: a journey is planned and shown on the map.
- [ ] 100 fixed queries in **profile/release**: p50/p95 and pack load time recorded in spec §12 (targets < 150 ms p95, < 500 ms load).
- [ ] Pack freshness banner shows when sha differs (tested with a fake sha).
- [ ] `android.yml` CI job (arm64 cross-build, `--locked`) green; OSM + feed attribution visible in the app.
- [ ] RoTransit changes are on their own branch, nothing in RoTransit `main` touched.

**Edge cases to test**
- Engine opened twice / concurrently (provider rebuild); plan called while engine is loading.
- Missing pack file; pack on a full disk during decompression; device clock in a different timezone than the feed.

**Tester will check**
- Builds and integration test verbatim; FRB versions identical in all three places.
- Corrupt-pack and garbage-input cases do not crash the app.
- Phone numbers come from release/profile builds; Savin confirms the on-device run (only he has the A54).

### Common pitfalls
- Panics crossing FFI → crash. Keep `panic = "unwind"` for `rustline-ffi` (FRB catches panics) — don't set `panic="abort"` there.
- Mismatched FRB versions (codegen vs Dart vs Rust crate) → regenerate after every upgrade.
- Gradle/NDK version mismatch with RoTransit's Android config → align `ndkVersion` in `build.gradle`.
- Running the engine in debug mode and concluding it's slow.

**Time (est.):** 10–14 evenings.

---

## M7 — Server (optional)

**Goal:** the same engine answers `POST /v1/plan` over HTTP, with identical results to the library.
**Prerequisites:** M5 (does not need M6).
**Spec:** [§11.4](Rustline-Spec.md#114-http-json-rustline-server-optional).

### Steps
1. **Crate** `crates/rustline-server` with `axum`, `tokio`, `serde_json`, `tower-http` (CORS, tracing). *Why:* axum is the standard, lightweight Rust web framework.
2. **Routes:** `GET /v1/info`, `POST /v1/plan`. Engine in `Arc<Engine>` shared state (it's `Send + Sync`).
3. **Error mapping:** `InvalidRequest`→400, `NoRoute`→404, `OutsideCoverage`/`DateOutsideFeed`→422, JSON body `{"error": "..."}`.
4. **JSON schema:** derive `schemars::JsonSchema` on request/response types; write `schema/plan.v1.json` from a test and fail if it changes unexpectedly.
5. **Docker:** multi-stage `Dockerfile` (`rust:…` build → `debian:stable-slim` run), pack mounted as a volume.

### How to check it works
```bash
cargo run --locked --release -p rustline-server -- --pack out/*.rlp --port 8088
curl -s localhost:8088/v1/plan -H 'content-type: application/json' \
  -d '{"from":{"lat":45.6427,"lon":25.5887},"to":{"lat":45.6579,"lon":25.6012},"departAt":"2026-10-15T08:00:00+03:00"}' | jq
```

### Done when
- [ ] `/v1/plan` and `/v1/info` work; errors use the right status codes
- [ ] Test: server response == `Engine::plan` for the golden queries
- [ ] Schema file generated and checked in tests
- [ ] Docker image builds and runs

### Definition of done (strict)
- [ ] Every request body validated; malformed JSON → 400 with `{"error": ...}`, never 500 or a panic. Body size limit set.
- [ ] Test: for every golden query, server JSON == `Engine::plan` serialised (byte-equal).
- [ ] Plan runs in `spawn_blocking`; 100 concurrent requests in a test complete without errors.
- [ ] Deps (axum, tokio, schemars) satisfy MSRV 1.85 or the MSRV bump is agreed with Savin first.
- **Tester will check:** curl command verbatim, bad-input fuzz (empty, huge, wrong types), status codes, Docker build.

### Common pitfalls
- Blocking the async runtime with a long plan → wrap in `tokio::task::spawn_blocking`.
- Exposing it publicly without limits — if you put it behind your Cloudflare tunnel, add rate limiting.

**Time (est.):** 3–5 evenings.

---

## M8 — Quality+

**Goal:** nicer answers and thesis-ready material: walking as a 3rd criterion, arrive-by, "leave between" ranges, charts.
**Prerequisites:** M6.
**Spec:** [§8](Rustline-Spec.md#8-algorithms-and-why), [§9](Rustline-Spec.md#9-multi-criteria).

### Steps
1. **McRAPTOR:** replace single arrival per stop with a small "bag" of labels `(arrival, walk_secs)` and keep only non-dominated ones. *Why:* lets the user see "2 min later but 600 m less walking".
2. **Depart-late pass** (spec §8.1 step 7): rerun backwards from the found arrival to cut waiting at stops.
3. **Arrive-by:** reverse RAPTOR (scan backwards in time from the target). Add `TimeSpec::ArriveBy`.
4. **rRAPTOR range queries:** run for each departure in a window, reusing labels.
5. **Thesis material:** benchmark charts (`criterion` HTML reports), RAPTOR vs CSA comparison, pack size table, phone vs PC timings. `cargo doc` for the API.
6. Update golden tests (`cargo insta review`) and approve intentional changes.

### Done when
- [ ] McRAPTOR returns walking trade-offs; property tests still pass
- [ ] Arrive-by works in CLI and app
- [ ] Golden tests updated and reviewed
- [ ] Charts + docs ready for the thesis

### Definition of done (strict)
- [ ] McRAPTOR bags capped (cap documented); result sets are Pareto on (arrival, rides, walk) – property test.
- [ ] Arrive-by: for 1 000 random queries, arrival ≤ requested time and no later departure exists with the same arrival bound (checked against brute force on a fixture).
- [ ] Phone p95 re-measured after McRAPTOR and still < 150 ms, or the regression is reported.
- **Tester will check:** M3–M5 property tests still pass; golden diffs reviewed one by one; phone timings.

### Common pitfalls
- Bags growing large → cap per stop and prune aggressively; re-check phone p95.

**Time (est.):** 10–15 evenings.

---

## M9 — Future (separate spec)

Not planned in detail; write a new spec when you get here ([spec §14](Rustline-Spec.md#14-realtime-future), [§17](Rustline-Spec.md#17-milestones)):
- **Backend-served packs:** backend_v2 builds a pack when FeedService detects a new sha and serves `/api/feeds/{id}/routing-pack` with the same ETag logic; the app downloads it instead of waiting for an app update.
- **Realtime:** GTFS‑RT delay overlay (only if RATBV publishes it).
- **Multi-city:** the other 12 feeds — re-check pack sizes and phone memory.
- **iOS:** add `aarch64-apple-ios` targets and a macOS CI runner.

---

## Appendix A — Crates you'll add, and when

| Crate | Milestone | Used for |
|---|---|---|
| `serde`, `thiserror`, `anyhow` | M1 | data types, errors |
| `gtfs-structures`, `chrono` | M1 | GTFS parsing, dates |
| `clap` | M1 | CLI |
| `rkyv` or `bytemuck`, `zstd`, `sha2` | M2 | pack format |
| `fixedbitset`, `insta`, `rand` | M3 | RAPTOR, golden tests, random queries |
| `osmpbf`, `rstar` | M4 | OSM, snapping |
| `chrono-tz`, `proptest`, `criterion` | M5 | timezone, property tests, benches |
| `flutter_rust_bridge` | M6 | FFI |
| `axum`, `tokio`, `schemars`, `tower-http` | M7 | server |

## Appendix B — Daily loop

```bash
git switch -c horia/mN-something
# write a test → make it pass → refactor
cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add -p && git commit
git push -u origin HEAD     # open PR, CI must be green, merge
```
