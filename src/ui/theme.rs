use eframe::egui::{
    style::ScrollStyle, Color32, Context, FontData, FontDefinitions, FontFamily, Rounding, Stroke,
    Style, Vec2, Visuals,
};

pub const BG: Color32 = Color32::from_rgb(13, 12, 16);
pub const BG_SOFT: Color32 = Color32::from_rgb(18, 17, 22);
pub const CARD: Color32 = Color32::from_rgb(25, 23, 30);
pub const CARD_HI: Color32 = Color32::from_rgb(32, 30, 38);
pub const FIELD: Color32 = Color32::from_rgb(33, 31, 39);
pub const FIELD_HI: Color32 = Color32::from_rgb(42, 39, 49);
pub const FIELD_LO: Color32 = Color32::from_rgb(52, 48, 60);
pub const LINE: Color32 = Color32::from_rgb(41, 38, 47);
pub const INK: Color32 = Color32::from_rgb(238, 233, 241);
pub const DIM: Color32 = Color32::from_rgb(152, 145, 162);
pub const MINT: Color32 = Color32::from_rgb(126, 231, 186);
pub const MINT_HOVER: Color32 = Color32::from_rgb(146, 243, 201);
pub const MINT_DIM: Color32 = Color32::from_rgb(63, 132, 107);
pub const MINT_INK: Color32 = Color32::from_rgb(9, 27, 21);
pub const ERR: Color32 = Color32::from_rgb(240, 120, 133);
pub const WARN: Color32 = Color32::from_rgb(236, 196, 128);

pub const PILL: f32 = 14.0;
pub const RADIUS: f32 = 16.0;
pub const RADIUS_SM: f32 = 11.0;

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
    ctx.set_fonts(fonts);
}

pub fn apply_theme(ctx: &Context) {
    install_fonts(ctx);

    let mut v = Visuals::dark();
    v.override_text_color = None;
    v.panel_fill = BG;
    v.window_fill = CARD;
    v.window_stroke = Stroke::NONE;
    v.window_rounding = Rounding::same(RADIUS);
    v.menu_rounding = Rounding::same(RADIUS_SM);
    v.extreme_bg_color = FIELD;
    v.faint_bg_color = CARD;
    v.hyperlink_color = MINT;
    v.selection.bg_fill = Color32::from_rgb(43, 88, 73);
    v.selection.stroke = Stroke::NONE;
    v.text_cursor.stroke = Stroke::new(1.5_f32, MINT);

    let r = Rounding::same(PILL);
    v.widgets.noninteractive.bg_fill = BG;
    v.widgets.noninteractive.weak_bg_fill = BG;
    v.widgets.noninteractive.bg_stroke = Stroke::NONE;
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, INK);
    v.widgets.noninteractive.rounding = r;

    for (w, fill, grow) in [
        (&mut v.widgets.inactive, FIELD, 0.0_f32),
        (&mut v.widgets.hovered, FIELD_HI, 1.0),
        (&mut v.widgets.active, FIELD_LO, -1.0),
    ] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.bg_stroke = Stroke::NONE;
        w.fg_stroke = Stroke::new(1.0_f32, INK);
        w.rounding = r;
        w.expansion = grow;
    }
    v.widgets.open.bg_fill = FIELD_HI;
    v.widgets.open.weak_bg_fill = FIELD_HI;
    v.widgets.open.bg_stroke = Stroke::NONE;
    v.widgets.open.rounding = r;

    let mut style = Style::default();
    style.visuals = v;
    style.spacing.item_spacing = Vec2::new(8.0, 8.0);
    style.spacing.button_padding = Vec2::new(16.0, 7.0);
    style.spacing.interact_size = Vec2::new(40.0, 34.0);
    style.spacing.scroll = ScrollStyle { bar_width: 6.0, ..ScrollStyle::floating() };
    ctx.set_style(style);
}

pub fn hex_color(hex: &str) -> Color32 {
    let h = hex.trim_start_matches('#');
    match (h.len() == 6).then(|| u32::from_str_radix(h, 16).ok()).flatten() {
        Some(v) => Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8),
        None => MINT,
    }
}
