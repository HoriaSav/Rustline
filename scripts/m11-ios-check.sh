#!/usr/bin/env bash
# M11 (iOS) checks that run on Linux, without a Mac (Tester, 2026-10-26).
# What this proves and what it cannot: see the Tester note
# "2026-10-26 Rustline M11 tests-first (Tester)". The simulator run, the real backup
# flag, linking, Xcode 26.6 and the recorded sizes only happen in ios.yml on macOS.
#
# Steps:
#   1. actionlint (+ shellcheck if installed) on .github/workflows/ios.yml
#   2. the static M11 config tests (cargo test --test m11_ios_config --include-ignored)
#   3. cargo check + clippy -D warnings of rustline-ffi for aarch64-apple-ios and
#      aarch64-apple-ios-sim. There is no Apple SDK here, so the C parts (zstd-sys,
#      dart-sys) go through a stub compiler that writes empty objects: Rust code is
#      fully type-checked for the iOS targets, C code is not, and nothing links.
#   4. a link attempt, expected to fail on Linux (reported, not fatal)
#   5. the plugin's M11 storage unit tests and flutter analyze (plugin + example)
# Run one at a time (one cargo/flutter process). Exit code = number of failed steps.
set -uo pipefail
cd "$(dirname "$0")/.." || exit 99
export PATH="$HOME/.local/bin:$HOME/development/flutter/bin:$PATH"
TARGETS=(aarch64-apple-ios aarch64-apple-ios-sim)
fails=0
step() { echo; echo "== $*"; }
ok() { echo "PASS: $*"; }
ko() { echo "FAIL: $*"; fails=$((fails + 1)); }
check() { # name cmd...
  local name=$1
  shift
  if "$@"; then ok "$name"; else ko "$name"; fi
}

step "1. actionlint .github/workflows/ios.yml"
if [ ! -f .github/workflows/ios.yml ]; then
  ko "ios.yml missing"
elif ! command -v actionlint >/dev/null; then
  ko "actionlint not installed (no sudo needed: release binary into ~/.local/bin)"
else
  check actionlint actionlint .github/workflows/ios.yml
fi

step "2. static M11 config tests"
check m11_ios_config cargo test --locked -p rustline-ffi --test m11_ios_config -- --include-ignored

step "3. cargo check + clippy for the Apple targets (check-only)"
stub=$(mktemp -d)
trap 'rm -rf "$stub"' EXIT
mkdir -p "$stub/sdk"
cat >"$stub/cc" <<'STUB'
#!/bin/sh
# Check-only stub: no C is compiled; an empty file is written where the object goes.
out=""; prev=""
for a in "$@"; do
  [ "$prev" = "-o" ] && out="$a"
  case "$a" in -Fo*) out="${a#-Fo}";; esac
  prev="$a"
done
[ -n "$out" ] && : >"$out"
exit 0
STUB
chmod +x "$stub/cc"
apple() { # target cmd... : run cmd with the stub toolchain for target
  local u=${1//-/_}
  shift
  env SDKROOT="$stub/sdk" IPHONEOS_DEPLOYMENT_TARGET=15.0 CARGO_TARGET_DIR=target/m11-applestub \
    "CC_$u=$stub/cc" "CXX_$u=$stub/cc" "AR_$u=llvm-ar" "$@"
}
for t in "${TARGETS[@]}"; do
  if ! rustup target list --installed | grep -qx "$t"; then
    rustup target add "$t" || { ko "rustup target add $t"; continue; }
  fi
  check "cargo check $t" apple "$t" cargo check --locked -p rustline-ffi --target "$t"
  check "clippy $t" apple "$t" cargo clippy --locked -p rustline-ffi --all-targets --target "$t" -- -D warnings
done

step "4. link attempt (expected to fail without the Apple SDK)"
if apple aarch64-apple-ios-sim cargo build --locked -p rustline-ffi --lib \
  --target aarch64-apple-ios-sim >/dev/null 2>&1; then
  echo "NOTE: linked on this machine (unexpected on Linux)"
else
  echo "NOTE: does not link here (expected): linking is confirmed only on macOS CI"
fi

step "5. plugin M11 storage unit tests + analyze"
check m11_storage_test bash -c 'cd rustline_flutter && flutter test test/m11_storage_test.dart'
check "analyze plugin" bash -c 'cd rustline_flutter && flutter analyze'
check "analyze example" bash -c 'cd rustline_flutter/example && flutter analyze'

echo; echo "M11 Linux checks: $fails failed step(s)"
exit "$fails"
