use crate::ui::icons::{self, Icon};
use crate::ui::theme::*;
use eframe::egui::{
    self, epaint::Vertex, Color32, CursorIcon, FontId, Frame, Margin, Mesh, Painter, Pos2, Rect, Response, RichText,
    Rounding, Sense, Shape, Stroke, TextEdit, Ui, Vec2,
};
use std::f32::consts::PI;


pub fn lc(s: &str) -> String {
    s.to_lowercase()
}

pub fn muted(text: impl AsRef<str>) -> RichText {
    RichText::new(lc(text.as_ref())).size(13.0).color(DIM)
}

pub fn page_header(ui: &mut Ui, title: &str, subtitle: &str) {
    ui.label(RichText::new(lc(title)).size(24.0).color(INK));
    if !subtitle.is_empty() {
        ui.label(muted(subtitle));
    }
    ui.add_space(14.0);
}

pub fn card<R>(ui: &mut Ui, title: Option<&str>, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::none()
        .fill(CARD)
        .rounding(18.0)
        .inner_margin(Margin::same(20.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if let Some(t) = title {
                ui.label(RichText::new(lc(t)).size(13.0).color(DIM));
                ui.add_space(6.0);
            }
            add(ui)
        })
        .inner
}

pub fn field(ui: &mut Ui, label: &str, value: &mut String, hint: &str) {
    ui.label(muted(label));
    text_input(ui, egui::Id::new(("field", label)), value, hint, f32::INFINITY);
    ui.add_space(6.0);
}

pub fn text_input(ui: &mut Ui, id: egui::Id, value: &mut String, hint: &str, width: f32) -> Response {
    let focused = ui.memory(|m| m.has_focus(id));
    let outer = if width.is_finite() { width } else { ui.available_width() };
    let resp = Frame::none()
        .fill(if focused { FIELD_HI } else { FIELD })
        .rounding(14.0)
        .inner_margin(Margin::symmetric(14.0, 9.0))
        .show(ui, |ui| {
            ui.add(
                TextEdit::singleline(value)
                    .id(id)
                    .frame(false)
                    .hint_text(RichText::new(lc(hint)).color(DIM))
                    .desired_width(outer - 28.0),
            )
        });

    let r = resp.response.rect;
    ui.painter().line_segment(
        [pos2(r.left() + 12.0, r.top() + 1.0), pos2(r.right() - 12.0, r.top() + 1.0)],
        Stroke::new(1.0_f32, Color32::from_black_alpha(46)),
    );
    resp.inner
}

fn pos2(x: f32, y: f32) -> Pos2 {
    Pos2::new(x, y)
}


pub fn rounded_outline(rect: Rect, r: f32) -> Vec<Pos2> {
    let r = r.min(rect.height() * 0.5).min(rect.width() * 0.5);
    let mut pts = Vec::new();
    for (cx, cy, a0) in [
        (rect.right() - r, rect.top() + r, -PI / 2.0),
        (rect.right() - r, rect.bottom() - r, 0.0),
        (rect.left() + r, rect.bottom() - r, PI / 2.0),
        (rect.left() + r, rect.top() + r, PI),
    ] {
        for i in 0..=8 {
            let a = a0 + (PI / 2.0) * i as f32 / 8.0;
            pts.push(pos2(cx + r * a.cos(), cy + r * a.sin()));
        }
    }
    pts
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    a.lerp_to_gamma(b, t.clamp(0.0, 1.0))
}


pub fn gradient_rrect(painter: &Painter, rect: Rect, r: f32, top: Color32, bottom: Color32) {
    let ring = rounded_outline(rect, r);
    let at = |p: Pos2| lerp_color(top, bottom, (p.y - rect.top()) / rect.height().max(1.0));
    let mut mesh = Mesh::default();
    let center = rect.center();
    mesh.vertices.push(Vertex { pos: center, uv: egui::epaint::WHITE_UV, color: at(center) });
    for p in &ring {
        mesh.vertices.push(Vertex { pos: *p, uv: egui::epaint::WHITE_UV, color: at(*p) });
    }
    let n = ring.len() as u32;
    for i in 0..n {
        mesh.indices.extend_from_slice(&[0, 1 + i, 1 + (i + 1) % n]);
    }
    painter.add(Shape::mesh(mesh));
}


