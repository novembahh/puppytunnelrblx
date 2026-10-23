use crate::ui::icons::{self, Icon};
use crate::ui::theme::*;
use eframe::egui::{
    self, Align, Color32, CursorIcon, FontId, Frame, Layout, Margin, Pos2, Rect, Response, RichText, Rounding, Sense,
    Shape, Stroke, TextEdit, Ui, Vec2,
};

pub fn lc(s: &str) -> String {
    s.to_lowercase()
}

pub fn muted(text: impl AsRef<str>) -> RichText {
    RichText::new(lc(text.as_ref())).size(T_META).color(TX_DIM)
}

pub fn strong(text: impl AsRef<str>) -> RichText {
    RichText::new(lc(text.as_ref())).size(T_HEAD).color(BONE)
}

pub fn section_label(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(lc(text)).size(T_LABEL).color(TX_FAINT));
}

pub fn page_tabs_status(
    ui: &mut Ui,
    heading: &str,
    status: Option<(&str, Color32)>,
    tabs: &[Tab<'_>],
    active: usize,
) -> Option<usize> {
    let mut hit = None;
    let head_w = ui
        .painter()
        .layout_no_wrap(lc(heading), FontId::proportional(T_DISPLAY), Color32::PLACEHOLDER)
        .size()
        .x;
    let tabs_w: f32 = tabs.iter().map(|t| tab_width(ui, t)).sum::<f32>() + 4.0 * tabs.len().saturating_sub(1) as f32;
    let status_w = status.map_or(0.0, |(label, _)| {
        ui.painter()
            .layout_no_wrap(lc(label), FontId::proportional(T_META), Color32::PLACEHOLDER)
            .size()
            .x
            + 12.0
    });
    let inline = !tabs.is_empty() && head_w + tabs_w + status_w + 40.0 <= ui.available_width();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.label(RichText::new(lc(heading)).size(T_DISPLAY).color(BONE));
        if let Some((label, color)) = status {
            ui.add_space(10.0);
            ui.label(RichText::new(lc(label)).size(T_META).color(color));
        }
        if inline {
            hit = tab_bar_end(ui, tabs, active);
        }
    });
    if !inline && !tabs.is_empty() {
        ui.add_space(4.0);
        hit = tab_bar(ui, tabs, active);
    }
    ui.add_space(24.0);
    hit
}

pub fn title_with(
    ui: &mut Ui,
    title: &str,
    status: Option<(&str, Color32)>,
    add: impl FnOnce(&mut Ui),
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        ui.label(RichText::new(lc(title)).size(T_DISPLAY).color(BONE));
        if let Some((text, color)) = status {
            ui.label(RichText::new(lc(text)).size(T_META).color(color));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), add);
    });
    ui.add_space(28.0);
}

pub fn title(ui: &mut Ui, text: &str, status: Option<(&str, Color32)>) {
    title_with(ui, text, status, |_| {});
}

pub fn index_item(ui: &mut Ui, label: &str, active: bool, count: Option<&str>) -> Response {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 30.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.12);
        let on = ui.ctx().animate_bool_with_time(resp.id.with("on"), active, 0.16);
        let color = mix(mix(TX_FAINT, TX_DIM, t), BONE, on);
        if on > 0.02 {
            let bar = Rect::from_min_size(
                pos2(rect.left() - 12.0, rect.center().y - 7.0),
                Vec2::new(1.5, 14.0 * on),
            );
            ui.painter().rect_filled(bar, Rounding::same(0.75), tint(BRUISE_LIT, 0.9 * on));
        }
        ui.painter().text(
            pos2(rect.left(), rect.center().y),
            egui::Align2::LEFT_CENTER,
            lc(label),
            FontId::proportional(T_BODY),
            color,
        );
        if let Some(c) = count {
            let cc = if active { AMBER } else { TX_FAINT };
            ui.painter().text(
                pos2(rect.right(), rect.center().y),
                egui::Align2::RIGHT_CENTER,
                c,
                FontId::proportional(T_TINY),
                cc,
            );
        }
    }
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    if resp.clicked() {
        crate::ui::sound::play_click();
    }
    resp
}

