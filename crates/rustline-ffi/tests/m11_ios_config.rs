//! M11 tests-first (Tester, 2026-10-26): static checks of the iOS build config
//! (plan M11 steps 1-5, Research note "2026-10-26 M11 iOS"), runnable on Linux.
//! The dev PC has no Mac, so these only read files: `.github/workflows/ios.yml`,
//! the plugin podspec, the example's Podfile and Xcode project, and the README.
//! What needs iOS itself is in `rustline_flutter/example/integration_test/m11_ios_test.dart`,
//! which ios.yml runs on the simulator.
//!
//! Expected shape (Leader, 2026-10-26): `runs-on: macos-26`, Xcode 26.6 selected,
//! an "iPhone 17" simulator, Rust targets `aarch64-apple-ios` + `aarch64-apple-ios-sim`,
//! cargokit through CocoaPods (Flutter's SwiftPM fallback), iOS 15.0 everywhere, the
//! pack in Application Support excluded from backup, sizes recorded.
//!
//! Builder: remove the `#[ignore = "M11 not implemented"]` lines when M11 lands. The two
//! tests without an ignore are guards that pass today and must keep passing.
//! Run: `cargo test --locked -p rustline-ffi --test m11_ios_config -- --include-ignored`

use std::path::{Path, PathBuf};

const IOS_YML: &str = ".github/workflows/ios.yml";
const PODSPEC: &str = "rustline_flutter/ios/rustline_flutter.podspec";
const PODFILE: &str = "rustline_flutter/example/ios/Podfile";
const PBXPROJ: &str = "rustline_flutter/example/ios/Runner.xcodeproj/project.pbxproj";

/// The repo root. `RUSTLINE_M11_ROOT` points the checks at another tree (Tester's
/// self-test: a known-good sample config and mutants of it).
fn root() -> PathBuf {
    match std::env::var_os("RUSTLINE_M11_ROOT") {
        Some(r) if !r.is_empty() => PathBuf::from(r),
        _ => Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
    }
}

fn read(rel: &str) -> String {
    let p = root().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("M11: cannot read {rel}: {e}"))
}

/// Drops `#` comments (YAML / Ruby / shell), outside quotes, so a comment that names
/// `macos-26` or `15.0` never satisfies a check.
fn strip_comments(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let (mut sq, mut dq) = (false, false);
        let mut prev_ws = true;
        let mut cut = line.len();
        for (i, c) in line.char_indices() {
            match c {
                '\'' if !dq => sq = !sq,
                '"' if !sq => dq = !dq,
                '#' if !sq && !dq && prev_ws => {
                    cut = i;
                    break;
                }
                _ => {}
            }
            prev_ws = c.is_whitespace();
        }
        out.push_str(line[..cut].trim_end());
        out.push('\n');
    }
    out
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// `(job id, job body)` for each job under the top-level `jobs:` key (comments stripped).
fn jobs(yaml: &str) -> Vec<(String, String)> {
    let text = strip_comments(yaml);
    let lines: Vec<&str> = text.lines().collect();
    let start = lines
        .iter()
        .position(|l| *l == "jobs:")
        .expect("ios.yml has a top-level `jobs:`");
    let mut out: Vec<(String, String)> = Vec::new();
    let mut job_indent = None;
    for l in &lines[start + 1..] {
        if l.trim().is_empty() {
            continue;
        }
        let ind = indent(l);
        if ind == 0 {
            break;
        }
        let ji = *job_indent.get_or_insert(ind);
        if ind == ji && l.trim_end().ends_with(':') {
            out.push((l.trim().trim_end_matches(':').to_string(), String::new()));
        } else if let Some(j) = out.last_mut() {
            j.1.push_str(l);
            j.1.push('\n');
        }
    }
    assert!(!out.is_empty(), "ios.yml has no jobs");
    out
}

/// The step items (`- name:` / `- uses:` / `- run:` …) of a job body, in order.
fn steps(job: &str) -> Vec<String> {
    let lines: Vec<&str> = job.lines().collect();
    let Some(s) = lines.iter().position(|l| l.trim() == "steps:") else {
        return Vec::new();
    };
    let items: Vec<usize> = (s + 1..lines.len())
        .filter(|&i| lines[i].trim_start().starts_with("- "))
        .collect();
    let Some(step_ind) = items.iter().map(|&i| indent(lines[i])).min() else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for l in &lines[s + 1..] {
        if l.trim().is_empty() {
            continue;
        }
        if indent(l) < step_ind {
            break;
        }
        if indent(l) == step_ind && l.trim_start().starts_with("- ") {
            out.push(String::new());
        }
        if let Some(st) = out.last_mut() {
            st.push_str(l);
            st.push('\n');
        }
    }
    out
}

/// Whitespace runs folded to one space and shell line continuations removed, so
/// `cargo  test \\\n --locked` matches `cargo test --locked`.
fn flat(s: &str) -> String {
    s.replace("\\\n", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Quotes removed too: `xcode-version: '26.6'` == `xcode-version: 26.6`.
fn bare(s: &str) -> String {
    flat(s).replace(['\'', '"'], "")
}

fn step_idx(job: &str, pred: impl Fn(&str) -> bool) -> Option<usize> {
    steps(job).iter().position(|s| pred(&bare(s)))
}

/// A step that runs the example's integration tests: through XCTest (`flutter build ios
/// --config-only` + `xcodebuild test`, the M11 shape; a warm-up `build-for-testing` step
/// does not match), or the earlier `flutter test integration_test` / `flutter drive`.
fn runs_integration_tests(step: &str) -> bool {
    (step.contains("xcodebuild test ") && step.contains("--config-only"))
        || step.contains("flutter test integration_test")
        || step.contains("flutter drive")
}

/// The job that runs the example's integration tests on the simulator.
fn sim_job(yaml: &str) -> (String, String) {
    jobs(yaml)
        .into_iter()
        .find(|(_, b)| steps(b).iter().any(|s| runs_integration_tests(&bare(s))))
        .expect("ios.yml: no job runs `flutter test integration_test` (plan M11 step 4)")
}

/// `name` as a whole word: "iPhone 17" but not "iPhone 17 Pro", "iPhone 17e".
fn mentions_exact(hay: &str, name: &str) -> bool {
    hay.match_indices(name).any(|(i, _)| {
        let rest = &hay[i + name.len()..];
        let next = rest.chars().next();
        let word_follows = rest.starts_with(" Pro")
            || rest.starts_with(" Plus")
            || rest.starts_with(" Air")
            || rest.starts_with(" mini");
        !word_follows && !next.is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '_')
    })
}

fn selects_xcode_26_6(s: &str) -> bool {
    s.contains("xcode-select -s /Applications/Xcode_26.6.app")
        || s.contains("xcode-select --switch /Applications/Xcode_26.6.app")
        || s.contains("DEVELOPER_DIR: /Applications/Xcode_26.6.app")
        || (s.contains("setup-xcode") && s.contains("xcode-version: 26.6 "))
        || (s.contains("setup-xcode") && s.ends_with("xcode-version: 26.6"))
}

fn builds_or_tests(s: &str) -> bool {
    s.contains("flutter build")
        || s.contains("flutter test")
        || s.contains("flutter drive")
        || s.contains("pod install")
        || s.contains("xcodebuild")
        || s.contains("cargo build")
        || s.contains("cargo test")
}

// ---- ios.yml -------------------------------------------------------------------

#[test]
fn m11_ios_yml_runs_on_pinned_macos_26_only() {
    let y = read(IOS_YML);
    let text = strip_comments(&y);
    assert!(
        !text.contains("macos-latest"),
        "pin the label: macos-latest migrates (Research §2)"
    );
    let (id, body) = sim_job(&y);
    assert!(
        bare(&body).contains("runs-on: macos-26"),
        "job {id}: runs-on must be macos-26"
    );
    for (id, body) in jobs(&y) {
        let b = bare(&body);
        if b.contains("runs-on: macos") {
            assert!(
                b.contains("runs-on: macos-26"),
                "job {id}: only macos-26 (Xcode 26.6 image) is expected"
            );
        }
    }
}

#[test]
fn m11_ios_yml_selects_xcode_26_6_before_any_build() {
    let y = read(IOS_YML);
    for (id, body) in jobs(&y) {
        let ss: Vec<String> = steps(&body).iter().map(|s| bare(s)).collect();
        let Some(first_build) = ss.iter().position(|s| builds_or_tests(s)) else {
            continue;
        };
        if !bare(&body).contains("runs-on: macos") {
            continue;
        }
        let job_env = bare(body.split("steps:").next().unwrap_or_default());
        let sel = ss.iter().position(|s| selects_xcode_26_6(s));
        assert!(
            selects_xcode_26_6(&job_env) || sel.is_some_and(|i| i < first_build),
            "job {id}: select Xcode 26.6 (xcode-select -s /Applications/Xcode_26.6.app) \
             before the first build/test step"
        );
    }
}

#[test]
fn m11_ios_yml_boots_an_iphone_17_simulator_and_tests_on_it() {
    let y = read(IOS_YML);
    let (id, body) = sim_job(&y);
    let boot = step_idx(&body, |s| {
        mentions_exact(s, "iPhone 17")
            && (s.contains("simulator-action")
                || s.contains("simctl boot")
                || s.contains("simctl create"))
    })
    .unwrap_or_else(|| {
        panic!("job {id}: no step boots an \"iPhone 17\" simulator (exactly that model)")
    });
    let test = step_idx(&body, runs_integration_tests).unwrap();
    assert!(
        boot < test,
        "job {id}: boot the simulator before the integration test"
    );
    let t = bare(&steps(&body)[test]);
    assert!(
        t.contains(" -d ")
            || t.contains(" --device-id")
            || t.contains("-destination platform=iOS Simulator,id="),
        "job {id}: pass the simulator (flutter -d, or xcodebuild -destination 'platform=iOS Simulator,id=…'), else any device is picked"
    );
    assert!(
        bare(&body).contains("timeout-minutes:"),
        "job {id}: needs timeout-minutes (integration tests can hang, Research §2)"
    );
}

#[test]
fn m11_ios_yml_installs_both_apple_rust_targets() {
    let y = read(IOS_YML);
    let (id, body) = sim_job(&y);
    let b = bare(&body);
    let device = b
        .match_indices("aarch64-apple-ios")
        .any(|(i, _)| !b[i + "aarch64-apple-ios".len()..].starts_with("-sim"));
    assert!(device, "job {id}: install aarch64-apple-ios (device slice, used by flutter build ios --no-codesign)");
    assert!(
        b.contains("aarch64-apple-ios-sim"),
        "job {id}: install aarch64-apple-ios-sim"
    );
    assert!(
        b.contains("targets:") || b.contains("rustup target add"),
        "job {id}: targets must be installed (dtolnay `targets:` or `rustup target add`)"
    );
    if let Some(i) = b.find("IPHONEOS_DEPLOYMENT_TARGET:") {
        assert!(
            b[i..].starts_with("IPHONEOS_DEPLOYMENT_TARGET: 15.0"),
            "job {id}: IPHONEOS_DEPLOYMENT_TARGET must be 15.0"
        );
    }
    let all = bare(&strip_comments(&y));
    if let Some(i) = all.find("IPHONEOS_DEPLOYMENT_TARGET:") {
        assert!(all[i..].starts_with("IPHONEOS_DEPLOYMENT_TARGET: 15.0"));
    }
}

#[test]
fn m11_ios_yml_builds_the_example_for_a_device_without_codesign() {
    let y = read(IOS_YML);
    let ok = jobs(&y).iter().any(|(_, b)| {
        steps(b).iter().map(|s| bare(s)).any(|s| {
            s.contains("flutter build ios")
                && s.contains("--no-codesign")
                && !s.contains("--simulator")
        })
    });
    assert!(
        ok,
        "plan M11 step 4: `flutter build ios --no-codesign` (device arm64, so aarch64-apple-ios \
         is really compiled; the simulator test only builds aarch64-apple-ios-sim)"
    );
}

/// The run script of a step as shell lines: `\` continuations joined, each line
/// `bare` (whitespace folded, quotes removed), empty lines dropped. Comments are
/// already gone (`jobs` strips them).
fn shell_lines(step: &str) -> Vec<String> {
    step.replace("\\\n", " ")
        .lines()
        .map(bare)
        .filter(|l| !l.is_empty())
        .collect()
}

/// `word` as a whole shell word in `line` (`continue`, but not `continued`).
fn has_word(line: &str, word: &str) -> bool {
    line.split(|c: char| c.is_whitespace() || c == ';')
        .any(|w| w == word)
}

/// An `exit` command with a non-zero status (`exit 1`, `exit $rc`), not `exit 0`, a bare
/// `exit`, or the word inside an `echo` (quotes are gone after `bare`, so only `exit` at
/// the start of the line or after `;`, `&&`, `||`, `then` counts).
fn exits_nonzero(line: &str) -> bool {
    let cmd = |rest: &str| rest.starts_with("exit ") && !rest["exit ".len()..].starts_with('0');
    cmd(line)
        || ["; ", "&& ", "|| ", "then "].iter().any(|sep| {
            line.match_indices(sep)
                .any(|(i, _)| cmd(&line[i + sep.len()..]))
        })
}

/// Both pack defines on the `flutter test` line, REQUIRE_PACK only ever `true`.
fn check_pack_defines(id: &str, inv: &str) {
    let zst = "--dart-define=RUSTLINE_M11_PACK_ZST=";
    assert!(
        inv.match_indices(zst)
            .any(|(i, _)| inv[i + zst.len()..].chars().next().is_some_and(|c| c != ' ')),
        "job {id}: pass --dart-define=RUSTLINE_M11_PACK_ZST=<.rlp.zst> on the flutter build/test line (m11_ios_test.dart)"
    );
    let req = "RUSTLINE_M11_REQUIRE_PACK=";
    let values: Vec<&str> = inv
        .match_indices(req)
        .map(|(i, _)| inv[i + req.len()..].split(' ').next().unwrap_or_default())
        .collect();
    assert!(
        inv.contains("--dart-define=RUSTLINE_M11_REQUIRE_PACK=true")
            && !values.is_empty()
            && values.iter().all(|v| *v == "true"),
        "job {id}: pass --dart-define=RUSTLINE_M11_REQUIRE_PACK=true on the flutter build/test line so a missing \
         pack fails instead of skipping (else the job is green with nothing tested); got {values:?}"
    );
}

/// The `flutter test` line must not swallow its own failure.
fn check_no_swallow(id: &str, inv: &str) {
    assert!(
        !inv.contains("||")
            && !inv.contains("; true")
            && !inv.contains("; :")
            && !inv.ends_with('&'),
        "job {id}: a failing or timed-out flutter test must fail the step; no `|| ...`, `; true` \
         or `&` on its line: {inv}"
    );
}

/// The files the simulator job runs (Leader, R54): only `m11_ios_test.dart`. The M6
/// files (smoke, lifecycle, pack_safety, panic, perf) need a Brașov pack with streets
/// that CI lacks and only soft-pass without it: a known gap (R54 H2) that Leader logs;
/// not a failure here. When they come back, add them (or the glob) here.
const SIM_FILES: &[&str] = &["integration_test/m11_ios_test.dart"];
/// The example app, the working directory of the simulator test steps.
const EXAMPLE: &str = "rustline_flutter/example";

/// The loop: `for V in <words>; do … done` (or `do` on the next line). Returns the loop
/// variable, its word list, the body lines and the index of the line after `done`.
fn integration_test_loop(lines: &[String]) -> Option<(String, Vec<String>, Vec<String>, usize)> {
    let h = lines.iter().position(|l| l.starts_with("for "))?;
    let (var, list) = lines[h]["for ".len()..].split_once(" in")?;
    let list = list.trim_start();
    let (list, start) = if let Some(l) = list.strip_suffix("; do") {
        (l, h + 1)
    } else if lines.get(h + 1).is_some_and(|l| l == "do") {
        (list, h + 2)
    } else {
        return None;
    };
    if var.is_empty() || !var.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    let words = list
        .split(' ')
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect();
    let mut depth = 1;
    for (i, l) in lines.iter().enumerate().skip(start) {
        if ["for ", "while ", "until "]
            .iter()
            .any(|k| l.starts_with(k))
        {
            depth += 1;
        }
        if l == "done" || l.starts_with("done ") {
            depth -= 1;
            if depth == 0 {
                return Some((var.to_string(), words, lines[start..i].to_vec(), i + 1));
            }
        }
    }
    None
}

/// Shell functions defined in a step's script (`name() {` … `}` at the same indent
/// level as other lines, after `shell_lines`): `(name, index of the def line, body)`.
fn shell_functions(lines: &[String]) -> Vec<(String, usize, Vec<String>)> {
    let mut out = Vec::new();
    for (d, l) in lines.iter().enumerate() {
        let Some(name) = l.strip_suffix("() {").or_else(|| {
            l.strip_prefix("function ")
                .and_then(|r| r.strip_suffix(" {"))
        }) else {
            continue;
        };
        if !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            continue;
        }
        let body: Vec<String> = lines[d + 1..]
            .iter()
            .take_while(|b| *b != "}")
            .cloned()
            .collect();
        out.push((name.to_string(), d, body));
    }
    out
}