pub fn soft_shadow(painter: &Painter, rect: Rect, radius: f32, depth: f32) {
    for (grow, dy, a) in [(1.5, 3.0, 20u8), (0.5, 2.0, 28), (0.0, 1.0, 34)] {
        let r = rect.translate(Vec2::new(0.0, dy * depth)).expand(grow * depth);
        painter.add(Shape::convex_polygon(rounded_outline(r, radius + grow), Color32::from_black_alpha(a), Stroke::NONE));
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Tone {
    Mint,
    Soft,
    Danger,
}

fn tone_colors(tone: Tone) -> (Color32, Color32, Color32) {

    match tone {
        Tone::Mint => (Color32::from_rgb(126, 240, 194), Color32::from_rgb(78, 207, 154), MINT_INK),
        Tone::Soft => (Color32::from_rgb(62, 56, 54), Color32::from_rgb(44, 40, 39), INK),
        Tone::Danger => (Color32::from_rgb(236, 98, 86), Color32::from_rgb(196, 62, 52), Color32::WHITE),
    }
}


fn jelly_body(ui: &Ui, rect: Rect, resp: &Response, tone: Tone, enabled: bool) -> Color32 {
    let (fill, text) = match tone {
        Tone::Mint => (MINT, MINT_INK),
        Tone::Soft => (FIELD, INK),
        Tone::Danger => (ERR, Color32::WHITE),
    };
    let hover = ui.ctx().animate_bool_with_time(resp.id, resp.hovered() && enabled, 0.12);
    let fill = if tone == Tone::Mint { lerp_color(fill, MINT_HOVER, hover) } else { lerp_color(fill, FIELD_HI, hover) };
    let fill = if enabled { fill } else { lerp_color(fill, BG, 0.5) };
    ui.painter().rect_filled(rect, Rounding::same(rect.height() * 0.5), fill);
    if enabled { text } else { lerp_color(text, BG, 0.5) }
}

fn jelly_button(ui: &mut Ui, text: &str, icon: Option<Icon>, tone: Tone, enabled: bool, height: f32) -> Response {
    let font = FontId::proportional(14.0);
    let label = lc(text);
    let galley = ui.painter().layout_no_wrap(label, font, INK);
    let icon_w = if icon.is_some() { 18.0 + if text.is_empty() { 0.0 } else { 7.0 } } else { 0.0 };
    let pad = if text.is_empty() { 8.0 } else { 18.0 };
    let w = (galley.size().x + icon_w + pad * 2.0).max(height);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, height), if enabled { Sense::click() } else { Sense::hover() });
    if ui.is_rect_visible(rect) {
        let color = jelly_body(ui, rect, &resp, tone, enabled);
        let down = resp.is_pointer_button_down_on() && enabled;
        let rect = rect.translate(Vec2::new(0.0, if down { 1.0 } else { 0.0 }));
        let mut x = rect.center().x - (galley.size().x + icon_w) * 0.5;
        if let Some(ic) = icon {
            icons::paint(ui.painter(), pos2(x + 9.0, rect.center().y), 17.0, ic, color);
            x += icon_w;
        }
        if !text.is_empty() {
            let galley = ui.painter().layout_no_wrap(lc(text), FontId::proportional(14.0), color);
            ui.painter().galley(pos2(x, rect.center().y - galley.size().y * 0.5), galley, color);
        }
    }
    if enabled {
        resp.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        resp
    }
}

fn click_feedback(resp: &Response) -> bool {
    let clicked = resp.clicked();
    if clicked {
        crate::ui::sound::play_click();
    }
    clicked
}

pub fn primary(ui: &mut Ui, text: &str, enabled: bool) -> bool {
    click_feedback(&jelly_button(ui, text, None, Tone::Mint, enabled, 36.0))
}

pub fn primary_icon(ui: &mut Ui, icon: Icon, text: &str, enabled: bool) -> bool {
    click_feedback(&jelly_button(ui, text, Some(icon), Tone::Mint, enabled, 36.0))
}

pub fn secondary(ui: &mut Ui, text: &str) -> bool {
    click_feedback(&jelly_button(ui, text, None, Tone::Soft, true, 36.0))
}

pub fn secondary_icon(ui: &mut Ui, icon: Icon, text: &str) -> bool {
    click_feedback(&jelly_button(ui, text, Some(icon), Tone::Soft, true, 36.0))
}

pub fn danger(ui: &mut Ui, text: &str) -> bool {
    click_feedback(&jelly_button(ui, text, None, Tone::Danger, true, 36.0))
}


pub fn icon_button(ui: &mut Ui, icon: Icon, tip: &str) -> bool {
    let r = jelly_button(ui, "", Some(icon), Tone::Soft, true, 32.0).on_hover_text(lc(tip));
    click_feedback(&r)
}


