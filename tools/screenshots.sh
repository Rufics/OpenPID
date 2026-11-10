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
# the app reads prefs from this directory: a clean one keeps the dark theme;
# the first-run tour is marked as seen so it does not cover the tabs
export OPENPID_CONFIG_DIR="$(mktemp -d)"
printf '{"tour_done":true,"show_tour":false}' > "$OPENPID_CONFIG_DIR/prefs.json"

shot() { # shot <out.png> <args...>
  out="$1"; shift
  "$BIN" "$@" >/dev/null 2>&1 &
  APP=$!
  sleep "$WAIT"
  import -display "$DISPLAY" -window root "$out"
  kill $APP 2>/dev/null || true
  wait $APP 2>/dev/null || true
  echo "$out"
}

# the start screen (no log), and three logs compared on the step tab
shot "$OUT/start.png"
shot "$OUT/slots-step.png" "$LOG" tests/logs/betaflight-4.5.1-two-sessions.bbl \
  tests/logs/betaflight-4.2.0.BFL --tab step

for tab in log step spectrum throttle time filter advice sliders settings; do
  shot "$OUT/$tab.png" "$LOG" --tab "$tab"
done

# Setup info compares logs: two of them side by side
shot "$OUT/setup.png" tests/logs/betaflight-4.5.0-gps-rescue.bbl \
  tests/logs/betaflight-4.5.1-two-sessions.bbl --tab setup

# the log viewer once more in Ukrainian (Settings → Language), for the README
printf '{"lang":"ua","tour_done":true,"show_tour":false}' > "$OPENPID_CONFIG_DIR/prefs.json"
shot "$OUT/log-ua.png" "$LOG" --tab log