pub fn sub_index(ui: &mut Ui, items: &[(&str, Option<String>)], active: usize) -> Option<usize> {
    let mut hit = None;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        for (i, (label, count)) in items.iter().enumerate() {
            if index_item(ui, label, i == active, count.as_deref()).clicked() && i != active {
                hit = Some(i);
            }
        }
    });
    hit
}

pub fn surface_shape(rect: Rect, r: f32, base: Color32, lift: f32) -> Shape {
    Shape::rect_filled(rect, Rounding::same(r), mix(base, MILK, lift))
}

pub fn card<R>(ui: &mut Ui, heading: Option<&str>, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.vertical(|ui| {
        ui.set_width(ui.available_width());
        if let Some(h) = heading {
            if !h.is_empty() {
                ui.add_space(6.0);
                ui.label(RichText::new(lc(h)).size(T_META).color(TX_FAINT));
                ui.add_space(6.0);
            }
        }
        add(ui)
    })
    .inner
}

pub fn sheet<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.vertical(|ui| add(ui)).inner
}

pub fn panel<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let bg = ui.painter().add(Shape::Noop);
    let inner = Frame::none()
        .rounding(Rounding::same(R_PANE))
        .inner_margin(Margin::symmetric(26.0, 22.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        });
    let rect = inner.response.rect;
    if ui.is_rect_visible(rect) {
        ui.painter().set(bg, Shape::rect_filled(rect, Rounding::same(R_PANE), mix(INK, MILK, 0.018)));
    }
    inner.inner
}

pub fn pane<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    card(ui, None, add)
}

pub fn field(ui: &mut Ui, label: &str, val: &mut String, hint: &str) {
    ui.label(RichText::new(lc(label)).size(T_META).color(TX_DIM));
    ui.add_space(4.0);
    text_input(ui, egui::Id::new(("field", label)), val, hint, f32::INFINITY);
    ui.add_space(8.0);
}

pub fn text_input(ui: &mut Ui, id: egui::Id, val: &mut String, hint: &str, width: f32) -> Response {
    let focused = ui.memory(|m| m.has_focus(id));
    let outer = if width.is_finite() { width } else { ui.available_width() };
    let bg = ui.painter().add(Shape::Noop);
    let focus_line = ui.painter().add(Shape::Noop);
    let resp = Frame::none()
        .rounding(Rounding::same(R_CTRL))
        .inner_margin(Margin::symmetric(10.0, 6.0))
        .show(ui, |ui| {
            ui.set_width(outer - 20.0);
            ui.add(
                TextEdit::singleline(val)
                    .id(id)
                    .frame(false)
                    .text_color(TX)
                    .hint_text(RichText::new(lc(hint)).color(TX_FAINT))
                    .desired_width(outer - 20.0),
            )
        });
    let rect = Rect::from_min_size(resp.response.rect.min, Vec2::new(outer, resp.response.rect.height().max(CTRL_H)));
    if ui.is_rect_visible(rect) {
        let hover = ui.ctx().animate_bool_with_time(id.with("hover"), resp.inner.hovered() || focused, 0.14);
        let fill = if focused {
            SURF_3
        } else {
            mix(SURF_2, SURF_3, hover)
        };
        ui.painter().set(bg, Shape::rect_filled(rect, Rounding::same(R_CTRL), fill));
        let ring = if focused {
            Shape::rect_stroke(rect, Rounding::same(R_CTRL), Stroke::new(1.0_f32, tint(BRUISE_LIT, 0.55)))
        } else {
            Shape::Noop
        };
        ui.painter().set(focus_line, ring);
    }
    resp.inner
}

fn pos2(x: f32, y: f32) -> Pos2 {
    Pos2::new(x, y)
}

