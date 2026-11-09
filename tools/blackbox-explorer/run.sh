#!/usr/bin/env sh
# Reference data from the Blackbox Explorer parser for the conformance test
# crates/openpid-cli/tests/explorer_conformance.rs (see README.md in this directory).
#
# Usage, from anywhere in the repository:
#   sh tools/blackbox-explorer/run.sh <betaflight/blackbox-log-viewer checkout> \
#                                     <rotorflight/rotorflight-blackbox checkout>
#
# Needs Node.js >= 20.6 and npm (for semver, Explorer's one parser dependency).
# Writes tests/logs/explorer/<fixture>.json for every Blackbox fixture: Rotorflight
# logs go through the Rotorflight fork, everything else through Betaflight's Explorer.
# Re-running replaces the previous output.
set -eu

betaflight="${1:-}"
rotorflight="${2:-}"
if [ -z "$betaflight" ] || [ ! -f "$betaflight/src/flightlog_parser.js" ] ||
   [ -z "$rotorflight" ] || [ ! -f "$rotorflight/js/flightlog_parser.js" ]; then
  echo "usage: sh tools/blackbox-explorer/run.sh <betaflight/blackbox-log-viewer checkout> <rotorflight/rotorflight-blackbox checkout>" >&2
  exit 2
fi
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
betaflight="$(cd "$betaflight" && pwd)"
rotorflight="$(cd "$rotorflight" && pwd)"

node -e 'const [a, b] = process.versions.node.split(".").map(Number); process.exit(a > 20 || (a === 20 && b >= 6) ? 0 : 1)' ||
  { echo "Node.js >= 20.6 is needed (module.register)" >&2; exit 1; }
(cd "$here" && npm ci --no-audit --no-fund --silent)

describe() {
  printf '%s %s (%s)' "$1" "$(git -C "$2" rev-parse --short HEAD 2>/dev/null || echo unknown)" \
    "$(git -C "$2" log -1 --format=%cs 2>/dev/null || echo unknown)"
}
bf_label="$(describe betaflight/blackbox-log-viewer "$betaflight"), 32-bit wrap of PR #862 restored"
rf_label="$(describe rotorflight/rotorflight-blackbox "$rotorflight")"

cd "$root"
out=tests/logs/explorer
mkdir -p "$out"
rm -f "$out"/*.json
for log in tests/logs/*.bbl tests/logs/*.BBL tests/logs/*.bfl tests/logs/*.BFL \
           tests/logs/*.txt tests/logs/*.TXT; do
  [ -f "$log" ] || continue
  if head -c 40 "$log" | grep -q 'git-lfs'; then
    echo "$log is a Git LFS pointer: run 'git lfs install && git lfs pull' first" >&2
    exit 1
  fi
  name="$(basename "$log")"
  if grep -a -m 1 -q 'H Firmware revision:Rotorflight' "$log"; then
    node "$here/rotorflight.mjs" "$rotorflight" "$rf_label" "$out/$name.json" "$log"
  else
    node "$here/betaflight.mjs" "$betaflight" "$bf_label" "$out/$name.json" "$log"
  fi
done
echo
echo "Betaflight:  $bf_label"
echo "Rotorflight: $rf_label"
echo "next: cargo test -p openpid-cli --test explorer_conformance, then git add $out"
