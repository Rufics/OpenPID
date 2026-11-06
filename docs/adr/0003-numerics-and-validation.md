# ADR-0003: Number policy and validation of the mathematics

Status: accepted (2026-11-04)

## Decision
1. Every computation is `f64`. The raw log counters stay integers until the scaling, and the scale is applied once.
2. Data validity is an explicit bit mask in `Series`; NaN is never a signal.
3. Units are the `Unit` enum in every `Channel`; an analysis refuses an unexpected unit (an error, not a silent conversion).
4. Windows and the PSD are normalised explicitly (coherent gain, ENBW); the output names its units (`(°/s)²/Hz`, `°/s rms`).
5. Non-uniform time: resampling onto a uniform grid with linear interpolation within the tolerance; gaps are excluded from the segments; the statistics are shown to the user.
6. Every `openpid-dsp` function has: (a) a test against a NumPy/SciPy reference from `tools/reference` (fixtures in the repository), (b) property tests of invariants (Parseval, linearity, round trip), (c) edge-case tests (empty / short series, a constant, a single gap).
7. Tolerances: ≤ 1e-9 relative for linear transforms, ≤ 1e-6 for iterative / smoothed ones.
8. Determinism: the same log and parameters give a bit-identical JSON output on every platform (golden tests in CI on three platforms).

## Consequences
- Python is needed only by developers to regenerate the fixtures; it is absent from the runtime and the build.
- An algorithm change requires updating the golden files with an explanation in the PR.
