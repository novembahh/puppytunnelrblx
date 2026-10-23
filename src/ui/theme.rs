use eframe::egui::{
    epaint::Shadow, style::ScrollStyle, Color32, Context, FontData, FontDefinitions, FontFamily, FontId, Margin,
    Rounding, Stroke, Style, TextStyle, Vec2, Visuals,
};
use std::collections::BTreeMap;

pub const VOID: Color32 = Color32::from_rgb(0x0B, 0x0A, 0x10);
pub const INK: Color32 = Color32::from_rgb(0x12, 0x10, 0x19);
pub const INK2: Color32 = Color32::from_rgb(0x19, 0x16, 0x21);
pub const MILK: Color32 = Color32::from_rgb(0xD3, 0xCE, 0xC4);
pub const MILK_DIM: Color32 = Color32::from_rgb(0xA9, 0xA2, 0x9A);
pub const MILK_FAINT: Color32 = Color32::from_rgb(0x6F, 0x68, 0x62);
pub const RED: Color32 = Color32::from_rgb(0xAC, 0x32, 0x32);
pub const RED_LIT: Color32 = Color32::from_rgb(0xE0, 0x61, 0x4F);
pub const BRUISE: Color32 = Color32::from_rgb(0x6A, 0x30, 0x50);
pub const BRUISE_LIT: Color32 = Color32::from_rgb(0x9A, 0x4A, 0x74);
pub const AMBER: Color32 = Color32::from_rgb(0xCF, 0x9D, 0x4F);
pub const BONE: Color32 = Color32::from_rgb(0xF0, 0xE9, 0xDD);

pub const SURF_0: Color32 = VOID;
pub const SURF_1: Color32 = INK;
pub const SURF_2: Color32 = INK2;
pub const SURF_3: Color32 = Color32::from_rgb(0x21, 0x1D, 0x29);
pub const SURF_4: Color32 = Color32::from_rgb(0x27, 0x22, 0x30);

pub const TX: Color32 = MILK;
pub const TX_DIM: Color32 = MILK_DIM;
pub const TX_FAINT: Color32 = MILK_FAINT;

pub const R_SM: f32 = 6.0;
pub const R_CTRL: f32 = 8.0;
pub const R_BLOCK: f32 = 11.0;
pub const R_PANE: f32 = 14.0;
pub const R_WINDOW: f32 = 16.0;

pub const CTRL_H: f32 = 32.0;
pub const GAP: f32 = 8.0;
pub const GAP_L: f32 = 16.0;
pub const GAP_XL: f32 = 32.0;

pub const T_DISPLAY: f32 = 24.0;
pub const T_TITLE: f32 = 17.0;
pub const T_HEAD: f32 = 13.5;
pub const T_BODY: f32 = 13.0;
pub const T_LABEL: f32 = 10.5;
pub const T_META: f32 = 11.0;
pub const T_TINY: f32 = 10.0;

pub fn tint(c: Color32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a.clamp(0.0, 1.0) * 255.0) as u8)
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    a.lerp_to_gamma(b, t.clamp(0.0, 1.0))
}

pub fn hairline_color() -> Color32 {
    tint(MILK, 0.045)
}

fn install_fonts(ctx: &Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "roboto".into(),
        FontData::from_static(include_bytes!("../../assets/fonts/Roboto-Medium.ttf")),
    );
    fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "roboto".into());
    fonts.font_data.insert(
        "icons".into(),
        FontData::from_static(include_bytes!("../../assets/fonts/MaterialIcons-Regular.ttf")),
    );
    fonts.families.insert(FontFamily::Name("icons".into()), vec!["icons".into()]);
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, "roboto".into());
    ctx.set_fonts(fonts);
}

