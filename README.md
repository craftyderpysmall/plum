# Plum

A minimal, fast, native whiteboard written in Rust. Infinite canvas, freehand
pen, shapes, lines/arrows and text — every item is a selectable object you can
move, resize, rotate and flip. Boards are saved as `.plum` files, and the whole
look and every shortcut can be customized with TOML themes.

Built on [egui](https://github.com/emilk/egui)/eframe (wgpu) for Linux: native
Wayland, with X11 as a fallback. No web runtime.

## Features

- **Tools:** select, hand, pen, rectangle, ellipse, diamond, line, arrow, text, fill bucket.
- **Selection:** click, Shift-click, marquee; drag to move; 8 resize handles; rotate handle (Shift snaps 15°); flip, rotate 90°, layer order, duplicate, delete. A `z: n/total` label shows layer position.
- **Infinite canvas:** pan/zoom anywhere (f64 world coordinates, no jitter far from the origin), smooth zoom with soft limits, optional snap to grid.
- **Styles:** palette colors, fill, stroke width, opacity. Palette colors follow the theme, so dark ink on a light theme becomes light ink on a dark one.
- **Undo/redo**, in-app unsaved-changes prompt, rebindable keys (Menu → Settings).
- **Fast at scale:** viewport culling via an R-tree, cached meshes, level of detail. 100k strokes stay interactive.

## Installing

Download `plum-X.Y.Z-x86_64-linux.tar.gz` from
[Releases](https://github.com/craftyderpysmall/plum/releases), extract it and run
the installer:

```sh
tar xzf plum-*-x86_64-linux.tar.gz
cd plum-*-x86_64-linux
./install.sh                          # into ~/.local (no root needed)
sudo ./install.sh --prefix /usr/local # or system-wide
```

This installs the `plum` binary, a desktop entry (so it shows up in your
launcher and gets its icon on Wayland), the icon, and the `.plum` file type so
boards open with a double-click. `./install.sh --uninstall` removes it again.

Runtime requirements: a Vulkan or OpenGL driver, and for the open/save dialogs
`xdg-desktop-portal` (any backend) or `zenity`. Wayland, X11 and GPU libraries
are loaded at runtime, so there are no other dependencies.

## Building

Requires a recent stable Rust (edition 2024). No system `-dev` packages are
needed.

```sh
cargo run --release -p plum-app            # start with an empty board
cargo run --release -p plum-app -- my.plum # open a board
cargo test --workspace
packaging/linux/install.sh                 # install the release build into ~/.local
```

Plum uses Wayland when `WAYLAND_DISPLAY` is set and X11 otherwise; run
`env -u WAYLAND_DISPLAY plum` to force X11 (XWayland).

Demo boards: `cargo run -p plum-format --example make_demo -- out.plum [--stress 100000]`.
Press **F3** in the app for the performance overlay.

## Customizing

Config lives in `$XDG_CONFIG_HOME/plum/` (usually `~/.config/plum/`):

```
config.toml          your overrides, e.g. theme = "light"
themes/<name>.toml   custom themes
default.toml.example every available option, with comments
```

Settings resolve in layers — built-in `default.toml` → theme (and any theme it
`extends`) → `config.toml` — and each layer only needs the keys it changes.
Files hot-reload while Plum runs. Bundled themes: `dark` (default) and `light`.

You can configure colors, sizes, toolbar position, font (`ui.font_file`;
JetBrains Mono by default; `~/` and paths relative to the config folder work),
grid, palette, tool defaults, every keybind, animation speed, and performance
knobs.

Ctrl+C / Ctrl+V copy and paste board elements. Text copied in another app is
pasted as a new text box.

## The `.plum` format

A 16-byte header (`PLUM\x89\r\n\x1a`, major/minor version) followed by tagged
chunks, each with a CRC32 and optional LZ4 compression:

| Chunk  | Contents                                         |
|--------|--------------------------------------------------|
| `META` | board name, timestamps                           |
| `VIEW` | last camera position/zoom                        |
| `ELM2` | element records (transform, style, kind payload) |
| `ELEM` | legacy 1.0 elements (read-only)                  |

It's built to be extended without breaking anything:

- Each element kind (`STRK`, `SHAP`, `LINE`, `TEXT`) has its own versioned codec.
- Unknown chunks and unknown element kinds are kept and written back unchanged.
- A corrupt chunk is skipped with a warning instead of failing the whole load.
- Saves are atomic.

## Project layout

```
crates/
  plum-core/    data model: elements, transforms, board + spatial index, selection ops, undo
  plum-format/  .plum reader/writer and codec registry
  plum-config/  TOML layering, typed config, bundled themes, file watching
  plum-app/     the eframe app: rendering, tools, UI, keymap
packaging/linux/  desktop entry, MIME type, install script
```

## License

Plum is released under the [MIT License](LICENSE).

JetBrains Mono is bundled under the SIL Open Font License
(`crates/plum-app/assets/fonts/JetBrainsMono-OFL.txt`).
