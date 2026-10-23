use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use egui::{CursorIcon, Id, Key, Modifiers, PointerButton, Pos2, Sense, ViewportCommand};
use glam::DVec2;
use plum_config::{Config, ConfigDirs, ConfigWatcher};
use plum_core::selection::GroupOp;
use plum_core::{Board, Change, Command, Element, ElementId, ElementKind, History, Selection, Style, Text, TextAlign, Transform};

use crate::camera::Camera;
use crate::keymap::{Action, Keymap};
use crate::motion::Motion;
use crate::render::{FrameCtx, Renderer, draw_grid, grid_step, measure_text};
use crate::theme::{self, c32};
use crate::tools::{Tool, ToolCtx, ToolEvent, ToolKind, ToolStyles, ViewCtx, snap_point};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Warn,
    Error,
}

pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    pub born: f64,
    pub until: f64,
}

/// Who owns the pointer between press and release.
#[derive(Clone, Copy, PartialEq)]
enum Pointer {
    Idle,
    Tool,
    Pan { last: Pos2 },
}

/// Something waiting on the "save changes?" prompt.
#[derive(Clone, Debug, PartialEq)]
pub enum Pending {
    New,
    Open,
    Close,
}

/// Before-state of a style-panel edit in progress. Only styles are kept,
/// plus full copies of text elements (whose font size may change).
struct StyleEdit {
    styles: Vec<(ElementId, Style)>,
    texts: Vec<Element>,
}

/// An open text editor (new text or editing an existing element).
pub struct TextSession {
    pub target: Option<ElementId>,
    pub transform: Transform,
    pub text: Text,
    pub style: Style,
    pub opened: bool,
}

pub struct PlumApp {
    pub(crate) dirs: ConfigDirs,
    pub(crate) cfg: Config,
    pub(crate) keymap: Keymap,
    pub(crate) styles: ToolStyles,
    font_file: Option<String>,
    reload: Arc<AtomicBool>,
    _watcher: Option<ConfigWatcher>,

    pub(crate) board: Board,
    pub(crate) history: History,
    pub(crate) selection: Selection,
    pub(crate) cam: Camera,
    renderer: Renderer,

    pub(crate) tool_kind: ToolKind,
    pub(crate) tool: Box<dyn Tool>,
    pointer: Pointer,
    pub(crate) text_edit: Option<TextSession>,
    clipboard: Vec<Element>,

    pub(crate) path: Option<PathBuf>,
    allow_close: bool,
    title: String,
    pub(crate) show_perf: bool,
    pub(crate) cpu_ms: f32,
    pub(crate) toasts: Vec<Toast>,
    /// Style edits from the style panel, coalesced into one undo step.
    style_edit: Option<StyleEdit>,
    pub(crate) snap: bool,
    /// Action waiting on the unsaved-changes prompt.
    pub(crate) confirm: Option<Pending>,
    pub(crate) show_settings: bool,
    /// Keybind name currently waiting for a key press in Settings.
    pub(crate) capturing: Option<&'static str>,
    zoom_anchor: Pos2,
    /// Where an animated camera move (zoom buttons, fit, reset) is heading.
    cam_goal: Option<Camera>,
    /// Text box sizes depend on the font; refresh them after a font change or load.
    remeasure_text: bool,
    pub(crate) motion: Motion,
    pub(crate) settings_filter: String,
}

impl PlumApp {
    pub fn new(cc: &eframe::CreationContext, open: Option<PathBuf>) -> Self {
        let ctx = cc.egui_ctx.clone();
        let dirs = ConfigDirs::system();
        let mut toasts = Vec::new();
        if let Err(e) = dirs.ensure_scaffold() {
            toasts.push(Toast { text: format!("Couldn't create config folder: {e}"), kind: ToastKind::Warn, born: 0.0, until: 6.0 });
        }
        let reload = Arc::new(AtomicBool::new(false));
        let watcher = dirs.root.as_ref().and_then(|root| {
            let flag = reload.clone();
            let ctx = ctx.clone();
            plum_config::watch(root, move || {
                flag.store(true, Ordering::Relaxed);
                ctx.request_repaint();
            })
            .ok()
        });
        let fallback = plum_config::resolve(None, |n| Err(plum_config::ConfigError::ThemeNotFound(n.into()))).expect("defaults").config;
        let mut app = Self {
            keymap: Keymap::default(),
            styles: ToolStyles::from_config(&fallback),
            cfg: fallback,
            dirs,
            font_file: None,
            reload,
            _watcher: watcher,
            board: Board::new(),
            history: History::default(),
            selection: Selection::default(),
            cam: Camera::default(),
            renderer: Renderer::default(),
            tool_kind: ToolKind::Select,
            tool: ToolKind::Select.create(),
            pointer: Pointer::Idle,
            text_edit: None,
            clipboard: Vec::new(),
            path: None,
            allow_close: false,
            title: String::new(),
            show_perf: false,
            cpu_ms: 0.0,
            toasts,
            style_edit: None,
            snap: false,
            confirm: None,
            show_settings: false,
            capturing: None,
            zoom_anchor: Pos2::ZERO,
            cam_goal: None,
            remeasure_text: true,
            motion: Motion { speed: 1.0 },
            settings_filter: String::new(),
        };
        app.reload_config(&ctx);
        app.snap = app.cfg.canvas.snap_to_grid;
        if let Some(p) = open {
            app.load_path(&ctx, p);
        }
        app
    }

