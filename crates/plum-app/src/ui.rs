//! Floating chrome around the canvas: toolbar, context bar, style panel, zoom
//! controls, toasts and dialogs. Everything is themed from the config and
//! animated through [`Motion`]; widgets are drawn by hand so they match the
//! board's look rather than egui's defaults.

use std::ops::RangeInclusive;

use egui::{Align2, Area, Color32, FontId, Id, Order, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2};
use plum_config::ToolbarPosition;
use plum_core::{ElementKind, Paint};

use crate::app::{PlumApp, ToastKind};
use crate::icons::Icon;
use crate::keymap::{Action, display_chord_str};
use crate::motion::{SMOOTH, SNAPPY, ease_out};
use crate::theme::{self, accent_soft, c32, lerp};
use crate::tools::{StyleSlot, ToolKind, select::selection_screen_rect};

pub fn chrome(app: &mut PlumApp, ctx: &egui::Context) {
    snap_toggle(app, ctx);
    toolbar(app, ctx);
    if app.cfg.ui.show_context_bar {
        context_bar(app, ctx);
    }
    if app.cfg.ui.show_style_panel {
        style_panel(app, ctx);
    }
    zoom_controls(app, ctx);
    if app.show_perf {
        perf_overlay(app, ctx);
    }
    toasts(app, ctx);
}

// ---------------------------------------------------------------- building blocks

fn muted(app: &PlumApp) -> Color32 {
    c32(app.cfg.colors.text_muted)
}

/// Radius for things nested inside an island, so the curves stay concentric.
fn inner_radius(app: &PlumApp) -> egui::CornerRadius {
    theme::radius((app.cfg.ui.corner_radius - 4.0).max(2.0))
}

fn mono(app: &PlumApp, scale: f32) -> FontId {
    FontId::monospace(app.cfg.ui.font_size * scale)
}

/// Small uppercase section label, e.g. "STROKE".
fn section(ui: &mut Ui, app: &PlumApp, text: &str) {
    ui.label(egui::RichText::new(text.to_uppercase()).font(mono(app, 0.74)).color(muted(app)).extra_letter_spacing(0.6));
}

/// A keyboard-shortcut chip.
fn kbd(ui: &mut Ui, app: &PlumApp, text: &str) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(text.to_string(), mono(app, 0.8), muted(app));
    let (rect, resp) = ui.allocate_exact_size(galley.size() + vec2(10.0, 5.0), Sense::hover());
    let p = ui.painter();
    p.rect(rect, theme::radius(5.0), c32(app.cfg.colors.button_hover), Stroke::new(1.0, c32(app.cfg.colors.panel_border)), StrokeKind::Inside);
    p.galley(rect.center() - galley.size() / 2.0, galley, muted(app));
    resp
}

/// Tooltip body: the name plus its shortcut chip.
fn tip_ui(ui: &mut Ui, app: &PlumApp, name: &str, action: Option<Action>) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.label(name);
        if let Some(k) = action.and_then(|a| app.keymap.label(a)) {
            kbd(ui, app, &k);
        }
    });
}

/// Square icon button with an animated hover wash and a little press dip.
fn icon_button(ui: &mut Ui, app: &PlumApp, icon: Icon, size: f32, active: bool, enabled: bool) -> egui::Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), sense);
    let ctx = ui.ctx();
    let m = app.motion;
    let hover = m.fade(ctx, resp.id.with("h"), enabled && resp.hovered(), SNAPPY);
    let press = m.fade(ctx, resp.id.with("p"), enabled && resp.is_pointer_button_down_on(), 32.0);
    let col = &app.cfg.colors;
    if hover > 0.0 && !active {
        ui.painter().rect_filled(rect, inner_radius(app), c32(col.button_hover).gamma_multiply(hover));
    }
    let rest = lerp(muted(app), c32(col.text), 0.65);
    let fg = if active {
        c32(col.accent)
    } else if enabled {
        lerp(rest, c32(col.text), hover)
    } else {
        muted(app).gamma_multiply(0.45)
    };
    let s = app.cfg.ui.icon_size * (size / app.cfg.ui.button_size).min(1.0) * (1.0 - 0.1 * press);
    icon.paint(ui.painter(), Rect::from_center_size(rect.center(), vec2(s, s)), fg);
    resp
}

fn divider(ui: &mut Ui, app: &PlumApp, vertical: bool) {
    let len = app.cfg.ui.button_size * 0.5;
    let size = if vertical { vec2(len, 9.0) } else { vec2(9.0, len) };
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let (a, b) = if vertical { (rect.left_center(), rect.right_center()) } else { (rect.center_top(), rect.center_bottom()) };
    ui.painter().line_segment([a, b], Stroke::new(1.0, c32(app.cfg.colors.panel_border)));
}

/// Pill switch with a sliding knob.
fn switch(ui: &mut Ui, app: &PlumApp, on: bool, id: Id) -> Rect {
    let (rect, _) = ui.allocate_exact_size(vec2(28.0, 16.0), Sense::hover());
    let t = app.motion.fade(ui.ctx(), id.with("knob"), on, SNAPPY);
    let col = &app.cfg.colors;
    let track = lerp(muted(app).gamma_multiply(0.35), c32(col.accent), t);
    let p = ui.painter();
    p.rect_filled(rect, theme::radius(8.0), track);
    let x = egui::lerp(rect.left() + 8.0..=rect.right() - 8.0, t);
    p.circle_filled(pos2(x, rect.center().y), 6.0, Color32::WHITE);
    rect
}