/// `VAR=…` → `VAR` (shell assignment at the start of a line).
fn assigned_var(line: &str) -> Option<&str> {
    let (v, _) = line.split_once('=')?;
    (!v.is_empty() && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')).then_some(v)
}

/// `$v` or `${v}` as a whole token of `line`.
fn uses_var(line: &str, v: &str) -> bool {
    let (a, b) = (format!("${v}"), format!("${{{v}}}"));
    line.match_indices(&a).any(|(i, _)| {
        !line[i + a.len()..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
    }) || line.contains(&b)
}

/// A `return N` / `exit N` with N ≠ 0 at command position.
fn returns_nonzero(line: &str) -> bool {
    let cmd = |rest: &str| {
        rest.strip_prefix("return ").is_some_and(|r| {
            r.starts_with(|c: char| ('1'..='9').contains(&c)) || r.starts_with('$')
        })
    };
    exits_nonzero(line)
        || cmd(line)
        || ["; ", "&& ", "|| ", "then "].iter().any(|sep| {
            line.match_indices(sep)
                .any(|(i, _)| cmd(&line[i + sep.len()..]))
        })
}

/// The lines of the `if …; then` block that starts at `lines[i]` (up to its `fi`).
fn if_block(lines: &[String], i: usize) -> Vec<String> {
    let mut depth = 0;
    let mut out = Vec::new();
    for l in &lines[i + 1..] {
        if l.starts_with("if ") {
            depth += 1;
        }
        if l == "fi" || l.starts_with("fi;") {
            if depth == 0 {
                break;
            }
            depth -= 1;
        }
        out.push(l.clone());
    }
    out
}

/// The python3 snippet of `check_summary` (raw lines, heredoc body): it must load the
/// summary JSON named by argv[1] and exit 0 only if passedTests > 0, failedTests == 0
/// and skippedTests == 0 (each key read without a default, so a missing key fails).
fn check_summary_python(id: &str, py: &[&str]) {
    let text = py.join("\n");
    assert!(
        text.contains("json.load") && text.contains("sys.argv[1]"),
        "job {id}: check_summary's python3 must json.load the file given as argv[1]"
    );
    let quoted = |s: &str| -> Vec<String> {
        let mut out = Vec::new();
        let mut it = s.split(['"', '\'']);
        it.next();
        while let Some(q) = it.next() {
            out.push(q.to_string());
            it.next();
        }
        out
    };
    for (i, _) in text.match_indices(".get(") {
        let args = text[i + 5..].split(')').next().unwrap_or_default();
        assert!(
            !args.contains(','),
            "job {id}: check_summary's python3 must not default a missing count (.get(k, …))"
        );
    }
    // Bind python names to the summary keys: `p, f, k = (… ("passedTests", …))` or
    // `p = s["passedTests"]`.
    let keys = ["passedTests", "failedTests", "skippedTests"];
    let mut bound: Vec<(String, String)> = Vec::new();
    let mut assigned: Vec<String> = Vec::new();
    for l in py.iter().map(|l| l.trim()) {
        let Some((lhs, rhs)) = l.split_once(" = ") else {
            continue;
        };
        if lhs.contains(['(', '[', '=', '!', '<', '>']) {
            continue;
        }
        let names: Vec<String> = lhs.split(',').map(|n| n.trim().to_string()).collect();
        assigned.extend(names.clone());
        let qs: Vec<String> = quoted(rhs)
            .into_iter()
            .filter(|q| keys.contains(&q.as_str()))
            .collect();
        if names.len() == qs.len() {
            bound.extend(names.into_iter().zip(qs));
        }
    }
    for (n, _) in &bound {
        assert!(
            assigned.iter().filter(|a| *a == n).count() == 1,
            "job {id}: check_summary's python3 reassigns {n}"
        );
    }
    let exits: Vec<&str> = py
        .iter()
        .map(|l| l.trim())
        .filter(|l| l.contains("exit("))
        .collect();
    assert!(
        exits.len() == 1,
        "job {id}: check_summary's python3 must have exactly one exit: sys.exit(0 if … else 1)"
    );
    let cond = exits[0]
        .strip_prefix("sys.exit(0 if ")
        .and_then(|r| r.strip_suffix(" else 1)"))
        .unwrap_or_else(|| {
            panic!(
                "job {id}: python3: sys.exit(0 if … else 1), got {}",
                exits[0]
            )
        });
    assert!(
        !cond.contains(" or ") && !cond.contains("not "),
        "job {id}: python3 exit condition: all of passed > 0, failed == 0, skipped == 0: {cond}"
    );
    let conj: Vec<&str> = cond.split(" and ").map(str::trim).collect();
    let is_key = |e: &str, k: &str| {
        bound.iter().any(|(n, kk)| n == e && kk == k)
            || e.ends_with(&format!("[\"{k}\"]"))
            || e.ends_with(&format!("['{k}']"))
            || e.ends_with(&format!(".get(\"{k}\")"))
            || e.ends_with(&format!(".get('{k}')"))
    };
    let has = |k: &str, ops: &[&str]| {
        conj.iter().any(|c| {
            ops.iter().any(|op| {
                c.strip_suffix(op)
                    .is_some_and(|e| is_key(e.trim_end_matches(' '), k))
            })
        })
    };
    assert!(
        has("passedTests", &[" > 0", " >= 1"]),
        "job {id}: python3 must require passedTests > 0 (a run with no tests passes otherwise): {cond}"
    );
    assert!(
        has("failedTests", &[" == 0"]),
        "job {id}: python3 must require failedTests == 0: {cond}"
    );
    assert!(
        has("skippedTests", &[" == 0"]),
        "job {id}: python3 must require skippedTests == 0 (a skipped Dart test proves nothing): {cond}"
    );
}

/// `check_summary NAME` (R54): its own bundle must exist (else fail), `xcrun xcresulttool
/// get test-results summary --path <bundle>` must succeed into a JSON file, and python3
/// over that file must succeed; every failure returns non-zero, nothing returns 0 early.
/// `raw` is the comment-stripped step text (quotes intact) for the python3 heredoc.
fn check_summary_fn(id: &str, cs: &str, body: &[String], bundle: &str, raw: &str) {
    for l in body {
        assert!(
            !l.contains("|| true")
                && !l.contains("; true")
                && !l.contains("exit 0")
                && !l.contains("return 0")
                && l != "return"
                && !l.ends_with("; return")
                && l != "set +e",
            "job {id}: {cs} must not swallow a failure: {l}"
        );
    }
    let mut vars: Vec<(String, String)> = Vec::new();
    for l in body {
        let l = l.strip_prefix("local ").unwrap_or(l);
        for w in l.split(' ') {
            if let Some(v) = assigned_var(w) {
                vars.push((v.to_string(), w[v.len() + 1..].to_string()));
            }
        }
    }
    let resolve = |w: &str| -> String {
        let n = w
            .strip_prefix("${")
            .and_then(|r| r.strip_suffix('}'))
            .or_else(|| w.strip_prefix('$'));
        n.and_then(|n| vars.iter().rev().find(|(k, _)| k == n))
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| w.to_string())
    };
    let missing = body.iter().position(|l| {
        let t = l
            .strip_prefix("if [ ! -d ")
            .or_else(|| l.strip_prefix("if [ ! -e "))
            .or_else(|| l.strip_prefix("if [[ ! -d "))
            .or_else(|| l.strip_prefix("if [[ ! -e "));
        t.and_then(|t| t.split(' ').next())
            .is_some_and(|p| resolve(p) == bundle)
    });
    assert!(
        missing.is_some_and(|i| if_block(body, i).iter().any(|l| returns_nonzero(l))),
        "job {id}: {cs}: a missing result bundle ({bundle}) must fail the file \
         (`if [ ! -d \"$xr\" ]; then … return 1`)"
    );
    let xrt = "xcrun xcresulttool get test-results summary --path ";
    let xi = body
        .iter()
        .position(|l| l.contains(xrt))
        .unwrap_or_else(|| panic!("job {id}: {cs}: {xrt}<bundle>"));
    let xl = &body[xi];
    let args = xl.split_once(xrt).map(|(_, r)| r).unwrap_or_default();
    let path = args.split(' ').next().unwrap_or_default();
    assert!(
        resolve(path) == bundle,
        "job {id}: {cs}: xcresulttool must read the file's own bundle {bundle}, got {path}"
    );
    let js = args
        .split_once("> ")
        .and_then(|(_, r)| r.split([' ', ';']).next())
        .unwrap_or_default()
        .to_string();
    assert!(
        !js.is_empty() && !resolve(&js).is_empty(),
        "job {id}: {cs}: xcresulttool's summary goes to a file (> $js) for python3"
    );
    let xr_checked = (xl.starts_with("if ! ")
        && xl.ends_with("; then")
        && if_block(body, xi).iter().any(|l| returns_nonzero(l)))
        || (xl.starts_with("xcrun ") && returns_nonzero(xl) && xl.contains(" || "));
    assert!(
        xr_checked,
        "job {id}: {cs}: an xcresulttool failure must fail the file (`if ! xcrun xcresulttool …; then return 1`)"
    );
    let pi = body
        .iter()
        .position(|l| l.contains("python3 ") && l.contains("<<"))
        .unwrap_or_else(|| {
            panic!("job {id}: {cs}: python3 - \"$js\" <<'PY' … PY over the summary")
        });
    let pl = &body[pi];
    let delim = pl
        .split_once("<<")
        .map(|(_, r)| r.trim_start_matches('-').trim())
        .unwrap_or_default()
        .to_string();
    assert!(
        pl.starts_with("if ! python3 ") && pl.split(' ').any(|w| w == js.as_str()),
        "job {id}: {cs}: `if ! python3 - \"$js\" <<'PY'` over xcresulttool's output: {pl}"
    );
    let end = body[pi + 1..]
        .iter()
        .position(|l| *l == delim)
        .map(|p| p + pi + 1)
        .unwrap_or_else(|| panic!("job {id}: {cs}: heredoc {delim} not closed"));
    assert!(
        body.get(end + 1).is_some_and(|l| l == "then")
            && if_block(body, end + 1).iter().any(|l| returns_nonzero(l)),
        "job {id}: {cs}: a python3 failure must fail the file (`then … return 1`)"
    );
    // The heredoc's python with its quotes, from the raw step text.
    let raw_lines: Vec<&str> = raw.lines().collect();
    let r0 = raw_lines
        .iter()
        .position(|l| l.contains("python3 ") && l.contains("<<"))
        .unwrap_or_default();
    let py: Vec<&str> = raw_lines[r0 + 1..]
        .iter()
        .take_while(|l| l.trim() != delim)
        .copied()
        .collect();
    check_summary_python(id, &py);
}

/// Integration tests on the simulator with the tiny pack (plan M11 step 4), through
/// XCTest (Leader, 2026-10-27: `flutter test -d` hangs on flutter/flutter#181771).
/// Expected shape of the test step:
/// - a function (`test_file`) that runs `flutter build ios --config-only --simulator
///   integration_test/"$1".dart` with both pack defines, then (`&&`) `xcodebuild test …
///   -destination …id=$UDID -resultBundlePath <per-file path>` as its last command, so
///   the function's status is xcodebuild's;
/// - `for f in integration_test/m11_ios_test.dart` (exactly SIM_FILES, each in the repo)
///   calling it exactly once per file with the file's name, optionally through
///   `run_with_timeout <secs> bash -c 'test_file "$1"' _ "$name"` (then `export -f
///   test_file`), no skips, no swallowed status; the wrapper limit ends before the step's;
/// - the status captured (`rc=${PIPESTATUS[0]}`) or `set -e` in force, and a failure
///   turned into a non-zero exit (directly or via a variable checked after the loop);
/// - R54: `check_summary "$name"` on the file's bundle (xcresulttool summary + python3:
///   passedTests > 0, failedTests 0, skippedTests 0; a missing bundle fails), its failure
///   setting the status (XCTest reports nothing for Dart tests that never ran, so a green
///   xcodebuild alone proves nothing).
#[test]
fn m11_ios_yml_runs_the_integration_test_with_a_real_pack() {
    let y = read(IOS_YML);
    let (id, body) = sim_job(&y);
    let ss: Vec<String> = steps(&body).iter().map(|s| bare(s)).collect();
    let test = ss.iter().position(|s| runs_integration_tests(s)).unwrap();
    let lines = shell_lines(&steps(&body)[test]);
    let step = lines.join("\n");
    assert!(
        !step.contains("continue-on-error: true") && !step.contains("exit 0"),
        "job {id}: the integration test step must be able to fail (no continue-on-error, no exit 0)"
    );
    assert!(
        !step.contains("nullglob"),
        "job {id}: no nullglob (an empty glob would run nothing and pass)"
    );

    // The per-file function: config-only, then xcodebuild test.
    let funcs = shell_functions(&lines);
    let (tf, tf_def, tf_body) = funcs
        .iter()
        .find(|(_, _, b)| {
            b.iter()
                .any(|l| l.contains("flutter build ios") && l.contains("--config-only"))
        })
        .unwrap_or_else(|| {
            panic!(
                "job {id}: a function (test_file) must run `flutter build ios --config-only \
                 --simulator integration_test/$1.dart` then `xcodebuild test`"
            )
        });
    let cfgs: Vec<usize> = (0..tf_body.len())
        .filter(|&i| {
            tf_body[i].contains("flutter build ios") || tf_body[i].contains("flutter test")
        })
        .collect();
    assert!(
        cfgs.len() == 1,
        "job {id}: {tf} must run exactly one flutter build (config-only), found {}",
        cfgs.len()
    );
    let c = cfgs[0];
    let cfg = &tf_body[c];
    assert!(
        cfg.contains("--config-only") && cfg.contains("--simulator"),
        "job {id}: {tf}: flutter build ios --config-only --simulator: {cfg}"
    );
    let targets: Vec<&str> = cfg
        .split(' ')
        .filter(|w| w.starts_with("integration_test") || *w == "$1" || *w == "${1}")
        .collect();
    let by_name =
        targets == ["integration_test/$1.dart"] || targets == ["integration_test/${1}.dart"];
    let by_path = targets == ["$1"] || targets == ["${1}"];
    assert!(
        by_name || by_path,
        "job {id}: {tf}: the config-only build must target the one file passed in \
         (integration_test/\"$1\".dart), not the whole dir or a fixed file; got {targets:?}"
    );
    check_pack_defines(&id, cfg);
    let xcb: Vec<usize> = (0..tf_body.len())
        .filter(|&i| tf_body[i].contains("xcodebuild"))
        .collect();
    assert!(
        xcb.len() == 1 && tf_body[xcb[0]].starts_with("xcodebuild test "),
        "job {id}: {tf} must run `xcodebuild test` exactly once"
    );
    let x = xcb[0];
    let xl = &tf_body[x];
    assert!(
        x == c + 1
            && (cfg.ends_with(" &&") || cfg.ends_with("|| return 1") || cfg.ends_with("|| exit 1")),
        "job {id}: {tf}: xcodebuild test must follow the config-only build and only run if it \
         succeeded (`… &&`), else it tests the previous file's config"
    );
    assert!(
        x + 1 == tf_body.len(),
        "job {id}: {tf}: xcodebuild test must be the last command, so its status is the function's"
    );
    check_no_swallow(&id, xl);
    assert!(
        !["-only-testing", "-skip-testing", "-testPlan", "-xctestrun"]
            .iter()
            .any(|f| xl.contains(f)),
        "job {id}: {tf}: xcodebuild test must run every RunnerTests case (no -only-testing, \
         -skip-testing, -testPlan or -xctestrun)"
    );
    assert!(
        xl.contains("-destination") && uses_var(xl, "UDID"),
        "job {id}: {tf}: xcodebuild test -destination 'platform=iOS Simulator,id=$UDID'"
    );
    let bundle = xl
        .split_once("-resultBundlePath ")
        .map(|(_, r)| r.split(' ').next().unwrap_or_default())
        .unwrap_or_default();
    assert!(
        uses_var(bundle, "1"),
        "job {id}: {tf}: -resultBundlePath <dir>/$1.xcresult (one bundle per file; xcodebuild \
         refuses an existing path); got {bundle:?}"
    );

    // The loop over the listed files (R54: exactly SIM_FILES, each in the repo).
    let (var, files, lb, end) = integration_test_loop(&lines).unwrap_or_else(|| {
        panic!("job {id}: loop `for f in integration_test/m11_ios_test.dart; do … done`")
    });
    assert!(
        files
            .iter()
            .map(String::as_str)
            .eq(SIM_FILES.iter().copied()),
        "job {id}: the loop must run exactly {SIM_FILES:?} (Leader R54; M6 files are the known \
         gap R54 H2), got {files:?}"
    );
    for f in &files {
        assert!(
            root().join(EXAMPLE).join(f).is_file(),
            "job {id}: {f} is not in {EXAMPLE}: the loop would test a file that is not there"
        );
    }
    assert!(
        ss[test].contains(&format!("working-directory: {EXAMPLE}")),
        "job {id}: the integration test step runs in {EXAMPLE} (the loop's paths are relative)"
    );
    let after = lines[end..].to_vec();
    let calls_tf = |l: &String| has_word(l, tf) && !l.starts_with("export -f");
    let in_loop: Vec<usize> = (0..lb.len()).filter(|&i| calls_tf(&lb[i])).collect();
    assert!(
        in_loop.len() == 1,
        "job {id}: the loop must run {tf} exactly once per file, found {}",
        in_loop.len()
    );
    let loop_start = lines
        .iter()
        .position(|l| l.starts_with("for "))
        .unwrap_or_default();
    let outside = lines
        .iter()
        .enumerate()
        .filter(|(i, l)| {
            (*i < loop_start || *i >= end) && *i != *tf_def && calls_tf(l) && !l.ends_with("() {")
        })
        .count();
    assert!(
        outside == 0,
        "job {id}: {tf} runs only in the loop (a call outside it runs a file twice or a fixed one)"
    );
    assert!(
        !lb.iter()
            .any(|l| has_word(l, "continue") || has_word(l, "break")),
        "job {id}: the integration_test loop must not skip files (continue/break)"
    );
    let at = in_loop[0];
    for l in &lb[..at] {
        assert!(
            !["if ", "case ", "[ ", "[[", "test ", "&&", "||"]
                .iter()
                .any(|k| l.contains(k)),
            "job {id}: no condition before {tf} in the loop (could skip files): {l}"
        );
    }
    let call = &lb[at];
    check_no_swallow(&id, call);
    // `[<wrapper> <secs>] <tf> ARG` or `[<wrapper> <secs>] bash -c <tf> $1 _ ARG`.
    let words: Vec<&str> = call.split(' ').collect();
    let t = words.iter().position(|w| *w == tf.as_str()).unwrap();
    let (prefix, arg) = if t >= 2 && words[t - 2] == "bash" && words[t - 1] == "-c" {
        assert!(
            words.get(t + 1) == Some(&"$1") && words.get(t + 2) == Some(&"_"),
            "job {id}: bash -c '{tf} \"$1\"' _ \"$name\": {call}"
        );
        assert!(
            lines.iter().any(|l| l == &format!("export -f {tf}")),
            "job {id}: bash -c needs `export -f {tf}`"
        );
        (
            &words[..t - 2],
            words.get(t + 3).copied().unwrap_or_default(),
        )
    } else {
        (&words[..t], words.get(t + 1).copied().unwrap_or_default())
    };
    let wrapper = match prefix {
        [] => None,
        [w, secs]
            if secs
                .trim_end_matches(['s', 'm'])
                .chars()
                .all(|c| c.is_ascii_digit())
                && w.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') =>
        {
            Some(*w)
        }
        _ => panic!("job {id}: only a timeout wrapper may come before {tf}: {call}"),
    };
    if let Some(w) = wrapper {
        if let Some((_, _, def)) = funcs.iter().find(|(n, _, _)| n == w) {
            assert!(
                !def.iter().any(|l| l.contains("|| true")
                    || l.contains("exit 0")
                    || l.contains("exit(0)")),
                "job {id}: the timeout wrapper {w} must pass the test's failure on"
            );
        }
    }
    let base = lb[..at].iter().find_map(|l| {
        let n = assigned_var(l)?;
        let ok = l.ends_with(&format!("=$(basename ${var} .dart)"))
            || l.ends_with(&format!("=$(basename ${{{var}}} .dart)"));
        ok.then(|| n.to_string())
    });
    let arg_is = |v: &str| arg == format!("${v}") || arg == format!("${{{v}}}");
    let per_file = if by_name {
        base.as_deref().is_some_and(arg_is)
    } else {
        arg_is(&var)
    };
    assert!(
        per_file,
        "job {id}: {tf} must get the loop's file (${var}, or its basename for \
         integration_test/$1.dart): {call}"
    );

    // A failure in any file fails the step.
    let piped = call.replace("||", "").contains(" | ");
    let before: Vec<&String> = lines[..loop_start].iter().chain(&lb[..at]).collect();
    let errexit_off = before
        .iter()
        .rev()
        .find(|l| **l == "set +e" || **l == "set -e")
        .is_some_and(|l| *l == "set +e");
    let status = lb.get(at + 1).and_then(|l| {
        let x = assigned_var(l)?;
        let ok = l.ends_with("=${PIPESTATUS[0]}") || (!piped && l.ends_with("=$?"));
        ok.then(|| x.to_string())
    });
    // Records a failure: a non-zero exit, or `F=…` with `[ -n $F ]` → exit after the loop.
    let records = |block: &[&String]| {
        block.iter().any(|l| exits_nonzero(l))
            || block.iter().filter_map(|l| assigned_var(l)).any(|f| {
                let Some(k) = after.iter().position(|l| {
                    l.contains(&format!("-n ${f}")) || l.contains(&format!("-n ${{{f}}}"))
                }) else {
                    return false;
                };
                after[k..]
                    .iter()
                    .take_while(|l| *l != "fi")
                    .any(|l| exits_nonzero(l))
            })
    };
    let then_block = |from: usize| -> Vec<&String> {
        lb[from + 1..].iter().take_while(|l| *l != "fi").collect()
    };
    let mut status_check = None;
    let propagated = if !errexit_off {
        // Default macOS run shell is `bash -e`: the step stops on the failing file.
        !piped || step.contains("pipefail")
    } else if let Some(x) = &status {
        let checks = |l: &String| {
            [" -ne 0", " != 0", " -gt 0"]
                .iter()
                .any(|op| l.contains(&format!("${x}{op}")) || l.contains(&format!("${{{x}}}{op}")))
        };
        status_check = lb.iter().skip(at + 2).position(checks).map(|p| p + at + 2);
        status_check.is_some_and(|c| {
            // Nothing between the capture and the check may reset the status to 0.
            !lb[at + 2..c].iter().any(|l| {
                l.starts_with(&format!("{x}="))
                    && !l[x.len() + 1..].starts_with(|ch: char| ('1'..='9').contains(&ch))
            }) && records(&then_block(c))
        })
    } else {
        false
    };
    assert!(
        propagated,
        "job {id}: a failing or timed-out integration test file must fail the step (set -e, or \
         rc=${{PIPESTATUS[0]}} checked and turned into a non-zero exit)"
    );

    // The result summary (R54), replacing the console count: `check_summary NAME` on the
    // file's own bundle; its failure fails the file.
    let raw = &steps(&body)[test];
    let (cs, cs_def, cs_body) = funcs
        .iter()
        .find(|(_, _, b)| {
            b.iter()
                .any(|l| l.contains("xcresulttool get test-results summary"))
        })
        .unwrap_or_else(|| {
            panic!(
                "job {id}: a function (check_summary) must read `xcrun xcresulttool get \
                 test-results summary --path $xr` of each file's bundle"
            )
        });
    check_summary_fn(&id, cs, cs_body, bundle, raw);
    let calls_cs = |l: &String| has_word(l, cs) && !l.starts_with("export -f");
    let cs_in: Vec<usize> = (0..lb.len()).filter(|&i| calls_cs(&lb[i])).collect();
    let cs_out = lines
        .iter()
        .enumerate()
        .filter(|(i, l)| (*i < loop_start || *i >= end) && *i != *cs_def && calls_cs(l))
        .count();
    assert!(
        cs_in.len() == 1 && cs_out == 0,
        "job {id}: the loop must run {cs} exactly once per file (found {} in the loop, {cs_out} \
         outside)",
        cs_in.len()
    );
    let k = cs_in[0];
    let cl = &lb[k];
    let call_cs = format!("{cs} {arg}");
    let x = status.clone().unwrap_or_default();
    let opened = lb[at + 1..k]
        .iter()
        .filter(|l| l.starts_with("if "))
        .count();
    let closed = lb[at + 1..k].iter().filter(|l| *l == "fi").count();
    assert!(
        k > at && opened == closed,
        "job {id}: {cs} runs after the file's test, unconditionally (not inside another if)"
    );
    let sets_nonzero = |l: &String| {
        !x.is_empty()
            && l.starts_with(&format!("{x}="))
            && l[x.len() + 1..].starts_with(|c: char| ('1'..='9').contains(&c))
    };
    let before_check = status_check.is_some_and(|c| k < c);
    let wired = if let Some(cond) = cl
        .strip_prefix("if ")
        .and_then(|r| r.strip_suffix("; then"))
    {
        // `if [ $rc -ne 124 ] && ! check_summary $name && [ $rc -eq 0 ]; then rc=1; fi`:
        // every other test is true when the file's test passed (rc 0), so the summary
        // always runs then.
        let parts: Vec<&str> = cond.split(" && ").collect();
        let others_ok = parts
            .iter()
            .filter(|p| **p != format!("! {call_cs}"))
            .all(|p| {
                let Some(t) = p.strip_prefix("[ ").and_then(|r| r.strip_suffix(" ]")) else {
                    return false;
                };
                let w: Vec<&str> = t.split(' ').collect();
                w.len() == 3
                    && (w[0] == format!("${x}") || w[0] == format!("${{{x}}}"))
                    && match (w[1], w[2]) {
                        ("-eq" | "=", "0") => true,
                        ("-ne" | "!=", n) => n != "0" && n.chars().all(|c| c.is_ascii_digit()),
                        _ => false,
                    }
            });
        let block: Vec<&String> = lb[k + 1..].iter().take_while(|l| *l != "fi").collect();
        !cond.contains("||")
            && parts
                .iter()
                .filter(|p| **p == format!("! {call_cs}"))
                .count()
                == 1
            && others_ok
            && ((before_check && block.iter().any(|l| sets_nonzero(l))) || records(&block))
    } else if let Some(r) = cl.strip_prefix(&format!("{call_cs} || ")) {
        (before_check && sets_nonzero(&r.to_string())) || exits_nonzero(cl)
    } else {
        // A bare call with errexit on: a failure stops the step.
        let lastset = lines[..loop_start]
            .iter()
            .chain(&lb[..k])
            .rev()
            .find(|l| *l == "set +e" || *l == "set -e");
        *cl == call_cs && lastset.is_none_or(|l| l == "set -e")
    };
    assert!(
        wired,
        "job {id}: {cs}'s failure must fail the file: `if [ \"$rc\" -ne 124 ] && ! {cs} \"$name\" \
         && [ \"$rc\" -eq 0 ]; then rc=1; fi` before the status check (or `{cs} \"$name\" || \
         rc=1`): {cl}"
    );

    // The per-file wrapper limit must end (with its kill grace) before the step's own
    // timeout, else GitHub kills the step first and the wrapper's message and the
    // simulator terminate never happen.
    if let Some(w) = wrapper {
        let secs: u64 = {
            let s = words[..t]
                .iter()
                .rev()
                .find(|s| s.starts_with(|c: char| c.is_ascii_digit()))
                .copied()
                .unwrap_or_default();
            let n: u64 = s
                .trim_end_matches(['s', 'm'])
                .parse()
                .unwrap_or(u64::MAX / 2);
            if s.ends_with('m') {
                n * 60
            } else {
                n
            }
        };
        let grace: u64 = funcs
            .iter()
            .find(|(n, _, _)| n == w)
            .and_then(|(_, _, d)| {
                d.iter().find_map(|l| {
                    l.split("sleep ")
                        .nth(1)
                        .and_then(|r| r.split([';', ' ']).next())
                        .and_then(|n| n.parse().ok())
                })
            })
            .unwrap_or(0);
        let step_min: u64 = ss[test]
            .split("timeout-minutes: ")
            .nth(1)
            .and_then(|r| r.split(' ').next())
            .and_then(|n| n.parse().ok())
            .unwrap_or_else(|| panic!("job {id}: the integration test step needs timeout-minutes"));
        assert!(
            (secs + grace).saturating_mul(SIM_FILES.len() as u64) + 60 <= step_min * 60,
            "job {id}: {w} {secs}s (+{grace}s kill grace, +60s for {cs}) per file x {} must end \
             before the step's timeout-minutes: {step_min}",
            SIM_FILES.len()
        );
    }

    let pack = ss
        .iter()
        .position(|s| s.contains("build-pack") && s.contains("data/fixtures/tiny"));
    assert!(
        pack.is_some_and(|p| p < test),
        "job {id}: build the tiny fixture pack (rustline build-pack --gtfs data/fixtures/tiny) before the test"
    );
}

/// The warm-up (R54): one `flutter build ios --config-only --simulator <file>` of the same
/// file as the loop (SIM_FILES), with both pack defines, then `xcodebuild
/// build-for-testing … CODE_SIGNING_ALLOWED=NO`; before the integration test step, in
/// the example dir, failures not swallowed.
#[test]
fn m11_ios_yml_warms_up_the_same_file_as_the_loop() {
    let y = read(IOS_YML);
    let (id, body) = sim_job(&y);
    let raw = steps(&body);
    let ss: Vec<String> = raw.iter().map(|s| bare(s)).collect();
    let test = ss.iter().position(|s| runs_integration_tests(s)).unwrap();
    let w = ss
        .iter()
        .position(|s| s.contains("xcodebuild build-for-testing") && s.contains("--config-only"))
        .unwrap_or_else(|| {
            panic!("job {id}: a warm-up step (config-only + xcodebuild build-for-testing)")
        });
    assert!(
        w < test,
        "job {id}: the warm-up runs before the integration tests"
    );
    assert!(
        ss[w].contains(&format!("working-directory: {EXAMPLE}")),
        "job {id}: the warm-up runs in {EXAMPLE}"
    );
    let lines = shell_lines(&raw[w]);
    let cfgs: Vec<usize> = (0..lines.len())
        .filter(|&i| lines[i].contains("flutter build ios") || lines[i].contains("flutter test"))
        .collect();
    assert!(
        cfgs.len() == 1,
        "job {id}: the warm-up runs one flutter build"
    );
    let cfg = &lines[cfgs[0]];
    assert!(
        cfg.contains("--config-only") && cfg.contains("--simulator"),
        "job {id}: warm-up: flutter build ios --config-only --simulator <file>: {cfg}"
    );
    check_pack_defines(&id, cfg);
    let targets: Vec<&str> = cfg
        .split(' ')
        .filter(|t| t.starts_with("integration_test") || t.starts_with('$') && !t.contains('='))
        .collect();
    let target = match targets.as_slice() {
        [t] if *t == "$1" || *t == "${1}" => lines[..cfgs[0]]
            .iter()
            .rev()
            .find_map(|l| l.strip_prefix("set -- "))
            .unwrap_or_default()
            .to_string(),
        [t] => t.to_string(),
        _ => String::new(),
    };
    assert!(
        SIM_FILES.contains(&target.as_str()),
        "job {id}: the warm-up must build the loop's file ({SIM_FILES:?}), got {target:?}"
    );
    let xb: Vec<&String> = lines.iter().filter(|l| l.contains("xcodebuild")).collect();
    assert!(
        xb.len() == 1 && xb[0].starts_with("xcodebuild build-for-testing "),
        "job {id}: the warm-up runs xcodebuild build-for-testing once"
    );
    for l in lines.iter() {
        check_no_swallow(&id, l);
    }
    let step = lines.join("\n");
    assert!(
        !step.contains("exit 0")
            && !step.contains("set +e")
            && !ss[w].contains("continue-on-error: true"),
        "job {id}: a failing warm-up must fail the job"
    );
    assert!(
        !step.contains(" | ") || ss[w].contains("pipefail"),
        "job {id}: the warm-up pipes into tee, so its shell needs pipefail"
    );
}

/// R54: SwiftPM off (`flutter config --no-enable-swift-package-manager`) before `flutter pub
/// get` in the simulator job, so integration_test comes through CocoaPods and RunnerTests'
/// `inherit! :search_paths` resolves `@import integration_test`.
#[test]
fn m11_ios_yml_turns_swiftpm_off_before_pub_get() {
    let y = read(IOS_YML);
    let (id, body) = sim_job(&y);
    let all: Vec<(usize, usize, String)> = steps(&body)
        .iter()
        .enumerate()
        .flat_map(|(s, st)| {
            shell_lines(st)
                .into_iter()
                .enumerate()
                .map(move |(i, l)| (s, i, l))
        })
        .collect();
    let find = |pat: &str| {
        all.iter()
            .enumerate()
            .find_map(|(k, (_, _, l))| l.find(pat).map(|o| (k, o)))
    };
    let off = find("flutter config --no-enable-swift-package-manager");
    let pub_get = find("flutter pub get");
    assert!(
        off.is_some(),
        "job {id}: run `flutter config --no-enable-swift-package-manager` (R54)"
    );
    assert!(
        pub_get.is_some_and(|p| off.is_some_and(|o| o < p)),
        "job {id}: `flutter config --no-enable-swift-package-manager` must run before `flutter pub get` \
         (pub get writes the plugin list for the chosen package manager)"
    );
    let ol = &all[off.unwrap().0].2;
    check_no_swallow(&id, ol);
    assert!(
        !all.iter()
            .any(|(_, _, l)| l.contains("flutter config --enable-swift-package-manager")),
        "job {id}: SwiftPM must stay off for the job"
    );
}

/// R54: the simulator job's timeout is 90 min (Leader L1), and every xcodebuild in it
/// (warm-up and test) passes CODE_SIGNING_ALLOWED=NO.
#[test]
fn m11_ios_yml_sim_job_timeout_and_no_codesign() {
    let y = read(IOS_YML);
    let (id, body) = sim_job(&y);
    let head: Vec<&str> = body.lines().take_while(|l| l.trim() != "steps:").collect();
    let t: Vec<&str> = head
        .iter()
        .filter_map(|l| l.trim().strip_prefix("timeout-minutes:"))
        .map(str::trim)
        .collect();
    assert!(
        t == ["90"],
        "job {id}: timeout-minutes: 90 (Leader R54 L1), got {t:?}"
    );
    let xb: Vec<String> = steps(&body)
        .iter()
        .flat_map(|s| shell_lines(s))
        .filter(|l| l.split(' ').any(|w| w == "xcodebuild"))
        .collect();
    assert!(
        xb.iter()
            .any(|l| l.contains("xcodebuild build-for-testing "))
            && xb.iter().any(|l| l.contains("xcodebuild test ")),
        "job {id}: xcodebuild build-for-testing (warm-up) and xcodebuild test"
    );
    for l in &xb {
        assert!(
            l.split(' ').any(|w| w == "CODE_SIGNING_ALLOWED=NO"),
            "job {id}: CODE_SIGNING_ALLOWED=NO on every xcodebuild (no signing identity on CI): {l}"
        );
    }
}

#[test]
fn m11_ios_yml_runs_the_ffi_tests_on_macos() {
    let y = read(IOS_YML);
    let ok = jobs(&y).iter().any(|(_, b)| {
        bare(b).contains("runs-on: macos-26")
            && steps(b).iter().map(|s| bare(s)).any(|s| {
                s.contains("cargo test")
                    && s.contains("--locked")
                    && (s.contains("-p rustline-ffi") || s.contains("--package rustline-ffi"))
            })
    });
    assert!(
        ok,
        "plan M11 step 4: `cargo test --locked -p rustline-ffi` on macOS (flock, statvfs, \
         the free-space override and the 256 MiB cap on Darwin)"
    );
}

#[test]
fn m11_ios_yml_records_lib_and_app_sizes() {
    let y = read(IOS_YML);
    let all: Vec<(String, Vec<String>)> = jobs(&y)
        .into_iter()
        .map(|(id, b)| (id, steps(&b).iter().map(|s| bare(s)).collect()))
        .collect();
    let size = all.iter().find_map(|(id, ss)| {
        ss.iter()
            .position(|s| s.contains("librustline_ffi.a") && s.contains("Runner.app"))
            .map(|i| (id, ss, i))
    });
    let (id, ss, i) = size.expect(
        "plan M11 step 5: a step that measures librustline_ffi.a and Runner.app (the app size)",
    );
    assert!(
        ss[i].contains("GITHUB_STEP_SUMMARY"),
        "job {id}: write the sizes to $GITHUB_STEP_SUMMARY so they are visible in the run"
    );
    assert!(
        ss[i + 1..]
            .iter()
            .any(|s| s.contains("actions/upload-artifact@") && s.to_lowercase().contains("size")),
        "job {id}: upload the size report as an artifact (name or path containing \"size\") after measuring"
    );
}

#[test]
fn m11_ios_yml_follows_the_m10_workflow_rules() {
    let y = read(IOS_YML);
    let text = strip_comments(&y);
    let b = bare(&text);
    assert!(
        b.contains("workflow_dispatch"),
        "manual trigger (macOS minutes are ~10x, Research §2)"
    );
    assert!(
        !b.contains("pull_request_target"),
        "never pull_request_target (M10)"
    );
    assert!(
        b.contains("permissions: contents: read"),
        "top-level permissions: contents: read"
    );
    for line in y.lines() {
        let t = line.trim_start().trim_start_matches("- ");
        let Some(r) = t.strip_prefix("uses:") else {
            continue;
        };
        let r = r.trim();
        if r.starts_with("./") {
            continue;
        }
        let (_, rest) = r
            .split_once('@')
            .unwrap_or_else(|| panic!("unpinned action: {line}"));
        let sha: String = rest.chars().take_while(|c| !c.is_whitespace()).collect();
        assert!(
            sha.len() == 40 && sha.chars().all(|c| c.is_ascii_hexdigit()),
            "pin actions to a full commit SHA with a `# vX` comment (M10): {line}"
        );
        assert!(
            rest.contains("# v") || rest.contains("# master") || rest.contains("# main"),
            "tag comment missing: {line}"
        );
    }
    for (id, body) in jobs(&y) {
        assert!(
            bare(&body).contains("timeout-minutes:"),
            "job {id}: timeout-minutes"
        );
        for s in steps(&body).iter().map(|s| bare(s)) {
            if s.contains("actions/checkout@") {
                assert!(
                    s.contains("persist-credentials: false"),
                    "job {id}: checkout persist-credentials: false"
                );
            }
        }
    }
}

// ---- iOS project config --------------------------------------------------------

#[test]
fn m11_podspec_builds_rustline_ffi_with_cargokit_for_ios_15() {
    let p = bare(&strip_comments(&read(PODSPEC)));
    assert!(
        p.contains("s.platform = :ios, 15.0") || p.contains("s.ios.deployment_target = 15.0"),
        "podspec: iOS 15.0 minimum (template says 11.0; Flutter 3.47 needs 15.0)"
    );
    for old in ["11.0", "12.0", "13.0", "14.0"] {
        assert!(
            !p.contains(&format!(":ios, {old}")),
            "podspec still targets iOS {old}"
        );
    }
    assert!(p.contains("script_phase"), "podspec: cargokit script_phase");
    assert!(
        p.contains("cargokit/build_pod.sh"),
        "podspec: runs cargokit/build_pod.sh"
    );
    assert!(
        p.contains("crates/rustline-ffi") && p.contains("rustline_ffi"),
        "podspec: build_pod.sh gets the manifest dir (../../crates/rustline-ffi) and lib name rustline_ffi"
    );
    assert!(
        p.contains(":execution_position => :before_compile")
            || p.contains("execution_position: :before_compile"),
        "podspec: Rust must build before compile"
    );
    assert!(
        p.contains("-force_load") && p.contains("librustline_ffi.a"),
        "podspec: OTHER_LDFLAGS -force_load ${{BUILT_PRODUCTS_DIR}}/librustline_ffi.a \
         (else the unused FFI symbols are dead-stripped and Dart's lookup fails)"
    );
    assert!(
        p.contains("s.dependency Flutter"),
        "podspec: depends on Flutter"
    );
}

/// M11 is arm64-only (Leader): the plugin pod must not build an x86_64 (or i386)
/// simulator slice, else cargokit also builds x86_64-apple-ios, which ios.yml does not
/// install. Every `EXCLUDED_ARCHS[sdk=iphonesimulator*]` lists both; no EXCLUDED_ARCHS
/// entry (simulator, device or unqualified) excludes arm64.
#[test]
fn m11_podspec_excludes_intel_simulator_archs() {
    let p = strip_comments(&read(PODSPEC));
    let mut sim = 0;
    for (i, _) in p.match_indices("EXCLUDED_ARCHS") {
        let rest = &p[i + "EXCLUDED_ARCHS".len()..];
        let key_end = rest.find(['\'', '"']).unwrap_or(rest.len());
        let key = &rest[..key_end];
        let Some((_, after)) = rest.split_once("=>") else {
            continue;
        };
        let after = after.trim_start();
        let q = after.chars().next().unwrap_or(' ');
        let value: Vec<&str> = if q == '\'' || q == '"' {
            after[1..]
                .split(q)
                .next()
                .unwrap_or_default()
                .split_whitespace()
                .collect()
        } else {
            Vec::new()
        };
        assert!(
            !value.contains(&"arm64"),
            "podspec: EXCLUDED_ARCHS{key} excludes arm64 (the only M11 arch)"
        );
        if key.contains("sdk=iphonesimulator") {
            sim += 1;
            assert!(
                value.contains(&"x86_64") && value.contains(&"i386"),
                "podspec: EXCLUDED_ARCHS{key} must list i386 and x86_64 (arm64-only simulator); got {value:?}"
            );
        }
    }
    assert!(
        sim > 0,
        "podspec: pod_target_xcconfig 'EXCLUDED_ARCHS[sdk=iphonesimulator*]' => 'i386 x86_64'"
    );
}

#[test]
fn m11_plugin_uses_cocoapods_not_swiftpm_yet() {
    // Leader/Research: ship M11 on cargokit through Flutter's CocoaPods fallback; a
    // Package.swift would take the SwiftPM path, which frb's cargokit backend lacks.
    // If the team moves to native-assets/SwiftPM, replace this test.
    assert!(root().join(PODSPEC).is_file(), "missing {PODSPEC}");
    for p in [
        "rustline_flutter/ios/rustline_flutter/Package.swift",
        "rustline_flutter/ios/Package.swift",
    ] {
        assert!(
            !root().join(p).exists(),
            "{p} exists: that is the SwiftPM path, not the CocoaPods fallback"
        );
    }
    let pubspec = bare(&strip_comments(&read("rustline_flutter/pubspec.yaml")));
    assert!(
        pubspec.contains("ios: ffiPlugin: true"),
        "plugin pubspec: ios ffiPlugin: true"
    );
    let podfile = bare(&strip_comments(&read(PODFILE)));
    assert!(
        podfile.contains("platform :ios, 15.0"),
        "example Podfile: uncommented `platform :ios, '15.0'`"
    );
}

#[test]
fn m11_example_xcode_project_targets_ios_15() {
    let pbx = read(PBXPROJ);
    let vals: Vec<&str> = pbx
        .lines()
        .filter_map(|l| l.trim().strip_prefix("IPHONEOS_DEPLOYMENT_TARGET = "))
        .map(|v| v.trim_end_matches(';').trim_matches('"'))
        .collect();
    assert!(
        !vals.is_empty(),
        "project.pbxproj sets no IPHONEOS_DEPLOYMENT_TARGET"
    );
    assert!(
        vals.iter().all(|v| *v == "15.0"),
        "every IPHONEOS_DEPLOYMENT_TARGET must be 15.0, got {vals:?}"
    );
}

/// pbxproj objects by id: one-line `ID /* … */ = {…};` or a block up to its `};`.
fn pbx_objects(pbx: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = pbx.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        let t = l.trim_start_matches('\t');
        let id: String = t.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
        if l.starts_with("\t\t") && !l.starts_with("\t\t\t") && id.len() == 24 && t.contains(" = {")
        {
            if t.trim_end().ends_with("};") {
                out.push((id, t.to_string()));
            } else {
                let mut text = t.to_string();
                i += 1;
                while i < lines.len() && lines[i] != "\t\t};" {
                    text.push('\n');
                    text.push_str(lines[i].trim());
                    i += 1;
                }
                out.push((id, text));
            }
        }
        i += 1;
    }
    out
}

