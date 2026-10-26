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
| Q1 On-device or server first | **On-device** (confirmed) | M6 before M12 |
| Q2 Public open-source? | Public repo, **MIT OR Apache-2.0**, not published to crates.io yet | M0 licence |
| Q3 Brașov only or 13 feeds | **Brașov only** | Size/perf targets in M2, M5, M6 |
| Q4 iOS? | **Android first**, iOS in M11 (Savin, 2026-10-20) | M0 targets, M6, M11 |
| Q5 Who builds packs | **Your PC** with `rustline build-pack` | M2, M4; backend later in M14 |
| Q6 Defaults (walk speed etc.) | Spec defaults: 1.3 m/s, 1 000 m access, 500 m transfer, 3 transfers | `PlanRequest::default()` |
| Q7 Arrive-by in v1? | **Yes** — M8 (Savin, 2026-10-20) | M3 API only has `DepartAt` |
| Q8 RATBV realtime | Assume none | M14 |
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

**4. Bug classes feed forward (Leader, whenever Tester or Reviewer finds a bug)**
- When a finding is a *class* of bug (not a one-off typo), Leader adds it to the **Builder checklist** (usually §11) or to the **Edge cases (must be tested)** list of every later milestone it can affect, in the same round the fix is requested.
- The hand-over report names which checklist/edge-case lines were added. *Why:* M1/M2 found the same kind of bug twice (sizing memory from an untrusted header); the second time should have been impossible.

**5. Progress note (Leader, after each approved milestone)**
- Leader adds a 2–3 line entry to the Obsidian note `Projects/Rustline/Progress.md`: what was done (with the commit hash), issues found in testing/review, and what's next.

### Builder checklist (applies to every milestone)

Learned from the M0/M1 reviews. Every item is checked by Tester/Reviewer; skipping one costs a review round.

**1. Generic engine**
- No city, agency, timezone, bbox, feed id or coordinate literal in `crates/*/src`. Everything comes from the feed, the OSM file or CLI/env arguments.
- Real coordinates only in fixtures and ignored real-feed tests, never in engine code. Prefer neutral coordinates in unit tests.
- Check: `rg -n -i 'brasov|brașov|ratbv|bucharest|europe/|45\.6|25\.5' crates/*/src` returns nothing.

**2. Toolchain**
- MSRV is **1.88** (`rust-version` in the workspace; raised from 1.85 on 2026-10-26 because zip 8.6 needs 1.88). Before adding or bumping a dependency, check its own `rust-version`; if a newer version needs more, pin the last compatible one with a comment and a Dependabot `ignore` for the incompatible range.
- Every cargo command in code, docs and CI uses `--locked` (except `cargo fmt`, which has no such flag). `Cargo.lock` is committed.
- Check: `cargo +1.88 test --locked --workspace` passes.

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

**11. Untrusted input and resource safety**