pub fn pill(ui: &mut Ui, text: &str, active: bool, mint_fill: bool) -> Response {
    pill_icon(ui, None, text, active, mint_fill)
}

pub fn pill_icon(ui: &mut Ui, icon: Option<Icon>, text: &str, active: bool, mint_fill: bool) -> Response {
    let label = lc(text);
    let galley = ui.painter().layout_no_wrap(label.clone(), FontId::proportional(14.0), INK);
    let icon_w = if icon.is_some() { 18.0 + if text.is_empty() { 0.0 } else { 7.0 } } else { 0.0 };
    let (w, h) = (galley.size().x + icon_w + 32.0, 34.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());
    if ui.is_rect_visible(rect) {
        let hover = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.12);
        let text_color = if active && mint_fill {
            let color = jelly_body(ui, rect, &resp, Tone::Mint, true);
            color
        } else if active {
            let r = rect.height() * 0.5;
            ui.painter().rect_filled(rect, Rounding::same(r), FIELD);
            MINT
        } else {
            if hover > 0.01 {
                let a = (hover * 255.0 * 0.07) as u8;
                ui.painter().rect_filled(rect, Rounding::same(rect.height() * 0.5), Color32::from_white_alpha(a.max(8)));
            }
            lerp_color(DIM, INK, hover)
        };
        let mut x = rect.center().x - (galley.size().x + icon_w) * 0.5;
        if let Some(ic) = icon {
            icons::paint(ui.painter(), pos2(x + 9.0, rect.center().y), 17.0, ic, text_color);
            x += icon_w;
        }
        let g = ui.painter().layout_no_wrap(label, FontId::proportional(14.0), text_color);
        ui.painter().galley(pos2(x, rect.center().y - g.size().y * 0.5), g, text_color);
    }
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    if resp.clicked() {
        crate::ui::sound::play_click();
    }
    resp
}

pub fn segmented(ui: &mut Ui, current: &mut String, options: &[(&str, &str)]) -> bool {
    let mut changed = false;
    Frame::none()
        .fill(FIELD)
        .rounding(Rounding::same(PILL + 3.0))
        .inner_margin(Margin::same(3.0))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.horizontal(|ui| {
                for (value, label) in options {
                    if pill(ui, label, current == value, true).clicked() && current != value {
                        *current = value.to_string();
                        changed = true;
                    }
                }
            });
        });
    changed
}

pub fn toggle(ui: &mut Ui, value: &mut bool, label: &str) {
    ui.horizontal(|ui| {
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(42.0, 26.0), Sense::click());
        let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
        let t = ui.ctx().animate_bool_with_time(resp.id, *value, 0.16);
        let r = rect.height() * 0.5;
        let track_top = lerp_color(Color32::from_rgb(46, 42, 41), Color32::from_rgb(126, 240, 194), t);
        let track_bottom = lerp_color(Color32::from_rgb(62, 56, 54), Color32::from_rgb(78, 207, 154), t);
        let _ = (track_bottom, r);
        ui.painter().rect_filled(rect, Rounding::same(rect.height() * 0.5), track_top);
        let knob_r = if resp.hovered() { 10.0 } else { 9.5 };
        let x = egui::lerp((rect.left() + r)..=(rect.right() - r), t);
        let c = pos2(x, rect.center().y);
        ui.painter().circle_filled(c + Vec2::new(0.0, 1.5), knob_r, Color32::from_black_alpha(60));
        ui.painter().circle_filled(c, knob_r, lerp_color(Color32::from_rgb(236, 228, 221), Color32::from_rgb(250, 248, 245), t));
        let text = ui
            .add(egui::Label::new(RichText::new(lc(label)).size(14.0).color(INK)).sense(Sense::click()))
            .on_hover_cursor(CursorIcon::PointingHand);
        if resp.clicked() || text.clicked() {
            *value = !*value;
            crate::ui::sound::play_click();
        }
    });
}


pub fn row_frame<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::none()
        .fill(CARD)
        .rounding(14.0)
        .inner_margin(Margin::symmetric(16.0, 12.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}


pub fn chip(ui: &mut Ui, text: &str, color: Color32) {
    let galley = ui.painter().layout_no_wrap(lc(text), FontId::proportional(12.0), color);
    let (rect, _) = ui.allocate_exact_size(galley.size() + Vec2::new(20.0, 8.0), Sense::hover());
    let bg = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 28);
    ui.painter().rect_filled(rect, Rounding::same(rect.height() * 0.5), bg);
    ui.painter().galley(rect.center() - galley.size() * 0.5, galley, color);
}