/// The ids in `key = ( … );` of an object's text.
fn pbx_list(obj: &str, key: &str) -> Vec<String> {
    let Some((_, r)) = obj.split_once(&format!("{key} = (")) else {
        return Vec::new();
    };
    r.split(");")
        .next()
        .unwrap_or_default()
        .split(',')
        .filter_map(|e| {
            let id: String = e
                .trim()
                .chars()
                .take_while(|c| c.is_ascii_hexdigit())
                .collect();
            (id.len() == 24).then_some(id)
        })
        .collect()
}

/// R54: RunnerTests no longer links the SwiftPM package (with SwiftPM off, integration_test
/// comes through CocoaPods; linking FlutterGeneratedPluginSwiftPackage into the test bundle
/// as well could give it a second copy of the plugin). Runner may still reference it.
#[test]
fn m11_runner_tests_does_not_link_the_swiftpm_package() {
    const PKG: &str = "FlutterGeneratedPluginSwiftPackage";
    let pbx = read(PBXPROJ);
    let objs = pbx_objects(&pbx);
    let get = |id: &str| -> &str {
        objs.iter()
            .find(|(i, _)| i == id)
            .map(|(_, t)| t.as_str())
            .unwrap_or("")
    };
    let target = |name: &str| {
        objs.iter()
            .find(|(_, t)| {
                t.contains("isa = PBXNativeTarget;") && t.contains(&format!("\nname = {name};"))
            })
            .map(|(_, t)| t.clone())
            .unwrap_or_else(|| panic!("project.pbxproj: no {name} native target"))
    };
    let rt = target("RunnerTests");
    target("Runner");
    for p in pbx_list(&rt, "packageProductDependencies") {
        assert!(
            !get(&p).contains(PKG) && !rt.contains(&format!("{p} /* {PKG}")),
            "project.pbxproj: RunnerTests must not depend on {PKG} (packageProductDependencies)"
        );
    }
    for ph in pbx_list(&rt, "buildPhases") {
        let phase = get(&ph);
        for f in pbx_list(phase, "files") {
            let bf = get(&f);
            let refs: Vec<&str> = ["productRef = ", "fileRef = "]
                .iter()
                .filter_map(|k| bf.split_once(k).map(|(_, r)| r.get(..24).unwrap_or("")))
                .collect();
            assert!(
                !bf.contains(PKG)
                    && !phase.contains(&format!("{f} /* {PKG}"))
                    && refs.iter().all(|r| !get(r).contains(PKG)),
                "project.pbxproj: RunnerTests' build phase {ph} links {PKG}"
            );
        }
    }
}

