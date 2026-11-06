# Plum — native Rust whiteboard (egui/eframe 0.37, wgpu, edition 2024)

## Build / verify
- Linux only: native Wayland (preferred) with X11 fallback. eframe runs with `default-features = false`, so `wayland`, `x11` and `clipboard` must stay in its feature list: without `wayland` the Wayland `app_id` is silently dropped (no .desktop/icon matching), without `clipboard` text boxes can't paste.
- Wayland/X11/Vulkan/GL/xkbcommon are all dlopen'd: building needs no system `-dev` packages.
- Gate: `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`.
- Run: `cargo run --release -p plum-app -- [file.plum]` (binary is `plum`). Demo/stress boards: `cargo run -p plum-format --example make_demo -- out.plum [--stress N]`.
- egui 0.37 APIs differ from older docs (`App::ui(&mut Ui, &mut Frame)`, `Id::unique`, `Popup::menu`, reworked fonts). Check sources in `~/.cargo/registry/src/*/egui-0.37.0` before guessing.

## Releasing
- Version source of truth = `crates/plum-app/Cargo.toml` (other crates stay put). Bump it, then run any cargo build so `Cargo.lock` updates: CI builds with `--locked` and fails on a stale lock.
- Run the gate, commit, then `git tag vX.Y.Z` (must equal the plum-app version; CI checks) and `git push origin main vX.Y.Z`.
- `.github/workflows/release.yml` (ubuntu-22.04 for an older glibc baseline) runs the gate, builds, and publishes `plum-X.Y.Z-x86_64-linux.tar.gz` (+ `.sha256`) containing `plum`, `install.sh`, `plum.desktop`, `plum.xml` (MIME type), icons, README, LICENSE.
- Watch with `gh run watch` / `gh run list --workflow release.yml`; verify with `gh release view vX.Y.Z`. If CI fails before publishing: fix, delete the tag locally and remotely (`git push origin :refs/tags/vX.Y.Z`), retag, push.
- Packaging lives in `packaging/linux/`. The desktop file name, `Icon=` and `StartupWMClass=` must match the app id `plum` set in `main.rs`.
- App icon: `crates/plum-app/assets/` (`icon.svg` source, `icon.png` 256px window/launcher icon). Regenerate the png from the svg with ImageMagick.

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
- Native file pickers only (rfd → xdg-desktop-portal, zenity fallback), always run on a background thread (`start_file_dialog`/`poll_file_dialog`): blocking the UI thread makes Wayland compositors flag the app as unresponsive. Prompts are in-app.
- egui-winit turns Ctrl+C/X/V into `Event::Copy/Cut/Paste`, never key events; `Keymap::match_event` maps them back. Shifted chords (`Shift+1` arrives as `!`) fall back to the physical key.
- User config lives in `$XDG_CONFIG_HOME/plum/`. For GUI tests run with `XDG_CONFIG_HOME=<scratch dir>` instead of touching the real one.
- When driving the GUI with synthetic input, verify Plum is the focused window first (`hyprctl activewindow -j`) before every key press: keys otherwise land in whatever has focus. On this machine (Hyprland 0.57, Lua dispatch): move the cursor with `hyprctl eval 'hl.dispatch(hl.dsp.cursor.move({x=X, y=Y}))'`, inject buttons/keys via `/dev/uinput` (user has ACL access; real keycodes go through the user's layout, unlike `wtype`), screenshot with `grim -g`.
