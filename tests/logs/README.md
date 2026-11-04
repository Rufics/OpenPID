# Test logs

Small real-world Blackbox logs used by conformance, golden and robustness
tests. All files here are redistributable under MIT/Apache-2.0.

| File | Firmware | Source | Notes |
|------|----------|--------|-------|
| `betaflight-4.2.11-error-recovery.bbl` | Betaflight 4.2.11 | [blackbox-log](https://github.com/blackbox-log/blackbox-log) `tests/logs/error-recovery.bbl` (MIT/Apache-2.0) | Deliberately corrupted frames |
| `betaflight-4.2.11-two-logs.bbl` | Betaflight 4.2.11 | [fc-blackbox](https://github.com/ilya-epifanov/fc-blackbox) `src/test-data/btfl_001.bbl` (MIT/Apache-2.0) | Two logs in one file |
| `betaflight-4.2.0.BFL` | Betaflight 4.2.0 | fc-blackbox `LOG00037.BFL` | Single log |
| `betaflight-4.2.8-44-logs.bbl` | Betaflight 4.2.8 | fc-blackbox `btfl_all.bbl` | 44 short logs (arming sessions) |
| `emuflight-3.7.0-truncated.BFL` | EmuFlight 3.7.0 | fc-blackbox `crashing-LOG00002.BFL`, first 1 MiB | Truncated mid-frame on purpose |
| `inav-3.0.1-truncated.TXT` | INAV 3.0.1 | fc-blackbox `LOG00004.TXT`, first 1 MiB | `P interval:1/2`, GPS and slow frames; INAV is a phase-2 target |

Additional community logs (one per Betaflight release line 4.3, 4.4, 4.5,
2025.12, plus Rotorflight and EmuFlight 0.4) are fetched on demand into
`tests/logs/external/` (git-ignored) by `tools/fetch-community-logs.sh`; the
tests use them when present. Their source repositories carry no MIT-compatible
licence, which is why they are not committed. See `docs/BLACKBOX-VERSIONS.md`.

The project owner's own logs are reserved for final verification and are not
part of this directory.