pub fn soft_shadow_shape(rect: Rect, radius: f32, depth: f32) -> Shape {
    let mut layers = Vec::new();
    for (grow, dy, a) in [(5.0, 5.0, 20u8), (2.0, 2.0, 24), (1.0, 1.0, 28)] {
        let r = rect.translate(Vec2::new(0.0, dy * depth)).expand(grow * depth);
        layers.push(Shape::rect_filled(r, Rounding::same(radius + grow), Color32::from_black_alpha(a)));
    }
    Shape::Vec(layers)
}

pub fn modal(
    ctx: &egui::Context,
    id: egui::Id,
    title: &str,
    offset_y: f32,
    add: impl FnOnce(&mut Ui) -> bool,
) -> bool {
    let screen = ctx.screen_rect();
    let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    egui::Area::new(id.with("scrim"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let (bg, resp) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter().rect_filled(bg, 0.0, Color32::from_black_alpha(150));
            if resp.clicked() {
                close = true;
            }

            let w = 360.0_f32.min(screen.width() - 32.0);
            let x = screen.center().x - w * 0.5;
            let y = (screen.center().y - offset_y).max(screen.min.y + 24.0);
            let area = Rect::from_min_size(pos2(x, y), Vec2::new(w, 10.0));
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(area), |ui| {
                let shadow = ui.painter().add(Shape::Noop);
                let back = ui.painter().add(Shape::Noop);
                let resp = Frame::none()
                    .rounding(Rounding::same(R_PANE))
                    .inner_margin(Margin::same(20.0))
                    .show(ui, |ui| {
                        ui.set_width(w - 40.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(lc(title)).size(T_HEAD).color(BONE));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if icon_button(ui, Icon::Close, "close") {
                                    close = true;
                                }
                            });
                        });
                        ui.add_space(12.0);
                        add(ui)
                    });
                let rect = resp.response.rect;
                ui.painter().set(shadow, soft_shadow_shape(rect, R_PANE, 1.2));
                ui.painter().set(back, surface_shape(rect, R_PANE, SURF_2, 0.03));
                if resp.inner {
                    close = true;
                }
            });
        });
    close
}

#[derive(Clone, Copy, PartialEq)]
pub enum Tone {
    Primary,
    Soft,
    Quiet,
    Danger,
    Bruise,
}

pub struct Btn {
    pub tone: Tone,
    pub icon: Option<Icon>,
    pub height: f32,
    pub enabled: bool,
}

impl Btn {
    pub fn new(tone: Tone) -> Self {
        Self { tone, icon: None, height: CTRL_H, enabled: true }
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn height(mut self, h: f32) -> Self {
        self.height = h;
        self
    }

    pub fn enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }
}

fn tone_palette(tone: Tone) -> (Color32, Color32, Color32, Color32) {
    match tone {
        Tone::Primary => (BONE, mix(BONE, Color32::WHITE, 0.35), VOID, VOID),
        Tone::Soft => (Color32::TRANSPARENT, SURF_2, TX_DIM, BONE),
        Tone::Quiet => (Color32::TRANSPARENT, SURF_2, TX_FAINT, TX_DIM),
        Tone::Danger => (Color32::TRANSPARENT, tint(RED, 0.16), RED_LIT, BONE),
        Tone::Bruise => (Color32::TRANSPARENT, tint(BRUISE, 0.22), BRUISE_LIT, BONE),
    }
}

fn label_color(text: Color32, hover_text: Color32, t: f32) -> Color32 {
    mix(text, hover_text, t)
}

