#!/usr/bin/env sh
# Regenerate docs/screenshots/*.png: one PNG per GUI tab, taken on a virtual
# X display (Xvfb) with ImageMagick's `import`. Run from the repository root:
#   cargo build --release -p openpid-app && sh tools/screenshots.sh [log]
# The default log is the Betaflight 2025.12 fixture (Git LFS): it carries the
# unfiltered gyro, eRPM and the simplified-tuning headers, so every tab has
# something to show.
set -eu

LOG="${1:-tests/logs/betaflight-2025.12.2-btfl_027.bbl}"
BIN="${BIN:-target/release/openpid-gui}"
OUT="${OUT:-docs/screenshots}"
SIZE="${SIZE:-1280x800}"
WAIT="${WAIT:-6}"

command -v Xvfb >/dev/null || { echo "Xvfb is missing (apt install xvfb)" >&2; exit 1; }
command -v import >/dev/null || { echo "ImageMagick 'import' is missing" >&2; exit 1; }
[ -x "$BIN" ] || { echo "$BIN not built: cargo build --release -p openpid-app" >&2; exit 1; }
mkdir -p "$OUT"

DISPLAY_NO=${DISPLAY_NO:-97}
Xvfb ":$DISPLAY_NO" -screen 0 "${SIZE}x24" >/dev/null 2>&1 &
XVFB=$!
trap 'kill $XVFB 2>/dev/null || true' EXIT
sleep 1
export DISPLAY=":$DISPLAY_NO"
# the app reads prefs from this directory: a clean one keeps the dark theme
export OPENPID_CONFIG_DIR="$(mktemp -d)"

for tab in log step spectrum throttle time filter motors sliders diagnostics settings; do
  "$BIN" "$LOG" --tab "$tab" >/dev/null 2>&1 &
  APP=$!
  sleep "$WAIT"
  import -display "$DISPLAY" -window root "$OUT/$tab.png"
  kill $APP 2>/dev/null || true
  wait $APP 2>/dev/null || true
  echo "$OUT/$tab.png"
done
