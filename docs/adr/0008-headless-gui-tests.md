# ADR-0008: Headless GUI tests with `egui_kittest`

- Status: accepted
- Date: 2026-11-04

## Context

`openpid-app` (about a quarter of the code) had no tests beyond two unit tests of pure functions and was excluded from coverage (ADR-0007), because `eframe` needs a window and a GPU. The state logic (slots, the analysis cache, tabs, drag-and-drop, touch mode) and the seven tabs were checked by hand only.

## Decision

- The dev-dependency **`egui_kittest`** (the official egui harness, MIT OR Apache-2.0, the same version as `eframe`), **without** the `snapshot`/`wgpu`/`eframe` features (the last one enables `eframe/accesskit`, which pulls `zbus` with unmaintained advisories): egui frames run without a window, widgets are found through the AccessKit tree (`get_by_label`, `click`, `type_text`). No GPU, no display dependency, it runs on all three CI runners.
- `App` is built with `with_context(&egui::Context, …)` and drawn with `frame(&mut Ui)`; `impl eframe::App` and `run_native` live in `main.rs`, the only file that needs a window and the only one excluded from Sonar coverage. The harness holds the `App` as its state and calls `frame` every frame; a test reads the state directly (`harness.state()`).
- The background worker is not mocked: tests step frames until the slot has loaded or the cache has no `Pending` entries (with a timeout). That exercises the real path "file → thread → cache → tab".
- The harness's accessibility check (`with_accessibility_check`) is off for now: `DragValue`/`Slider` without labels have no accessible name. Enabling it is a separate task together with labels for screen readers.
- There is no 4.3+ log with sliders in the repository (licences, see `tests/logs/README.md`), so the sliders tab is tested on a session of `betaflight-4.2.0.BFL` with a substituted firmware version and added `simplified_*` headers. The slider model itself is checked on real logs in `openpid-fmt-blackbox`.
- `openpid-app` counts towards the CI and Sonar coverage like every other crate.

## Consequences

- CI has no `--exclude openpid-app`; the 90 % threshold applies to the whole workspace.
- New widgets and tabs get a test in `crates/openpid-app/src/tests.rs`; widget labels are a contract for the tests and change together with them.
- `rfd` (the native file dialog) is never called in tests; opening files is tested through `App::open` and drag-and-drop events.