fn button(ui: &mut Ui, text: &str, spec: Btn) -> Response {
    let font = FontId::proportional(T_BODY);
    let galley = ui.painter().layout_no_wrap(lc(text), font, Color32::PLACEHOLDER);
    let icon_w = match spec.icon {
        Some(_) => 15.0 + if text.is_empty() { 0.0 } else { 6.0 },
        None => 0.0,
    };
    let pad = if text.is_empty() { 7.0 } else { 12.0 };
    let w = (galley.size().x + icon_w + pad * 2.0).max(spec.height);
    let sense = if spec.enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, spec.height), sense);
    if ui.is_rect_visible(rect) {
        let (fill, fill_hover, text_col, text_hover) = tone_palette(spec.tone);
        let t = ui.ctx().animate_bool_with_time(resp.id, resp.hovered() && spec.enabled, 0.12);
        let down = resp.is_pointer_button_down_on() && spec.enabled;
        let rect = rect.translate(Vec2::new(0.0, if down { 1.0 } else { 0.0 }));
        let bg = if spec.enabled { mix(fill, fill_hover, t) } else { SURF_1 };
        if bg.a() > 0 {
            ui.painter().rect_filled(rect, Rounding::same(R_CTRL), bg);
        }
        let color = if spec.enabled {
            label_color(text_col, text_hover, t)
        } else {
            TX_FAINT
        };
        let color = if down && spec.enabled { mix(color, bg, 0.15) } else { color };
        let mut x = rect.center().x - (galley.size().x + icon_w) * 0.5;
        if let Some(ic) = spec.icon {
            icons::paint(ui.painter(), pos2(x + 7.5, rect.center().y), 14.0, ic, color);
            x += icon_w;
        }
        if !text.is_empty() {
            ui.painter().galley(pos2(x, rect.center().y - galley.size().y * 0.5), galley.clone(), color);
        }
    }
    if spec.enabled {
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

pub fn action(ui: &mut Ui, text: &str, spec: Btn) -> bool {
    click_feedback(&button(ui, text, spec))
}

pub fn primary(ui: &mut Ui, text: &str, enabled: bool) -> bool {
    action(ui, text, Btn::new(Tone::Primary).enabled(enabled))
}

pub fn primary_icon(ui: &mut Ui, icon: Icon, text: &str, enabled: bool) -> bool {
    action(ui, text, Btn::new(Tone::Primary).icon(icon).enabled(enabled))
}

pub fn secondary(ui: &mut Ui, text: &str) -> bool {
    action(ui, text, Btn::new(Tone::Soft))
}

pub fn secondary_icon(ui: &mut Ui, icon: Icon, text: &str) -> bool {
    action(ui, text, Btn::new(Tone::Soft).icon(icon))
}

pub fn quiet(ui: &mut Ui, text: &str) -> bool {
    action(ui, text, Btn::new(Tone::Quiet))
}

pub fn inline_action(ui: &mut Ui, text: &str, active: bool) -> bool {
    let galley = ui.painter().layout_no_wrap(lc(text), FontId::proportional(T_META), Color32::PLACEHOLDER);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(galley.size().x + 4.0, 22.0), Sense::click());
    let t = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.12);
    let color = mix(mix(TX_FAINT, TX_DIM, t), BONE, if active { 1.0 } else { 0.0 });
    ui.painter().galley(
        pos2(rect.left() + 2.0, rect.center().y - galley.size().y * 0.5),
        galley,
        color,
    );
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    if resp.clicked() {
        crate::ui::sound::play_click();
    }
    resp.clicked()
}

pub fn danger(ui: &mut Ui, text: &str) -> bool {
    action(ui, text, Btn::new(Tone::Danger))
}

pub fn icon_button(ui: &mut Ui, icon: Icon, tip: &str) -> bool {
    let r = button(ui, "", Btn::new(Tone::Quiet).icon(icon).height(28.0)).on_hover_text(lc(tip));
    click_feedback(&r)
}

pub struct Tab<'a> {
    pub label: &'a str,
    pub icon: Option<Icon>,
    pub badge: Option<String>,
}

