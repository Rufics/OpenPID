#!/usr/bin/env sh
# Seed corpus for every fuzz target from the committed test logs (Git LFS).
# Run from the repository root; the corpus directories are not committed
# (fuzz/.gitignore), libFuzzer grows them.
#
# Blackbox: a 24 KiB segment at each of the first six log starts of every
# fixture (an SD-card dump holds dozens of logs with different headers) plus a
# 4 KiB head; the headers and the first frames are where the parsing logic
# lives, and short inputs keep the fuzzer fast. ArduPilot: the first 64 KiB
# and 8 KiB of every .bin. CSV: a scaled and a raw export from the CLI.
set -eu
cd "$(dirname "$0")/.."
python3 -I - <<'EOF'
import glob, os
MARKER = b"H Product:Blackbox flight data recorder by Nicholas Sherlock"
for target in ("blackbox_open", "blackbox_headers"):
    out = os.path.join("fuzz", "corpus", target)
    os.makedirs(out, exist_ok=True)
    for path in sorted(glob.glob("tests/logs/*.bbl") + glob.glob("tests/logs/*.BFL") + glob.glob("tests/logs/*.TXT")):
        data = open(path, "rb").read()
        name = os.path.basename(path)
        starts, i = [], data.find(MARKER)
        while i >= 0 and len(starts) < 6:
            starts.append(i)
            i = data.find(MARKER, i + 1)
        if not starts:
            starts = [0]
        for n, s in enumerate(starts):
            open(os.path.join(out, f"{name}.{n}.24k"), "wb").write(data[s:s + 24 * 1024])
        open(os.path.join(out, f"{name}.4k"), "wb").write(data[:4096])
out = os.path.join("fuzz", "corpus", "ardupilot_open")
os.makedirs(out, exist_ok=True)
for path in sorted(glob.glob("tests/logs/*.bin")):
    data = open(path, "rb").read()
    name = os.path.basename(path)
    open(os.path.join(out, f"{name}.64k"), "wb").write(data[:64 * 1024])
    open(os.path.join(out, f"{name}.8k"), "wb").write(data[:8 * 1024])
EOF
mkdir -p fuzz/corpus/csv_open
cargo run --release -q -p openpid-cli -- decode tests/logs/betaflight-4.2.0.BFL --scaled --out fuzz/corpus/csv_open/scaled.csv.full
cargo run --release -q -p openpid-cli -- decode tests/logs/betaflight-4.2.0.BFL --out fuzz/corpus/csv_open/raw.csv.full
for f in fuzz/corpus/csv_open/*.full; do
  head -c 16384 "$f" > "${f%.full}"
  rm "$f"
done
echo "corpus:"; for d in fuzz/corpus/*; do echo "  $(basename "$d"): $(ls "$d" | wc -l) files"; done