#[test]
fn m11_readme_explains_the_mac_build() {
    let r = read("README.md").to_lowercase();
    let lines: Vec<&str> = r.lines().collect();
    let h = lines
        .iter()
        .position(|l| l.starts_with('#') && l.contains("ios"))
        .expect("README: a heading about iOS (plan M11 Done when: README explains how to build on a Mac)");
    let section = lines[h + 1..]
        .iter()
        .take_while(|l| !l.starts_with('#'))
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    for want in [
        "xcode",
        "15.0",
        "flutter build ios",
        "aarch64-apple-ios-sim",
    ] {
        assert!(
            section.contains(want),
            "README iOS section should mention `{want}`"
        );
    }
}

// ---- guards that hold now (no ignore) -----------------------------------------

/// DoD (strict): "No iOS-only code path in rustline-core; only rustline-ffi/plugin may differ".
#[test]
fn m11_no_apple_only_code_outside_rustline_ffi() {
    let mut hits = Vec::new();
    let crates = root().join("crates");
    for c in std::fs::read_dir(&crates).unwrap() {
        let c = c.unwrap().path();
        let name = c.file_name().unwrap().to_string_lossy().into_owned();
        if name == "rustline-ffi" {
            continue;
        }
        let mut stack = vec![c.join("src")];
        if let Ok(t) = std::fs::read_to_string(c.join("Cargo.toml")) {
            let t: String = t.split_whitespace().collect();
            for bad in [
                "objc2",
                "target_os=\"ios\"",
                "target_vendor=\"apple\"",
                "target_os=\"macos\"",
            ] {
                if t.contains(bad) {
                    hits.push(format!("{name}/Cargo.toml: {bad}"));
                }
            }
        }
        while let Some(d) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else {
                continue;
            };
            for e in rd {
                let p = e.unwrap().path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    let s: String = std::fs::read_to_string(&p)
                        .unwrap()
                        .split_whitespace()
                        .collect();
                    for bad in [
                        "target_os=\"ios\"",
                        "target_os=\"macos\"",
                        "target_vendor=\"apple\"",
                        "target_abi=\"sim\"",
                        "objc2",
                    ] {
                        if s.contains(bad) {
                            hits.push(format!("{}: {bad}", p.display()));
                        }
                    }
                }
            }
        }
    }
    assert!(
        hits.is_empty(),
        "Apple-only code outside rustline-ffi: {hits:#?}"
    );
}

