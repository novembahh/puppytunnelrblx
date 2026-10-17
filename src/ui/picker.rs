
use crate::ui::theme::*;
use crate::ui::widgets::*;
use eframe::egui::{self, Align, Align2, Color32, Layout, Margin, RichText, Rounding, ScrollArea, Sense, Vec2};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PickTarget {
    Map,
    Studio,
    Models,
}

pub enum PickResult {
    Open,
    Cancel,
    Native(PickTarget),
    Done(PickTarget, Vec<PathBuf>),
}

pub struct Picker {
    target: PickTarget,
    title: &'static str,
    exts: &'static [&'static str],
    multi: bool,
    dir: PathBuf,
    path_text: String,
    selected: Vec<PathBuf>,
}

fn home_dir() -> PathBuf {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn drives() -> Vec<PathBuf> {
    ('A'..='Z')
        .map(|c| PathBuf::from(format!("{}:\\", c)))
        .filter(|p| p.exists())
        .collect()
}

impl Picker {
    pub fn new(target: PickTarget, title: &'static str, start: &str, exts: &'static [&'static str], multi: bool) -> Self {
        let p = Path::new(start.trim());
        let dir = if p.is_dir() {
            p.to_path_buf()
        } else {
            p.parent().filter(|d| d.is_dir()).map(Path::to_path_buf).unwrap_or_else(home_dir)
        };
        let path_text = dir.to_string_lossy().to_string();
        Self { target, title, exts, multi, dir, path_text, selected: Vec::new() }
    }

    fn go(&mut self, dir: PathBuf) {
        self.path_text = dir.to_string_lossy().to_string();
        self.dir = dir;
        self.selected.clear();
    }

    fn entries(&self) -> (Vec<PathBuf>, Vec<PathBuf>) {
        if self.dir.as_os_str().is_empty() {
            return (drives(), Vec::new());
        }
        let mut dirs = Vec::new();
        let mut files = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&self.dir) {
            for e in rd.flatten() {
                let p = e.path();
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with('.') {
                    continue;
                }
                if p.is_dir() {
                    dirs.push(p);
                } else if self.exts.is_empty()
                    || p.extension()
                        .and_then(|x| x.to_str())
                        .map_or(false, |x| self.exts.iter().any(|w| w.eq_ignore_ascii_case(x)))
                {
                    files.push(p);
                }
            }
        }
        let key = |p: &PathBuf| p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        dirs.sort_by_key(key);
        files.sort_by_key(key);
        (dirs, files)
    }

    pub fn show(&mut self, ctx: &egui::Context) -> PickResult {
        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("picker_backdrop"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (rect, _) = ui.allocate_exact_size(screen.size(), Sense::click_and_drag());
                ui.painter().rect_filled(rect, Rounding::ZERO, Color32::from_black_alpha(170));
            });

        let mut result = PickResult::Open;
        let width = (screen.width() - 48.0).clamp(320.0, 560.0);
        egui::Area::new(egui::Id::new("picker"))
            .order(egui::Order::Tooltip)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(CARD)
                    .rounding(20.0)
                    .inner_margin(Margin::same(22.0))
                    .show(ui, |ui| {
                        ui.set_width(width);
                        ui.label(RichText::new(self.title).size(20.0).color(INK));
                        ui.add_space(10.0);

                        let id = egui::Id::new("picker_path");
                        let r = text_input(ui, id, &mut self.path_text, "Path", f32::INFINITY);
                        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            let p = PathBuf::from(self.path_text.trim());
                            if p.is_dir() {
                                self.go(p);
                            }
                        }
                        ui.add_space(6.0);

                        let (dirs, files) = self.entries();
                        let mut navigate = None;
                        let mut pick = None;
                        let mut accept = false;
                        ScrollArea::vertical().id_salt("picker_scroll_1").max_height(300.0).auto_shrink([false, false]).show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            if !self.dir.as_os_str().is_empty() {
                                let up = self.dir.parent().map(Path::to_path_buf).unwrap_or_default();
                                if list_row(ui, "..", false, false).clicked() {
                                    navigate = Some(up);
                                }
                            }
                            for d in &dirs {
                                let name = d.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| d.to_string_lossy().to_string());
                                if list_row(ui, &format!("{}", name), true, false).clicked() {
                                    navigate = Some(d.clone());
                                }
                            }
                            for f in &files {
                                let name = f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                                let sel = self.selected.contains(f);
                                let resp = list_row(ui, &name, false, sel);
                                if resp.clicked() {
                                    pick = Some(f.clone());
                                }
                                if resp.double_clicked() && !self.multi {
                                    accept = true;
                                }
                            }
                            if dirs.is_empty() && files.is_empty() {
                                ui.label(muted("Nothing here."));
                            }
                        });

                        if let Some(d) = navigate {
                            self.go(d);
                        }
                        if let Some(f) = pick {
                            if self.multi {
                                if let Some(i) = self.selected.iter().position(|s| *s == f) {
                                    self.selected.remove(i);
                                } else {
                                    self.selected.push(f);
                                }
                            } else {
                                self.selected = vec![f];
                            }
                        }

                        ui.add_space(12.0);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let label = if self.multi && self.selected.len() > 1 {
                                format!("Select {}", self.selected.len())
                            } else {
                                "Select".to_string()
                            };
                            if primary(ui, &label, !self.selected.is_empty()) || accept {
                                result = PickResult::Done(self.target, self.selected.clone());
                            }
                            if secondary(ui, "Cancel") {
                                result = PickResult::Cancel;
                            }
                            if secondary(ui, "System dialog") {
                                result = PickResult::Native(self.target);
                            }
                        });
                    });
            });
        result
    }
}

fn list_row(ui: &mut egui::Ui, text: &str, folder: bool, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 32.0), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if selected {
        ui.painter().rect_filled(rect, Rounding::same(10.0), FIELD_HI);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, Rounding::same(10.0), FIELD);
    }
    let color = if selected {
        MINT
    } else if folder {
        INK
    } else {
        DIM
    };
    let label = if folder { format!("{}/", text) } else { text.to_string() };
    ui.painter().text(
        rect.left_center() + Vec2::new(12.0, 0.0),
        Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(14.0),
        color,
    );
    if resp.clicked() {
        crate::ui::sound::play_click();
    }
    resp
}


pub fn native_pick(target: PickTarget, title: &str, current: &str, exts: &[&str], multi: bool) -> Vec<PathBuf> {
    let mut dlg = rfd::FileDialog::new().set_title(title);
    if !exts.is_empty() {
        dlg = dlg.add_filter("Files", exts);
    }
    let cur = Path::new(current.trim());
    if let Some(dir) = if cur.is_dir() { Some(cur) } else { cur.parent().filter(|d| d.is_dir()) } {
        dlg = dlg.set_directory(dir);
    }
    let _ = target;
    if multi {
        dlg.pick_files().unwrap_or_default()
    } else {
        dlg.pick_file().into_iter().collect()
    }
}
