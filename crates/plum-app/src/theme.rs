//! Maps the resolved [`Config`] onto egui styling and fonts.

use std::sync::Arc;

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Shadow, Stroke, TextStyle};
use plum_config::{Color, ColorRef, Config};
use plum_core::{Paint, Rgba};

pub fn c32(c: Color) -> Color32 {
    let [r, g, b, a] = c.0;
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

pub fn rgba(c: Color) -> Rgba {
    Rgba(c.0)
}

/// Concrete color of an element paint under the current theme, with opacity applied.
/// Palette slots the theme doesn't define fall back to the text color.
pub fn paint_c32(p: Paint, opacity: f32, cfg: &Config) -> Color32 {
    let c = match p {
        Paint::Rgba(c) => c,
        Paint::Palette(i) => rgba(cfg.palette.colors.get(i as usize).copied().unwrap_or(cfg.colors.text)),
    };
    let [r, g, b, a] = c.0;
    Color32::from_rgba_unmultiplied(r, g, b, (a as f32 * opacity.clamp(0.0, 1.0)).round() as u8)
}

pub fn color_ref(c: ColorRef) -> Paint {
    match c {
        ColorRef::Palette(i) => Paint::Palette(i),
        ColorRef::Fixed(c) => Paint::Rgba(rgba(c)),
    }
}

/// Bundled default font (SIL OFL, see assets/fonts/JetBrainsMono-OFL.txt).
const JETBRAINS_MONO: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf");

pub fn radius(r: f32) -> CornerRadius {
    CornerRadius::same(r.clamp(0.0, 255.0) as u8)
}

pub fn panel_shadow(cfg: &Config) -> Shadow {
    if cfg.ui.shadow {
        Shadow { offset: [0, 6], blur: 22, spread: 0, color: c32(cfg.colors.panel_shadow) }
    } else {
        Shadow::NONE
    }
}

/// Floating "island" behind every group of controls.
pub fn panel_frame(cfg: &Config) -> egui::Frame {
    egui::Frame::new()
        .fill(c32(cfg.colors.panel))
        .stroke(Stroke::new(1.0, c32(cfg.colors.panel_border)))
        .corner_radius(radius(cfg.ui.corner_radius))
        .inner_margin(egui::Margin::same(5))
        .shadow(panel_shadow(cfg))
}

/// The accent at low strength: active tool backgrounds, selected rows.
pub fn accent_soft(cfg: &Config) -> Color32 {
    c32(cfg.colors.accent).gamma_multiply(0.2)
}

/// Mix two colors (`t` = 0 → `a`, 1 → `b`).
pub fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    a.lerp_to_gamma(b, t.clamp(0.0, 1.0))
}

/// Apply colors/sizes to both of egui's light and dark styles.
pub fn apply_style(ctx: &egui::Context, cfg: &Config) {
    let col = &cfg.colors;
    let dark = luminance(col.canvas) < 0.5;
    ctx.all_styles_mut(|s| {
        let v = &mut s.visuals;
        v.dark_mode = dark;
        v.override_text_color = Some(c32(col.text));
        v.panel_fill = c32(col.panel);
        v.window_fill = c32(col.panel);
        v.window_corner_radius = radius(cfg.ui.corner_radius + 2.0);
        v.menu_corner_radius = radius(cfg.ui.corner_radius);
        v.window_shadow = Shadow { offset: [0, 14], blur: 40, spread: 0, color: c32(col.panel_shadow) };
        v.extreme_bg_color = c32(col.canvas);
        v.faint_bg_color = c32(col.button_hover);
        v.hyperlink_color = c32(col.accent);
        v.selection.bg_fill = c32(col.accent).gamma_multiply(0.35);
        v.selection.stroke = Stroke::new(1.0, c32(col.accent));
        v.window_stroke = Stroke::new(1.0, c32(col.panel_border));
        v.popup_shadow = panel_shadow(cfg);
        let r = radius(cfg.ui.corner_radius * 0.6);
        for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
            w.corner_radius = r;
            w.fg_stroke = Stroke::new(1.0, c32(col.text));
            w.bg_stroke = Stroke::NONE;
            w.expansion = 0.0;
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, c32(col.panel_border));
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, c32(col.text_muted));
        v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
        v.widgets.inactive.bg_fill = c32(col.button_hover);
        v.widgets.hovered.weak_bg_fill = c32(col.button_hover);
        v.widgets.hovered.bg_fill = c32(col.button_hover);
        v.widgets.active.weak_bg_fill = accent_soft(cfg);
        v.widgets.active.bg_fill = accent_soft(cfg);
        v.widgets.active.fg_stroke = Stroke::new(1.0, c32(col.accent));
        v.widgets.open.weak_bg_fill = c32(col.button_hover);
        v.text_cursor.stroke = Stroke::new(2.0, c32(col.accent));
        v.striped = false;
        v.button_frame = true;
        v.collapsing_header_frame = false;

        let fs = cfg.ui.font_size.max(6.0);
        s.text_styles = [
            (TextStyle::Small, FontId::proportional(fs * 0.82)),
            (TextStyle::Body, FontId::proportional(fs)),
            (TextStyle::Button, FontId::proportional(fs)),
            (TextStyle::Heading, FontId::proportional(fs * 1.25)),
            (TextStyle::Monospace, FontId::monospace(fs * 0.92)),
        ]
        .into();
        // Floating panels size to their content; never wrap their labels
        // (areas remember last frame's width, so wrapping would lag a frame behind).
        s.wrap_mode = Some(egui::TextWrapMode::Extend);
        s.spacing.item_spacing = egui::vec2(cfg.ui.spacing.max(2.0) * 2.0, cfg.ui.spacing.max(2.0) * 2.0);
        s.spacing.button_padding = egui::vec2(10.0, 5.0);
        s.spacing.menu_margin = egui::Margin::same(6);
        s.spacing.window_margin = egui::Margin::same(16);
        s.spacing.interact_size.y = 26.0;
        // egui's own transitions (popups, windows, tooltips) follow the motion setting.
        let speed = cfg.ui.animation_speed;
        s.animation_time = if speed > 0.0 { 0.11 / speed } else { 0.0 };
        s.interaction.tooltip_delay = 0.35;
        s.interaction.show_tooltips_only_when_still = false;
    });
}

/// Install the configured font file, or the bundled JetBrains Mono. egui's own
/// fonts stay behind it as fallbacks (symbols, emoji). Returns an error message on failure.
pub fn apply_fonts(ctx: &egui::Context, cfg: &Config) -> Result<(), String> {
    let mut defs = FontDefinitions::default();
    let mut install = |name: &str, data: FontData| {
        defs.font_data.insert(name.into(), Arc::new(data));
        for fam in [FontFamily::Proportional, FontFamily::Monospace] {
            defs.families.entry(fam).or_default().insert(0, name.into());
        }
    };
    install("jetbrains-mono", FontData::from_static(JETBRAINS_MONO));
    let path = cfg.ui.font_file.trim();
    let result = if path.is_empty() {
        Ok(())
    } else {
        match std::fs::read(path) {
            Ok(bytes) => {
                install("user", FontData::from_owned(bytes));
                Ok(())
            }
            Err(e) => Err(format!("font_file {path:?}: {e}")),
        }
    };
    ctx.set_fonts(defs);
    result
}

fn luminance(c: Color) -> f32 {
    let [r, g, b, _] = c.0;
    (0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32) / 255.0
}