/// Round color swatch; `None` is "no fill". Selected swatches get a ring.
fn swatch(ui: &mut Ui, app: &PlumApp, color: Option<Color32>, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
    let m = app.motion;
    let sel = m.fade(ui.ctx(), resp.id.with("s"), selected, SNAPPY);
    let hover = m.fade(ui.ctx(), resp.id.with("h"), resp.hovered(), SNAPPY);
    let (c, r) = (rect.center(), 7.5 + hover);
    let p = ui.painter();
    let border = Stroke::new(1.0, c32(app.cfg.colors.panel_border));
    match color {
        Some(fill) => {
            p.circle_filled(c, r, fill);
            p.circle_stroke(c, r, border);
        }
        None => {
            p.circle_stroke(c, r, Stroke::new(1.2, muted(app)));
            let d = vec2(r, -r) * std::f32::consts::FRAC_1_SQRT_2;
            p.line_segment([c - d, c + d], Stroke::new(1.4, c32(app.cfg.colors.error)));
        }
    }
    if sel > 0.0 {
        p.circle_stroke(c, r + 3.0 + (1.0 - sel) * 2.0, Stroke::new(1.6, c32(app.cfg.colors.accent).gamma_multiply(sel)));
    }
    resp
}

/// Thin slider: rounded rail, accent fill, knob that grows while hovered or dragged.
fn slider(ui: &mut Ui, app: &PlumApp, value: &mut f32, range: RangeInclusive<f32>, log: bool) -> egui::Response {
    let (lo, hi) = (*range.start(), *range.end());
    let to_t = |v: f32| if log { (v.max(lo).ln() - lo.ln()) / (hi.ln() - lo.ln()) } else { (v - lo) / (hi - lo) };
    let from_t = |t: f32| if log { (lo.ln() + t * (hi.ln() - lo.ln())).exp() } else { lo + t * (hi - lo) };
    let (rect, mut resp) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::click_and_drag());
    let rail = rect.shrink2(vec2(7.0, 0.0));
    if resp.is_pointer_button_down_on()
        && let Some(p) = resp.interact_pointer_pos()
    {
        let v = from_t(((p.x - rail.left()) / rail.width()).clamp(0.0, 1.0));
        if v != *value {
            *value = v;
            resp.mark_changed();
        }
    }
    let t = to_t(*value).clamp(0.0, 1.0);
    let grab = app.motion.fade(ui.ctx(), resp.id.with("g"), resp.hovered() || resp.dragged(), SNAPPY);
    let col = &app.cfg.colors;
    let p = ui.painter();
    let y = rect.center().y;
    let x = rail.left() + rail.width() * t;
    p.rect_filled(Rect::from_x_y_ranges(rail.x_range(), y - 2.0..=y + 2.0), theme::radius(2.0), muted(app).gamma_multiply(0.25));
    p.rect_filled(Rect::from_x_y_ranges(rail.left()..=x, y - 2.0..=y + 2.0), theme::radius(2.0), c32(col.accent));
    p.circle_filled(pos2(x, y), 6.0 + 1.5 * grab, c32(col.handle_fill));
    p.circle_stroke(pos2(x, y), 6.0 + 1.5 * grab, Stroke::new(2.0, c32(col.accent)));
    resp
}

/// "LABEL ............ value" row above a slider.
fn slider_header(ui: &mut Ui, app: &PlumApp, label: &str, value: String) {
    ui.horizontal(|ui| {
        section(ui, app, label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(value).font(mono(app, 0.8)).color(c32(app.cfg.colors.text)));
        });
    });
}

// ---------------------------------------------------------------- panels

fn snap_toggle(app: &mut PlumApp, ctx: &egui::Context) {
    let m = app.cfg.ui.margin;
    let mut toggle = false;
    Area::new(Id::unique("plum-snap")).order(Order::Middle).anchor(Align2::LEFT_TOP, vec2(m, m)).show(ctx, |ui| {
        theme::panel_frame(&app.cfg).inner_margin(egui::Margin::symmetric(10, 7)).show(ui, |ui| {
            let r = ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                switch(ui, app, app.snap, Id::unique("plum-snap-switch"));
                ui.label("Snap to grid");
            });
            let resp = ui.interact(r.response.rect, Id::unique("plum-snap-row"), Sense::click());
            if resp.clicked() {
                toggle = true;
            }
            resp.on_hover_ui(|ui| tip_ui(ui, app, "Snap shapes, lines, text and moves to the grid", Some(Action::ToggleSnap)));
        });
    });
    if toggle {
        app.snap = !app.snap;
    }
}

