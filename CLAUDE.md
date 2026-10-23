# Plum — native Rust whiteboard (egui/eframe 0.37, wgpu, edition 2024)

## Build / verify
- This machine has no MSVC linker: always `cargo +stable-x86_64-pc-windows-gnu <cmd>`.
- Gate: `cargo +stable-x86_64-pc-windows-gnu clippy --workspace --all-targets -- -D warnings` and `... test --workspace`.
- Run: `... run --release -p plum-app -- [file.plum]`. Demo/stress boards: `-p plum-format --example make_demo -- out.plum [--stress N]`.
- egui 0.37 APIs differ from older docs (`App::ui(&mut Ui, &mut Frame)`, `Id::unique`, `Popup::menu`, reworked fonts). Check sources in `~/.cargo/registry/src/*/egui-0.37.0` (rustup via winget; `~/.cargo`, `~/.rustup`) before guessing.

## Releasing
- Version source of truth = `crates/plum-app/Cargo.toml` (other crates stay put). Bump it, then run any cargo build so `Cargo.lock` updates: CI builds with `--locked` and fails on a stale lock.
- Run the gate, commit, then `git tag vX.Y.Z` (must equal the plum-app version; CI checks) and `git push origin main vX.Y.Z`.
- `.github/workflows/release.yml` (windows-latest, MSVC) builds, zips `plum.exe` as `plum-X.Y.Z-x86_64-windows.zip`, creates the GitHub release, then runs `wingetcreate update craftyderpysmall.Plum ... --submit` to open a PR on microsoft/winget-pkgs (zip with nested portable `plum.exe`, alias `plum`). That step is skipped unless the `WINGET_TOKEN` repo secret (classic PAT, `public_repo`) is set. No manifests live in this repo.
- Watch with `gh run watch` / `gh run list --workflow release.yml`; verify with `gh release view vX.Y.Z` and the winget-pkgs PR (`gh pr list -R microsoft/winget-pkgs --search craftyderpysmall.Plum`). If CI fails before publishing: fix, delete the tag locally and remotely (`git push origin :refs/tags/vX.Y.Z`), retag, push.
- Users install/update with `winget install craftyderpysmall.Plum` / `winget upgrade craftyderpysmall.Plum` once the PR is merged (usually hours to a day).
- App icon: `crates/plum-app/assets/` (`icon.svg` source, `icon.png` window icon, `icon.ico` embedded in the exe by `build.rs` via `winresource`). Regenerate png/ico from the svg with ImageMagick (installed via winget).

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
