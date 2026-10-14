#!/usr/bin/env bash
# Download (if missing) and verify the shared GTFS test feeds listed in fixtures/manifest.tsv.
# Usage: scripts/fetch-fixtures.sh            fetch missing files, then verify all
#        scripts/fetch-fixtures.sh --verify   only verify, never download
# Upstream "latest" URLs change over time. If a download no longer matches the pinned
# sha256, it is kept as <file>.new and the script fails; update the manifest on purpose
# (new sha256, bytes, date) instead of silently using a different feed.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
manifest="$root/fixtures/manifest.tsv"
dir="$root/fixtures/feeds"
verify_only=0; [[ "${1:-}" == "--verify" ]] && verify_only=1
mkdir -p "$dir"
fail=0
while IFS=$'\t' read -r name file url sha bytes date note licence; do
  [[ -z "$name" || "$name" == \#* || "$name" == name ]] && continue
  path="$dir/$file"
  if [[ ! -f "$path" ]]; then
    if (( verify_only )) || [[ "$url" == "-" ]]; then
      echo "MISSING  $name ($file)${url:+ url=$url}"; fail=1; continue
    fi
    echo "FETCH    $name <- $url"
    if ! curl --proto '=https' -fsSL --retry 3 -o "$path.part" "$url"; then
      echo "ERROR    $name: download failed"; rm -f "$path.part"; fail=1; continue
    fi
    got="$(sha256sum "$path.part" | cut -d' ' -f1)"
    if [[ "$got" != "$sha" ]]; then
      mv "$path.part" "$path.new"
      echo "CHANGED  $name: upstream sha256 $got != pinned $sha (saved as $file.new)"; fail=1; continue
    fi
    mv "$path.part" "$path"
  fi
  got="$(sha256sum "$path" | cut -d' ' -f1)"
  if [[ "$got" == "$sha" ]]; then echo "OK       $name ($file)"
  else echo "BAD      $name: sha256 $got != $sha"; fail=1; fi
done < "$manifest"
exit $fail