fn toolbar(app: &mut PlumApp, ctx: &egui::Context) {
    let m = app.cfg.ui.margin;
    let (align, offset, vertical) = match app.cfg.ui.toolbar_position {
        ToolbarPosition::Left => (Align2::LEFT_CENTER, vec2(m, 0.0), true),
        ToolbarPosition::Right => (Align2::RIGHT_CENTER, vec2(-m, 0.0), true),
        ToolbarPosition::Top => (Align2::CENTER_TOP, vec2(0.0, m), false),
        ToolbarPosition::Bottom => (Align2::CENTER_BOTTOM, vec2(0.0, -m), false),
    };
    let mut clicked_tool = None;
    let mut action = None;
    let size = app.cfg.ui.button_size;
    Area::new(Id::unique("plum-toolbar")).order(Order::Middle).anchor(align, offset).show(ctx, |ui| {
        theme::panel_frame(&app.cfg).show(ui, |ui| {
            let layout = if vertical { egui::Layout::top_down(egui::Align::Center) } else { egui::Layout::left_to_right(egui::Align::Center) };
            ui.with_layout(layout, |ui| {
                ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
                // Reserve the slot behind the buttons for the sliding "active tool" pill.
                let pill = ui.painter().add(Shape::Noop);
                let mut active_rect = None;
                for kind in ToolKind::ALL {
                    let r = icon_button(ui, app, kind.icon(), size, app.tool_kind == kind, true);
                    if app.tool_kind == kind {
                        active_rect = Some(r.rect);
                    }
                    if r.on_hover_ui(|ui| tip_ui(ui, app, kind.label(), Some(Action::Tool(kind)))).clicked() {
                        clicked_tool = Some(kind);
                    }
                    if matches!(kind, ToolKind::Hand | ToolKind::Arrow) {
                        divider(ui, app, vertical);
                    }
                }
                divider(ui, app, vertical);
                for (icon, name, a, enabled) in [(Icon::Undo, "Undo", Action::Undo, app.history.can_undo()), (Icon::Redo, "Redo", Action::Redo, app.history.can_redo())] {
                    if icon_button(ui, app, icon, size, false, enabled).on_hover_ui(|ui| tip_ui(ui, app, name, Some(a))).clicked() {
                        action = Some(a);
                    }
                }
                divider(ui, app, vertical);
                let menu = icon_button(ui, app, Icon::Menu, size, false, true);
                main_menu(app, &menu, &mut action);
                if let Some(r) = active_rect {
                    let id = Id::unique("plum-tool-pill");
                    let x = app.motion.approach(ctx, id.with("x"), r.min.x, SNAPPY);
                    let y = app.motion.approach(ctx, id.with("y"), r.min.y, SNAPPY);
                    let rect = Rect::from_min_size(pos2(x, y), r.size());
                    ui.painter().set(pill, Shape::rect_filled(rect, inner_radius(app), accent_soft(&app.cfg)));
                }
            });
        });
    });
    if let Some(k) = clicked_tool {
        app.set_tool(ctx, k, false);
    }
    if let Some(a) = action {
        app.do_action(ctx, a);
    }
}

/// One full-width menu row: label on the left, shortcut on the right.
fn menu_row(ui: &mut Ui, app: &PlumApp, label: &str, shortcut: Option<String>, checked: Option<bool>) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
    let hover = app.motion.fade(ui.ctx(), resp.id.with("h"), resp.hovered(), SNAPPY);
    let p = ui.painter();
    if hover > 0.0 {
        p.rect_filled(rect, theme::radius(6.0), c32(app.cfg.colors.button_hover).gamma_multiply(hover));
    }
    let text = c32(app.cfg.colors.text);
    // Every row keeps the check column so labels line up.
    if checked == Some(true) {
        Icon::Check.paint(p, Rect::from_center_size(pos2(rect.left() + 16.0, rect.center().y), vec2(14.0, 14.0)), c32(app.cfg.colors.accent));
    }
    p.text(pos2(rect.left() + 30.0, rect.center().y), Align2::LEFT_CENTER, label, FontId::proportional(app.cfg.ui.font_size), text);
    if let Some(k) = shortcut {
        p.text(rect.right_center() - vec2(10.0, 0.0), Align2::RIGHT_CENTER, k, mono(app, 0.8), muted(app));
    }
    resp
}

fn menu_section(ui: &mut Ui, app: &PlumApp, title: &str) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(30.0);
        section(ui, app, title);
    });
}

fn main_menu(app: &mut PlumApp, button: &egui::Response, action: &mut Option<Action>) {
    let mut theme_pick = None;
    let mut reload = false;
    let mut open_folder = false;
    let mut stress = false;
    let r = button.clone().on_hover_ui(|ui| tip_ui(ui, app, "Menu", None));
    egui::Popup::menu(&r).show(|ui| {
        ui.set_width(240.0);
        ui.spacing_mut().item_spacing.y = 1.0;
        let item = |ui: &mut Ui, label: &str, a: Action, checked: Option<bool>, action: &mut Option<Action>| {
            if menu_row(ui, app, label, app.keymap.label(a), checked).clicked() {
                *action = Some(a);
                ui.close();
            }
        };
        menu_section(ui, app, "File");
        item(ui, "New", Action::New, None, action);
        item(ui, "Open…", Action::Open, None, action);
        item(ui, "Save", Action::Save, None, action);
        item(ui, "Save as…", Action::SaveAs, None, action);
        ui.separator();
        menu_section(ui, app, "View");
        item(ui, "Zoom to fit", Action::ZoomFit, None, action);
        item(ui, "Reset zoom", Action::ZoomReset, None, action);
        item(ui, "Reset position", Action::ResetPosition, None, action);
        item(ui, "Snap to grid", Action::ToggleSnap, Some(app.snap), action);
        item(ui, "Performance overlay", Action::PerfOverlay, Some(app.show_perf), action);
        ui.separator();
        menu_section(ui, app, "Theme");
        for name in app.dirs.theme_names() {
            if menu_row(ui, app, &name, None, Some(app.cfg.theme == name)).clicked() {
                theme_pick = Some(name);
                ui.close();
            }
        }
        ui.separator();
        item(ui, "Settings…", Action::Settings, None, action);
        if menu_row(ui, app, "Open config folder", None, None).clicked() {
            open_folder = true;
            ui.close();
        }
        if menu_row(ui, app, "Reload config", None, None).clicked() {
            reload = true;
            ui.close();
        }
        if menu_row(ui, app, "Stress test (+100k strokes)", None, None).clicked() {
            stress = true;
            ui.close();
        }
    });
    let ctx = button.ctx.clone();
    if let Some(name) = theme_pick {
        app.set_theme(&ctx, &name);
    }
    if reload {
        app.reload_config(&ctx);
    }
    if open_folder
        && let Some(root) = app.dirs.root.clone()
        && let Err(e) = open_in_file_manager(&root)
    {
        app.toast(&ctx, format!("Couldn't open {}: {e}", root.display()), ToastKind::Error);
    }
    if stress {
        app.stress_test(100_000);
    }
}

