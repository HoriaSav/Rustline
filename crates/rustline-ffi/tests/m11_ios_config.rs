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

fn runs_integration_tests(step: &str) -> bool {
    step.contains("flutter test integration_test") || step.contains("flutter drive")
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
        t.contains(" -d ") || t.contains(" --device-id"),
        "job {id}: pass the simulator to flutter with -d (udid or name), else flutter picks any device"
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

#[test]
fn m11_ios_yml_runs_the_integration_test_with_a_real_pack() {
    let y = read(IOS_YML);
    let (id, body) = sim_job(&y);
    let ss: Vec<String> = steps(&body).iter().map(|s| bare(s)).collect();
    let test = ss.iter().position(|s| runs_integration_tests(s)).unwrap();
    let t = &ss[test];
    assert!(
        t.contains("flutter test integration_test ") || t.ends_with("flutter test integration_test")
            || t.contains("flutter test integration_test/ "),
        "job {id}: run the whole integration_test dir, so the M6 panic/error-code tests run on iOS too \
         (DoD: same FFI tests as Android)"
    );
    assert!(
        t.contains("RUSTLINE_M11_PACK_ZST="),
        "job {id}: pass --dart-define=RUSTLINE_M11_PACK_ZST=<.rlp.zst> (m11_ios_test.dart)"
    );
    assert!(
        t.contains("RUSTLINE_M11_REQUIRE_PACK=true"),
        "job {id}: pass --dart-define=RUSTLINE_M11_REQUIRE_PACK=true so a missing pack fails \
         instead of skipping (else the job is green with nothing tested)"
    );
    let pack = ss
        .iter()
        .position(|s| s.contains("build-pack") && s.contains("data/fixtures/tiny"));
    assert!(
        pack.is_some_and(|p| p < test),
        "job {id}: build the tiny fixture pack (rustline build-pack --gtfs data/fixtures/tiny) before the test"
    );
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