impl<'a> Tab<'a> {
    pub fn new(label: &'a str) -> Self {
        Self { label, icon: None, badge: None }
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn badge(mut self, n: usize) -> Self {
        if n > 0 {
            self.badge = Some(n.to_string());
        }
        self
    }

    pub fn dot(mut self, on: bool) -> Self {
        if on {
            self.badge = Some(String::new());
        }
        self
    }
}

fn tab_width(ui: &Ui, tab: &Tab<'_>) -> f32 {
    let galley = ui.painter().layout_no_wrap(lc(tab.label), FontId::proportional(T_META + 0.5), Color32::PLACEHOLDER);
    let has_text = galley.size().x > 0.0;
    let icon_w = match tab.icon {
        Some(_) => if has_text { 19.0 } else { 15.0 },
        None => 0.0,
    };
    let badge_w = tab.badge.as_ref().map_or(0.0, |b| if b.is_empty() { 5.0 } else { 16.0 });
    let gap = if badge_w > 0.0 { 6.0 } else { 0.0 };
    let pad = if has_text { 9.0 } else { 8.0 };
    pad * 2.0 + icon_w + galley.size().x + gap + badge_w
}

pub fn nav_item(ui: &mut Ui, tab: &Tab<'_>, active: bool) -> Response {
    let galley = ui.painter().layout_no_wrap(lc(tab.label), FontId::proportional(T_META + 0.5), Color32::PLACEHOLDER);
    let has_text = galley.size().x > 0.0;
    let icon_w = match tab.icon {
        Some(_) => if has_text { 19.0 } else { 15.0 },
        None => 0.0,
    };
    let badge_w = tab.badge.as_ref().map_or(0.0, |b| {
        if b.is_empty() {
            0.0
        } else {
            ui.painter().layout_no_wrap(b.clone(), FontId::proportional(T_TINY), Color32::PLACEHOLDER).size().x + 4.0
        }
    });
    let gap = if badge_w > 0.0 { 6.0 } else { 0.0 };
    let pad = if has_text { 8.0 } else { 7.0 };
    let w = pad * 2.0 + icon_w + galley.size().x + gap + badge_w;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 28.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.12);
        let sel = ui.ctx().animate_bool_with_time(resp.id.with("sel"), active, 0.16);
        let color = mix(mix(TX_FAINT, TX_DIM, t), BONE, sel);
        let mut x = rect.left() + pad;
        if let Some(ic) = tab.icon {
            icons::paint(ui.painter(), pos2(x + 6.5, rect.center().y), 13.0, ic, color);
            x += icon_w;
        }
        if has_text {
            let tw = galley.size().x;
            ui.painter().galley(pos2(x, rect.center().y - galley.size().y * 0.5), galley.clone(), color);
            x += tw;
        }
        if sel > 0.01 && has_text {
            let bar = Rect::from_min_size(
                pos2(rect.left() + pad, rect.bottom() - 1.0),
                Vec2::new(galley.size().x + icon_w, 1.0),
            );
            ui.painter().rect_filled(bar, Rounding::ZERO, tint(BRUISE_LIT, 0.8 * sel));
        }
        if let Some(b) = &tab.badge {
            x += gap;
            let c = pos2(x + badge_w * 0.5, rect.center().y);
            if b.is_empty() {
                ui.painter().circle_filled(c, 2.0, AMBER);
            } else {
                let g = ui.painter().layout_no_wrap(b.clone(), FontId::proportional(T_TINY), Color32::PLACEHOLDER);
                ui.painter().galley(c - g.size() * 0.5, g, AMBER);
            }
        }
    }
    if resp.clicked() {
        crate::ui::sound::play_click();
    }
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