fn open_in_file_manager(path: &std::path::Path) -> std::io::Result<()> {
    let cmd = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(cmd).arg(path).spawn().map(|_| ())
}

fn context_bar(app: &mut PlumApp, ctx: &egui::Context) {
    let id = Id::unique("plum-context-bar");
    let shown = !app.selection.is_empty() && !app.tool.busy() && app.text_edit.is_none() && app.tool_kind == ToolKind::Select;
    let rect = if shown { selection_screen_rect(&app.board, &app.selection, &app.cam) } else { None };
    let Some(sel) = rect else {
        // Reset so the bar eases in again next time it appears.
        app.motion.set(ctx, id.with("t"), 0.0);
        return;
    };
    let t = app.motion.fade(ctx, id.with("t"), true, SMOOTH);
    let gap = app.cfg.ui.rotate_handle_distance + app.cfg.ui.handle_size + 12.0;
    let anchor = Pos2::new(sel.center().x, sel.top() - gap + (1.0 - t) * 8.0);
    let buttons = [
        (Icon::FlipH, "Flip horizontal", Action::FlipH),
        (Icon::FlipV, "Flip vertical", Action::FlipV),
        (Icon::RotateCcw, "Rotate left 90°", Action::RotateCcw),
        (Icon::RotateCw, "Rotate right 90°", Action::RotateCw),
        (Icon::Backward, "Send backward", Action::SendBackward),
        (Icon::Forward, "Bring forward", Action::BringForward),
        (Icon::Duplicate, "Duplicate", Action::Duplicate),
        (Icon::Delete, "Delete", Action::Delete),
    ];
    let mut action = None;
    Area::new(id).order(Order::Middle).pivot(Align2::CENTER_BOTTOM).fixed_pos(anchor).constrain(true).show(ctx, |ui| {
        ui.multiply_opacity(t);
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 6.0);
            z_label(ui, app);
            theme::panel_frame(&app.cfg).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
                    let size = (app.cfg.ui.button_size * 0.88).round();
                    for (icon, name, a) in buttons {
                        if a == Action::Duplicate {
                            divider(ui, app, false);
                        }
                        let r = icon_button(ui, app, icon, size, false, true);
                        if a == Action::Delete && r.hovered() {
                            // Destructive action: tint the icon on hover.
                            icon.paint(ui.painter(), Rect::from_center_size(r.rect.center(), vec2(app.cfg.ui.icon_size * 0.88, app.cfg.ui.icon_size * 0.88)), c32(app.cfg.colors.error));
                        }
                        if r.on_hover_ui(|ui| tip_ui(ui, app, name, Some(a))).clicked() {
                            action = Some(a);
                        }
                    }
                });
            });
        });
    });
    if let Some(a) = action {
        app.do_action(ctx, a);
    }
}

/// Small pill above the action bar: "z: 3/12", or "z: 2–7/12" for several (1 = backmost).
fn z_label(ui: &mut Ui, app: &PlumApp) {
    let zs: Vec<usize> = app.selection.ordered(&app.board).iter().filter_map(|id| app.board.z_index(*id)).collect();
    let (Some(lo), Some(hi)) = (zs.first(), zs.last()) else { return };
    let n = app.board.len();
    let text = if lo == hi { format!("z: {}/{n}", lo + 1) } else { format!("z: {}–{}/{n}", lo + 1, hi + 1) };
    // Painted by hand so the pill hugs its text instead of stretching to the bar's width.
    let galley = ui.painter().layout_no_wrap(text, mono(app, 0.78), muted(app));
    let (rect, resp) = ui.allocate_exact_size(galley.size() + vec2(14.0, 6.0), Sense::hover());
    let p = ui.painter();
    p.rect(rect, theme::radius(rect.height() / 2.0), c32(app.cfg.colors.panel), Stroke::new(1.0, c32(app.cfg.colors.panel_border)), StrokeKind::Inside);
    p.galley(rect.center() - galley.size() / 2.0, galley, muted(app));
    resp.on_hover_text("Layer position (1 = back). Use bring forward / send backward to change it.");
}

/// What the style panel is editing this frame.
enum StyleTarget {
    Selection,
    /// The text box being typed in (and, for new text, the text tool's defaults).
    Session,
    Tool(StyleSlot),
}

