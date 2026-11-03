#!/usr/bin/env bash
# Per-change test selection for ci.yml, android.yml and ios.yml.
# Reads changed paths (one per line) on stdin and prints GitHub outputs:
#   full=true|false   everything runs
#   rust=true|false   any Rust crate to test (ci rust and msrv jobs)
#   cargo_args=...    `--workspace` or `-p <crate> ...` for clippy and test
#   ffi=true|false    rustline-ffi tests (ios ffi job)
#   flutter=true|false  plugin jobs: ci flutter, android, ios device
#   ios=true|false    ios simulator job (also needs the CLI for the tiny pack)
# `--full` (workflow_dispatch, push to main, a new branch) selects everything
# unless every changed path is docs; with no stdin at all it is always full.
#
# A changed crate selects itself and every crate that depends on it, dev
# dependencies included (their tests use it). The graph comes from
# `cargo metadata`, so it follows the Cargo.toml files. The plugin
# (rustline_flutter) links rustline-ffi, so it is selected when rustline-ffi
# or a normal (non-dev) dependency of it changes.
#
# Full suite: Cargo.toml, Cargo.lock, rust-toolchain*, .github/workflows/**,
# this script, and any path not listed below (data/, fixtures/, scripts/,
# benches/, clippy.toml, ...), so an unknown input never skips tests.
# Docs only (*.md, Documentation/**, LICENSE): nothing runs.
set -euo pipefail

force_full=false
[ "${1:-}" = "--full" ] && force_full=true

meta=$(cargo metadata --locked --no-deps --format-version 1)
root=$(jq -r .workspace_root <<<"$meta")
# "dir<TAB>name" per crate, e.g. "crates/rustline-core<TAB>rustline-core".
crates=$(jq -r --arg r "$root/" '.packages[] | "\(.manifest_path | ltrimstr($r) | rtrimstr("/Cargo.toml"))\t\(.name)"' <<<"$meta")

full=$force_full
docs_only=true
any=false
declare -A changed=()
while IFS= read -r p; do
  [ -n "$p" ] || continue
  any=true
  case "$p" in
    *.md | Documentation/* | LICENSE | */LICENSE) continue ;;
  esac
  docs_only=false
  case "$p" in
    rustline_flutter/*) changed[rustline_flutter]=1; continue ;;
  esac
  hit=""
  while IFS=$'\t' read -r dir name; do
    case "$p" in
      "$dir"/Cargo.toml | "$dir"/*) hit=$name ;;
    esac
  done <<<"$crates"
  if [ -n "$hit" ]; then changed[$hit]=1; else full=true; fi
done
$any || { docs_only=false; full=true; }

# Reverse closure: crates that (transitively) depend on any of "$@".
# $1 = "all" (dev dependencies count) or "normal".
dependents() {
  local kind=$1; shift
  jq -r --arg kind "$kind" --args '
    [.packages[] | {name, deps: [.dependencies[] | select(.path)
      | select($kind == "all" or .kind == null) | .name]}] as $g
    | def up($s): ([$g[] | select(any(.deps[]; . as $d | $s | index($d))) | .name] + $s | unique)
        as $n | if $n == $s then $s else up($n) end;
    up($ARGS.positional) | .[]' <<<"$meta" "$@"
}

if $docs_only; then
  full=false
  sel=""
elif $full; then
  sel=$(jq -r '.packages[].name' <<<"$meta")
else
  sel=$(dependents all "${!changed[@]}" | grep -v '^rustline_flutter$' || true)
fi
has() { grep -qx "$1" <<<"$sel"; }

if $full; then
  flutter=true ios=true
elif $docs_only; then
  flutter=false ios=false
else
  norm=$(dependents normal "${!changed[@]}")
  flutter=false ios=false
  if [ -n "${changed[rustline_flutter]:-}" ] || grep -qx rustline-ffi <<<"$norm"; then flutter=true; fi
  if $flutter || grep -qx rustline-cli <<<"$norm"; then ios=true; fi
fi

if $full; then args="--workspace"; elif [ -n "$sel" ]; then args=$(sed "s/^/-p /" <<<"$sel" | sort | paste -sd" " -); else args=""; fi
rust=false; [ -n "$sel" ] && rust=true
ffi=false; has rustline-ffi && ffi=true

echo "full=$full"
echo "rust=$rust"
echo "cargo_args=$args"
echo "ffi=$ffi"
echo "flutter=$flutter"
echo "ios=$ios"