pub fn rail_item(ui: &mut Ui, icon: Icon, active: bool, badge: bool) -> Response {
    let size = 32.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.12);
        let on = ui.ctx().animate_bool_with_time(resp.id.with("on"), active, 0.16);
        if t > 0.01 {
            ui.painter().rect_filled(rect, Rounding::same(R_SM), tint(MILK, 0.035 * t));
        }
        let color = mix(mix(TX_FAINT, TX_DIM, t), BONE, on);
        icons::paint(ui.painter(), rect.center(), 18.0, icon, color);
        if on > 0.02 {
            let bar = Rect::from_center_size(
                pos2(rect.left() - 6.0, rect.center().y),
                Vec2::new(1.5, 14.0 * on),
            );
            ui.painter().rect_filled(bar, Rounding::same(1.0), tint(BRUISE_LIT, 0.9 * on));
        }
        if badge {
            ui.painter().circle_filled(pos2(rect.right() - 7.0, rect.top() + 7.0), 2.0, AMBER);
        }
    }
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    if resp.clicked() {
        crate::ui::sound::play_click();
    }
    resp
}

pub fn tab_bar(ui: &mut Ui, tabs: &[Tab<'_>], active: usize) -> Option<usize> {
    let mut hit = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (i, tab) in tabs.iter().enumerate() {
            if nav_item(ui, tab, i == active).clicked() && i != active {
                hit = Some(i);
            }
        }
    });
    hit
}

pub fn tab_bar_end(ui: &mut Ui, tabs: &[Tab<'_>], active: usize) -> Option<usize> {
    let mut hit = None;
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (i, tab) in tabs.iter().enumerate().rev() {
            if nav_item(ui, tab, i == active).clicked() && i != active {
                hit = Some(i);
            }
        }
    });
    hit
}

pub fn pill(ui: &mut Ui, text: &str, active: bool, filled: bool) -> Response {
    pill_icon(ui, None, text, active, filled)
}

pub fn pill_icon(ui: &mut Ui, icon: Option<Icon>, text: &str, active: bool, filled: bool) -> Response {
    let galley = ui.painter().layout_no_wrap(lc(text), FontId::proportional(T_META + 0.5), Color32::PLACEHOLDER);
    let icon_w = match icon {
        Some(_) => 14.0 + if galley.size().x > 0.0 { 5.0 } else { 0.0 },
        None => 0.0,
    };
    let w = galley.size().x + icon_w + 14.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 24.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.12);
        let on = ui.ctx().animate_bool_with_time(resp.id.with("on"), active, 0.16);
        let color = mix(mix(TX_FAINT, TX_DIM, t), BONE, on);
        let _ = filled;
        let mut x = rect.center().x - (galley.size().x + icon_w) * 0.5;
        if let Some(ic) = icon {
            icons::paint(ui.painter(), pos2(x + 7.0, rect.center().y), 13.0, ic, color);
            x += icon_w;
        }
        if galley.size().x > 0.0 {
            ui.painter().galley(pos2(x, rect.center().y - galley.size().y * 0.5), galley, color);
        }
    }
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    if resp.clicked() {
        crate::ui::sound::play_click();
    }
    resp
}

pub fn segmented(ui: &mut Ui, current: &mut String, options: &[(&str, &str)]) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (val, label) in options {
            let on = current == val;
            let galley = ui.painter().layout_no_wrap(lc(label), FontId::proportional(T_META + 0.5), Color32::PLACEHOLDER);
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(galley.size().x + 12.0, 24.0), Sense::click());
            if ui.is_rect_visible(rect) {
                let t = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.12);
                let col = mix(mix(TX_FAINT, TX_DIM, t), BONE, if on { 1.0 } else { 0.0 });
                ui.painter().galley(rect.center() - galley.size() * 0.5, galley, col);
                if on {
                    ui.painter().rect_filled(
                        Rect::from_min_size(pos2(rect.left() + 6.0, rect.bottom() - 1.0), Vec2::new(rect.width() - 12.0, 1.0)),
                        Rounding::ZERO,
                        tint(BRUISE_LIT, 0.8),
                    );
                }
            }
            let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
            if resp.clicked() {
                crate::ui::sound::play_click();
                if !on {
                    *current = val.to_string();
                    changed = true;
                }
            }
        }
    });
    changed
}