/// Cargokit's iOS path links `lib<libname>.a`, so the FFI crate must stay a staticlib
/// named rustline_ffi (plan M11 step 2: the library-name patch must cover iOS).
#[test]
fn m11_ffi_crate_is_a_staticlib_named_rustline_ffi() {
    let t: String = read("crates/rustline-ffi/Cargo.toml")
        .split_whitespace()
        .collect();
    assert!(t.contains("name=\"rustline_ffi\""), "lib name rustline_ffi");
    let ct = t
        .split("crate-type=[")
        .nth(1)
        .and_then(|r| r.split(']').next())
        .expect("crate-type");
    assert!(
        ct.contains("\"staticlib\""),
        "crate-type must include staticlib for iOS: {ct}"
    );
}

/// The value after `<key>{key}</key>` in a plist, if the key is there.
fn plist_value(plist: &str, key: &str) -> Option<String> {
    let flat: String = plist.split_whitespace().collect();
    let after = flat.split(&format!("<key>{key}</key>")).nth(1)?;
    let v = after.strip_prefix("<string>")?.split("</string>").next()?;
    Some(v.to_string())
}

/// iOS 15.0 must also hold in the example's plists: Runner/Info.plist must exist, and
/// any MinimumOSVersion (Flutter's AppFrameworkInfo.plist has carried one) is 15.0.
#[test]
fn m11_example_info_plists_target_ios_15() {
    let runner = read("rustline_flutter/example/ios/Runner/Info.plist");
    assert!(
        runner.contains("<plist"),
        "Runner/Info.plist is not a plist"
    );
    let mut seen = Vec::new();
    for rel in [
        "rustline_flutter/example/ios/Runner/Info.plist",
        "rustline_flutter/example/ios/Flutter/AppFrameworkInfo.plist",
    ] {
        if let Ok(text) = std::fs::read_to_string(root().join(rel)) {
            if let Some(v) = plist_value(&text, "MinimumOSVersion") {
                seen.push((rel, v));
            }
        }
    }
    assert!(
        seen.iter().all(|(_, v)| v == "15.0"),
        "every MinimumOSVersion must be 15.0, got {seen:?}"
    );
}

