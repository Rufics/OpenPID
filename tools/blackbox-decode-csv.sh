#!/usr/bin/env sh
# Reference CSVs from the official Betaflight `blackbox_decode` for the
# conformance test `crates/openpid-cli/tests/csv_conformance.rs` (#49).
#
# Usage, from the repository root:
#   sh tools/blackbox-decode-csv.sh <path to a betaflight/blackbox-tools checkout>
#
# It builds `obj/blackbox_decode` in that checkout (no libraries needed), runs
# it on every Blackbox fixture in tests/logs/ and writes, per flight log in
# the file (session N counts from 0, blackbox_decode numbers them from 1):
#   tests/logs/<fixture>.<N>.csv   the header and the first $ROWS frames
#   tests/logs/<fixture>.<N>.rows  the total number of frames blackbox_decode wrote
# plus tests/logs/blackbox_decode.version (the decoder's commit) and
# tests/logs/blackbox_decode.skip: fixtures the official decoder cannot
# process (it hangs or writes nothing), one per line with the reason; the
# test accepts a missing reference only for those. Each run of the decoder is
# limited to $DECODE_TIMEOUT seconds (60 by default). The CSVs
# are cut to keep Git LFS small; the .rows file keeps the full frame count
# comparable. The decoder's own messages go to target/blackbox-decode-csv/.
# Re-running replaces the previous output.
set -eu

ROWS="${ROWS:-2000}"
DECODE_TIMEOUT="${DECODE_TIMEOUT:-60}"
tools="${1:-}"
if [ -z "$tools" ] || [ ! -f "$tools/src/blackbox_decode.c" ]; then
  echo "usage: sh tools/blackbox-decode-csv.sh <path to a betaflight/blackbox-tools checkout>" >&2
  exit 2
fi
cd "$(dirname "$0")/.."
root="$(pwd)"
tools="$(cd "$tools" && pwd)"

echo "building blackbox_decode in $tools"
(cd "$tools" && make obj/blackbox_decode >/dev/null)
decoder="$tools/obj/blackbox_decode"
[ -x "$decoder" ] || { echo "build failed: $decoder missing" >&2; exit 1; }
commit="$(git -C "$tools" rev-parse --short HEAD 2>/dev/null || echo unknown)"
date="$(git -C "$tools" log -1 --format=%cs 2>/dev/null || echo unknown)"

# the test logs must be the real files, not Git LFS pointers
for f in tests/logs/*.bbl tests/logs/*.BBL tests/logs/*.bfl tests/logs/*.BFL \
         tests/logs/*.txt tests/logs/*.TXT; do
  if [ -f "$f" ] && head -c 40 "$f" | grep -q 'git-lfs'; then
    echo "$f is a Git LFS pointer: run 'git lfs install && git lfs pull' first" >&2
    exit 1
  fi
done

out="$root/target/blackbox-decode-csv"
rm -rf "$out"
mkdir -p "$out"
rm -f tests/logs/*.csv tests/logs/*.rows tests/logs/blackbox_decode.version tests/logs/blackbox_decode.skip
: > tests/logs/blackbox_decode.skip

# Run "$@" with stdout/stderr already redirected by the caller; kill it after
# $DECODE_TIMEOUT seconds. Prints nothing; returns 124 on a timeout.
run_limited() {
  "$@" &
  pid=$!
  ( sleep "$DECODE_TIMEOUT"; kill -9 "$pid" 2>/dev/null ) &
  watcher=$!
  status=0
  wait "$pid" || status=$?
  if kill -0 "$watcher" 2>/dev/null; then
    kill "$watcher" 2>/dev/null || true
    wait "$watcher" 2>/dev/null || true
  elif [ "$status" -ne 0 ]; then
    status=124
  fi
  return "$status"
}

ok=0
failed=""
for log in tests/logs/*.bbl tests/logs/*.BBL tests/logs/*.bfl tests/logs/*.BFL \
           tests/logs/*.txt tests/logs/*.TXT; do
  [ -f "$log" ] || continue
  name="$(basename "$log")"
  prefix="${name%.*}"
  work="$out/$name"
  mkdir -p "$work"
  echo "  $name: decoding (limit ${DECODE_TIMEOUT} s)..."
  status=0
  run_limited "$decoder" --output-dir "$work" "$log" >"$work/stdout.txt" 2>"$work/stderr.txt" || status=$?
  if [ "$status" -eq 124 ]; then
    printf '%s\ttimed out after %s s in blackbox_decode %s\n' "$name" "$DECODE_TIMEOUT" "$commit" >> tests/logs/blackbox_decode.skip
    failed="$failed $name"
    echo "  $name: blackbox_decode did not finish in ${DECODE_TIMEOUT} s, skipped (listed in blackbox_decode.skip)"
    continue
  fi
  found=0
  for csv in "$work/$prefix".[0-9][0-9].csv; do
    [ -f "$csv" ] || continue
    number="${csv%.csv}"
    number="${number##*.}"              # NN, counted from 01
    session=$(( $(echo "$number" | sed 's/^0*//') - 1 ))
    rows=$(( $(wc -l < "$csv") - 1 ))
    head -n $(( ROWS + 1 )) "$csv" > "tests/logs/$name.$session.csv"
    echo "$rows" > "tests/logs/$name.$session.rows"
    found=$(( found + 1 ))
  done
  if [ "$found" -eq 0 ]; then
    reason="$(head -n 1 "$work/stderr.txt" | tr '\t' ' ')"
    printf '%s\tno CSV written (exit %s): %s\n' "$name" "$status" "${reason:-no message}" >> tests/logs/blackbox_decode.skip
    failed="$failed $name"
    echo "  $name: no CSV written, skipped (see target/blackbox-decode-csv/$name/stderr.txt)"
  else
    echo "  $name: $found log(s)"
    ok=$(( ok + 1 ))
  fi
done

printf 'blackbox_decode %s (%s), betaflight/blackbox-tools\n' "$commit" "$date" > tests/logs/blackbox_decode.version
echo
echo "decoder: betaflight/blackbox-tools $commit ($date)"
echo "fixtures decoded: $ok"
if [ -n "$failed" ]; then
  echo "skipped (see tests/logs/blackbox_decode.skip):$failed"
  echo "(the decoder's messages are in target/blackbox-decode-csv/<fixture>/stderr.txt)"
fi
echo "next: git add tests/logs && git commit && git push"
