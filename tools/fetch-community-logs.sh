#!/usr/bin/env sh
# Download extra community test logs that are not committed to the repository
# (they live in GPL-licensed repositories, so we only reference them).
set -eu
dir="$(dirname "$0")/../tests/logs/external"
mkdir -p "$dir"
base="https://raw.githubusercontent.com/gimbal-ghost/gimbal-ghost/main/test"
for f in rtfl_001.bbl emuf_001.bbl LOG00001.BFL btfl_001.bbl btfl_002.bbl; do
  if [ ! -f "$dir/$f" ]; then
    echo "fetching $f"
    curl -fsSL "$base/$f" -o "$dir/$f"
  fi
done
echo "done: $(ls "$dir" | wc -l) files in $dir"