/// If M11 adds Apple crates to rustline-ffi (e.g. objc2-foundation for the backup
/// flag), they must sit under an Apple/iOS target table so Android and Linux builds
/// never pull them. Passes today (no such crates).
#[test]
fn m11_apple_crates_in_ffi_are_target_gated() {
    const APPLE: [&str; 4] = ["objc2", "core-foundation", "security-framework", "block2"];
    let gated = |section: &str| {
        section.starts_with("[target.")
            && ["ios", "macos", "apple"]
                .iter()
                .any(|t| section.contains(t))
            && !section.contains("not(")
    };
    let toml = read("crates/rustline-ffi/Cargo.toml");
    let mut section = String::new();
    let mut bad = Vec::new();
    for line in toml.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            section = l.replace(['"', '\'', ' '], "");
            // `[dependencies.objc2]` / `[target.x.dependencies.objc2]` tables.
            let dep = section.split("dependencies.").nth(1).unwrap_or("");
            if APPLE.iter().any(|c| dep.starts_with(c)) && !gated(&section) {
                bad.push(section.clone());
            }
            continue;
        }
        if APPLE.iter().any(|c| l.starts_with(c)) && !gated(&section) {
            bad.push(format!("{section} {l}"));
        }
    }
    assert!(
        bad.is_empty(),
        "Apple crates outside an Apple target table: {bad:?}"
    );
}