pub fn toggle(ui: &mut Ui, on: &mut bool, label: &str) {
    ui.horizontal(|ui| {
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(30.0, 17.0), Sense::click());
        let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
        let t = ui.ctx().animate_bool_with_time(resp.id, *on, 0.18);
        let hov = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), 0.14);
        let off = mix(SURF_2, SURF_3, hov);
        let on_c = mix(BRUISE, BRUISE_LIT, hov * 0.45);
        ui.painter().rect_filled(rect, Rounding::same(8.5), mix(off, on_c, t));
        let knob_r = 5.5;
        let x = egui::lerp((rect.left() + 8.0)..=(rect.right() - 8.0), t);
        let c = pos2(x, rect.center().y);
        ui.painter().circle_filled(c, knob_r, mix(TX_DIM, BONE, t));
        if label.is_empty() {
            if resp.clicked() {
                *on = !*on;
                crate::ui::sound::play_click();
            }
            return;
        }
        let text = ui
            .add(egui::Label::new(RichText::new(lc(label)).size(T_BODY).color(TX)).sense(Sense::click()))
            .on_hover_cursor(CursorIcon::PointingHand);
        if resp.clicked() || text.clicked() {
            *on = !*on;
            crate::ui::sound::play_click();
        }
    });
}

pub fn switch_row(ui: &mut Ui, on: &mut bool, label: &str, hint: &str) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.add_space(2.0);
            ui.label(RichText::new(lc(label)).size(T_BODY).color(TX));
            if !hint.is_empty() {
                ui.label(RichText::new(lc(hint)).size(T_META).color(TX_FAINT));
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            toggle(ui, on, "");
        });
    });
    ui.add_space(6.0);
}

pub fn block<R>(
    ui: &mut Ui,
    radius: f32,
    margin: Margin,
    base: Color32,
    lift: f32,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    let bg = ui.painter().add(Shape::Noop);
    let r = Frame::none()
        .rounding(Rounding::same(radius))
        .inner_margin(margin)
        .show(ui, |ui| add(ui));
    if ui.is_rect_visible(r.response.rect) {
        ui.painter().set(bg, surface_shape(r.response.rect, radius, base, lift));
    }
    r.inner
}

pub const LABEL_W: f32 = 112.0;

pub fn key_row<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let out = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::new(LABEL_W, CTRL_H), Sense::hover());
        ui.painter().text(
            pos2(rect.left(), rect.center().y),
            egui::Align2::LEFT_CENTER,
            lc(label),
            FontId::proportional(T_META),
            TX_FAINT,
        );
        add(ui)
    });
    ui.add_space(1.0);
    out.inner
}

pub fn group(ui: &mut Ui, label: &str) {
    ui.add_space(22.0);
    ui.label(RichText::new(lc(label)).size(T_META).color(TX_FAINT));
    ui.add_space(5.0);
}

pub fn toggle_row(ui: &mut Ui, on: &mut bool, label: &str) {
    key_row(ui, label, |ui| {
        ui.add_space(4.0);
        toggle(ui, on, "");
    });
}

pub fn hairline(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, Rounding::ZERO, hairline_color());
    ui.add_space(12.0);
}

pub fn row_frame<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let out = block(ui, R_CTRL, Margin::symmetric(14.0, 10.0), mix(INK, MILK, 0.015), 0.0, |ui| {
        ui.set_width(ui.available_width());
        add(ui)
    });
    ui.add_space(6.0);
    out
}

pub fn chip(ui: &mut Ui, text: &str, color: Color32) {
    let galley = ui.painter().layout_no_wrap(lc(text), FontId::proportional(T_META - 0.5), color);
    let (rect, _) = ui.allocate_exact_size(galley.size() + Vec2::new(2.0, 2.0), Sense::hover());
    ui.painter().galley(rect.center() - galley.size() * 0.5, galley, color);
}

pub fn dot(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(6.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.5, tint(color, 0.14));
    ui.painter().circle_filled(rect.center(), 2.5, color);
}