    // ------------------------------------------------------------------ config

    pub(crate) fn reload_config(&mut self, ctx: &egui::Context) {
        match self.dirs.load() {
            Ok(r) => {
                for w in r.warnings {
                    self.toast(ctx, w, ToastKind::Warn);
                }
                self.apply_config(ctx, r.config);
            }
            Err(e) => self.toast(ctx, format!("Config not applied: {e}"), ToastKind::Error),
        }
    }

    fn apply_config(&mut self, ctx: &egui::Context, cfg: Config) {
        theme::apply_style(ctx, &cfg);
        if self.font_file.as_deref() != Some(cfg.ui.font_file.as_str()) {
            if let Err(e) = theme::apply_fonts(ctx, &cfg) {
                self.toast(ctx, e, ToastKind::Warn);
            }
            self.font_file = Some(cfg.ui.font_file.clone());
            self.remeasure_text = true;
        }
        let (keymap, warnings) = Keymap::from_config(&cfg.keybinds);
        for w in warnings {
            self.toast(ctx, w, ToastKind::Warn);
        }
        self.keymap = keymap;
        self.styles = ToolStyles::from_config(&cfg);
        self.history.set_limit(cfg.performance.undo_limit);
        self.renderer.clear_cache();
        self.motion = Motion { speed: cfg.ui.animation_speed.max(0.0) };
        self.cfg = cfg;
    }

    pub(crate) fn set_theme(&mut self, ctx: &egui::Context, name: &str) {
        if let Err(e) = self.dirs.set_theme(name) {
            self.toast(ctx, format!("Couldn't write config.toml: {e}"), ToastKind::Error);
        }
        self.reload_config(ctx);
    }

    pub(crate) fn toast(&mut self, ctx: &egui::Context, text: impl Into<String>, kind: ToastKind) {
        let secs = if kind == ToastKind::Info { 2.5 } else { 7.0 };
        let now = ctx.input(|i| i.time);
        self.toasts.push(Toast { text: text.into(), kind, born: now, until: now + secs });
    }

    // ------------------------------------------------------------------ tools

    /// Current snap step in world units, if snapping is on.
    pub(crate) fn snap_step(&self) -> Option<f64> {
        self.snap.then(|| grid_step(&self.cam, &self.cfg))
    }