/// PR #15 Xcode link failure: cargokit's build_pod.dart lipo'd the static lib to
/// `lib<package name>.a` (librustline-ffi.a), while cargo writes librustline_ffi.a and
/// the podspec -force_loads `${BUILT_PRODUCTS_DIR}/librustline_ffi.a`. This replays
/// build_pod.dart's name: it takes the `"lib$<var>.a"` output, follows `<var>` back to
/// `environment.crateInfo.packageName` plus any `.replaceAll('x', 'y')` calls, applies
/// them to the package name in the Cargo.toml that build_pod.sh is pointed at, and
/// requires the podspec's -force_load and output_files (and ios.yml's size step) to
/// name that same file.
#[test]
fn m11_build_pod_lib_name_matches_podspec_force_load() {
    const BUILD_POD: &str = "rustline_flutter/cargokit/build_tool/lib/src/build_pod.dart";
    let dart = read(BUILD_POD);
    let code: String = dart
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");

    // The output file: path.join(outputDir, "lib$libName.a").
    let var = code
        .split("\"lib$")
        .skip(1)
        .find_map(|rest| rest.split_once(".a\"").map(|(v, _)| v.to_string()))
        .unwrap_or_else(|| panic!("{BUILD_POD}: no \"lib$<name>.a\" output file"));
    let decl = format!("final {var} =");
    let expr = code
        .split_once(&decl)
        .and_then(|(_, rest)| rest.split_once(';'))
        .map(|(e, _)| e.split_whitespace().collect::<String>())
        .unwrap_or_else(|| panic!("{BUILD_POD}: no `final {var} = ...;`"));
    let mut chain = expr
        .strip_prefix("environment.crateInfo.packageName")
        .unwrap_or_else(|| {
            panic!("{BUILD_POD}: {var} = {expr}, expected environment.crateInfo.packageName...")
        });

    // The package name build_pod.dart reads: [package] name of the manifest dir that
    // the podspec passes to build_pod.sh (relative to rustline_flutter/ios).
    let p = bare(&strip_comments(&read(PODSPEC)));
    let manifest_dir = p
        .split("build_pod.sh")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .map(|d| d.trim_matches(|c| c == '"' || c == '\'' || c == '\\'))
        .unwrap_or_else(|| panic!("podspec: no build_pod.sh <manifest dir>"));
    let cargo_toml = format!("rustline_flutter/ios/{manifest_dir}/Cargo.toml");
    let toml = read(&cargo_toml);
    let mut in_package = false;
    let mut package = None;
    for line in toml.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_package = l == "[package]";
        } else if in_package && l.starts_with("name") {
            package = l.split('"').nth(1).map(str::to_string);
            break;
        }
    }
    let package = package.unwrap_or_else(|| panic!("{cargo_toml}: no [package] name"));

    let mut lib = package.clone();
    while !chain.is_empty() {
        let args = chain
            .strip_prefix(".replaceAll(")
            .and_then(|r| r.split_once(')'))
            .unwrap_or_else(|| panic!("{BUILD_POD}: cannot replay `{chain}` in {var} = {expr}"));
        let quoted: Vec<&str> = args.0.split(['\'', '"']).collect();
        // ['', from, ',', to, ''] for ('from','to').
        assert!(
            quoted.len() == 5 && quoted[2] == ",",
            "{BUILD_POD}: unexpected replaceAll args ({})",
            args.0
        );
        lib = lib.replace(quoted[1], quoted[3]);
        chain = args.1;
    }
    let built = format!("lib{lib}.a");
    let cargo_writes = format!("lib{}.a", package.replace('-', "_"));
    assert_eq!(
        built, cargo_writes,
        "{BUILD_POD} writes {built} but cargo builds {cargo_writes} \
         (use packageName.replaceAll('-', '_'))"
    );

    let force_load = p
        .split("-force_load")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .map(|f| f.trim_matches(|c| c == '"' || c == '\'' || c == ','))
        .unwrap_or_else(|| panic!("podspec: no -force_load <file>"));
    let force_load_file = force_load.rsplit('/').next().unwrap_or(force_load);
    assert_eq!(
        force_load_file, built,
        "podspec -force_load {force_load} must name the file build_pod.dart writes ({built})"
    );
    assert!(
        p.contains(&format!("${{BUILT_PRODUCTS_DIR}}/{built}")),
        "podspec output_files/-force_load: ${{BUILT_PRODUCTS_DIR}}/{built}"
    );
    assert!(
        bare(&strip_comments(&read(IOS_YML))).contains(&format!("rustline_flutter/{built}")),
        "ios.yml size step must look for rustline_flutter/{built}"
    );
}

// ---- XCTest host environment (CI run 40500485826) ------------------------------
//
// Dart's `Platform.environment` is always empty on iOS (dart-lang/sdk
// runtime/bin/platform_macos.cc), so HOME and the simulator's variables must come
// from the example's AppDelegate over the `rustline_m11_test` channel.

const APP_DELEGATE: &str = "rustline_flutter/example/ios/Runner/AppDelegate.swift";
const M11_ADAPTER: &str = "rustline_flutter/example/integration_test/m11_adapter.dart";
const M11_IOS_TEST: &str = "rustline_flutter/example/integration_test/m11_ios_test.dart";

/// Drops `//` comments (Swift / Dart) outside string literals; keeps line breaks.
fn strip_slash_comments(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let b = line.as_bytes();
        let (mut q, mut cut) = (None::<u8>, line.len());
        let mut i = 0;
        while i < b.len() {
            match (q, b[i]) {
                (None, b'"' | b'\'') => q = Some(b[i]),
                (Some(c), x) if x == c => q = None,
                (Some(_), b'\\') => i += 1,
                (None, b'/') if b.get(i + 1) == Some(&b'/') => {
                    cut = i;
                    break;
                }
                _ => {}
            }
            i += 1;
        }
        out.push_str(&line[..cut]);
        out.push('\n');
    }
    out
}

