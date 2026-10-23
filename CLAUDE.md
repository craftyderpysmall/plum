# Plum — native Rust whiteboard (egui/eframe 0.37, wgpu, edition 2024)

## Build / verify
- This machine has no MSVC linker: always `cargo +stable-x86_64-pc-windows-gnu <cmd>`.
- Gate: `cargo +stable-x86_64-pc-windows-gnu clippy --workspace --all-targets -- -D warnings` and `... test --workspace`.
- Run: `... run --release -p plum-app -- [file.plum]`. Demo/stress boards: `-p plum-format --example make_demo -- out.plum [--stress N]`.
- egui 0.37 APIs differ from older docs (`App::ui(&mut Ui, &mut Frame)`, `Id::unique`, `Popup::menu`, reworked fonts). Check sources in `~/scoop/persist/rustup/.cargo/registry/src/*/egui-0.37.0` before guessing.

## Crates
- `plum-core`: `Element {id, transform (f64 pos/rotation/scale; negative scale = flip), style, kind, version, cached bounds}`. Kinds: Stroke/Shape/Line/Text/Unknown. `Board` = z-ordered Vec + id map + rstar `SpatialIndex`. Mutate only via Board methods; use `set_transform`/`set_style` fast paths. `version` bumps on kind/style change (renderer cache key). `Selection` + `GroupOp` (transform-only). `History`: compact `Change::{Transform,Style,Modify,Insert,Remove,MoveZ}`, VecDeque.
- `plum-format`: header + chunks (tag, flags, len, payload, CRC32, LZ4 if >4 KB). Chunks META, VIEW, ELM2 (current, palette paints + opacity), ELEM (1.0, read-only). Element kind codecs are versioned (`STRK` v2 = quantized second-order deltas; must still decode v1). Unknown chunks/kinds round-trip verbatim. Format is 1.1; only append, never change existing wire structs.
- `plum-config`: embedded `default.toml` defines EVERY key → theme chain (`meta.extends`) → user `config.toml`, deep-merged, then typed (`model.rs`). Adding a key = add it to `default.toml` + `model.rs`. Bundled themes in `themes/`. Hot reload via `notify`. Keybinds = `[keybinds]` name → chords; app-side table `keymap::ACTIONS` must match (test enforces).
- `plum-app`: `app.rs` (state, actions, files, input), `render.rs` (culling, per-element mesh cache by zoom bucket, text cache keyed on atlas generation, LOD, grid), `tools/` (`Tool` trait; select gizmo in `select.rs`), `ui.rs` (all chrome, hand-drawn widgets), `motion.rs` (exponential easing; repaint only while moving), `theme.rs` (config → egui style, `paint_c32` resolves palette slots), `keymap.rs` (exact-modifier matching; `chord_display` for UI, `chord_string` for config).

## Conventions / gotchas
- Colors on elements are `Paint::Palette(i)` (follow theme) or `Paint::Rgba`.
- Never call `ctx.*` inside a `ctx.input(|i| ...)` closure: it deadlocks.
- No hardcoded UI colors/sizes/keys; read from `Config`.
- Prefer refs (`selection.elements`, `transforms`) over cloning elements per frame; perf matters (100k strokes).
- Native OS dialogs only for file pickers (rfd); prompts are in-app.
- User config lives in `%APPDATA%\plum\`. Back it up before tests that write it, and restore after.
- When driving the GUI with synthetic input, verify Plum is the foreground window first.