fn style_panel(app: &mut PlumApp, ctx: &egui::Context) {
    // Which controls apply: (target, style, stroke colors, fill, font size, width/opacity sliders).
    let (target, style, show_stroke, show_fill, font, show_body) = if let Some(s) = &app.text_edit {
        (StyleTarget::Session, s.style, true, false, Some(s.text.font_size), true)
    } else if app.tool_kind == ToolKind::Select && !app.selection.is_empty() {
        let els = app.selection.elements(&app.board);
        let first = els[0];
        let has_fill = els.iter().any(|e| match &e.kind {
            ElementKind::Shape(_) => true,
            ElementKind::Stroke(s) => s.is_closed(e.style.width),
            _ => false,
        });
        let font = els.iter().find_map(|e| match &e.kind {
            ElementKind::Text(t) => Some(t.font_size),
            _ => None,
        });
        (StyleTarget::Selection, first.style, true, has_fill, font, true)
    } else if let Some(slot) = app.tool_kind.style_slot() {
        let font = (slot == StyleSlot::Text).then_some(app.styles.font_size);
        let bucket = slot == StyleSlot::Bucket;
        (StyleTarget::Tool(slot), *app.styles.get(slot), !bucket, matches!(slot, StyleSlot::Shape | StyleSlot::Bucket), font, !bucket)
    } else {
        app.motion.set(ctx, Id::unique("plum-style-panel").with("t"), 0.0);
        return;
    };

    // Swatches are palette slots, so colors follow the theme.
    let swatches: Vec<(Paint, Color32)> =
        app.cfg.palette.colors.iter().enumerate().map(|(i, c)| (Paint::Palette(i as u8), c32(*c))).collect();
    let mut new_stroke = None;
    let mut new_fill: Option<Option<Paint>> = None;
    let mut new_width = None;
    let mut new_font = None;
    let mut new_opacity = None;
    let mut drag_done = false;

    let id = Id::unique("plum-style-panel");
    let t = app.motion.fade(ctx, id.with("t"), true, SMOOTH);
    let m = app.cfg.ui.margin;
    let (align, offset) = match app.cfg.ui.toolbar_position {
        ToolbarPosition::Right => (Align2::LEFT_TOP, vec2(m - (1.0 - t) * 10.0, m)),
        _ => (Align2::RIGHT_TOP, vec2(-m + (1.0 - t) * 10.0, m)),
    };
    Area::new(id).order(Order::Middle).anchor(align, offset).show(ctx, |ui| {
        ui.multiply_opacity(t);
        theme::panel_frame(&app.cfg).inner_margin(egui::Margin::symmetric(12, 10)).show(ui, |ui| {
            ui.set_width(5.0 * 26.0);
            ui.spacing_mut().item_spacing = vec2(2.0, 4.0);
            let swatch_grid = |ui: &mut Ui, current: Option<Paint>, with_none: bool, out: &mut Option<Option<Paint>>| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
                    if with_none && swatch(ui, app, None, current.is_none()).on_hover_text("No fill").clicked() {
                        *out = Some(None);
                    }
                    for (paint, color) in &swatches {
                        if swatch(ui, app, Some(*color), current == Some(*paint)).clicked() {
                            *out = Some(Some(*paint));
                        }
                    }
                });
            };
            if show_stroke {
                section(ui, app, if font.is_some() { "Color" } else { "Stroke" });
                let mut pick = None;
                swatch_grid(ui, Some(style.stroke), false, &mut pick);
                new_stroke = pick.flatten();
            }
            if show_fill {
                ui.add_space(6.0);
                section(ui, app, "Fill");
                swatch_grid(ui, style.fill, true, &mut new_fill);
            }
            if !show_body {
                return;
            }
            ui.add_space(6.0);
            match font {
                Some(size) => {
                    slider_header(ui, app, "Size", format!("{size:.0}"));
                    let mut s = size;
                    let r = slider(ui, app, &mut s, 4.0..=200.0, true);
                    if r.changed() {
                        new_font = Some(s.round());
                    }
                    drag_done |= r.drag_stopped() || r.clicked();
                }
                None => {
                    slider_header(ui, app, "Width", format!("{:.1}", style.width));
                    let mut w = style.width;
                    let r = slider(ui, app, &mut w, 0.0..=40.0, false);
                    if r.changed() {
                        new_width = Some((w * 2.0).round() / 2.0);
                    }
                    drag_done |= r.drag_stopped() || r.clicked();
                }
            }
            ui.add_space(4.0);
            slider_header(ui, app, "Opacity", format!("{:.0}%", style.opacity * 100.0));
            let mut o = style.opacity;
            let r = slider(ui, app, &mut o, 0.0..=1.0, false);
            if r.changed() {
                new_opacity = Some((o * 100.0).round() / 100.0);
            }
            drag_done |= r.drag_stopped() || r.clicked();
        });
    });

    let apply = |s: &mut plum_core::Style| {
        if let Some(c) = new_stroke {
            s.stroke = c;
        }
        if let Some(f) = new_fill {
            s.fill = f;
        }
        if let Some(w) = new_width {
            s.width = w;
        }
        if let Some(o) = new_opacity {
            s.opacity = o;
        }
    };
    let changed = new_stroke.is_some() || new_fill.is_some() || new_width.is_some() || new_font.is_some() || new_opacity.is_some();
    match target {
        StyleTarget::Selection => {
            if changed {
                app.edit_selection_style(apply);
                if let Some(f) = new_font {
                    app.edit_selection_font(f, ctx);
                }
                // Swatch clicks are one-shot edits; slider drags coalesce until release.
                if new_stroke.is_some() || new_fill.is_some() {
                    app.finish_style_edit();
                }
            }
            if drag_done {
                app.finish_style_edit();
            }
        }
        StyleTarget::Session => {
            let Some(s) = app.text_edit.as_mut() else { return };
            apply(&mut s.style);
            if let Some(f) = new_font {
                s.text.font_size = f;
            }
            // Brand-new text also sets the defaults for the next text box.
            if s.target.is_none() && changed {
                let style = s.style;
                app.styles.text = style;
            }
        }
        StyleTarget::Tool(slot) => {
            apply(app.styles.get_mut(slot));
            if let Some(f) = new_font {
                app.styles.font_size = f;
            }
        }
    }
}

