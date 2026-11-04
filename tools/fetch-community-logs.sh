#!/usr/bin/env sh
# Download community test logs that are NOT committed to this repository
# (their source repositories carry no licence or a non-MIT one, so we only
# reference them). Files land in tests/logs/external/ (git-ignored); the test
# suite picks them up automatically when present.
#
# One log per Betaflight release line from 4.2 onward, plus other firmwares.
set -eu
dir="$(dirname "$0")/../tests/logs/external"
mkdir -p "$dir"
raw="https://raw.githubusercontent.com"

fetch() { # <target name> <owner/repo> <path in repo>
  if [ ! -f "$dir/$1" ]; then
    echo "fetching $1 from $2"
    curl -fsSL "$raw/$2/HEAD/$3" -o "$dir/$1.part" && mv "$dir/$1.part" "$dir/$1"
  fi
}

# Betaflight, one per release line (4.2 lives in the committed fixtures)
fetch betaflight-4.3.0-LOG00001.BFL          gimbal-ghost/gimbal-ghost                test/LOG00001.BFL
fetch betaflight-4.4.3-btfl_001.bbl          vlad-avr/BBLHelper                       data/btfl_001.bbl
fetch betaflight-4.4.2-simulation-demo.bbl   k-vanier/iz_pid                          demo/demo_3_vols.bbl
fetch betaflight-4.5.1-LOG00022.BFL          aprskalo1/betaflight-blackbox-analyzer   LOG00022.BFL
fetch betaflight-4.5.1-LOG00001.BFL          aprskalo1/betaflight-blackbox-analyzer   13_9_2026_bfl/LOG00001.BFL
fetch betaflight-4.5.2-Run2.bbl              jrwrodgers/bbl_decoder                   Run2.bbl
fetch betaflight-2025.12.0-RC4-short.bbl     mdimitri/bbx-analyzer                    tests/data/short.bbl
# Betaflight 2026.6: no public log found yet (see docs/BLACKBOX-VERSIONS.md)

# Other firmwares
fetch rotorflight-4.3.0-rtfl_001.bbl         gimbal-ghost/gimbal-ghost                test/rtfl_001.bbl
fetch emuflight-0.4.1-emuf_001.bbl           gimbal-ghost/gimbal-ghost                test/emuf_001.bbl

echo "done: $(ls "$dir" | wc -l) files in $dir"
