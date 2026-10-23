//! Keybinds from config → actions. Modifiers match *exactly*, so `H` and
//! `Shift+H` can be bound to different actions.

use egui::{Event, Key, KeyboardShortcut, Modifiers};
use plum_config::Keybinds;

use crate::tools::ToolKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Tool(ToolKind),
    Undo,
    Redo,
    SelectAll,
    Delete,
    Duplicate,
    Copy,
    Paste,
    Cut,
    FlipH,
    FlipV,
    RotateCw,
    RotateCcw,
    BringForward,
    SendBackward,
    BringToFront,
    SendToBack,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ZoomFit,
    ResetPosition,
    ToggleSnap,
    Settings,
    New,
    Open,
    Save,
    SaveAs,
    PerfOverlay,
    Deselect,
    /// Move the selection by (dx, dy) nudge steps; `large` uses `nudge_large`.
    Nudge { dx: i8, dy: i8, large: bool },
}

/// Every bindable action: (action, `[keybinds]` name, label shown in Settings).
pub const ACTIONS: &[(Action, &str, &str)] = &[
    (Action::Tool(ToolKind::Select), "tool_select", "Select tool"),
    (Action::Tool(ToolKind::Hand), "tool_hand", "Hand tool"),
    (Action::Tool(ToolKind::Pen), "tool_pen", "Pen tool"),
    (Action::Tool(ToolKind::Rect), "tool_rect", "Rectangle tool"),
    (Action::Tool(ToolKind::Ellipse), "tool_ellipse", "Ellipse tool"),
    (Action::Tool(ToolKind::Diamond), "tool_diamond", "Diamond tool"),
    (Action::Tool(ToolKind::Line), "tool_line", "Line tool"),
    (Action::Tool(ToolKind::Arrow), "tool_arrow", "Arrow tool"),
    (Action::Tool(ToolKind::Text), "tool_text", "Text tool"),
    (Action::Tool(ToolKind::Bucket), "tool_bucket", "Fill bucket"),
    (Action::Undo, "undo", "Undo"),
    (Action::Redo, "redo", "Redo"),
    (Action::SelectAll, "select_all", "Select all"),
    (Action::Deselect, "deselect", "Deselect / cancel"),
    (Action::Delete, "delete", "Delete"),
    (Action::Duplicate, "duplicate", "Duplicate"),
    (Action::Copy, "copy", "Copy"),
    (Action::Cut, "cut", "Cut"),
    (Action::Paste, "paste", "Paste"),
    (Action::FlipH, "flip_h", "Flip horizontal"),
    (Action::FlipV, "flip_v", "Flip vertical"),
    (Action::RotateCw, "rotate_cw", "Rotate right 90°"),
    (Action::RotateCcw, "rotate_ccw", "Rotate left 90°"),
    (Action::BringForward, "bring_forward", "Bring forward"),
    (Action::SendBackward, "send_backward", "Send backward"),
    (Action::BringToFront, "bring_to_front", "Bring to front"),
    (Action::SendToBack, "send_to_back", "Send to back"),
    (Action::Nudge { dx: -1, dy: 0, large: false }, "nudge_left", "Nudge left"),
    (Action::Nudge { dx: 1, dy: 0, large: false }, "nudge_right", "Nudge right"),
    (Action::Nudge { dx: 0, dy: -1, large: false }, "nudge_up", "Nudge up"),
    (Action::Nudge { dx: 0, dy: 1, large: false }, "nudge_down", "Nudge down"),
    (Action::Nudge { dx: -1, dy: 0, large: true }, "nudge_left_large", "Nudge left (large)"),
    (Action::Nudge { dx: 1, dy: 0, large: true }, "nudge_right_large", "Nudge right (large)"),
    (Action::Nudge { dx: 0, dy: -1, large: true }, "nudge_up_large", "Nudge up (large)"),
    (Action::Nudge { dx: 0, dy: 1, large: true }, "nudge_down_large", "Nudge down (large)"),
    (Action::ZoomIn, "zoom_in", "Zoom in"),
    (Action::ZoomOut, "zoom_out", "Zoom out"),
    (Action::ZoomReset, "zoom_reset", "Reset zoom"),
    (Action::ZoomFit, "zoom_fit", "Zoom to fit"),
    (Action::ResetPosition, "reset_position", "Reset position"),
    (Action::ToggleSnap, "toggle_snap", "Snap to grid"),
    (Action::New, "new", "New board"),
    (Action::Open, "open", "Open…"),
    (Action::Save, "save", "Save"),
    (Action::SaveAs, "save_as", "Save as…"),
    (Action::Settings, "settings", "Settings"),
    (Action::PerfOverlay, "perf_overlay", "Performance overlay"),
];

