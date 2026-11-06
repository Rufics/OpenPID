#!/usr/bin/env sh
# Seed corpus for every fuzz target from the committed test logs (Git LFS):
# the first kilobytes of each log hold the headers and the first frames, which
# is where the parsing logic lives. Run from the repository root; the corpus
# directories are not committed (fuzz/.gitignore), libFuzzer grows them.
set -eu
cd "$(dirname "$0")/.."
slice() { # slice <file> <bytes> <out>
  head -c "$2" "$1" > "$3"
}
for t in blackbox_open blackbox_headers; do
  mkdir -p "fuzz/corpus/$t"
  for f in tests/logs/*.bbl tests/logs/*.BFL tests/logs/*.TXT; do
    [ -f "$f" ] || continue
    n=$(basename "$f")
    slice "$f" 24576 "fuzz/corpus/$t/$n.24k"
    slice "$f" 4096 "fuzz/corpus/$t/$n.4k"
  done
done
mkdir -p fuzz/corpus/ardupilot_open
for f in tests/logs/*.bin; do
  [ -f "$f" ] || continue
  n=$(basename "$f")
  slice "$f" 65536 "fuzz/corpus/ardupilot_open/$n.64k"
  slice "$f" 8192 "fuzz/corpus/ardupilot_open/$n.8k"
done
# the CSV corpus comes from the decoder itself: a scaled export and a raw one
mkdir -p fuzz/corpus/csv_open
cargo run --release -q -p openpid-cli -- decode tests/logs/betaflight-4.2.0.BFL --scaled --out fuzz/corpus/csv_open/scaled.csv.full
cargo run --release -q -p openpid-cli -- decode tests/logs/betaflight-4.2.0.BFL --out fuzz/corpus/csv_open/raw.csv.full
for f in fuzz/corpus/csv_open/*.full; do
  slice "$f" 16384 "${f%.full}"
  rm "$f"
done
echo "corpus:"; for d in fuzz/corpus/*; do echo "  $(basename "$d"): $(ls "$d" | wc -l) files"; done