pub fn text_editor(app: &mut PlumApp, ctx: &egui::Context) {
    let Some(session) = app.text_edit.as_mut() else { return };
    let cam = app.cam;
    let pos = cam.world_to_screen(session.transform.pos);
    let scale = session.transform.scale.y.abs() * cam.zoom;
    let font = FontId::proportional((session.text.font_size as f64 * scale).max(4.0) as f32);
    let color = theme::paint_c32(session.style.stroke, session.style.opacity, &app.cfg);
    let measured = crate::render::measure_text(ctx, &session.text);
    let width = (measured.x as f64 * session.transform.scale.x.abs() * cam.zoom) as f32 + font.size;
    let mut lost = false;
    // Lines never wrap: the box grows as you type; Enter starts a new line.
    let layout_font = font.clone();
    let mut layouter = move |ui: &Ui, buf: &dyn egui::TextBuffer, _wrap_width: f32| {
        let job = egui::text::LayoutJob::simple(buf.as_str().to_owned(), layout_font.clone(), color, f32::INFINITY);
        ui.ctx().fonts_mut(|f| f.layout_job(job))
    };
    let outline = c32(app.cfg.colors.selection);
    Area::new(Id::unique("plum-text-editor")).order(Order::Foreground).fixed_pos(pos).constrain(false).show(ctx, |ui| {
        let edit = egui::TextEdit::multiline(&mut session.text.content)
            .font(font)
            .text_color(color)
            .frame(egui::Frame::NONE)
            .desired_rows(1)
            .desired_width(width.max(40.0))
            .layouter(&mut layouter)
            .margin(vec2(0.0, 0.0));
        let r = ui.add(edit);
        if !session.opened {
            r.request_focus();
            session.opened = true;
        }
        lost = r.lost_focus();
        ui.painter().rect_stroke(r.rect.expand(4.0), 4.0, Stroke::new(1.0, outline), StrokeKind::Outside);
    });
    if lost {
        app.commit_text(ctx);
    }
}

fn zoom_controls(app: &mut PlumApp, ctx: &egui::Context) {
    let m = app.cfg.ui.margin;
    let (align, offset) = match app.cfg.ui.toolbar_position {
        ToolbarPosition::Bottom => (Align2::LEFT_TOP, vec2(m, m + 46.0)),
        _ => (Align2::LEFT_BOTTOM, vec2(m, -m)),
    };
    let mut action = None;
    Area::new(Id::unique("plum-zoom")).order(Order::Middle).anchor(align, offset).show(ctx, |ui| {
        theme::panel_frame(&app.cfg).inner_margin(egui::Margin::same(3)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                let size = (app.cfg.ui.button_size * 0.82).round();
                if icon_button(ui, app, Icon::Minus, size, false, true).on_hover_ui(|ui| tip_ui(ui, app, "Zoom out", Some(Action::ZoomOut))).clicked() {
                    action = Some(Action::ZoomOut);
                }
                // Fixed width (monospace) so the controls don't shift as the number changes.
                let (rect, r) = ui.allocate_exact_size(vec2(56.0, size), Sense::click());
                let hover = app.motion.fade(ctx, r.id.with("h"), r.hovered(), SNAPPY);
                if hover > 0.0 {
                    ui.painter().rect_filled(rect, inner_radius(app), c32(app.cfg.colors.button_hover).gamma_multiply(hover));
                }
                let label = format!("{:.0}%", app.cam.zoom * 100.0);
                ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, mono(app, 0.86), c32(app.cfg.colors.text));
                if r.on_hover_ui(|ui| tip_ui(ui, app, "Reset zoom", Some(Action::ZoomReset))).clicked() {
                    action = Some(Action::ZoomReset);
                }
                if icon_button(ui, app, Icon::Plus, size, false, true).on_hover_ui(|ui| tip_ui(ui, app, "Zoom in", Some(Action::ZoomIn))).clicked() {
                    action = Some(Action::ZoomIn);
                }
            });
        });
    });
    if let Some(a) = action {
        app.do_action(ctx, a);
    }
}