Learned from M1/M2 (zstd bomb, header-sized `resize`, silent bit-flips, release-only overflow). Treat every pack, GTFS zip, OSM file, CLI argument, FFI call and HTTP body as hostile.
- **Never size an allocation from an untrusted length.** No `resize`/`with_capacity`/`vec![0; n]` with `n` from a file or request. Check `n` against a cap (and against the file's actual size) first, then grow incrementally (≈ 1 MiB chunks) or use `try_reserve` and turn failure into an error.
- **Every read and decompression has a size cap**, configurable, with a phone-safe default of **256 MiB**, checked *before* the work (declared zstd content size, header lengths, zip entry sizes, OSM blob sizes).
- **Every persisted artefact has an integrity checksum** (pack CRC trailer + zstd frame checksum, and the same for any new section or file). The format version is checked first and gives a clear `UnsupportedVersion`, not a checksum error.
- **No panics on any input:** no `unwrap`/`expect`/`[i]` indexing/`as` casts on data-derived values. Use `get()`, `checked_*`/`saturating_*`, `try_from`. Public APIs return `Result`. Fuzz/mutation tests (bit flips, truncation, random bytes, huge lengths) run in **both debug and release**.
- **Bound time as well as memory.** No loop, search or recursion whose length comes from crafted input without a cap (rounds, Dijkstra settled nodes, label bags, reconstruction length, retries). Each milestone states its time and memory budget and tests a hostile input against it.
- **Validation must bound derived work, not just ranges:** any structure built from pack data (grids, indexes) must have a cap and a tamper test. *(Added after M4 review R28 MF1: in-range street coordinates spread over the globe made the snapping grid need 3.5 GB at load.)*
- **No `as` casts on derived values:** any value computed from input (shifted times, offsets, counts, lengths) uses `checked_*`/`try_from`, and an overflow becomes a reported error or a skipped row, never a silent wrap. Every user-visible time prints its UTC offset. *(Added after M5 review MF1/MF2: frequency expansion wrapped late times with `as u32`; CLI legs printed times without offsets, so DST-day legs were ambiguous.)*
- **Shared files need exclusive ownership:** any write that ends in a rename (installs, caches, packs) holds a lock for the whole operation and uses its own `create_new` temp file; leftovers are cleaned only under that lock; a two-writer test is required. Panic/error text that crosses a boundary is fixed text, never the panic payload or a file:line. *(Added after M6 review MF1/MF2.)*
- **Measure peak RSS** (`/usr/bin/time -v`) on the largest feed (MBTA) *and* on the hostile inputs; record both in the report.
- **Docs commands run verbatim** from `fixtures/feeds/`, with explicit output paths (no `out/*.rlp` globs that can match two files).
- **Licence/attribution** recorded in `fixtures/manifest.tsv` for every feed and OSM extract; anything shown to users carries the attribution.
- **Reproducible output:** same inputs + `SOURCE_DATE_EPOCH` → byte-identical artefacts; no `HashMap` order, paths, hostnames or local time in the output.
- **Types that passed validation must not expose mutable internals** (no pub fields or `DerefMut` on validated data).
- **Algorithms with several arrival kinds (ride/walk) must keep separate labels**; test with an oracle on the largest feed across several seeds.

### Decisions (Leader, 2026-10-20)
- Savin reordered the plan after M6: **M7** cleanup (old M8's "Moved from M1" gaps + M6 Reviewer leftovers R1–R6), **M8** arrive-by, **M9** rRAPTOR options, **M10** GitHub CI, **M11** iOS build (GitHub macOS runners, dev PC is Linux), **M12** server (optional, not now; was M7), **M13** RoTransit integration (not now; M6 steps 4–6), **M14** future (was M9; also holds old M8's McRAPTOR walking criterion and thesis charts).
- RoTransit integration stays last. The engine stays generic for any GTFS feed; Builder checklist §11 applies to every new milestone.
- References elsewhere ("moved to M8" in README Known gaps) now read M7.

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
| M7 | Cleanup | M1 Known gaps #1–3, #5, #6 + M6 Reviewer leftovers | 5–8 evenings |
| M8 | Arrive-by | "Be there by 09:00": reverse RAPTOR, depart-late pass | 6–9 evenings |
| M9 | rRAPTOR options | Pareto (arrival vs transfers) + departure-window range queries | 6–10 evenings |
| M10 | GitHub CI | fmt, clippy, tests debug+release, Flutter tests, Android build | 2–4 evenings |
| M11 | iOS build | xcframework + Cargokit iOS, tested on GitHub macOS runners | 4–6 evenings |
| M12 | Server (optional, not now) | Same engine behind `POST /v1/plan` | 3–5 evenings |
| M13 | RoTransit integration (not now) | M6 steps 4–6 inside RoTransit | 6–10 evenings |
| M14 | Future | Backend-served packs, realtime, multi-city, McRAPTOR walking, thesis charts (own spec) | — |

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
  Leave out `rustline-ffi` and `rustline-server` for now (M0 step 9 and M12).
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
cargo run --locked --release -p rustline-cli -- inspect out/rustline-brasov-manifest-627559bac2a3-v3.rlp
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
- [ ] Format decision (rkyv vs bytemuck) written into spec §7.5 with numbers. The chosen crates satisfy the MSRV (1.85 at M2, 1.88 since 2026-10-26).
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
     *Transfer rule (Leader, M3 decision 8):* changing trips at the same stop takes 0 s plus that stop's own `transfers.txt` row (`X → X`, `min_transfer_time`) if there is one. The spec says nothing about sibling platforms of one parent station, so in M3 they are linked **only** by a `transfers.txt` row; without one there is no transfer between them until the M4 footpaths.
  4. stop when nothing is marked.
- *Why:* round k = journeys with k rides, so you get the arrival × transfers Pareto set for free.

**6. Journey reconstruction**
- *How:* for each round k where the target improved, follow `back` pointers from target to origin, building `Leg::Transit` / `Leg::Walk` (types from spec §11.1, in `rustline-model`).
- *Why:* the algorithm only stores times; you need the trail to show the user the legs.

**7. Ranking (first version)**
- *How:* keep per-round results, drop one with an extra ride unless ≥ 5 min earlier (`transfer_penalty`), cap `max_results`.
  *Note (M3 review MF2):* M3 has no `max_results` parameter; the result is the whole Pareto set after the 5-minute rule (at most 9 journeys; M9 drops the rule for door-to-door plans, `raptor_stop_to_stop` keeps it). `max_results` (including `max_results` = 0) arrives with `PlanRequest` in M4 and is tested there.

**8. CSA oracle (`tests/` or `transit/csa.rs` under `#[cfg(test)]` / feature `oracle`)**
- *How:* sort all connections (one trip hop) by departure, scan once, earliest arrival only. ~60 lines.
- *Why:* two different algorithms agreeing on 1 000 random queries is strong evidence both are right — great for the thesis.

**9. CLI + golden tests**
- *How:* `rustline plan --pack … --from-stop <gtfs_stop_id> --to-stop … --at …` pretty-prints legs. Add `insta` and 10 golden stop-to-stop queries in `tests/golden/` (check each once by hand against the RATBV timetable).

### How to check it works
```bash
cargo test --locked -p rustline-core                       # fixture tests: direct ride, one transfer, unreachable
cargo test --locked -p rustline-core --features oracle     # the oracle feature's CSA agrees with RAPTOR (tiny + 300 Brașov queries)
cargo test --locked --release -p rustline-core --test m3_raptor --test m3_budget -- --ignored --nocapture   # RAPTOR == CSA on 1 000 random queries per feed + MBTA budget (slow, ignored by default)
cargo insta review                                # approve golden outputs once
cargo run --locked --release -p rustline-cli -- build-pack --gtfs fixtures/feeds/brasov-manifest.zip --out out/ --feed-id brasov
cargo run --locked --release -p rustline-cli -- plan --pack out/rustline-brasov-627559bac2a3-v4.rlp --from-stop node/3652436629 --to-stop node/3708889570 --at 2026-10-15T08:00
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
- [ ] Safety (Builder checklist §11): `raptor_stop_to_stop` and `plan` never panic on any `StopIdx`/`TripIdx`/time from input; out-of-range index → error or empty result. Query memory is `O(stops × rounds)` allocated once per query, never sized from a pack field without the CSR check from M2.
- [ ] Bounded work: rounds ≤ `max_transfers + 1` (hard cap, e.g. 8, even if the caller asks for more); every loop over trips/patterns/stops terminates on a crafted pack (cycle in `back` pointers → reconstruction stops after `rounds` legs and returns an error, not an infinite loop).
- [ ] Budgets measured in release: MBTA 1 000 random queries p95 and peak RSS recorded; no single query > 1 s on MBTA; peak RSS of the query state ≤ 64 MiB on MBTA.
- [ ] All items in **Edge cases (must be tested)** below have a test (or a Known gap with the milestone that fixes it).

**Edge cases to test**
- Unknown stop id; stop with no patterns; departure time after the last trip of the day (empty result).
- Trip that visits the same stop twice (loop route).
- Two trips with equal departure; `max_transfers = 0`.
- Boarding at the stop you just alighted from in the same round.

**Edge cases (must be tested)**
*Query input*
- Origin == destination (zero legs, not an error); unknown/out-of-range `StopIdx` or GTFS stop id; empty pack (0 stops) and a pack with 1 stop.
- `dep` = 0, 23:59:59, ≥ 24:00:00, ≥ 48:00:00 and `u32::MAX - 1` → no overflow, no panic (saturating/checked).
- `max_transfers` = 0, 1, and 255 (clamped to the hard cap); `max_results` = 0.
- Date outside the feed's calendar; date where no service is active; feed with an empty calendar (only calendar_dates) and a calendar_dates-only feed.

*Timetable shape*
- Stop with no departures (only arrivals, or no patterns at all); stop served only by `pickup_type=1` or only by `drop_off_type=1` trips.
- Departure after the last trip of the day → empty result; destination unreachable (separate component of the network) → empty result.
- Two trips on the same pattern with identical times (tie in `earliest_trip`); overtaking trips are impossible within a pattern (M1 splits them) — assert it.
- Loop route visiting the same stop twice; very long trip (≥ 200 stops, ≥ 24 h span); trip with all `stop_times` at the same second (zero-duration hops).
- Parent stations: query from a station (`location_type=1`) resolves to its platforms, or returns a clear error; platform → sibling platform via the parent.
- `transfers.txt` absent; present with type 2 and `min_transfer_time` 0 (zero-length transfer) and very large; transfer from a stop to itself.
- Block/in-seat transfers (`block_id`): out of scope in M3 — assert they are treated as normal transfers and list it as a Known gap if the feed uses them.

*Results*
- Pareto ties: two journeys with equal arrival and equal rides → exactly one kept, chosen deterministically (lowest trip index).
- A journey with more rides that is ≥ 5 min earlier is kept; < 5 min earlier is dropped.
- Reconstruction on a crafted `back` cycle stops with an error, no hang.
- Result identical across 2 runs and between debug and release builds.

*Resources*
- MBTA query: time and peak RSS recorded; 10 000 random queries on Brașov in release → no panic, no single query > 100 ms.

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
- *Built (Leader/Builder decision, M4):* no `osmpbf`. A small hand-written reader (`rustline-osm/src/pbf.rs`, only `flate2` for zlib) reads nodes and ways in one pass (nodes must come first, as in sorted extracts). Why: every size from the file is checked against `OsmLimits` before use (blob ≤ 32 MiB, lists ≤ `max_blob_bytes / 64` items, node/way counts), no allocation is sized from a header, and it builds on the MSRV (1.88) with few dependencies. Fuzzed in debug and release (Tester `m4_osm_fuzz`, Reviewer R28: 16 000 mutations, 0 panics).
- *How (as planned):* `osmpbf` crate, two passes: ways with a walkable `highway` tag (list in spec §7.3) and not `foot=no`/`access=private`; then the nodes they use. Output `FootGraph { node_coords: Vec<LatLon>, edges_start: Vec<u32>, edge_to: Vec<u32>, edge_len_m: Vec<f32>, edge_kind: Vec<u8> }` (CSR, both directions).
- *Why:* pedestrians walk both ways on every street, so edges are added twice. CSR again for speed and easy saving.

**3. Clean the graph**
- *How:* keep the largest connected component (+ components containing stops); contract degree-2 nodes but keep their coordinates as edge geometry.
- *Why:* fewer nodes = smaller pack and faster Dijkstra; geometry is still there for drawing the line on the map.

**4. Snapping with an R-tree (`street/snap.rs`)**
- *How:* crate `rstar`; index edge segments; `fn snap(&self, p: LatLon) -> Option<Snap { edge, offset_m, dist_m }>`; > 300 m → `PlanError::OutsideCoverage`.
- *Built (Leader/Builder decision, M4):* no `rstar`. A uniform grid of 200 m cells (`rustline-core/src/street.rs`) is rebuilt from the pack's street arrays when the engine loads; each segment piece is listed in the cells its line crosses. Why: no dependency, simple to cap (≤ 1 M cells, ≤ 32 entries per piece on average, error beyond; real data ≈ 1.2), and nothing extra in the pack to validate. Cost: ~10–30 ms more load time on desktop (Brașov, București); storing the index in the pack is an M5 option if phones need it.
- *Why:* the user's position is rarely exactly on a node.

**5. Link stops**
- *How:* snap every GTFS stop at build time (≤ 100 m); list failures in the build report and as `Warning::StopNotLinkedToStreets`.

**6. Dijkstra / A\* (`street/search.rs`)**
- *How:* `BinaryHeap<Reverse<(u32 secs, u32 node)>>`; one-to-many with a max cost (access/egress), A\* with straight-line heuristic for direct A→B. Cost = length / 1.3 m/s, steps ×1.5.
- *Why:* classic, correct, and fast enough at city size; no contraction hierarchies needed (spec §8).

**7. Precompute footpaths (pack v4)**
- *How:* at build time Dijkstra from each stop up to 500 m → `footpaths` CSR section. Add the street graph + footpaths to the pack (the snapping grid is rebuilt at load, step 4), bump `format_version` to 4 (v3 was M3's pack), fill `osm_sha256`. Fallback without OSM: straight line × 1.3.
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
BBOX=$(cargo run -q --locked --release -p rustline-cli -- bbox --gtfs fixtures/feeds/brasov-latest.zip --margin-km 2)
cargo run --locked --release -p rustline-cli -- build-pack --gtfs fixtures/feeds/brasov-latest.zip --osm fixtures/feeds/romania-260101.osm.pbf --bbox "$BBOX" --out out/ --feed-id brasov --built-at 1700000000
cargo run --locked --release -p rustline-cli -- plan --pack out/rustline-brasov-ad0d22ca80b0-v4.rlp --from 45.6427,25.5887 --to 45.6579,25.6012 --at 2026-10-15T08:00 --geojson /tmp/j.geojson
cargo test --locked --workspace
```
(The same commands as the README; the feeds and the OSM extract come from `scripts/fetch-fixtures.sh`, no osmium needed: `--bbox` crops while reading.)
Open the GeoJSON in geojson.io and check the walking lines follow real streets.

### Done when
- [ ] Brașov foot graph built; stats (nodes, edges, size) printed
- [ ] All stops linked or listed in the report
- [ ] Door-to-door golden tests pass, including "walk is better" (UC3) and "point outside the city" (`OutsideCoverage`)
- [ ] Pack v4 loads; older (v3) packs rejected with `UnsupportedVersion`

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
- [ ] Pack v4 loads; v3 packs return `UnsupportedVersion`; pack without OSM uses the straight-line fallback and says so in `PackInfo`.
- [ ] Release timing for 1 000 door-to-door queries on Brașov noted in the report.
- [ ] Safety (§11): OSM reading capped (blob size, node/way counts vs a configurable limit); no allocation sized from PBF headers; new pack sections covered by the CRC; fuzzed `.osm.pbf` and v4 packs → no panic in debug and release.
- [ ] Bounded work: one-to-many Dijkstra stops at `max_access_m`/`max_transfer_m` and at a hard settled-node cap; A\* direct walk capped (e.g. 10 km); budgets recorded: Brașov query p95, `build-pack --osm` peak RSS for Brașov and a large extract (București).
- [ ] OSM licence (ODbL) and extract date recorded in `fixtures/manifest.tsv`; bbox comes from `rustline bbox`, never hard-coded.
- [ ] All items in **Edge cases (must be tested)** below have a test or a Known gap.

**Edge cases to test**
- Zero-length edges, duplicate nodes, ways with one node, ways referencing missing nodes (skip + report, no panic).
- `foot=no`, `access=private`, motorways excluded; `highway=steps` cost ×1.5.
- Bbox that contains no streets; OSM file that is not a PBF → `OsmError`.
- Points exactly on a node; on the antimeridian/poles in input → `InvalidRequest` or `OutsideCoverage`, no panic.

**Edge cases (must be tested)**
*Graph build*
- Disconnected components: small island kept only if it contains a stop; stop on a dropped component → listed and still reachable by straight-line fallback or reported.
- Missing or unknown `highway` tags, `foot=yes` on an otherwise excluded way, `foot=designated`, `access=no` + `foot=yes`.
- One-way footways (`oneway=yes` on `highway=footway`, `oneway:foot`): pedestrians still walk both ways unless `oneway:foot=yes`; document the rule and test it.
- `highway=steps` cost; elevation/`incline` ignored in v1 (stated, tested as ignored).
- Empty extract, extract with no walkable ways, huge bbox (whole country) → within the memory cap or a clear error; PBF referencing nodes outside the extract.

*Snapping and queries*
- A == B (same point) → zero-length walk or empty itinerary, no error; A and B 1 m apart.
- A or B outside the bbox, in water/a park far from any road (> 300 m) → `OutsideCoverage`; exactly at 300 m boundary.
- Snapping to the nearest *edge* (mid-segment), not just the nearest node; point equidistant from two edges → deterministic choice.
- Stops far from the graph (> 100 m) → reported, `Warning::StopNotLinkedToStreets`.
- Walking-only trip (no stop within `max_access_m`); `max_access_m` = 0; max walk distance respected exactly at the boundary.
- Footpath symmetry within 1 s; zero-length footpath between co-located stops.
- Destination reachable only by walking from the last stop across a component boundary → no route or straight-line fallback, never a hang.

*Resources*
- Fuzzed PBF (truncated, bit-flipped, giant declared blob size) → `OsmError` under the cap, no OOM.
- 1 000 random coordinate pairs incl. NaN/inf/garbage → no panic; per-query time ≤ 100 ms on Brașov release.

**Tester will check**
- Commands verbatim; GeoJSON of 3 queries opened and walking lines follow streets.
- Property test with a different seed; random coordinates incl. garbage → no panic.
- `build-pack` with and without `--osm` on 2 cities; genericity `rg` (no Brașov bbox in src).

### Common pitfalls
- Using degrees as metres. Use haversine (or an equirectangular approximation) for distances.
- Forgetting reverse edges → one-way walking.
- Snapping to a motorway or a private road → filter tags *before* building the snapping index.
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
- *How:* service-day reference = local noon of that date minus 12 h (as an absolute instant). On 2027‑03‑28 (spring forward) that is 23:00 the day before in wall-clock terms; on 2026‑10‑25 (fall back) it is 01:00+03:00 the same day. Test both DST dates: **2026‑10‑25** and **2027‑03‑28**.
- *Why:* it is the official GTFS definition and keeps 03:00 unambiguous on switch days.

**3. After midnight**
- *How:* when querying at time t on day D, RAPTOR considers trips active on D‑1, D, and D+1 for long searches. The shift between days is the real distance between their noon‑minus‑12h references (82 800 / 86 400 / 90 000 s on DST days), never a fixed 86 400 s. Implement in `earliest_trip` by checking the shifted day.
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
pub enum TimeSpec { DepartAt(DateTime<FixedOffset>), ArriveBy(DateTime<FixedOffset>) } // ArriveBy: M8
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
- [ ] Every M1 Known gap is now fixed or explicitly moved to M7 (cleanup; was M8 before the 2026-10-20 reorder) with Savin's OK.
- [ ] Safety (§11): searching D-1/D/D+1 at most triples the work (measured); time arithmetic with shifted days is checked (no wrap for times ≥ 48:00 or near `u32::MAX`); frequency expansion is capped (trips per frequency row and total) so a crafted `headway_secs=1` over 48 h cannot exhaust memory.
- [ ] All items in **Edge cases (must be tested)** below have a test or a Known gap.

**Edge cases to test**
- Times ≥ 48:00:00; trip spanning midnight across a DST switch; a service active only on D-1.
- Query exactly at 00:00:00 and 23:59:59; feed with a single service day.
- Feed whose validity ends today / started tomorrow.

**Edge cases (must be tested)**
*Time and calendar*
- Midnight crossing: 23:50 → 24:10, 00:20 → 24:10 via D-1; trip with times > 24:00 that also runs on D (same trip found twice → deduplicated).
- Service days around DST: the night of 2026-10-25 (01:00–04:00 repeated hour) and 2027-03-28 (03:00–04:00 missing hour); a query at a non-existent wall time (03:30 on spring-forward) and an ambiguous one (03:30 on fall-back) → documented choice, no panic.
- Query in a timezone other than the agency's (input `+00:00`, `-05:00`): converted correctly; output in the feed's zone. Multi-agency feed whose agencies declare different timezones → error or documented rule.
- Calendar_dates-only feed; empty `calendar.txt`; service with `start_date > end_date`; type 2 removing a day the calendar never had; type 1 and type 2 on the same day.
- Multi-day searches: query late on the last day of the feed; long searches that need D+1.

*Frequencies and transfers*
- `frequencies.txt` with `exact_times=0` and `1`, headway that doesn't divide the window, overlapping windows, window crossing midnight, `headway_secs=0` (reject).
- `transfers.txt` types 0, 1, 2 (with `min_transfer_time` 0, missing, and very large), 3 (forbidden, including stop to itself); unknown type 4/5 → warning; trip-to-trip/route-to-route transfers (GTFS extension) → ignored with a warning if unsupported.
- Blocks/in-seat transfers (`block_id`): implemented or explicitly a Known gap with a test.

*Validity and resources*
- `DateOutsideFeed` on the day before `valid_from` and the day after `valid_to`; `FeedExpiresSoon` at exactly 14 and 15 days.
- Crafted frequency rows that would expand to > the trip cap → build error, peak RSS under the cap.
- Bench reproduced on MBTA (non-Romanian timezone, largest feed): p95 and peak RSS recorded.

**Tester will check**
- All tests run with `TZ=UTC` and `TZ=Pacific/Auckland` set in the shell (result must not change).
- DST and midnight tests on two timezones; boundary dates; bench numbers reproduced within ±20 %.
- README Known gaps list matches the ignored tests exactly.

### Common pitfalls
- Using `Local` time (your PC's zone) anywhere → tests break in CI (UTC). Always use the pack's timezone; tests set it explicitly (Europe/Bucharest fixture **and** a non-Romanian one, e.g. America/Toronto).
- Treating a day as always 86 400 s in *wall-clock* terms; it isn't on DST days — that's exactly why the noon-minus-12h rule exists.

### Decisions (Leader, 2026-10-18, from Tester's tests-first gaps)
- transfers.txt: type 0 uses the footpath time (min_transfer_time ignored); type 1 = 0 min (min_transfer_time ignored); type 2 uses min_transfer_time, and when it is missing it falls back to the footpath time with a warning; type 3 forbidden. Unknown types (including values the parser rejects, e.g. 9) and trip-to-trip / route-to-route rows are skipped with a warning; the feed still loads (tolerant parse).
- calendar_dates: type 1 and type 2 on the same day means the service does not run, with a warning. `start_date > end_date`: the service never runs, with a warning.
- frequencies: `headway_secs = 0` drops that row with a warning, not the whole feed. Caps: per-row and total generated trips (and total stop_times); Builder picks values that keep the crafted 30-row/1 s/48 h case a clean build error under 512 MiB and let MBTA and Brașov build untouched; values written into spec §7.2.
- Validity: `FeedExpiresSoon` is measured from the query's service date, triggered when ≤ 14 days remain. A query whose service date is outside `valid_from..=valid_to` returns `DateOutsideFeed` (no D‑1 rescue), documented.
- Wall times (CLI `--at` without an offset only): an ambiguous time takes the earlier instant; a non-existent time moves forward past the gap. `FixedOffset` input is never ambiguous.
- Interfaces: the plan's names win and the spec is updated to match (`FeedExpiresSoon { valid_to }`); `DateOutsideFeed` stays a unit variant; `StopNotLinkedToStreets` keeps its existing `String` payload. `chrono-tz` becomes a rustline-core dependency.
- block_id stays Known gap #7. RAPTOR vs CSA across midnight is not checked (the oracle is one-day); recorded as a Known gap.

**Time (est.):** 6–9 evenings.

---

## M6 — Flutter integration (on the phone)

**Goal:** RoTransit plans a journey offline on your Samsung A54 using Rustline, shown in a prototype planner screen.
**Prerequisites:** M5, FRB hello-world from M0.

> **Scope now (Savin, 2026-10-14 / 2026-10-20):** build and test the engine *ready for integration* only. Do steps 1–3, 7 and 8 inside the Rustline repo, using `rustline_flutter/example` as the test app (smoke integration test, 100-query measurement, error/no-panic tests, open-pack safety, concurrency/cancellation). **Do not** do steps 4–6 or touch RoTransit (no RoTransit branch, bundling, freshness banner, Riverpod screen, or in-app attribution). Acceptance items that need RoTransit are deferred. Items that need the physical A54 are measured on the emulator now, and the A54 numbers are left for Savin to run. M5 follow-ups to include: a D-1 case in the m4_plan brute force, a trip whose frequency rows are all invalid (error or documented), and an explicit memory assertion (< 384 MiB) in the at-caps test.

### M6 decisions (Leader, 2026-10-20, from Tester's tests-first gaps)
- Every public FFI fn returns `Result`, including `info()`.
- Error codes: `invalid_request`, `outside_coverage`, `no_route`, `date_outside_feed`, `pack_corrupt`, `pack_version`, plus `pack_missing`, `pack_io`, `pack_too_large`, `no_space`, and `internal` (panics). Errors cross as a typed Dart `RustlineException` with a `code` field (an FRB-mapped error type, not parsed from text; `fromMessage` is allowed only as a fallback).
- Cancellation: a latest-wins `RustlinePlanner` in the plugin; a superseded query resolves to null. Each Rust call stays bounded (≤ 1 s).
- Pack install lives in `rustline-ffi`: `install_pack(src, dest)` (free-space check, temp file + atomic rename, partial-file recovery), with a debug-only free-space override for tests.
- `FfiPlanRequest`: from/to lat/lon, `time_kind` (M8), `time_ms` (M8: was `depart_at_ms`), `time_offset_secs` (M8: was `depart_offset_secs`), `max_results`, optional `max_access_m`, `walk_speed_mps`, `max_transfers`.
- Tests are staged in `pending/rustline-ffi/tests/`; the crate folder and its Cargo.toml land together (workspace glob `crates/*`).
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
- `rustline-ffi` depends on `flutter_rust_bridge = "=2.12.0"`; Dart package and codegen also 2.12.0 (RoTransit is on Dart 3.6). *Since 2026-10-26 all three are 2.13.0 on Flutter 3.47.8 (decision 8 of M10).*
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
- [ ] Safety (§11): pack opened with the 256 MiB default cap (or lower, documented for the phone); decompression to disk checks free space first and writes to a temp file + atomic rename; no panic crosses FFI (tested with an injected panic in a debug-only hook → Dart exception, app alive).
- [ ] Budgets on the A54: pack open time, peak memory of the app with the engine loaded, and memory during 100 queries recorded; a single query is cancellable or bounded (≤ 1 s).
- [ ] All items in **Edge cases (must be tested)** below have a test or a Known gap.

**Edge cases to test**
- Engine opened twice / concurrently (provider rebuild); plan called while engine is loading.
- Missing pack file; pack on a full disk during decompression; device clock in a different timezone than the feed.

**Edge cases (must be tested)**
*Lifecycle and concurrency*
- `plan` called before the pack has loaded → typed error (or awaited), never a null deref.
- Cancellation: user leaves the screen or starts a new query → old result discarded; the Rust call ends within its time bound.
- Concurrent queries from two widgets / fast repeated taps → correct, no data race (`Engine` is `Send + Sync`, shared via `Arc`).
- Pack swapped (new pack downloaded/decompressed) while a query runs → the running query finishes on the old `Arc`, the next uses the new one; half-written pack never opened.
- App backgrounded and killed during decompression → next start detects the partial file and redoes it.

*Platform and data*
- Low memory on the phone: open with a lower cap and on an emulator with little RAM → clean error, no OOM kill during load.
- Panics across the FFI boundary → Dart exception with a stable code; app keeps running.
- String encoding: stop names with Romanian diacritics (ș, ț, ă), emoji, invalid UTF-8 in a feed → shown or replaced, no crash; empty and very long strings from Dart.
- Android ABIs: arm64-v8a on the A54, x86_64 on the emulator; armeabi-v7a explicitly unsupported (app shows an error or the ABI is excluded from the APK).
- Device timezone ≠ feed timezone, device clock wrong by a day; airplane mode; corrupt, truncated, v1 and zero-byte packs.

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

## M7 — Cleanup (known gaps + M6 Reviewer leftovers)

**Goal:** close every open Known gap from M1 and the small M6 Reviewer leftovers, so later milestones build on a clean base.
**Prerequisites:** M6.
**Spec:** [§7.2](Rustline-Spec.md#7-ingest-and-validation), [§11.3](Rustline-Spec.md#113-dart-via-frb-rustline-ffi), [§12](Rustline-Spec.md#12-performance-targets-and-budgets-all-est-to-be-validated-in-m5).

### Moved from M1 (Savin OK, 2026-10-18)
- Own lenient stop_times/trips reader that fixes README Known gaps #1 (time range check), #2 (huge hours, `checked_*` arithmetic), #3 (duplicate trip_id), #5 (short CSV rows / strict loading) and #6 (duplicate stop_sequence), each with its un-ignored test.

### From the M6 review (Savin, 2026-10-20)
- **R1 Stricter leftover matching in `install_pack`:** only delete files that match exactly `{name}.{pid}.{n}.tmp` (digits only, same `name`), never a prefix match; test that `brasov.rlp.backup.tmp`, `brasov.rlpx.1.2.tmp` and a symlink are left alone.
- **R2 Retry `flock` on `EINTR`:** loop on `EINTR` (bounded retries, e.g. 100), any other errno → `pack_io`. Test with an injected `EINTR` (debug-only hook).
- **R3 Panic hook text:** the quiet hook prints one fixed line (e.g. `rustline: internal error`), no payload, no file:line; test captures stderr.
- **R4 Doc line on `debug_inject_panic`:** Rust doc + generated Dart doc say it is test-only and returns `invalid_request` in release.
- **R5 Streaming install:** `install_pack` copies/decompresses in ≈ 1 MiB chunks; the whole pack is never held in memory. Peak RSS during install of the MBTA pack recorded before/after (target: install adds < 16 MiB).
- **R6 Trim `flutter_rust_bridge` features:** `default-features = false` plus only what is used; `.so` size before/after recorded; all FFI tests still pass.

### M7 decisions (Leader, 2026-10-20)
1. **Lenient loading wins.** A bad time drops that row with a warning; a trip left with fewer than 2 stop times is dropped. `edge_cases.rs::garbage_times_reject_the_feed` becomes `garbage_times_drop_the_row`. A missing required column or a missing required file is still a hard error.
2. **Issue kinds (Tester's names):** `InvalidTime`, `DuplicateTripId`, `DuplicateStopSequence`, `ShortRow`, `RowTooLong`, `InvalidUtf8`. All are warnings (the row or trip is dropped), not errors.
3. **Hour cap 48 h:** `47:59:59` loads; `48:00:00` and above is an `InvalidTime` row.
4. **Streaming install:** copy in chunks to the per-install temp file, then fsync; validate by opening the temp file from disk (no full in-memory read); only then rename. Peak added memory < 16 MiB.
5. **Leftovers:** don't delete the old `{name}.tmp`; clean up only exact `{name}.{pid}.{n}.tmp` names, and only under the lock.
6. **R2 hook:** `debug_flock_fail(errno, times)`, retry bound 100.
7. **R3 panic text:** exactly `rustline: internal error`.
8. **OSM baseline:** before changing code, build the OSM packs from commit 9f2718c with the same `--osm` bbox (separate git worktree in /tmp, removed afterwards), so the byte-identical check is fair.

Second round (Leader, 2026-10-20, after the first M7 report):
9. **48 h cap kept**, and the m5 test edits that follow from it (`47:10` → `47:50`, `trip_just_under_the_48_00_cap`).
10. **`TooFewStopTimes` is a warning:** the trip is dropped, the feed is kept (as in decision 1).
11. **Body check before the rename:** a pack with a valid CRC32 but a bad body must not replace a good installed pack. `install_pack` checks the body by streaming it from the temp file (`verify_pack_file`: the rkyv layout rules and the `check()` rules, read through a small page cache, no full body in memory); added memory < 16 MiB, measured. A failure is `pack_corrupt` and the old pack stays. Test: `rustline-ffi/tests/m7_body.rs`. Header-only counts in the result are fine.
12. **`UnknownTrip` warning:** a stop_times.txt row whose `trip_id` is not in trips.txt is dropped and reported.
13. **OK from the first report:** bad UTF-8 bytes become U+FFFD (row kept, `InvalidUtf8`); a stop_times row with an unknown stop drops its trip (`UnknownStop`).

### Steps
1. Own CSV reader for `stop_times.txt` and `trips.txt` (spec §7.2): streaming, header-mapped, short rows padded/reported, `checked_*` time parse (`HH` up to a documented cap, `MM`/`SS` < 60).
2. Duplicate `trip_id` → first wins, others reported; duplicate `stop_sequence` → trip reported and skipped (or documented order rule).
3. Un-ignore the Known-gap tests #1, #2, #3, #5, #6; remove them from README "Known gaps (M1)".
4. R1–R6 above in `rustline-ffi`.
5. Multi-feed check (§8): `sample-feed-1.zip` now passes `inspect` and `build-pack`; all pack outputs for the other feeds unchanged (sha256 compared) unless the change is explained.

### Done when
- [ ] Known gaps #1, #2, #3, #5, #6 fixed, tests un-ignored, README updated
- [ ] R1–R6 done, each with a test or a recorded measurement
- [ ] Google sample feed builds a pack

### Definition of done (strict)
- [ ] Release *and* debug: `9999999:00:00`, `24:60:00`, `-1:00:00`, empty time → reported row, never a wrap or panic.
- [ ] Packs for Brașov/Cluj/Timișoara/București/MBTA byte-identical to M6 output, or every difference explained in the report.
- [ ] Peak RSS of `build-pack` on MBTA not worse than M6 by more than 10 %.
- [ ] Safety (§11): the reader never sizes a buffer from a header/row count; row length capped (e.g. 64 KiB); stop_times cap from M5 still applies.
- [ ] All items in **Edge cases (must be tested)** below have a test.
- **Edge cases (must be tested):** BOM in header; header-only file; columns in any order; extra unknown columns; quoted fields with commas/newlines; CRLF; row with fewer fields than header; row longer than the cap; non-UTF-8 bytes; duplicate trip_id with different routes; duplicate stop_sequence; stop_sequence gaps (allowed); times only on first/last stop (interpolation unchanged); two `install_pack` writers + a stale leftover of another name; EINTR during lock; install from a file that grows/shrinks during copy; disk full mid-stream.
- **Tester will check:** all M1–M6 tests still pass in debug and release; multi-feed table; RSS numbers.

**Time (est.):** 5–8 evenings.

---

## M8 — Arrive-by searches

**Goal:** answer "I need to be there by 09:00": the journey that arrives no later than the given time and leaves as late as possible. Works for any GTFS feed.
**Prerequisites:** M7.
**Spec:** [§8](Rustline-Spec.md#8-algorithms-and-why), [§11](Rustline-Spec.md#11-public-apis).

### Steps
1. **API:** add `TimeSpec::ArriveBy(DateTime<FixedOffset>)` in `rustline-core`; `FfiPlanRequest` gets `time_kind` (`depart_at` | `arrive_by`) next to the existing millis+offset; CLI gets `--arrive-by` (mutually exclusive with `--at`). *Why:* one request type, the engine chooses the direction.
2. **Reverse RAPTOR:** scan rounds backwards: from the egress stops at `T − egress`, for each route take the *latest* trip that reaches the stop by the label time, walk footpaths backwards, end at access stops. Keep ride and walk labels separate (§11 last rule).
3. **Service days:** search D, D−1 and D+1 with the same DST-aware day shifts as M5 (82 800 / 86 400 / 90 000 s), `checked_*` everywhere.
4. **Depart-late pass** (spec §8.1 step 7): for a depart-at answer, run backwards from its arrival to remove waiting at the origin; for arrive-by, run forwards from the found departure to tighten arrival. Neither may make the result worse.
5. **Tie-breaks (documented, deterministic):** latest departure, then fewer rides, then less walking, then earliest arrival, then lowest route/trip index.
6. **Walking-only:** if walking alone reaches the destination by `T`, it is offered under the same "only when competitive" rule as depart-at (spec §9).
7. **Warnings/errors:** reuse `NoRoute`, `DateOutsideFeed` (decided on the query's local date), `FeedExpiresSoon`, `ApproximateFrequencyTrip`; transfer types 0–3 from M5 apply unchanged.
8. Dart: `RustlinePlanner` accepts arrive-by; example app has a toggle.

### How to check it works
```bash
cargo run --locked --release -p rustline-cli -- plan --pack fixtures/out/brasov.rlp \
  --from 45.6427,25.5887 --to 45.6579,25.6012 --arrive-by 2026-10-15T09:00:00+03:00
```
Every leg prints its `%:z` offset; the last leg arrives ≤ 09:00.

### Done when
- [ ] Arrive-by works in core, CLI, FFI and the example app
- [ ] Brute-force oracle agrees on random queries
- [ ] Golden arrive-by queries added and reviewed

### Definition of done (strict)
- [ ] Oracle: for 1 000 random queries per seed (3 seeds) on a fixture and on the Brașov feed, arrival ≤ requested time, and no journey with a later departure (within the same transfer limit) arrives by the requested time (brute force / reversed CSA).
- [ ] Symmetry property: for an arrive-by answer with departure `d`, a depart-at query at `d` arrives no later than the requested time.
- [ ] Depart-late pass never makes arrival later (depart-at) or departure earlier (arrive-by) — property test.
- [ ] PC p95 of arrive-by within 1.2× of depart-at p95 on Brașov and MBTA (criterion, fixed seeds); emulator p95 recorded; A54 left for Savin.
- [ ] Safety (§11): round count capped by `max_transfers`, backwards footpath scan bounded, reconstruction length bounded; hostile pack test for the reverse indexes; peak RSS recorded.
- [ ] All items in **Edge cases (must be tested)** below have a test.
- **Edge cases (must be tested):**
  - *DST:* arrive-by time inside the spring-forward gap (moves forward, same rule as `--at`); inside the fall-back overlap (earlier instant); a journey that crosses the DST change; D−1 day of 23 h and 25 h.
  - *After midnight:* arrive-by 00:30 served by a D−1 trip with `24:xx` times; arrive-by 23:50 where the only trip is on D; trips with `>48:00` times.
  - *No route:* destination unreachable; arrive-by before the first trip of the day; requested time on a date with no service; date outside the feed (local date, incl. one day before `valid_from`).
  - *Tie-breaks:* two trips with the same departure and arrival; same departure, different rides; equal everything → stable by index across runs and builds.
  - *Walking-only:* origin == destination; origin within walking distance (walk wins / transit wins); no stop within access distance.
  - Frequency-based trips (exact and approximate); transfer type 2 without `min_transfer_time`; type 3 forbidden transfer on the only path; `max_transfers = 0`; arrive-by in the past relative to the device clock (allowed, no special case); feed in a non-Romanian timezone (MBTA).
- **Tester will check:** oracle across seeds in debug and release; golden diffs one by one; CLI command verbatim.

### Common pitfalls
- Reusing forward "earliest trip" search backwards → you need "latest trip arriving by"; write it separately and test it alone.
- Mixing up departure/arrival service-day shifts around DST.

### Carried from M7 review
- [x] Test that the in-memory and on-disk grid checks (`build_grid` / `check_disk_grid`) agree: the globe case and one just under the cell cap.
- [x] Comment at `Grid::for_box` that the snapping budget depends on its highest-latitude choice.
- [x] `open_with_limit` with a smaller limit can fail on a pack installed under the 256 MiB default: document it, or pass the limit to `install_pack`.

### M8 decisions (Leader, 2026-10-20)
1. FFI: `time_kind: FfiTimeKind { DepartAt, ArriveBy }`, and `depart_at_ms` is renamed `time_ms` (the departure or the deadline). Dart, the example (arrive-by toggle) and the docs follow.
2. `open_with_limit` with a limit below the file size returns `pack_too_large`; the size is checked before the body check. Documented at `open_with_limit`: `install_pack` checks the 256 MiB default.
3. Walking-only and origin == destination behave exactly like depart-at, mirrored. Walk-only is offered under the same competitiveness rule, leaving at the latest time that still meets the deadline.
4. `max_transfers` 255 is handled as in depart-at (capped at 8).
5. Tester's measured perf test stays; the criterion bench (`benches/plan.rs`) gets the same 1 000 queries as arrive-by.
6. CLI: `--at` is optional and exclusive with `--arrive-by`; exactly one is required (clap: "cannot be used with").
7. Journeys that need more than a day of waiting are out of scope, matching the D − 1 / D / D + 1 window, so a journey must fit the D − 1 service-day window (no D − 2 trips; even a 23.8 h wait on a D − 2 trip is out), and the depart-late property is limited to that window.

**Time (est.):** 6–9 evenings.

---

## M9 — rRAPTOR: several good options

**Goal:** one search returns a small set of good choices — the Pareto set of (arrival time, number of transfers), and over a departure window ("leave between 08:00 and 08:30") — instead of a single answer.
**Prerequisites:** M8.
**Spec:** [§8](Rustline-Spec.md#8-algorithms-and-why), [§9](Rustline-Spec.md#9-multi-criteria).

### Steps
1. **Pareto per round:** RAPTOR already computes the best arrival after *k* rides in round *k*; collect the target label per round → (arrival, transfers) front. Keep only non-dominated journeys (strictly better in one, not worse in the other).
2. **Range queries (rRAPTOR):** for a window `[t0, t1]`, collect departure times at access stops in the window, run rounds from latest to earliest departure, *reusing* labels (labels only improve). Arrive-by gets the mirrored range.
3. **Result set:** dedupe identical journeys; sort by departure then arrival; cap at `max_results` (default 5, hard cap 20) with a documented rule for which ones survive (always keep fastest and fewest-transfers).
4. **API:** `PlanRequest::window_secs` (default 0 = current behaviour), FFI and CLI `--window`; each itinerary marked with tags (`fastest`, `fewest_transfers`, `leaves_latest`).
5. Golden tests updated via `cargo insta review`; every intentional change approved.

### Done when
- [ ] Results are Pareto on (arrival, transfers); range queries work in CLI, FFI and example app
- [ ] Performance budget met or regression reported

### Definition of done (strict)
- [ ] Dominance property test (proptest + 1 000 random queries × 3 seeds): no returned journey is dominated by another returned one, and every Pareto-optimal (arrival, transfers) pair found by a brute-force oracle (per-k CSA) is present (unless removed by the cap, then the cap rule is checked).
- [ ] Range property: the answer for window `[t0, t1]` contains, for every departure `t` in the window, a journey at least as good as the single query at `t`.
- [ ] Window 0 gives exactly the M8 results (golden byte-equal).
- [ ] **Perf budget:** PC p95 with window 0 ≤ 1.1× the current M6/M8 p95; with a 30 min window ≤ 3× (Brașov and MBTA, criterion fixed seeds). Emulator p95 recorded. Over budget → report, do not merge silently.
- [ ] Safety (§11): window capped (e.g. ≤ 2 h, documented), number of departures per range capped (e.g. 256), rounds capped by `max_transfers` (hard max 8), result cap 20; worst case (many ties on MBTA) timed and peak RSS recorded.
- [ ] All items in **Edge cases (must be tested)** below have a test.
- **Edge cases (must be tested):**
  - *Dominance:* equal arrival, different transfers (keep fewer); equal transfers, different arrival; exact ties on both (deterministic pick, same across debug/release); walk-only vs transit in the front.
  - *Transfer limits:* `max_transfers` 0, 1, the hard max, and over the max (→ `invalid_request`); front where the fastest journey needs more transfers than allowed.
  - *Cap on options:* more Pareto journeys than `max_results`; `max_results` 0 (→ `invalid_request`) and 1; cap rule keeps fastest + fewest transfers.
  - *Ranges:* window 0 s; window = cap; over cap (error); window crossing midnight (D/D+1 trips); window across a DST change; no departures in the window; identical journeys from neighbouring departures (dedup).
  - Arrive-by ranges mirrored; frequency trips inside the window (many departures, capped).
- **Tester will check:** M3–M8 property tests still pass; golden diffs; perf table PC + emulator.

### Common pitfalls
- Clearing labels between departures in a range → loses the speed-up and can break the "labels only improve" invariant.
- Comparing journeys with `==` on floats/walking → use integer seconds/metres.

### M9 decisions (Leader, 2026-10-23)
1. `max_transfers` > 8 → `invalid_request` (**breaking**: M8 capped it to 8).
2. `max_results` 0 or > 20 (`MAX_RESULTS`) → `invalid_request`. Over the cap the fastest and the fewest-transfers itineraries are always kept, then the others in order.
3. No departure in the window → the same answer as window 0.
4. Walking only counts as 0 transfers in the front.
5. CLI `--window` takes plain seconds or `30m` / `2h` (cap `MAX_WINDOW_SECS` = 7 200 s); tags are printed; `--max-results` added (door to door only).
6. Perf baseline: env vars measured in the same session; no criterion wiring in the test. **Re-baselined to M8** (Savin, 2026-10-23): window 0 depart-at p95 ≤ 1.2× M8 acf46b7, measured with m9_perf's own 400-query harness (seed 0x6d39, release) on 2026-10-23: defaults Brașov 0.951 ms, MBTA 16.571 ms (the criterion numbers, 0.866 / 13.18 ms, are not comparable with the in-test timing). Why: M8's depart-late pass already made depart-at about 2× slower than M7; that cost comes from M8, not M9, and is accepted. A cheaper depart-late is a later task (backlog below).
7. Worst case on MBTA: 2 s accepted.
8. Same-stop transfers follow the engine's rules (raptor.rs `self_transfer`): 0 s unless the stop has its own transfers.txt row (`X → X`; type 3 forbids the change). The oracle's 0 s assumption is Tester's to adjust.
- The 5-minute rule is gone from door-to-door plans: the full (arrival, transfers) front is returned, tagged `Fastest` / `FewestTransfers` / `LeavesLatest` (ties get the tag too). Range answers are the front on (departure, arrival, transfers), sorted by departure then arrival; at most `MAX_RANGE_DEPARTURES` (256) departures are tried.
- 30 min window perf (Savin, 2026-10-23): absolute p95 limits replace the "≤ 3× window 0" ratio: Brașov < 10 ms, MBTA < 80 ms (measured 43.7 ms, so about 1.8× headroom). Worst case on MBTA stays < 2 s.
- Review R39 fixes (2026-10-23): the arrive-by range steps from the latest arrival found (minus 1 s), at most `MAX_RANGE_DEPARTURES` searches, and stops once the deadline leaves the window; the depart-at range keeps the 256 departures nearest the requested time (so the range property holds only for those 256 departures); the instant's journeys stay in a range answer only if they leave by the window's end; `FewestTransfers` and the cap's "fewest" compare transfers (walk only = one ride = 0). The fronts still compare rides: comparing transfers there drops a later walk-only option behind an earlier 1-ride one (breaks the M4 walk fallback tests), so that is Leader's call. FFI tags are an `FfiTag` enum; the example app has a window selector and shows tags. m9_perf (release, 2026-10-23 14:41): window 0 Brașov 1.044 ms (×1.10 M8), MBTA 17.283 ms (×1.04); 30 min depart-at Brașov 5.14 / MBTA 44.05 ms; 30 min arrive-by Brașov 2.84 / MBTA 44.27 ms; MBTA worst case depart-at 227.6 ms, arrive-by 58.1 ms, VmHWM 974 MB.
- Final M9 rulings (Leader, 2026-10-23): the fronts stay on rides, so a walk-only option is never dropped by an earlier 1-ride trip; the `FewestTransfers` tag uses transfers (decision 4); the late end of a capped frequency window is checked with a second query (accepted). Over `max_results`, both modes always keep the fastest, the fewest-transfers and the leaves-latest itineraries; the rest are filled in order for depart-at and latest departure first for arrive-by; output order is unchanged. `Fastest` means the earliest arrival in both modes (in arrive-by it is not the shortest trip).
- M9 recheck fix (2026-10-23): range answers are the Pareto set on (later departure, earlier arrival, fewer rides) in both directions. Root cause of the missing Brașov 08:28:31 → 08:55:17 option: `departures()` in raptor.rs seeded the access walks without `best_walk`, so a longer hop overwrote a shorter access walk and the door departure came out too early (outside the window). Fixed there. Arrive-by always-kept picks are latest, fewest, fastest (max_results 1 = the M8 answer); depart-at stays fastest, fewest, latest.
- **Backlog (after M9):** pin the Brașov fixture used by the m9_review Brașov tests (sha in the manifest), or replace them with synthetic feeds; the random-feed property tests have no walking between stops, so they can't catch access/hop bugs like the one above.
- **Backlog (after M9):** consider a shortest-duration tag/criterion for arrive-by (today `Fastest` = earliest arrival).
- **Backlog (after M9):** `carry(k)` is O(stops × rounds) per range departure (carry only changed stops, or lazily); `departures()` builds a whole `Search` for the walk from the door (a small walk pass); arrive-by range mirrored in back.rs with label reuse (like `raptor_range`) instead of repeated `arrive` searches.
- **Backlog (after M9):** a cheaper depart-late pass (today it is the M8 extra search in `plan.rs` `depart_at`), to win back the ~2× depart-at cost against M7.
- Golden re-bless (Leader-approved): `golden/m9_window0.txt` gained 1 line, nothing changed or removed: seed 35880 q32 E→J, `17:07:19 → 17:59:07, 2 rides [l0t66,l2t27]` (2 min 30 s earlier than the 1-ride journey, so not dominated; the 5-minute rule used to hide it).

**Time (est.):** 6–10 evenings.

---

## M10 — GitHub CI

**Goal:** every push and PR runs the full test suite and builds automatically on GitHub, so regressions are caught without anyone running commands by hand.
**Prerequisites:** M7 (can run in parallel with M8/M9).

### Steps
1. Extend `.github/workflows/ci.yml` (Ubuntu): `cargo fmt --all --check`; `cargo clippy --locked --workspace --all-targets -- -D warnings`; `cargo test --locked --workspace` in **debug and release**; MSRV job with Rust 1.88 (was 1.85 until 2026-10-26).
2. Flutter job: pinned Flutter version, `flutter analyze` and `flutter test` in `rustline_flutter` and `rustline_flutter/example`; check generated FRB code is up to date (`flutter_rust_bridge_codegen generate` then `git diff --exit-code`).
3. Android job (`android.yml`): `cargo ndk` for arm64-v8a + x86_64 with NDK 26.1.10909125, `flutter build apk --release` of the example; upload `.so` sizes as a job summary.
4. Optional emulator job: integration test on an x86_64 emulator (reactivecircus/android-emulator-runner), allowed to be a separate workflow if slow.
5. Caching (`Swatinem/rust-cache`, Gradle, pub), `concurrency` to cancel superseded runs, `permissions: contents: read`, actions pinned to a commit sha, Dependabot for actions.
6. Real-feed tests: CI has no feeds (gitignored); either they stay `#[ignore]` in CI, or a job runs `scripts/fetch-fixtures.sh` with the manifest sha256 check and caches the result. Decide and document.
7. **Open Dependabot PRs (#2–#7)** (opened 2026-10-20 after the first push; #1, the rust-toolchain 1.85 → 1.120 bump, was closed because 1.85 is the MSRV pin, and `dependabot.yml` now groups updates and ignores `dtolnay/rust-toolchain`). Goal: make CI pass, then merge the ones that pass. In each, only the "fmt, clippy, test" job fails; "cargo ndk arm64-v8a" passes.
   - #2 actions/checkout 4 → 7: `rustline-osm` test `osm_amplification::dense_nodes_with_10_million_entries_per_list` fails with heap peak 79 MiB over `HEAP_LIMIT`. The PR changes no Rust code and `main` CI passed at 7e9b40c, so this test is flaky or depends on the runner; fix the test or limit first.
   - #3 cargo minor/patch group (3 updates): the same `osm_amplification` heap-peak failure (79 MiB).
   - #4 zstd 0.13.3 → 0.14.3: the same `osm_amplification` heap-peak failure (79 MiB).
   - #5 zip 7.2.0 → 7.3.0: the same `osm_amplification` heap-peak failure (79 MiB).
   - #6 sha2 0.10.9 → 0.11.0: compile error in `rustline-cli`, E0277 `sha2::Sha256: std::io::Write` not satisfied (0.11 API change). Needs a code change, e.g. hash through `Digest::update` instead of `io::copy`.
   - #7 criterion 0.5.1 → 0.7.0: the same `osm_amplification` heap-peak failure (79 MiB); the benches compile, but check for API changes before merging.
   - After the fixes, the grouped weekly PRs should replace these, so close any that are superseded.
   - **Status 2026-10-23:** #3–#7 closed (superseded by the grouped #8, still open: sha2 0.11, zstd 0.14, criterion 0.7, flutter_rust_bridge 2.13.0, libc, gtfs-structures 0.48.2, zip 8.6); #2 replaced by the checkout v7.0.1 sha pin (close it). flutter_rust_bridge is now ignored by Dependabot (decision 7), so #8 should be recreated without it.

### Done when
- [ ] CI green on a PR for all jobs; a deliberately failing test/fmt/clippy turns it red
- [ ] README has a CI badge and says which tests run where

### Definition of done (strict)
- [ ] All commands `--locked`; same commands as the Builder checklist, copied verbatim.
- [ ] Total wall time of the required jobs recorded (target < 20 min with warm cache).
- [ ] No secrets needed for PR jobs; workflows from forks cannot write.
- [ ] Safety (§11): job `timeout-minutes` on every job; fixture downloads verified by sha256 before use.
- **Edge cases (must be tested):** cold cache run; Cargo.lock out of date → fails; generated FRB code stale → fails; MSRV dependency bump → MSRV job fails; flaky test → rerun policy documented (no auto-retry hiding failures).
- **Tester will check:** one green and one intentionally red run linked in the report.

**Time (est.):** 2–4 evenings. (Pushing workflows needs Savin's "push".)

### Decisions (Builder, 2026-10-23)
1. **Workflows.** `ci.yml` (required): `rust` (fmt, clippy, test debug + release on stable), `msrv` (`cargo +1.88 test --locked --workspace`, `RUSTUP_TOOLCHAIN=1.88` because `rust-toolchain.toml` says stable), `flutter` (Flutter 3.47.8: `pub get --enforce-lockfile`, `flutter analyze` in the plugin and the example, `flutter test` in the plugin, then `flutter_rust_bridge_codegen` 2.13.0 `generate` + `git diff --exit-code`). `android.yml` (required): `cargo ndk` arm64-v8a + x86_64 on 1.88 with NDK 26.1.10909125 and `.so` sizes in the job summary, and `flutter build apk --release` of the example with APK/`.so` sizes. `android-emulator.yml` (manual, step 4) and `real-feeds.yml` (manual + weekly, step 6) are not required checks. `bench.yml` stays manual.
2. **Hardening (step 5).** Every action pinned to a commit sha with the tag as a comment (`actions/checkout` v7.0.1 = `3d3c42e5aac5ba805825da76410c181273ba90b1`, from `git ls-remote`; this replaces Dependabot #2) (Dependabot updates both; `dtolnay/rust-toolchain` stays ignored and gets the toolchain via `with: toolchain`). `permissions: contents: read`, `concurrency` cancel-in-progress and `timeout-minutes` on every job. Only `pull_request` triggers (no `pull_request_target`), so PR jobs have no secrets and fork runs cannot write. Caches: `Swatinem/rust-cache` (one key per job), Gradle via `setup-java`, pub via `flutter-action`.
3. **Real feeds (step 6).** Required CI has no feeds: real-feed tests skip (missing file) or stay `#[ignore]`, as today. `real-feeds.yml` runs `scripts/fetch-fixtures.sh --skip-manual` (new flag: rows without a public URL, e.g. `brasov-manifest`, are SKIP instead of a failure), which verifies every file's manifest sha256 before the tests, caches `fixtures/feeds` by the manifest hash, then runs `cargo test --locked --workspace --release`. A changed upstream feed fails there (CHANGED), never on a PR. The m9_review Brașov tests now check the file against the manifest sha256 (`pinned_fixture`), closing the M9 backlog item; `sha2` 0.10 added as a rustline-core dev-dependency (Cargo.lock: one dependency line, no new crate).
4. **Reruns.** No automatic retries. A failure that passes on a manual rerun is flaky: record it (run link) and fix the test or the limit; never add a retry loop.
5. **Dependabot (step 7), state on 2026-10-23 (Leader ruling).** #3–#7 are closed; #8 (grouped cargo, 7 updates) and #2 (checkout 4 → 7) are open. `rustline-cli` `file_sha256` now hashes through `Digest::update`, which works with sha2 0.10 and 0.11 (the #8 compile error); not built against 0.11 yet. `osm_amplification::dense_nodes_with_10_million_entries_per_list` also failed on `main` (run 39405717253, rustc 1.99.1, heap peak 79 MiB vs 64 MiB); 70 local runs (rustc 1.99.0) all measured 48 MiB, so it is runner-dependent. Fix (Leader ruling): the test allocator counts per thread (thread-local `Cell<isize>`), so only the parsing thread's allocations count; limit stays 64 MiB; 5× debug + 5× release locally all 48 MiB. Likely cause: a process-wide count also saw other threads (unconfirmed, it never reproduced locally).
6. **Local self-check only:** actionlint 1.7.12 and yamllint 1.38 (`.yamllint.yml`) pass; `act` not used (needs docker). The Done-when runs (green + intentionally red PR, wall time, cold cache) need a push.
7. **flutter_rust_bridge upgrades are manual.** The crate, `flutter_rust_bridge_codegen` and the Dart package must move together with regenerated bindings, so `dependabot.yml` ignores the `flutter_rust_bridge` cargo crate. Pub is not a Dependabot ecosystem here, so the Dart package is never bumped by it. Upgrade all three by hand, regenerate, and let the `flutter` CI job's diff check prove it.

8. **Dependency update 2026-10-26 (Dependabot #12, Savin).** sha2 0.11, zstd 0.14, criterion 0.7, libc 0.2.191, gtfs-structures 0.48.2 (then 0.52.0, below), zip 8.6, crc32fast 1.5.3. MSRV 1.85 → **1.88** (zip 8.6.0 `rust-version = "1.88"`; per `cargo metadata` the next highest declared `rust-version` is 1.85, and 43 locked crates, gtfs-structures among them, declare none, so the `msrv` job's `cargo +1.88 test --locked --workspace` is what proves 1.88), in the workspace, the `msrv` job and the Android `ffi` jobs. cargo-ndk 4.1.2 (needs 1.86) now builds on the MSRV, so the stable workaround is gone. Dependabot no longer ignores gtfs-structures; it gets its own PR (excluded from the `cargo` group) because 0.x bumps change its API. The Dart flutter_rust_bridge 2.13.0 needs Dart ≥ 3.9.2, so Savin chose to upgrade Flutter: CI and local Flutter **3.47.8** (Dart 3.13.7), flutter_rust_bridge **2.13.0** in the crate, the Dart package and the codegen (bindings regenerated, decision 7). RoTransit (Flutter 3.27 / Dart 3.6) cannot use this plugin version until it upgrades Flutter too. gtfs-structures **=0.52.0**: since 0.49 it keeps extended route types (`build.rs` applies the old folding table, so packs are unchanged) and since 0.50 its dates are jiff (converted to chrono in `build.rs`). Since 0.52 one bad levels.txt row fails the whole feed, so `load.rs` drops levels like transfers. Known gap: gtfs-structures reads levels.txt (0.52), attributions.txt (0.50) and licenses.txt (0.51) with no size cap before Rustline drops or ignores them, as it does every file it parses; only trips.txt, stop_times.txt (own reader, row cap) and transfers.txt (256 MiB) have Rustline caps.

### Progress
- 2026-10-23 (Builder): workflows and script written, lint-clean; not pushed.
- 2026-10-23 (Builder, Leader rulings): heap test counts per thread; checkout pinned to v7.0.1 everywhere; Dependabot ignores flutter_rust_bridge; `file_sha256` confirmed building on sha2 0.10.9. Open: CI badge + "which tests run where" in README (Leader), first pushed green/red runs.
- **Backlog:** android-emulator.yml pushes no pack. A google-sample pack is not enough: the integration tests use fixed Brașov coordinates (`m6_adapter.dart` city box, `rustline_lifecycle_test.dart`), so pack tests would need a Brașov pack (no public pinned URL for brasov-manifest; brasov-latest is fetchable with its sha) or coordinates read from the pack.

---

## M11 — iOS build

**Goal:** the `rustline_flutter` plugin builds and runs on iOS (device + simulator), tested on GitHub macOS runners because the dev PC is Linux.
**Prerequisites:** M10.

### Steps
1. Rust targets `aarch64-apple-ios`, `aarch64-apple-ios-sim`, `x86_64-apple-ios`; build `rustline-ffi` as a static lib per target and bundle into an **xcframework** (`xcodebuild -create-xcframework`, sim slices merged with `lipo`).
2. Cargokit iOS: enable the iOS side of the plugin (podspec / Swift Package), set the deployment target (document it, e.g. iOS 13), make sure the Cargokit patch for the library name also covers iOS (`librustline_ffi.a`).
3. Pack install on iOS: Application Support / Documents path, `flock` + `statvfs` work on iOS (or a documented iOS-specific fallback), excluded from iCloud backup.
4. `ios.yml` workflow on `macos-latest` (pinned Xcode): build xcframework, `flutter build ios --no-codesign` for the example, run the integration test on an iOS simulator, run `cargo test --locked -p rustline-ffi` on macOS too.
5. Record static lib size and app size delta; no code signing / App Store in this milestone.

### Done when
- [ ] `ios.yml` green: xcframework built, example app built, integration test passed on the simulator
- [ ] Sizes recorded; README explains how to build on a Mac

### Definition of done (strict)
- [ ] Same FFI tests as Android pass on the simulator (error codes, injected panic → exception, install_pack two-writer test, cancellation).
- [ ] No iOS-only code path in `rustline-core`; only `rustline-ffi`/plugin may differ, and engine stays generic.
- [ ] Safety (§11): same caps as Android (256 MiB default); low-disk path tested via the debug free-space override.
- **Edge cases (must be tested):** simulator on Apple Silicon (arm64-sim) vs Intel (x86_64); app relaunch after kill during install; device timezone ≠ feed timezone; file path with spaces/Unicode; app sandbox path changes between launches (store relative paths).
- **Tester will check:** workflow logs; a physical-iPhone run is optional and left for Savin.

**Time (est.):** 4–6 evenings.

---

## M12 — Server (optional, not now)

**Goal:** the same engine answers `POST /v1/plan` over HTTP, with identical results to the library.
**Prerequisites:** M9 (so the server has arrive-by and options). Not scheduled; only if Savin asks.
**Spec:** [§11.4](Rustline-Spec.md#114-http-json-rustline-server-optional).

### Steps
1. **Crate** `crates/rustline-server` with `axum`, `tokio`, `serde_json`, `tower-http` (CORS, tracing). *Why:* axum is the standard, lightweight Rust web framework.
2. **Routes:** `GET /v1/info`, `POST /v1/plan`. Engine in `Arc<Engine>` shared state (it's `Send + Sync`).
3. **Error mapping:** `InvalidRequest`→400, `NoRoute`→404, `OutsideCoverage`/`DateOutsideFeed`→422, JSON body `{"error": "..."}`.
4. **JSON schema:** derive `schemars::JsonSchema` on request/response types; write `schema/plan.v1.json` from a test and fail if it changes unexpectedly.
5. **Docker:** multi-stage `Dockerfile` (`rust:…` build → `debian:stable-slim` run), pack mounted as a volume.

### How to check it works
```bash
cargo run --locked --release -p rustline-server -- --pack out/rustline-brasov-627559bac2a3-v3.rlp --port 8088
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
- [ ] Deps (axum, tokio, schemars) satisfy MSRV 1.88 or the MSRV bump is agreed with Savin first.
- [ ] Safety (§11): request body cap (e.g. 64 KiB), per-request time limit (plan in `spawn_blocking` with a timeout), concurrency limit; peak RSS under 100 concurrent requests on the MBTA pack recorded.
- [ ] All items in **Edge cases (must be tested)** below have a test.
- **Edge cases (must be tested):** empty body; body over the cap; wrong `content-type`; NaN/huge numbers in JSON; unknown fields; date string without offset; slow client (slowloris) → timeout; 100 concurrent requests; client disconnect mid-plan; pack file missing at startup → clear exit, not a panic; server and library answers byte-equal for the golden queries.
- **Tester will check:** curl command verbatim, bad-input fuzz (empty, huge, wrong types), status codes, Docker build.

### Common pitfalls
- Blocking the async runtime with a long plan → wrap in `tokio::task::spawn_blocking`.
- Exposing it publicly without limits — if you put it behind your Cloudflare tunnel, add rate limiting.

**Time (est.):** 3–5 evenings.

---

## M13 — RoTransit integration (not now)

**Goal:** RoTransit plans journeys offline with Rustline (the parts deferred from M6: steps 4–6).
**Prerequisites:** M11. **Not to be done now** (Savin, 2026-10-20); kept last on purpose.

### Steps
1. RoTransit branch with `rustline_flutter` as a path/git dependency; bundled pack decompressed via `install_pack`.
2. Pack freshness banner (`info().feedSha256` vs `/api/feeds`).
3. Riverpod journey feature: planner screen, depart-at / arrive-by toggle, option list from M9, polyline on the map, links to stop boards.
4. OSM/feed attribution in the app; A54 measurements (p50/p95, load time, memory).

### Definition of done (strict)
- [ ] All M6 acceptance items that needed RoTransit (APK delta, freshness banner, attribution, airplane-mode run on the A54, RoTransit `main` untouched).
- [ ] Safety (§11) and the M6 **Edge cases (must be tested)** list apply unchanged, plus arrive-by and multi-option UI states (no route, one option, cap reached).

---

## M14 — Future (separate spec)

**Edge cases to carry into the M14 spec:** downloaded packs are untrusted (size cap, checksum, version, signature or sha from `/api/feeds` checked before use); interrupted/resumed download; pack newer than the app supports; realtime updates for trips not in the pack, delays > 24 h, cancelled stops; stale realtime data; all Builder checklist §11 items apply.

Not planned in detail; write a new spec when you get here ([spec §14](Rustline-Spec.md#14-realtime-future), [§17](Rustline-Spec.md#17-milestones)):
- **Backend-served packs:** backend_v2 builds a pack when FeedService detects a new sha and serves `/api/feeds/{id}/routing-pack` with the same ETag logic; the app downloads it instead of waiting for an app update.
- **Realtime:** GTFS‑RT delay overlay (only if RATBV publishes it).
- **Multi-city:** the other 12 feeds — re-check pack sizes and phone memory.
- **M9 backlog:** carry only changed stops in `raptor_range`; a light walk pass in `departures()`; mirrored back.rs arrive-by range.
- **Cheaper depart-late** (from M9): cut the M8 depart-late pass cost (depart-at ~2× M7 p95).
- **Quality+ leftovers (from old M8):** McRAPTOR with walking as a 3rd criterion (capped bags, Pareto on arrival/rides/walk, phone p95 < 150 ms), walking weight, thesis material (criterion charts, RAPTOR vs CSA, pack size table, phone vs PC timings, `cargo doc`).
- Live delays (GTFS-RT), wheelchair trips, delta pack updates, fares, pack-reader fuzzing.
- **Public release:** make Rustline usable by other developers: a license, a versioned API with a changelog, docs for outside developers, publishing to crates.io and pub.dev, and possible Kotlin/Swift bindings. `rustline_flutter` stays in this repo, and the core stays framework-agnostic.

---

## Final check — real-feeds workflow

Added 2026-10-26. The weekly `real-feeds.yml` workflow (Mondays 04:00 UTC) has never run: GitHub skipped its first scheduled run on 2026-10-26, so the tests against real GTFS feeds have not yet passed in CI.

Before calling the project done:

- [ ] Run `real-feeds.yml` (scheduled or by hand with `gh workflow run real-feeds.yml`) and confirm it passes on main.
- [ ] Confirm the Monday schedule is firing on its own.

## Appendix A — Crates you'll add, and when

| Crate | Milestone | Used for |
|---|---|---|
| `serde`, `thiserror`, `anyhow` | M1 | data types, errors |
| `gtfs-structures`, `chrono` | M1 | GTFS parsing, dates |
| `clap` | M1 | CLI |
| `rkyv` or `bytemuck`, `zstd`, `sha2` | M2 | pack format |
| `fixedbitset`, `insta`, `rand` | M3 | RAPTOR, golden tests, random queries |
| `flate2` (zlib-rs) — *not* `osmpbf`/`rstar`, see M4 steps 2 and 4 | M4 | OSM blobs; snapping uses an own grid |
| `chrono-tz`, `proptest`, `criterion` | M5 | timezone, property tests, benches |
| `flutter_rust_bridge` | M6 | FFI |
| `axum`, `tokio`, `schemars`, `tower-http` | M12 | server |

## Appendix B — Daily loop

```bash
git switch -c horia/mN-something
# write a test → make it pass → refactor
cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add -p && git commit
git push -u origin HEAD     # open PR, CI must be green, merge
```
