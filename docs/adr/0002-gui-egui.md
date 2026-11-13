# ADR-0002: GUI on egui/eframe

Status: accepted (2026-11-04), to be reviewed after the phase-1 prototype

## Context
One code base for macOS/Windows/Linux with GPU rendering of large plots (millions of points, heatmaps) and multi-touch support. Candidates: egui 0.37, iced 0.14, slint 1.18, tauri.

## Decision
`eframe` (the `wgpu` backend) + `egui` + `egui_plot`.

Reasons:
- Pure Rust, one binary, no web stack and no own DSL.
- Immediate mode fits "a parameter changed → the plot is recomputed" without state synchronisation.
- Built-in multi-touch (`MultiTouchInfo`: pinch, pan, rotate), high DPI, file drag-and-drop.
- `egui_plot` gives line plots, markers, zoom/pan; the heatmap we render ourselves as a texture.
- The largest community among Rust GUIs, frequent releases.

Rejected:
- `iced`: a good architecture, but less ready-made for interactive plots; more code per widget.
- `slint`: a strong touch/embedded story, but its own markup language and licence model (GPLv3 / royalty-free with conditions / commercial) complicate the project licence choice.
- `tauri`: a web front-end contradicts ADR-0001.

## Consequences
- The GUI layer holds no mathematics: it only displays `Series`/`Grid2D` from `openpid-analysis`.
- Lines above ~50k points are decimated min/max per pixel in `openpid-app` (not in dsp).
- If the prototype shows a limit (for example the 3D surface of the notch overlay 3D), 3D is drawn with an own wgpu pass inside an egui widget, without changing the framework.

## Addendum (after the implementation, 2026-11-04)
- The rendering backend is `glow` (OpenGL), not `wgpu`: a smaller dependency graph and build time; `wgpu` stays an option through the `eframe/wgpu` feature if an own 3D pass is ever needed (notch overlay 3D in phase 2).
- The `system_fonts` feature is off: it pulls `yeslogic-fontconfig-sys` (the native libfontconfig on Linux at build time), against ADR-0001; the fonts built into egui are enough.
- The features of `eframe` are named one by one (`default-features = false`), so every one of its defaults is a decision (2026-11-13, #191):
  - **`clipboard` is on.** Without it nothing reaches the system clipboard: Ctrl+C on a selected text and the program's copy buttons do nothing. It brings `arboard`, on Windows `clipboard-win` and `error-code` (the Boost licence, BSL-1.0: permissive, allowed in `deny.toml` for them), on Wayland `smithay-clipboard`. Category (1) of ADR-0001: a platform API. A test keeps the feature named.
  - **`links` is off.** It brings `webbrowser` with `url`, `idna` and the ICU tables behind them, some thirty crates for the one link the program has. The program opens that link with the platform's own opener (`rundll32 url.dll,FileProtocolHandler`, `open`, `xdg-open`) in `crates/openpid-app/src/browser.rs`, for web addresses only.
  - The other defaults of `eframe` (`accesskit`, `web_screen_reader`) stay off, as before; nothing was decided about them here.
- Heatmaps are drawn with the built-in `egui_plot::Heatmap` (since 0.38), so no textures are needed.
- egui 0.37 changed its API: `App::ui(&mut Ui)` instead of `update(&Context)`, `Panel::top/bottom` instead of `TopBottomPanel`.

## History
- 2026-11-04 … 2026-11-13: `eframe` was built with `glow`, `default_fonts`, `x11` and `wayland` only. `clipboard` and `links` were left out without a decision, as defaults that `default-features = false` drops: copying from the program and its link to the user guide did nothing on any platform (#191).