fn perf_overlay(app: &PlumApp, ctx: &egui::Context) {
    let s = app.render_stats();
    let m = app.cfg.ui.margin;
    let (align, offset) = match app.cfg.ui.toolbar_position {
        ToolbarPosition::Right => (Align2::LEFT_BOTTOM, vec2(m, -m - 52.0)),
        _ => (Align2::RIGHT_BOTTOM, vec2(-m, -m)),
    };
    Area::new(Id::unique("plum-perf")).order(Order::Middle).anchor(align, offset).interactable(false).show(ctx, |ui| {
        theme::panel_frame(&app.cfg).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            let rows = [
                ("frame cpu", format!("{:.2} ms", app.cpu_ms)),
                ("elements", s.total.to_string()),
                ("visible", s.visible.to_string()),
                ("as dots", s.lod.to_string()),
                ("cached", s.cached.to_string()),
                ("rebuilt", s.rebuilt.to_string()),
                ("vertices", s.vertices.to_string()),
                ("center", format!("{:.0}, {:.0}", app.cam.center.x, app.cam.center.y)),
            ];
            for (k, v) in rows {
                ui.label(egui::RichText::new(format!("{k:<10}{v:>12}")).font(mono(app, 0.8)).color(muted(app)));
            }
        });
    });
}

fn toasts(app: &mut PlumApp, ctx: &egui::Context) {
    let now = ctx.input(|i| i.time);
    app.toasts.retain(|t| t.until > now);
    if app.toasts.is_empty() {
        return;
    }
    let animating = app.toasts.iter().any(|t| now - t.born < 0.25 || t.until - now < 0.25);
    if animating && app.motion.speed > 0.0 {
        ctx.request_repaint();
    } else {
        let next = app.toasts.iter().map(|t| t.until - 0.25).fold(f64::INFINITY, f64::min);
        ctx.request_repaint_after(std::time::Duration::from_secs_f64((next - now).max(0.016)));
    }
    let m = app.cfg.ui.margin;
    let bottom = matches!(app.cfg.ui.toolbar_position, ToolbarPosition::Bottom);
    let (align, offset) = if bottom { (Align2::CENTER_TOP, vec2(0.0, m)) } else { (Align2::CENTER_BOTTOM, vec2(0.0, -m)) };
    let speed = app.motion.speed.max(1e-3) as f64;
    Area::new(Id::unique("plum-toasts")).order(Order::Tooltip).anchor(align, offset).interactable(false).show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            for t in app.toasts.iter().rev().take(4).rev() {
                let t_in = ease_out(((now - t.born) * speed / 0.22) as f32);
                let t_out = (((t.until - now) * speed) / 0.22).clamp(0.0, 1.0) as f32;
                let a = t_in.min(t_out);
                let dot = match t.kind {
                    ToastKind::Info => c32(app.cfg.colors.accent),
                    ToastKind::Warn => Color32::from_rgb(0xe8, 0xa3, 0x17),
                    ToastKind::Error => c32(app.cfg.colors.error),
                };
                ui.scope(|ui| {
                    ui.multiply_opacity(a);
                    ui.add_space((1.0 - t_in) * 10.0);
                    theme::panel_frame(&app.cfg).corner_radius(theme::radius(20.0)).inner_margin(egui::Margin::symmetric(14, 8)).show(ui, |ui| {
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                        ui.set_max_width(520.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 10.0;
                            let (r, _) = ui.allocate_exact_size(vec2(7.0, 7.0), Sense::hover());
                            ui.painter().circle_filled(r.center(), 3.5, dot);
                            ui.label(&t.text);
                        });
                    });
                });
            }
        });
    });
}

/// A themed button: `primary` is filled with the accent; others are quiet until hovered.
fn dialog_button(ui: &mut Ui, app: &PlumApp, text: &str, primary: bool) -> egui::Response {
    let col = &app.cfg.colors;
    let galley = ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(app.cfg.ui.font_size), c32(col.text));
    let (rect, resp) = ui.allocate_exact_size(vec2((galley.size().x + 28.0).max(92.0), 32.0), Sense::click());
    let hover = app.motion.fade(ui.ctx(), resp.id.with("h"), resp.hovered(), SNAPPY);
    let p = ui.painter();
    let r = inner_radius(app);
    let fg = if primary {
        p.rect_filled(rect, r, lerp(c32(col.accent), c32(col.accent_text), hover * 0.12));
        c32(col.accent_text)
    } else {
        p.rect(rect, r, c32(col.button_hover).gamma_multiply(0.5 + hover * 0.5), Stroke::new(1.0, c32(col.panel_border)), StrokeKind::Inside);
        c32(col.text)
    };
    p.text(rect.center(), Align2::CENTER_CENTER, text, FontId::proportional(app.cfg.ui.font_size), fg);
    resp
}

/// In-app "save changes?" prompt (replaces the OS message box).
pub fn confirm_modal(app: &mut PlumApp, ctx: &egui::Context) {
    let fade_id = Id::unique("plum-confirm-t");
    let Some(pending) = app.confirm.clone() else {
        app.motion.set(ctx, fade_id, 0.0);
        return;
    };
    let t = app.motion.fade(ctx, fade_id, true, SMOOTH);
    let verb = match pending {
        crate::app::Pending::New => "starting a new board",
        crate::app::Pending::Open => "opening another board",
        crate::app::Pending::Close => "closing",
    };
    let mut answer: Option<Option<bool>> = None;
    let frame = theme::panel_frame(&app.cfg).inner_margin(egui::Margin::same(22)).corner_radius(theme::radius(app.cfg.ui.corner_radius + 4.0));
    let modal = egui::Modal::new(Id::unique("plum-confirm")).frame(frame).backdrop_color(Color32::from_black_alpha((70.0 * t) as u8)).show(ctx, |ui| {
        ui.multiply_opacity(t);
        ui.set_width(360.0);
        ui.label(egui::RichText::new("Unsaved changes").font(FontId::proportional(app.cfg.ui.font_size * 1.25)));
        ui.add_space(4.0);
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
        ui.label(egui::RichText::new(format!("Save changes to \u{201c}{}\u{201d} before {verb}?", app.display_name())).color(muted(app)));
        ui.add_space(18.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if dialog_button(ui, app, "Save", true).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                answer = Some(Some(true));
            }
            if dialog_button(ui, app, "Don't save", false).clicked() {
                answer = Some(Some(false));
            }
            if dialog_button(ui, app, "Cancel", false).clicked() {
                answer = Some(None);
            }
        });
    });
    if modal.should_close() && answer.is_none() {
        answer = Some(None);
    }
    if let Some(a) = answer {
        app.resolve_pending(ctx, a);
    }
}