pub fn status(ui: &mut Ui, text: &str, color: Color32) {
    ui.label(RichText::new(lc(text)).size(T_META).color(color));
}

pub fn notice(ui: &mut Ui, color: Color32, text: &str) {
    let galley = ui.painter().layout(
        lc(text),
        FontId::proportional(T_META),
        Color32::PLACEHOLDER,
        ui.available_width(),
    );
    ui.painter().galley(ui.cursor().min, galley.clone(), mix(color, BONE, 0.15));
    ui.allocate_exact_size(Vec2::new(ui.available_width(), galley.size().y), Sense::hover());
    ui.add_space(2.0);
}

pub fn empty_state(ui: &mut Ui, _icon: Icon, head: &str, sub: &str) {
    ui.add_space(24.0);
    ui.label(RichText::new(lc(head)).size(T_BODY).color(TX_DIM));
    if !sub.is_empty() {
        ui.add_space(2.0);
        ui.label(RichText::new(lc(sub)).size(T_META).color(TX_FAINT));
    }
    ui.add_space(8.0);
}

pub fn copy_row(ui: &mut Ui, label: &str, val: &str, big: bool) -> bool {
    copy_row_state(ui, label, val, big, false)
}

pub fn copy_row_state(ui: &mut Ui, label: &str, val: &str, big: bool, flash: bool) -> bool {
    let mut hit = false;
    let height = if big { 36.0 } else { 32.0 };
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    let t = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.12);
    if ui.is_rect_visible(rect) && t > 0.01 {
        ui.painter().rect_filled(rect, Rounding::same(R_SM), tint(MILK, 0.03 * t));
    }
    let label_w = if label.is_empty() { 0.0 } else { LABEL_W };
    if !label.is_empty() {
        ui.painter().text(
            pos2(rect.left() + 8.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            lc(label),
            FontId::proportional(T_META),
            TX_FAINT,
        );
    }
    let btn_w = 46.0;
    let text_rect = Rect::from_min_max(
        rect.left_top() + Vec2::new(label_w + 8.0, 0.0),
        rect.right_bottom() - Vec2::new(btn_w + 6.0, 0.0),
    );
    let galley = ui.painter().layout(
        val.to_string(),
        FontId::monospace(if big { T_BODY } else { T_META }),
        Color32::PLACEHOLDER,
        text_rect.width(),
    );
    let ty = text_rect.center().y - galley.size().y * 0.5;
    ui.painter().galley(pos2(text_rect.left(), ty), galley, TX);
    let button_rect = Rect::from_center_size(pos2(rect.right() - btn_w * 0.5 - 6.0, rect.center().y), Vec2::new(btn_w, 24.0));
    let br = ui.interact(button_rect, egui::Id::new(("copy_row", label, val)), Sense::click());
    let bt = ui.ctx().animate_bool_with_time(br.id, br.hovered(), 0.12);
    let text = if flash { "copied" } else { "copy" };
    let color = if flash { AMBER } else { mix(TX_FAINT, BONE, bt.max(0.25)) };
    ui.painter().text(button_rect.center(), egui::Align2::CENTER_CENTER, text, FontId::proportional(T_TINY), color);
    if resp.clicked() || br.on_hover_cursor(CursorIcon::PointingHand).clicked() {
        crate::ui::sound::play_click();
        hit = true;
    }
    hit
}

pub fn progress(ui: &mut Ui, frac: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 2.0), Sense::hover());
    ui.painter().rect_filled(rect, Rounding::ZERO, SURF_2);
    let w = rect.width() * frac.clamp(0.0, 1.0);
    if w > 1.0 {
        ui.painter().rect_filled(
            Rect::from_min_size(rect.left_top(), Vec2::new(w, rect.height())),
            Rounding::ZERO,
            BRUISE_LIT,
        );
    }
}

pub fn divider(ui: &mut Ui) {
    ui.add_space(8.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, Rounding::ZERO, hairline_color());
    ui.add_space(8.0);
}