    fn tool_ctx<'a>(&'a mut self, mods: Modifiers, events: &'a mut Vec<ToolEvent>) -> (&'a mut Box<dyn Tool>, ToolCtx<'a>) {
        let snap = self.snap_step();
        (
            &mut self.tool,
            ToolCtx {
                board: &mut self.board,
                history: &mut self.history,
                selection: &mut self.selection,
                cam: &mut self.cam,
                cfg: &self.cfg,
                styles: &self.styles,
                mods,
                events,
                snap,
            },
        )
    }

    /// Run a tool callback, then handle whatever it asked for.
    fn with_tool(&mut self, ctx: &egui::Context, mods: Modifiers, f: impl FnOnce(&mut dyn Tool, &mut ToolCtx)) {
        let mut events = Vec::new();
        {
            let (tool, mut cx) = self.tool_ctx(mods, &mut events);
            f(tool.as_mut(), &mut cx);
        }
        for ev in events {
            match ev {
                ToolEvent::SwitchTool(k) => self.set_tool(ctx, k, true),
                ToolEvent::EditText { target, at } => self.begin_text(ctx, target, at),
                ToolEvent::Notice(text) => self.toast(ctx, text, ToastKind::Info),
            }
        }
    }

    pub(crate) fn cancel_gesture(&mut self, ctx: &egui::Context) {
        if self.tool.busy() {
            self.with_tool(ctx, Modifiers::NONE, |t, cx| t.cancel(cx));
        }
        self.pointer = Pointer::Idle;
    }

    pub(crate) fn set_tool(&mut self, ctx: &egui::Context, kind: ToolKind, keep_selection: bool) {
        self.cancel_gesture(ctx);
        self.commit_text(ctx);
        self.tool_kind = kind;
        self.tool = kind.create();
        if !keep_selection && !matches!(kind, ToolKind::Select | ToolKind::Hand) {
            self.selection.clear();
        }
    }

    // ------------------------------------------------------------------ text

    fn begin_text(&mut self, ctx: &egui::Context, target: Option<ElementId>, at: DVec2) {
        self.commit_text(ctx);
        let existing = target.and_then(|id| self.board.get(id)).and_then(|e| match &e.kind {
            ElementKind::Text(t) => Some((e.id, e.transform, t.clone(), e.style)),
            _ => None,
        });
        self.text_edit = Some(match existing {
            Some((id, transform, text, style)) => {
                self.selection.set([id]);
                TextSession { target: Some(id), transform, text, style, opened: false }
            }
            None => {
                // New text appears at the configured size on screen, whatever the zoom.
                let font_size = (self.styles.font_size as f64 / self.cam.zoom) as f32;
                self.selection.clear();
                let origin = if self.snap { snap_point(at, self.snap_step()) } else { at - DVec2::new(0.0, font_size as f64 * 0.6) };
                TextSession {
                    target: None,
                    transform: Transform::at(origin),
                    text: Text { content: String::new(), font_size, align: TextAlign::Left, size: glam::Vec2::ZERO },
                    style: self.styles.text,
                    opened: false,
                }
            }
        });
    }

    pub(crate) fn commit_text(&mut self, ctx: &egui::Context) {
        let Some(mut s) = self.text_edit.take() else { return };
        let empty = s.text.content.trim().is_empty();
        s.text.size = measure_text(ctx, &s.text);
        match (s.target, empty) {
            (Some(id), true) => {
                let cmd = Command::remove("Delete text", &self.board, &[id]);
                self.history.execute(cmd, &mut self.board);
                self.selection.clear();
            }
            (Some(id), false) => {
                let Some(before) = self.board.get(id).cloned() else { return };
                let mut after = before.clone();
                after.kind = ElementKind::Text(s.text);
                after.style = s.style;
                self.history.execute(Command::modify("Edit text", &[before], &[after]), &mut self.board);
            }
            (None, true) => {}
            (None, false) => {
                let mut el = Element::new(ElementKind::Text(s.text), s.transform, s.style);
                el.id = self.board.alloc_id();
                let id = el.id;
                let mut cmd = Command::new("Add text");
                cmd.changes.push(Change::Insert { at: self.board.len(), element: el });
                self.history.execute(cmd, &mut self.board);
                self.selection.set([id]);
                if self.tool_kind == ToolKind::Text {
                    self.tool_kind = ToolKind::Select;
                    self.tool = ToolKind::Select.create();
                }
            }
        }
    }

    // ------------------------------------------------------------------ actions

    pub(crate) fn do_action(&mut self, ctx: &egui::Context, action: Action) {
        // Commands that change the board end any running gesture first.
        if !matches!(
            action,
            Action::PerfOverlay | Action::ZoomIn | Action::ZoomOut | Action::ZoomReset | Action::ZoomFit | Action::ResetPosition | Action::ToggleSnap | Action::Settings
        ) {
            self.cancel_gesture(ctx);
        }
        let center = self.cam.viewport.center();
        let (zmin, zmax) = (self.cfg.canvas.min_zoom, self.cfg.canvas.max_zoom);
        match action {
            Action::Tool(k) => self.set_tool(ctx, k, false),
            Action::Undo => {
                self.commit_text(ctx);
                self.history.undo(&mut self.board);
                self.selection.retain_existing(&self.board);
            }
            Action::Redo => {
                self.commit_text(ctx);
                self.history.redo(&mut self.board);
                self.selection.retain_existing(&self.board);
            }
            Action::SelectAll => {
                if self.tool_kind != ToolKind::Select {
                    self.set_tool(ctx, ToolKind::Select, true);
                }
                self.selection.set(self.board.elements().iter().map(|e| e.id));
            }
            Action::Delete => self.delete_selection(),
            Action::Duplicate => {
                let els = self.selection.snapshot(&self.board);
                let off = DVec2::splat(self.snap_step().unwrap_or(16.0 / self.cam.zoom));
                self.insert_copies(&els, off, "Duplicate");
            }
            Action::Copy => self.clipboard = self.selection.snapshot(&self.board),
            Action::Cut => {
                self.clipboard = self.selection.snapshot(&self.board);
                self.delete_selection();
            }
            Action::Paste => {
                let els = self.clipboard.clone();
                if let Some(obb) = plum_core::selection::group_obb(&els) {
                    let target = ctx.input(|i| i.pointer.hover_pos()).filter(|p| self.cam.viewport.contains(*p)).unwrap_or(center);
                    let off = snap_point(self.cam.screen_to_world(target) - obb.center, self.snap_step());
                    self.insert_copies(&els, off, "Paste");
                }
            }
            Action::FlipH | Action::FlipV | Action::RotateCw | Action::RotateCcw => {
                let Some(obb) = self.selection.obb(&self.board) else { return };
                let op = match action {
                    Action::FlipH => GroupOp::FlipH { pivot_x: obb.center.x },
                    Action::FlipV => GroupOp::FlipV { pivot_y: obb.center.y },
                    Action::RotateCw => GroupOp::Rotate { pivot: obb.center, angle: std::f64::consts::FRAC_PI_2 },
                    _ => GroupOp::Rotate { pivot: obb.center, angle: -std::f64::consts::FRAC_PI_2 },
                };
                let label = match action {
                    Action::FlipH | Action::FlipV => "Flip",
                    _ => "Rotate",
                };
                self.transform_selection(label, op);
            }
            Action::BringForward => self.reorder(1),
            Action::SendBackward => self.reorder(-1),
            Action::BringToFront => self.reorder(isize::MAX),
            Action::SendToBack => self.reorder(isize::MIN),
            Action::ZoomIn => self.animate_camera(|c| c.zoom_at(center, 1.25, zmin, zmax)),
            Action::ZoomOut => self.animate_camera(|c| c.zoom_at(center, 0.8, zmin, zmax)),
            Action::ZoomReset => self.animate_camera(|c| c.zoom_at(center, 1.0 / c.zoom, zmin, zmax)),
            Action::ZoomFit => {
                let area = if self.selection.is_empty() {
                    self.board.bounds()
                } else {
                    self.selection.elements(&self.board).iter().map(|e| e.world_aabb()).reduce(|a, b| a.union(&b))
                };
                if let Some(a) = area {
                    self.animate_camera(|c| c.fit(&a, 64.0, zmin, zmax));
                }
            }
            Action::ResetPosition => self.animate_camera(|c| c.center = DVec2::ZERO),
            Action::ToggleSnap => self.snap = !self.snap,
            Action::Settings => self.show_settings = !self.show_settings,
            Action::New => self.request(ctx, Pending::New),
            Action::Open => self.request(ctx, Pending::Open),
            Action::Save => {
                self.save(ctx);
            }
            Action::SaveAs => {
                self.save_as(ctx);
            }
            Action::PerfOverlay => self.show_perf = !self.show_perf,
            Action::Deselect => {
                if self.text_edit.is_some() {
                    self.commit_text(ctx);
                } else if !self.selection.is_empty() {
                    self.selection.clear();
                } else if self.tool_kind != ToolKind::Select {
                    self.set_tool(ctx, ToolKind::Select, false);
                }
            }
            Action::Nudge { dx, dy, large } => {
                let px = if large { self.cfg.canvas.nudge_large } else { self.cfg.canvas.nudge };
                // With snapping on, nudges move whole grid cells.
                let step = match self.snap_step() {
                    Some(s) => s * if large { self.cfg.grid.major_every.max(1) as f64 } else { 1.0 },
                    None => px / self.cam.zoom,
                };
                let d = DVec2::new(dx as f64, dy as f64) * step;
                self.transform_selection("Nudge", GroupOp::Translate(d));
            }
        }
    }

    /// Glide the camera to wherever `f` would put it. Repeated presses build on
    /// the pending destination, so tapping zoom-in three times zooms three steps.
    fn animate_camera(&mut self, f: impl FnOnce(&mut Camera)) {
        let mut goal = self.cam_goal.unwrap_or(self.cam);
        goal.viewport = self.cam.viewport;
        f(&mut goal);
        self.cam_goal = Some(goal);
    }

    /// Apply a group transform to the selection as one undoable step.
    fn transform_selection(&mut self, label: &'static str, op: GroupOp) {
        let items = self.selection.transforms(&self.board);
        let cmd = Command::transforms(label, items.iter().map(|(id, t)| (*id, *t, op.transform(t))));
        self.history.execute(cmd, &mut self.board);
    }

    fn delete_selection(&mut self) {
        let ids = self.selection.ordered(&self.board);
        if ids.is_empty() {
            return;
        }
        self.history.execute(Command::remove("Delete", &self.board, &ids), &mut self.board);
        self.selection.clear();
    }

    fn insert_copies(&mut self, els: &[Element], offset: DVec2, label: &'static str) {
        if els.is_empty() {
            return;
        }
        let mut cmd = Command::new(label);
        let mut ids = Vec::new();
        for (i, el) in els.iter().enumerate() {
            let mut c = el.clone();
            c.id = self.board.alloc_id();
            c.transform.pos += offset;
            ids.push(c.id);
            cmd.changes.push(Change::Insert { at: self.board.len() + i, element: c });
        }
        self.history.execute(cmd, &mut self.board);
        self.selection.set(ids);
    }

    /// Move the selection in z. `step` of ±1 swaps with neighbours; MIN/MAX go to the ends.
    fn reorder(&mut self, step: isize) {
        let mut ids = self.selection.ordered(&self.board);
        if ids.is_empty() {
            return;
        }
        let n = self.board.len();
        let mut cmd = Command::new("Reorder");
        // Process from the side we're moving towards so members don't leapfrog each other.
        if step > 0 {
            ids.reverse();
        }
        for (rank, id) in ids.iter().enumerate() {
            let Some(from) = self.board.z_index(*id) else { continue };
            let to = match step {
                isize::MAX => n - 1 - rank,
                isize::MIN => rank,
                s => {
                    let t = (from as isize + s).clamp(0, n as isize - 1) as usize;
                    // Don't swap past another selected element.
                    if self.selection.contains(self.board.elements()[t].id) { from } else { t }
                }
            };
            if from != to {
                self.board.move_z(*id, to);
                cmd.changes.push(Change::MoveZ { id: *id, from, to });
            }
        }
        self.history.push_applied(cmd);
    }

    fn style_edit(&mut self) -> &mut StyleEdit {
        let (sel, board) = (&self.selection, &self.board);
        self.style_edit.get_or_insert_with(|| StyleEdit {
            styles: sel.elements(board).iter().map(|e| (e.id, e.style)).collect(),
            texts: sel.elements(board).into_iter().filter(|e| matches!(e.kind, ElementKind::Text(_))).cloned().collect(),
        })
    }

    /// Change the style of the selection (live), coalescing until `finish_style_edit`.
    pub(crate) fn edit_selection_style(&mut self, f: impl Fn(&mut Style)) {
        let styles = self.style_edit().styles.clone();
        for (id, mut s) in styles {
            f(&mut s);
            self.board.set_style(id, s);
        }
    }

    pub(crate) fn edit_selection_font(&mut self, size: f32, ctx: &egui::Context) {
        let texts = self.style_edit().texts.clone();
        for el in texts {
            self.board.update(el.id, |e| {
                if let ElementKind::Text(t) = &mut e.kind {
                    t.font_size = size;
                    t.size = measure_text(ctx, t);
                }
            });
        }
    }

    pub(crate) fn finish_style_edit(&mut self) {
        let Some(edit) = self.style_edit.take() else { return };
        let board = &self.board;
        let mut cmd = Command::new("Style");
        // Text elements whose font changed get a full before/after (it includes the style too).
        let text_before: Vec<Element> = edit.texts.into_iter().filter(|b| board.get(b.id).is_some_and(|a| a.kind != b.kind)).collect();
        let text_after: Vec<Element> = text_before.iter().filter_map(|b| board.get(b.id).cloned()).collect();
        cmd.changes.extend(Command::modify("", &text_before, &text_after).changes);
        for (id, before) in edit.styles {
            if text_before.iter().any(|e| e.id == id) {
                continue;
            }
            if let Some(after) = board.get(id).map(|e| e.style).filter(|a| *a != before) {
                cmd.changes.push(Change::Style { id, before, after });
            }
        }
        self.history.push_applied(cmd);
    }

    pub(crate) fn stress_test(&mut self, n: usize) {
        let mut seed = 0x9e3779b97f4a7c15u64 ^ self.board.len() as u64;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let colors = self.cfg.palette.colors.clone();
        let mut cmd = Command::new("Stress test");
        let base = self.board.len();
        for i in 0..n {
            let center = self.cam.center + DVec2::new(rnd() - 0.5, rnd() - 0.5) * 100_000.0;
            let mut p = glam::Vec2::ZERO;
            let pts: Vec<glam::Vec2> = (0..40)
                .map(|_| {
                    p += glam::Vec2::new((rnd() as f32 - 0.5) * 20.0, (rnd() as f32 - 0.5) * 20.0);
                    p
                })
                .collect();
            let (stroke, off) = plum_core::Stroke::finish(&pts, 0.0);
            let color = plum_core::Paint::Palette((i % colors.len().max(1)) as u8);
            let style = Style { stroke: color, width: 2.0, ..Default::default() };
            let mut el = Element::new(ElementKind::Stroke(stroke), Transform::at(center + off.as_dvec2()), style);
            el.id = self.board.alloc_id();
            cmd.changes.push(Change::Insert { at: base + i, element: el });
        }
        self.history.execute(cmd, &mut self.board);
    }

    // ------------------------------------------------------------------ files

    pub(crate) fn display_name(&self) -> String {
        self.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Untitled".into())
    }

    /// Run `p` now, or first ask (in-app) whether to save unsaved changes.
    pub(crate) fn request(&mut self, ctx: &egui::Context, p: Pending) {
        self.commit_text(ctx);
        if self.history.is_dirty() {
            self.confirm = Some(p);
        } else {
            self.run_pending(ctx, p);
        }
    }

    /// Answer to the unsaved-changes prompt: `Some(true)` save, `Some(false)` discard, `None` cancel.
    pub(crate) fn resolve_pending(&mut self, ctx: &egui::Context, save: Option<bool>) {
        let Some(p) = self.confirm.take() else { return };
        let proceed = match save {
            Some(true) => self.save(ctx),
            Some(false) => true,
            None => false,
        };
        if proceed {
            self.run_pending(ctx, p);
        }
    }

    fn run_pending(&mut self, ctx: &egui::Context, p: Pending) {
        match p {
            Pending::New => self.replace_board(Board::new(), None),
            Pending::Open => self.open_dialog(ctx),
            Pending::Close => {
                self.allow_close = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }
    }

    fn replace_board(&mut self, board: Board, path: Option<PathBuf>) {
        self.cam.center = board.meta.view_center;
        self.cam.zoom = board.meta.view_zoom.clamp(self.cfg.canvas.min_zoom, self.cfg.canvas.max_zoom);
        self.board = board;
        self.history = History::new(self.cfg.performance.undo_limit);
        self.selection.clear();
        self.renderer.clear_cache();
        self.path = path;
        self.tool = self.tool_kind.create();
        self.pointer = Pointer::Idle;
        self.remeasure_text = true;
    }

    /// Re-measure every text box with the current font. Sizes are derived
    /// data, so this isn't an undoable edit.
    fn remeasure_texts(&mut self, ctx: &egui::Context) {
        let updates: Vec<(ElementId, glam::Vec2)> = self
            .board
            .elements()
            .iter()
            .filter_map(|e| match &e.kind {
                ElementKind::Text(t) => Some((e.id, measure_text(ctx, t))),
                _ => None,
            })
            .collect();
        for (id, size) in updates {
            self.board.update(id, |e| {
                if let ElementKind::Text(t) = &mut e.kind {
                    t.size = size;
                }
            });
        }
    }

    fn open_dialog(&mut self, ctx: &egui::Context) {
        let picked = rfd::FileDialog::new().add_filter("Plum whiteboard", &[plum_format::EXTENSION]).set_title("Open whiteboard").pick_file();
        if let Some(p) = picked {
            self.load_path(ctx, p);
        }
    }

    fn load_path(&mut self, ctx: &egui::Context, path: PathBuf) {
        match plum_format::load_from_path(&path) {
            Ok(loaded) => {
                for w in loaded.warnings {
                    self.toast(ctx, w, ToastKind::Warn);
                }
                self.replace_board(loaded.board, Some(path));
            }
            Err(e) => self.toast(ctx, format!("Couldn't open {}: {e}", path.display()), ToastKind::Error),
        }
    }

    pub(crate) fn save(&mut self, ctx: &egui::Context) -> bool {
        match self.path.clone() {
            Some(p) => self.write_to(ctx, p),
            None => self.save_as(ctx),
        }
    }

    pub(crate) fn save_as(&mut self, ctx: &egui::Context) -> bool {
        let picked = rfd::FileDialog::new()
            .add_filter("Plum whiteboard", &[plum_format::EXTENSION])
            .set_file_name(format!("{}.{}", self.display_name(), plum_format::EXTENSION))
            .set_title("Save whiteboard")
            .save_file();
        match picked {
            Some(mut p) => {
                if p.extension().is_none_or(|e| e != plum_format::EXTENSION) {
                    p.set_extension(plum_format::EXTENSION);
                }
                self.write_to(ctx, p)
            }
            None => false,
        }
    }

    fn write_to(&mut self, ctx: &egui::Context, path: PathBuf) -> bool {
        self.commit_text(ctx);
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let meta = &mut self.board.meta;
        if meta.created == 0 {
            meta.created = now;
        }
        meta.modified = now;
        meta.view_center = self.cam.center;
        meta.view_zoom = self.cam.zoom;
        if let Some(stem) = path.file_stem() {
            meta.name = stem.to_string_lossy().into_owned();
        }
        match plum_format::save_to_path(&self.board, &path) {
            Ok(()) => {
                self.history.mark_saved();
                self.path = Some(path);
                self.toast(ctx, "Saved", ToastKind::Info);
                true
            }
            Err(e) => {
                self.toast(ctx, format!("Save failed: {e}"), ToastKind::Error);
                false
            }
        }
    }

    // ------------------------------------------------------------------ frame

    fn handle_close(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().close_requested()) || self.allow_close {
            return;
        }
        self.commit_text(ctx);
        if self.history.is_dirty() {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.confirm = Some(Pending::Close);
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let title = format!("{}{} — Plum", self.display_name(), if self.history.is_dirty() { " •" } else { "" });
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    fn handle_canvas_input(&mut self, ctx: &egui::Context, canvas: &egui::Response) {
        let over = canvas.contains_pointer();
        let (zmin, zmax) = (self.cfg.canvas.min_zoom, self.cfg.canvas.max_zoom);

        // Wheel pans, Ctrl+wheel / pinch zooms around the pointer. Zooming past
        // the limits stretches a little, then springs back once input stops.
        let (zoom, scroll, hover, dt) = ctx.input(|i| (i.zoom_delta(), i.smooth_scroll_delta, i.pointer.hover_pos(), i.stable_dt));
        let mut zooming = false;
        if over {
            if zoom != 1.0
                && let Some(h) = hover
            {
                let f = (zoom as f64).powf(self.cfg.canvas.zoom_speed);
                self.cam_goal = None;
                self.cam.zoom_soft(h, f, zmin, zmax, self.cfg.canvas.zoom_overshoot);
                self.zoom_anchor = h;
                zooming = true;
            }
            if scroll != egui::Vec2::ZERO {
                self.cam_goal = None;
                self.cam.pan(scroll * self.cfg.canvas.pan_speed as f32);
            }
        }
        if let Some(goal) = self.cam_goal {
            if self.cam.blend_toward(&goal, self.motion.step(dt, crate::motion::SMOOTH) as f64) {
                self.cam_goal = None;
            } else {
                ctx.request_repaint();
            }
        } else if !zooming {
            // Speed 0 = no animation: settle in one go.
            let settle_dt = if self.motion.speed > 0.0 { dt as f64 * self.motion.speed as f64 } else { 1.0 };
            if self.cam.settle(self.zoom_anchor, zmin, zmax, settle_dt) {
                ctx.request_repaint();
            }
        }

        // Never call back into `ctx` from inside an `input` closure: it holds the
        // context lock, so a nested call deadlocks.
        let wants_keyboard = ctx.egui_wants_keyboard_input();
        let (pos, mods, pressed, released, mid_pressed, mid_released, space, dbl) = ctx.input(|i| {
            (
                i.pointer.interact_pos(),
                i.modifiers,
                i.pointer.primary_pressed(),
                i.pointer.primary_released(),
                i.pointer.button_pressed(PointerButton::Middle),
                i.pointer.button_released(PointerButton::Middle),
                i.key_down(Key::Space) && !wants_keyboard,
                i.pointer.button_double_clicked(PointerButton::Primary),
            )
        });
        let Some(pos) = pos else { return };

        match self.pointer {
            Pointer::Idle => {
                if over && (mid_pressed || (pressed && space)) {
                    self.pointer = Pointer::Pan { last: pos };
                } else if over && pressed {
                    if self.text_edit.is_some() {
                        // A click away from the editor just commits it.
                        self.commit_text(ctx);
                    } else {
                        self.finish_style_edit();
                        self.pointer = Pointer::Tool;
                        self.with_tool(ctx, mods, |t, cx| t.press(cx, pos));
                    }
                }
            }
            Pointer::Pan { last } => {
                self.cam_goal = None;
                self.cam.pan(pos - last);
                self.pointer = Pointer::Pan { last: pos };
                if mid_released || released {
                    self.pointer = Pointer::Idle;
                }
            }
            Pointer::Tool => {
                if released {
                    self.with_tool(ctx, mods, |t, cx| {
                        t.drag(cx, pos);
                        t.release(cx, pos);
                    });
                    self.pointer = Pointer::Idle;
                } else {
                    self.with_tool(ctx, mods, |t, cx| t.drag(cx, pos));
                }
            }
        }
        if over && dbl && self.pointer == Pointer::Idle && self.text_edit.is_none() {
            self.with_tool(ctx, mods, |t, cx| t.double_click(cx, pos));
        }
    }

    fn cursor(&self, ctx: &egui::Context, canvas: &egui::Response) -> Option<CursorIcon> {
        if !canvas.contains_pointer() && self.pointer == Pointer::Idle {
            return None;
        }
        let space = ctx.input(|i| i.key_down(Key::Space)) && !ctx.egui_wants_keyboard_input();
        Some(match self.pointer {
            Pointer::Pan { .. } => CursorIcon::Grabbing,
            Pointer::Idle if space => CursorIcon::Grab,
            _ => self.tool.cursor(&self.view_ctx(ctx)),
        })
    }

    fn view_ctx(&self, ctx: &egui::Context) -> ViewCtx<'_> {
        ViewCtx {
            board: &self.board,
            selection: &self.selection,
            cam: &self.cam,
            cfg: &self.cfg,
            hover: ctx.input(|i| i.pointer.hover_pos()),
        }
    }

    fn paint_canvas(&mut self, ctx: &egui::Context, painter: &egui::Painter) {
        // The background is the window clear color (see `clear_color`); no full-screen fill needed.
        draw_grid(painter, &self.cam, &self.cfg);
        let fc = FrameCtx {
            ctx,
            cfg: &self.cfg,
            cam: &self.cam,
            ppp: ctx.pixels_per_point(),
            font_tex_size: ctx.fonts(|f| f.font_image_size()),
            atlas_fill: ctx.fonts(|f| f.font_atlas_fill_ratio()),
        };
        let hidden = self.text_edit.as_ref().and_then(|t| t.target);
        let previews: Vec<&Element> = self.tool.preview().into_iter().collect();
        self.renderer.draw_board(painter, &fc, &self.board, hidden, &previews);
        if self.text_edit.is_none() {
            self.tool.paint(&self.view_ctx(ctx), painter);
        }
    }

    /// Settings is waiting for a key: the next key press becomes that action's binding.
    fn capture_keybind(&mut self, ctx: &egui::Context) {
        let Some(name) = self.capturing else { return };
        // Modifier keys arrive as key presses of their own; wait for the real key.
        let is_modifier = |k: &Key| {
            matches!(
                k,
                Key::ShiftLeft | Key::ShiftRight | Key::ControlLeft | Key::ControlRight | Key::AltLeft | Key::AltRight | Key::SuperLeft | Key::SuperRight
            )
        };
        let pressed = ctx.input_mut(|i| {
            let found = i.events.iter().position(|e| matches!(e, egui::Event::Key { key, pressed: true, .. } if !is_modifier(key)));
            found.map(|at| match i.events.remove(at) {
                egui::Event::Key { key, modifiers, .. } => (key, modifiers),
                _ => unreachable!(),
            })
        });
        let Some((key, mods)) = pressed else { return };
        self.capturing = None;
        if key == Key::Escape && mods.is_none() {
            return;
        }
        let chord = crate::keymap::capture_chord(key, mods);
        self.write_keybind(ctx, name, Some(&[chord]));
    }

    /// `None` restores the default binding; `Some(&[])` unbinds.
    pub(crate) fn write_keybind(&mut self, ctx: &egui::Context, name: &str, chords: Option<&[String]>) {
        match self.dirs.set_keybind(name, chords) {
            Ok(()) => self.reload_config(ctx),
            Err(e) => self.toast(ctx, format!("Couldn't save keybind: {e}"), ToastKind::Error),
        }
    }

    pub(crate) fn reset_keybinds(&mut self, ctx: &egui::Context) {
        match self.dirs.reset_keybinds() {
            Ok(()) => self.reload_config(ctx),
            Err(e) => self.toast(ctx, format!("Couldn't reset keybinds: {e}"), ToastKind::Error),
        }
    }

    pub(crate) fn render_stats(&self) -> crate::render::Stats {
        self.renderer.stats
    }
}