/// Settings window: rebind any action by clicking its shortcut and pressing keys.
pub fn settings_window(app: &mut PlumApp, ctx: &egui::Context) {
    if !app.show_settings {
        return;
    }
    let mut open = true;
    let mut rebind: Option<&'static str> = None;
    let mut reset: Option<&'static str> = None;
    let mut clear: Option<&'static str> = None;
    let mut reset_all = false;
    let mut filter = std::mem::take(&mut app.settings_filter);
    egui::Window::new("Settings")
        .order(Order::Foreground)
        .pivot(Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_size(vec2(560.0, 540.0))
        .show(ctx, |ui| {
            section(ui, app, "Keybinds");
            ui.label(egui::RichText::new("Click a shortcut, then press the new keys. Esc cancels. Saved to config.toml.").color(muted(app)));
            ui.add_space(8.0);
            ui.add(egui::TextEdit::singleline(&mut filter).hint_text("Filter actions…").desired_width(f32::INFINITY).margin(vec2(10.0, 6.0)));
            ui.add_space(6.0);
            let query = filter.trim().to_lowercase();
            egui::ScrollArea::vertical().auto_shrink([false, false]).max_height(ui.available_height() - 44.0).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for (action, name, label) in crate::keymap::ACTIONS {
                    if !query.is_empty() && !label.to_lowercase().contains(&query) {
                        continue;
                    }
                    let chords = app.cfg.keybinds.get(*name).map(|c| c.0.clone()).unwrap_or_default();
                    let capturing = app.capturing == Some(*name);
                    let (row, row_resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
                    let hover = app.motion.fade(ctx, row_resp.id.with("h"), row_resp.hovered() || capturing, SNAPPY);
                    if hover > 0.0 {
                        ui.painter().rect_filled(row, inner_radius(app), c32(app.cfg.colors.button_hover).gamma_multiply(hover));
                    }
                    let mut row_ui = ui.new_child(egui::UiBuilder::new().max_rect(row.shrink2(vec2(10.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
                    let ui = &mut row_ui;
                    ui.label(*label);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        // Row actions stay quiet until the row is hovered.
                        ui.scope(|ui| {
                            ui.multiply_opacity(0.25 + 0.75 * hover);
                            if ui.add(egui::Button::new(egui::RichText::new("reset").font(mono(app, 0.78)).color(muted(app))).frame(false)).on_hover_text("Use the default binding").clicked() {
                                reset = Some(*name);
                            }
                            if ui.add(egui::Button::new(egui::RichText::new("clear").font(mono(app, 0.78)).color(muted(app))).frame(false)).on_hover_text("Unbind").clicked() {
                                clear = Some(*name);
                            }
                        });
                        let conflicts: Vec<&str> = chords.iter().flat_map(|c| app.keymap.conflicts(*action, c)).map(crate::keymap::action_label).collect();
                        let shown: String = if capturing {
                            "press keys…".into()
                        } else if chords.is_empty() {
                            "—".into()
                        } else {
                            chords.iter().map(|c| display_chord_str(c)).collect::<Vec<_>>().join("  ·  ")
                        };
                        let col = &app.cfg.colors;
                        let (fg, fill, border) = if capturing {
                            (c32(col.accent), accent_soft(&app.cfg), c32(col.accent))
                        } else {
                            (c32(col.text), c32(col.button_hover), c32(col.panel_border))
                        };
                        let chip = egui::Button::new(egui::RichText::new(shown).font(mono(app, 0.82)).color(fg))
                            .fill(fill)
                            .stroke(Stroke::new(1.0, border))
                            .corner_radius(theme::radius(6.0))
                            .min_size(vec2(0.0, 24.0));
                        if ui.add(chip).on_hover_text("Click, then press a key combination").clicked() {
                            rebind = Some(*name);
                        }
                        if !conflicts.is_empty() {
                            ui.label(egui::RichText::new("!").font(mono(app, 0.9)).color(c32(col.error)))
                                .on_hover_text(format!("Also bound to: {}", conflicts.join(", ")));
                        }
                    });
                }
            });
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if dialog_button(ui, app, "Reset all keybinds", false).clicked() {
                    reset_all = true;
                }
            });
        });
    app.settings_filter = filter;
    if !open {
        app.show_settings = false;
        app.capturing = None;
    }
    if let Some(n) = rebind {
        app.capturing = Some(n);
    }
    if let Some(n) = clear {
        app.capturing = None;
        app.write_keybind(ctx, n, Some(&[]));
    }
    if let Some(n) = reset {
        app.capturing = None;
        app.write_keybind(ctx, n, None);
    }
    if reset_all {
        app.capturing = None;
        app.reset_keybinds(ctx);
    }
}