pub fn apply_theme(ctx: &Context) {
    install_fonts(ctx);
    ctx.tessellation_options_mut(|o| {
        o.feathering = true;
        o.feathering_size_in_pixels = 1.0;
    });

    let mut v = Visuals::dark();
    v.override_text_color = None;
    v.panel_fill = INK;
    v.window_fill = SURF_1;
    v.window_stroke = Stroke::NONE;
    v.window_rounding = Rounding::same(R_BLOCK);
    v.menu_rounding = Rounding::same(R_CTRL);
    v.window_shadow = Shadow { offset: Vec2::new(0.0, 8.0), blur: 24.0, spread: 0.0, color: Color32::from_black_alpha(110) };
    v.popup_shadow = Shadow { offset: Vec2::new(0.0, 5.0), blur: 16.0, spread: 0.0, color: Color32::from_black_alpha(100) };
    v.extreme_bg_color = SURF_2;
    v.faint_bg_color = SURF_1;
    v.code_bg_color = SURF_2;
    v.hyperlink_color = BRUISE_LIT;
    v.warn_fg_color = AMBER;
    v.error_fg_color = RED_LIT;
    v.selection.bg_fill = BRUISE;
    v.selection.stroke = Stroke::NONE;
    v.text_cursor.stroke = Stroke::new(1.5_f32, BONE);
    v.button_frame = false;
    v.collapsing_header_frame = false;
    v.indent_has_left_vline = false;
    v.striped = false;
    v.slider_trailing_fill = true;
    v.window_highlight_topmost = false;

    v.widgets.noninteractive.bg_fill = SURF_2;
    v.widgets.noninteractive.weak_bg_fill = SURF_2;
    v.widgets.noninteractive.bg_stroke = Stroke::NONE;
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TX);
    v.widgets.noninteractive.rounding = Rounding::same(R_CTRL);

    for (w, fill, text, expansion) in [
        (&mut v.widgets.inactive, SURF_2, TX, 0.0_f32),
        (&mut v.widgets.hovered, SURF_3, BONE, 0.0),
        (&mut v.widgets.active, SURF_4, BONE, 0.0),
    ] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.bg_stroke = Stroke::NONE;
        w.fg_stroke = Stroke::new(1.0_f32, text);
        w.rounding = Rounding::same(R_CTRL);
        w.expansion = expansion;
    }

    v.widgets.open.bg_fill = SURF_3;
    v.widgets.open.weak_bg_fill = SURF_3;
    v.widgets.open.bg_stroke = Stroke::NONE;
    v.widgets.open.fg_stroke = Stroke::new(1.0_f32, BONE);
    v.widgets.open.rounding = Rounding::same(R_CTRL);

    let mut text = BTreeMap::new();
    text.insert(TextStyle::Small, FontId::new(T_META, FontFamily::Proportional));
    text.insert(TextStyle::Body, FontId::new(T_BODY, FontFamily::Proportional));
    text.insert(TextStyle::Button, FontId::new(T_BODY, FontFamily::Proportional));
    text.insert(TextStyle::Heading, FontId::new(T_DISPLAY, FontFamily::Proportional));
    text.insert(TextStyle::Monospace, FontId::new(T_META, FontFamily::Monospace));

    let mut style = Style::default();
    style.visuals = v;
    style.text_styles = text;
    style.animation_time = 0.12;
    style.spacing.item_spacing = Vec2::new(6.0, 5.0);
    style.spacing.button_padding = Vec2::new(10.0, 5.0);
    style.spacing.interact_size = Vec2::new(32.0, CTRL_H);
    style.spacing.menu_margin = Margin::same(8.0);
    style.spacing.window_margin = Margin::same(14.0);
    style.spacing.combo_height = 200.0;
    style.spacing.slider_width = 110.0;
    style.spacing.indent = 16.0;
    style.interaction.tooltip_delay = 0.35;
    style.interaction.show_tooltips_only_when_still = false;
    style.spacing.scroll = ScrollStyle {
        bar_width: 4.0,
        floating_width: 3.0,
        floating_allocated_width: 0.0,
        handle_min_length: 24.0,
        bar_inner_margin: 4.0,
        bar_outer_margin: 2.0,
        foreground_color: false,
        dormant_background_opacity: 0.0,
        active_background_opacity: 0.15,
        interact_background_opacity: 0.2,
        dormant_handle_opacity: 0.0,
        active_handle_opacity: 0.3,
        interact_handle_opacity: 0.6,
        ..ScrollStyle::floating()
    };
    ctx.set_style(style);
}

pub fn hex_color(hex: &str) -> Color32 {
    let h = hex.trim_start_matches('#');
    match (h.len() == 6).then(|| u32::from_str_radix(h, 16).ok()).flatten() {
        Some(v) => Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8),
        None => BRUISE_LIT,
    }
}