impl eframe::App for PlumApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let started = Instant::now();
        let ctx = ui.ctx().clone();

        if self.reload.swap(false, Ordering::Relaxed) {
            self.reload_config(&ctx);
        }
        self.handle_close(&ctx);
        if std::mem::take(&mut self.remeasure_text) {
            self.remeasure_texts(&ctx);
        }

        let rect = ui.max_rect();
        self.cam.viewport = rect;
        let canvas = ui.interact(rect, Id::unique("plum-canvas"), Sense::click_and_drag());

        if self.capturing.is_some() {
            self.capture_keybind(&ctx);
        } else if !ctx.egui_wants_keyboard_input() && self.confirm.is_none() {
            for action in self.keymap.take_actions(&ctx) {
                self.do_action(&ctx, action);
            }
        }
        self.handle_canvas_input(&ctx, &canvas);

        let painter = ui.painter_at(rect);
        self.paint_canvas(&ctx, &painter);
        crate::ui::text_editor(self, &ctx);
        crate::ui::confirm_modal(self, &ctx);
        crate::ui::settings_window(self, &ctx);
        crate::ui::chrome(self, &ctx);

        if let Some(c) = self.cursor(&ctx, &canvas) {
            ctx.set_cursor_icon(c);
        }
        self.update_title(&ctx);
        self.cpu_ms = started.elapsed().as_secs_f32() * 1000.0;
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        c32(self.cfg.colors.canvas).to_normalized_gamma_f32()
    }
}