#[derive(Default)]
pub struct Keymap {
    bindings: Vec<(Action, KeyboardShortcut)>,
}

impl Keymap {
    /// Build from config; returns unparseable chords as warnings.
    pub fn from_config(k: &Keybinds) -> (Self, Vec<String>) {
        let mut bindings = Vec::new();
        let mut warnings = Vec::new();
        for (action, name, _) in ACTIONS {
            for chord in k.get(*name).map(|c| c.0.as_slice()).unwrap_or_default() {
                match parse_chord(chord) {
                    Some(s) => bindings.push((*action, s)),
                    None => warnings.push(format!("keybinds.{name}: can't parse {chord:?}")),
                }
            }
        }
        (Self { bindings }, warnings)
    }

    /// Consume this frame's key presses that match a binding.
    pub fn take_actions(&self, ctx: &egui::Context) -> Vec<Action> {
        let mut out = Vec::new();
        ctx.input_mut(|i| {
            i.events.retain(|ev| {
                let Event::Key { key, modifiers, pressed: true, .. } = ev else { return true };
                match self.bindings.iter().find(|(_, s)| s.logical_key == *key && modifiers.matches_exact(s.modifiers)) {
                    Some((a, _)) => {
                        out.push(*a);
                        false
                    }
                    None => true,
                }
            });
        });
        out
    }

    /// Human-readable first binding of an action, for tooltips.
    pub fn label(&self, action: Action) -> Option<String> {
        self.bindings.iter().find(|(a, _)| *a == action).map(|(_, s)| chord_display(s))
    }

    /// Other actions bound to the same shortcut as `chord`.
    pub fn conflicts(&self, action: Action, chord: &str) -> Vec<Action> {
        let Some(s) = parse_chord(chord) else { return Vec::new() };
        self.bindings.iter().filter(|(a, b)| *a != action && *b == s).map(|(a, _)| *a).collect()
    }
}

pub fn action_label(action: Action) -> &'static str {
    ACTIONS.iter().find(|(a, _, _)| *a == action).map(|(_, _, l)| *l).unwrap_or("?")
}

/// What a shortcut looks like on screen: symbols instead of key names
/// (`]` rather than `CloseBracket`). Config files keep [`chord_string`].
pub fn chord_display(s: &KeyboardShortcut) -> String {
    let key = match s.logical_key {
        Key::ArrowLeft => "←",
        Key::ArrowRight => "→",
        Key::ArrowUp => "↑",
        Key::ArrowDown => "↓",
        Key::Escape => "Esc",
        Key::Delete => "Del",
        k => k.symbol_or_name(),
    };
    modifier_parts(s).chain([key]).collect::<Vec<_>>().join("+")
}

/// Display form of a config chord string; unparseable strings are shown as-is.
pub fn display_chord_str(chord: &str) -> String {
    parse_chord(chord).map(|s| chord_display(&s)).unwrap_or_else(|| chord.to_string())
}

fn modifier_parts(s: &KeyboardShortcut) -> impl Iterator<Item = &'static str> {
    let m = s.modifiers;
    [(m.command || m.ctrl, "Ctrl"), (m.alt, "Alt"), (m.shift, "Shift")].into_iter().filter(|(on, _)| *on).map(|(_, n)| n)
}