/// Body lines of `case "<name>":` in a Swift switch, up to the next `case`/`default:`
/// at the same indent.
fn swift_case(src: &str, name: &str) -> Vec<String> {
    let lines: Vec<&str> = src.lines().collect();
    let head = format!("case \"{name}\":");
    let starts: Vec<usize> = (0..lines.len())
        .filter(|&i| lines[i].trim() == head)
        .collect();
    assert_eq!(starts.len(), 1, "{APP_DELEGATE}: need exactly one `{head}`");
    let at = indent(lines[starts[0]]);
    lines[starts[0] + 1..]
        .iter()
        .take_while(|l| {
            let t = l.trim();
            !(indent(l) <= at && (t.starts_with("case ") || t == "default:" || t == "}"))
        })
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

/// Lines of the `{ … }` block opened on line `i` (exclusive), each with its depth
/// relative to that block (1 = directly inside).
fn brace_block(lines: &[&str], i: usize) -> Vec<(usize, String)> {
    let mut depth = 0i32;
    let mut out = Vec::new();
    for (k, l) in lines[i..].iter().enumerate() {
        let before = depth;
        depth += l.matches('{').count() as i32 - l.matches('}').count() as i32;
        if k > 0 {
            if depth <= 0 {
                break;
            }
            out.push((before.min(depth).max(1) as usize, l.trim().to_string()));
        }
    }
    out
}

#[test]
fn m11_app_delegate_exposes_the_xctest_host_facts() {
    let src = strip_slash_comments(&read(APP_DELEGATE));
    assert!(
        src.contains("FlutterMethodChannel(name: \"rustline_m11_test\""),
        "{APP_DELEGATE}: the rustline_m11_test channel"
    );
    assert!(
        src.contains("let env = ProcessInfo.processInfo.environment"),
        "{APP_DELEGATE}: env = ProcessInfo.processInfo.environment"
    );
    let ex = swift_case(&src, "isExcludedFromBackup").join("\n");
    assert!(
        ex.contains(".resourceValues(forKeys: [.isExcludedFromBackupKey])")
            && ex.contains("result(values.isExcludedFromBackup ?? false)"),
        "isExcludedFromBackup must still read Apple's resource value"
    );
    assert_eq!(
        swift_case(&src, "homeDirectory"),
        ["result(NSHomeDirectory())"],
        "homeDirectory must answer NSHomeDirectory() only"
    );
    assert_eq!(
        swift_case(&src, "simulatorDeviceName"),
        ["result(env[\"SIMULATOR_DEVICE_NAME\"])"],
        "simulatorDeviceName: the simulator's variable only (nil when unset), no UIDevice fallback"
    );
    assert_eq!(
        swift_case(&src, "runtimeVersion"),
        ["result(env[\"SIMULATOR_RUNTIME_VERSION\"])"],
        "runtimeVersion: the simulator's variable only (nil when unset), no UIDevice fallback"
    );
    let di = swift_case(&src, "deviceInfo");
    let sim = [
        "#if targetEnvironment(simulator)",
        "let isSimulator = true",
        "#else",
        "let isSimulator = false",
        "#endif",
    ];
    assert!(
        di.windows(sim.len()).any(|w| w == sim),
        "deviceInfo: isSimulator from #if targetEnvironment(simulator) (true) / #else (false)"
    );
    assert!(
        di.iter()
            .any(|l| l == "let raw: (String) -> Any = { key in env[key] ?? NSNull() }"),
        "deviceInfo: raw env values (NSNull when unset), no fallback"
    );
    for (key, val) in [
        ("isSimulator", "isSimulator"),
        ("simulatorDeviceName", "raw(\"SIMULATOR_DEVICE_NAME\")"),
        (
            "simulatorModelIdentifier",
            "raw(\"SIMULATOR_MODEL_IDENTIFIER\")",
        ),
        (
            "simulatorRuntimeVersion",
            "raw(\"SIMULATOR_RUNTIME_VERSION\")",
        ),
        ("simulatorUdid", "raw(\"SIMULATOR_UDID\")"),
        ("deviceModel", "UIDevice.current.model"),
        ("systemName", "UIDevice.current.systemName"),
        ("systemVersion", "UIDevice.current.systemVersion"),
    ] {
        let want = format!("\"{key}\": {val},");
        assert_eq!(
            di.iter()
                .filter(|l| l.starts_with(&format!("\"{key}\":")))
                .collect::<Vec<_>>(),
            [&want],
            "deviceInfo: {want}"
        );
    }
    assert!(
        di.iter().any(|l| l == "result(info)"),
        "deviceInfo: result(info)"
    );
}

#[test]
fn m11_dart_harness_reads_host_facts_from_the_channel() {
    for rel in [M11_ADAPTER, M11_IOS_TEST] {
        let src = strip_slash_comments(&read(rel));
        assert!(
            !src.contains("Platform.environment"),
            "{rel}: Platform.environment is always empty on iOS; use the channel"
        );
    }
    let a = strip_slash_comments(&read(M11_ADAPTER));
    let fa = flat(&a);
    for want in [
        "Future<Directory> appSupportOracle() async => Directory('${await m11HomeDirectory()}/Library/Application Support');",
        "Future<String> m11HomeDirectory() => _m11String('homeDirectory');",
        "Future<String> m11SimulatorDeviceName() => _m11String('simulatorDeviceName');",
        "Future<String> m11RuntimeVersion() => _m11String('runtimeVersion');",
        "final m = await _m11Channel.invokeMapMethod<String, Object?>('deviceInfo'); if (m == null || m.isEmpty) { throw StateError(",
        "final v = await _m11Channel.invokeMethod<String>(method); if (v == null || v.isEmpty) { throw StateError(",
        "const _m11Channel = MethodChannel('rustline_m11_test');",
    ] {
        assert!(fa.contains(want), "{M11_ADAPTER}: `{want}`");
    }
    // The channel helpers must not swallow a missing handler or a null answer.
    let lines: Vec<&str> = a.lines().collect();
    for head in [
        "Future<String> _m11String(String method) async {",
        "Future<Map<String, Object?>> m11DeviceInfo() async {",
    ] {
        let i = lines
            .iter()
            .position(|l| l.trim() == head)
            .unwrap_or_else(|| panic!("{M11_ADAPTER}: `{head}`"));
        let body: Vec<String> = brace_block(&lines, i).into_iter().map(|(_, l)| l).collect();
        for l in &body {
            assert!(
                !l.contains("catch")
                    && !l.starts_with("} on ")
                    && !l.starts_with("try")
                    && !(l.contains("?? ") && !l.starts_with("throw ")),
                "{M11_ADAPTER}: `{head}` must not catch or default: `{l}`"
            );
        }
        assert!(
            body.iter().any(|l| l.starts_with("throw StateError(")),
            "{M11_ADAPTER}: `{head}` must throw on null/empty"
        );
    }
    assert_eq!(
        fa.matches("await m11HomeDirectory()").count(),
        1,
        "appSupportOracle only"
    );
}

/// A hard-coded simulator name or model id ('iPhone 17', iPhone18,3): the gate is
/// the booted simulator's UDID instead (runner images change, R56).
fn names_a_fixed_iphone(l: &str) -> bool {
    let b = l.as_bytes();
    l.match_indices("iPhone").any(|(i, _)| {
        let rest = &b[i + 6..];
        let rest = rest.strip_prefix(b" ").unwrap_or(rest);
        rest.first().is_some_and(|c| c.is_ascii_digit())
    })
}

#[test]
fn m11_ios_test_asserts_the_booted_simulator() {
    let src = strip_slash_comments(&read(M11_IOS_TEST));
    let lines: Vec<&str> = src.lines().collect();
    let t = lines
        .iter()
        .position(|l| {
            l.contains("testWidgets('runs on an arm64 iOS simulator (records the device)'")
        })
        .expect("the device test");
    let body = brace_block(&lines, t);
    let joined = body
        .iter()
        .map(|(_, l)| l.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        body.iter()
            .any(|(d, l)| *d == 1 && l == "final dev = await m11DeviceInfo();"),
        "device test: `final dev = await m11DeviceInfo();`"
    );
    for want in [
        "name=${dev['simulatorDeviceName']}",
        "model=${dev['simulatorModelIdentifier']}",
        "udid=${dev['simulatorUdid']}",
    ] {
        assert!(joined.contains(want), "device test must log `{want}`");
    }
    // No way out before the CI block, and skipped only off iOS / before M11.
    for (_, l) in &body {
        assert!(
            !l.split(|c: char| !c.is_alphanumeric())
                .any(|w| w == "return"),
            "device test must not return early: `{l}`"
        );
        assert!(
            !names_a_fixed_iphone(l),
            "device test: no hard-coded simulator name/model (gate on the UDID): `{l}`"
        );
    }
    let close = lines.get(t + body.len() + 1).map(|l| l.trim());
    assert_eq!(
        close,
        Some("}, skip: m11SkipReason != null);"),
        "device test: skip only on m11SkipReason"
    );
    let rp = t + lines[t..]
        .iter()
        .position(|l| l.trim() == "if (m11RequirePack) {")
        .expect("device test: if (m11RequirePack) {");
    // Directly inside the CI block, not under another condition, in this order.
    let ci: Vec<String> = brace_block(&lines, rp)
        .into_iter()
        .filter(|(d, _)| *d == 1)
        .map(|(_, l)| l)
        .collect();
    let mut at = 0;
    for want in [
        "expect(abi, Abi.iosArm64);",
        "expect(dev['isSimulator'], isTrue);",
        "expect(m11SimUdid, isNotEmpty",
        "expect(dev['simulatorUdid'], m11SimUdid);",
        "expect(dev['simulatorDeviceName'], allOf(isA<String>(), isNotEmpty));",
        "expect(dev['simulatorModelIdentifier'], allOf(isA<String>(), isNotEmpty));",
        "expect(dev['simulatorRuntimeVersion'], allOf(isA<String>(), isNotEmpty));",
        "expect(await m11SimulatorDeviceName(), dev['simulatorDeviceName']);",
        "final runtime = await m11RuntimeVersion();",
        "expect(runtime, dev['simulatorRuntimeVersion']);",
        "expect(dev['systemName'], 'iOS');",
        "expect(majorMinor(runtime), majorMinor(dev['systemVersion']! as String));",
        "expect(runtime, matches(RegExp(r'^\\d+\\.\\d+(\\.\\d+)?$')));",
        "expect(int.parse(runtime.split('.').first), greaterThanOrEqualTo(26));",
    ] {
        let k = ci[at..]
            .iter()
            .position(|l| {
                if want.ends_with(';') {
                    l == want
                } else {
                    l.starts_with(&format!("{want},")) || l.starts_with(&format!("{want})"))
                }
            })
            .unwrap_or_else(|| {
                panic!("device test, directly in `if (m11RequirePack)` (in order): `{want}`")
            });
        at += k + 1;
    }
    // The UDID check is never conditional (an unset define fails, not skips).
    for l in src.lines() {
        let l = l.trim();
        assert!(
            !((l.starts_with("if ") || l.contains("? ") || l.contains("&&") || l.contains("||"))
                && l.contains("m11SimUdid")),
            "{M11_IOS_TEST}: the UDID check must not be conditional: `{l}`"
        );
    }
    assert!(
        flat(&src).contains("String majorMinor(String v) => v.split('.').take(2).join('.');"),
        "majorMinor: the first two version fields"
    );
    let a = flat(&strip_slash_comments(&read(M11_ADAPTER)));
    assert!(
        a.contains("const m11SimUdid = String.fromEnvironment('RUSTLINE_M11_SIM_UDID');"),
        "{M11_ADAPTER}: m11SimUdid from RUSTLINE_M11_SIM_UDID"
    );
}

/// The `id:` of a workflow step, if any.
fn step_id(step: &str) -> Option<String> {
    step.lines()
        .map(|l| l.trim().trim_start_matches("- ").trim())
        .find_map(|l| l.strip_prefix("id:").map(|v| v.trim().to_string()))
}

#[test]
fn m11_ios_yml_passes_the_booted_sim_udid_to_the_app() {
    let y = strip_comments(&read(IOS_YML));
    let (id, body) = sim_job(&y);
    let st = steps(&body);
    let boot = st
        .iter()
        .position(|s| s.contains("futureware-tech/simulator-action@"))
        .unwrap_or_else(|| panic!("job {id}: the simulator-action boot step"));
    let sim = step_id(&st[boot]).unwrap_or_else(|| panic!("job {id}: the boot step needs an id"));
    let want_env = format!("UDID: ${{{{ steps.{sim}.outputs.udid }}}}");
    let mut with_cfg = Vec::new();
    for (k, s) in st.iter().enumerate() {
        let sh = shell_lines(s);
        let cfg: Vec<&String> = sh
            .iter()
            .filter(|l| l.contains("flutter build ios --config-only"))
            .collect();
        if cfg.is_empty() {
            continue;
        }
        with_cfg.push(k);
        assert!(k > boot, "job {id}: boot the simulator before step {k}");
        for l in &cfg {
            let defs: Vec<&str> = l
                .split_whitespace()
                .filter(|w| w.contains("RUSTLINE_M11_SIM_UDID"))
                .collect();
            assert!(
                defs == ["--dart-define=RUSTLINE_M11_SIM_UDID=$UDID"]
                    || defs == ["--dart-define=RUSTLINE_M11_SIM_UDID=${UDID}"],
                "job {id}: every `flutter build ios --config-only` line must pass \
                 --dart-define=RUSTLINE_M11_SIM_UDID=\"$UDID\" once; got {defs:?} in `{l}`"
            );
        }
        // $UDID is the booted simulator's, set on this step and not reassigned.
        assert_eq!(
            s.lines().filter(|l| l.trim() == want_env).count(),
            1,
            "job {id}: step {k} needs env `{want_env}`"
        );
        for l in &sh {
            for w in l.split(|c: char| c.is_whitespace() || c == ';' || c == '&' || c == '|') {
                assert!(
                    !(w.starts_with("UDID=") || w == "UDID"),
                    "job {id}: step {k} must not reassign UDID: `{l}`"
                );
            }
        }
    }
    let names: Vec<String> = with_cfg
        .iter()
        .map(|&k| {
            st[k]
                .lines()
                .find_map(|l| {
                    l.trim()
                        .strip_prefix("- name:")
                        .map(|n| n.trim().to_string())
                })
                .unwrap_or_default()
        })
        .collect();
    assert_eq!(
        names,
        ["Warm-up build", "Integration tests"],
        "job {id}: the config-only lines are the warm-up's and test_file's"
    );
}