/// Canonical config string for a shortcut, e.g. `Ctrl+Shift+Z`.
pub fn chord_string(s: &KeyboardShortcut) -> String {
    modifier_parts(s).chain([s.logical_key.name()]).collect::<Vec<_>>().join("+")
}

/// Turn a captured key press into a chord, ignoring bare modifier presses.
pub fn capture_chord(key: Key, modifiers: Modifiers) -> String {
    let mods = Modifiers { command: modifiers.command || modifiers.ctrl, ctrl: false, mac_cmd: false, ..modifiers };
    chord_string(&KeyboardShortcut::new(mods, key))
}

pub fn parse_chord(s: &str) -> Option<KeyboardShortcut> {
    let mut mods = Modifiers::NONE;
    let parts: Vec<&str> = s.split('+').map(str::trim).collect();
    // "Ctrl++" style: a trailing empty part means the key itself is '+'.
    let (key_part, mod_parts) = match parts.as_slice() {
        [rest @ .., "", ""] => ("Plus", rest),
        [rest @ .., last] => (*last, rest),
        [] => return None,
    };
    for m in mod_parts {
        match m.to_ascii_lowercase().as_str() {
            "ctrl" | "control" | "cmd" | "command" | "super" => mods |= Modifiers::COMMAND,
            "shift" => mods |= Modifiers::SHIFT,
            "alt" | "option" => mods |= Modifiers::ALT,
            _ => return None,
        }
    }
    let key = Key::from_name(key_part).or_else(|| Key::from_name(&key_part.to_ascii_uppercase()))?;
    Some(KeyboardShortcut::new(mods, key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_chords() {
        assert_eq!(parse_chord("Ctrl+Shift+Z"), Some(KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z)));
        assert_eq!(parse_chord("v"), Some(KeyboardShortcut::new(Modifiers::NONE, Key::V)));
        assert_eq!(parse_chord("Delete"), Some(KeyboardShortcut::new(Modifiers::NONE, Key::Delete)));
        assert_eq!(parse_chord("Ctrl+CloseBracket").map(|s| s.logical_key), Some(Key::CloseBracket));
        assert_eq!(parse_chord("Hyper+Q"), None);
    }

    #[test]
    fn captured_chords_round_trip() {
        for (key, mods) in [(Key::Z, Modifiers::CTRL | Modifiers::SHIFT), (Key::Comma, Modifiers::COMMAND), (Key::F3, Modifiers::NONE), (Key::ArrowLeft, Modifiers::ALT)] {
            let s = capture_chord(key, mods);
            let parsed = parse_chord(&s).unwrap_or_else(|| panic!("can't parse {s}"));
            assert_eq!(parsed.logical_key, key, "{s}");
            assert_eq!(chord_string(&parsed), s);
        }
    }

    #[test]
    fn display_uses_symbols() {
        assert_eq!(display_chord_str("CloseBracket"), "]");
        assert_eq!(display_chord_str("Ctrl+OpenBracket"), "Ctrl+[");
        assert_eq!(display_chord_str("Shift+ArrowLeft"), "Shift+←");
        assert_eq!(display_chord_str("Ctrl+Shift+Z"), "Ctrl+Shift+Z");
    }

    #[test]
    fn default_config_binds_every_action() {
        let cfg = plum_config::resolve(None, |_| Ok((String::new(), Default::default()))).unwrap().config;
        let (_, warnings) = Keymap::from_config(&cfg.keybinds);
        assert!(warnings.is_empty(), "{warnings:?}");
        for (_, name, _) in ACTIONS {
            assert!(cfg.keybinds.contains_key(*name), "default.toml lacks keybinds.{name}");
        }
        assert_eq!(cfg.keybinds.len(), ACTIONS.len(), "default.toml has keybinds with no action");
    }
}
