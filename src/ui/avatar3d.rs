

use crate::ui::theme::*;
use eframe::egui::{self, pos2, Color32, Pos2, Rounding, Sense, Shape, Stroke, Ui, Vec2};


fn brick_color(id: u32) -> Color32 {
    let (r, g, b) = match id {
        1 => (242, 243, 243),
        5 => (215, 197, 154),
        9 => (232, 186, 200),
        11 => (128, 187, 219),
        18 => (204, 142, 105),
        21 => (196, 40, 28),
        23 => (13, 105, 172),
        24 => (245, 205, 48),
        26 => (27, 42, 53),
        28 => (40, 127, 71),
        29 => (161, 196, 140),
        37 => (75, 151, 75),
        38 => (160, 95, 53),
        45 => (180, 210, 228),
        101 => (218, 134, 122),
        102 => (110, 153, 202),
        104 => (107, 50, 124),
        105 => (226, 155, 64),
        106 => (218, 133, 65),
        119 => (164, 189, 71),
        124 => (146, 57, 120),
        125 => (234, 184, 146),
        192 => (105, 64, 40),
        199 => (99, 95, 98),
        208 => (229, 228, 223),
        217 => (124, 92, 70),
        226 => (253, 234, 141),
        268 => (52, 43, 117),
        301 => (80, 109, 84),
        1001 => (248, 248, 248),
        1003 => (17, 17, 17),
        1004 => (255, 0, 0),
        1012 => (33, 84, 185),
        _ => (163, 162, 165),
    };
    Color32::from_rgb(r, g, b)
}

type V3 = [f32; 3];

struct Part {
    center: V3,
    half: V3,
    color: usize,
}

const PARTS: [Part; 6] = [
    Part { center: [0.0, 4.65, 0.0], half: [0.65, 0.65, 0.65], color: 0 },
    Part { center: [0.0, 3.0, 0.0], half: [1.0, 1.0, 0.5], color: 1 },
    Part { center: [-1.5, 3.0, 0.0], half: [0.5, 1.0, 0.5], color: 2 },
    Part { center: [1.5, 3.0, 0.0], half: [0.5, 1.0, 0.5], color: 3 },
    Part { center: [-0.5, 1.0, 0.0], half: [0.5, 1.0, 0.5], color: 4 },
    Part { center: [0.5, 1.0, 0.0], half: [0.5, 1.0, 0.5], color: 5 },
];


const FACES: [(V3, [V3; 4]); 6] = [
    ([0.0, 0.0, 1.0], [[-1.0, -1.0, 1.0], [1.0, -1.0, 1.0], [1.0, 1.0, 1.0], [-1.0, 1.0, 1.0]]),
    ([0.0, 0.0, -1.0], [[1.0, -1.0, -1.0], [-1.0, -1.0, -1.0], [-1.0, 1.0, -1.0], [1.0, 1.0, -1.0]]),
    ([1.0, 0.0, 0.0], [[1.0, -1.0, 1.0], [1.0, -1.0, -1.0], [1.0, 1.0, -1.0], [1.0, 1.0, 1.0]]),
    ([-1.0, 0.0, 0.0], [[-1.0, -1.0, -1.0], [-1.0, -1.0, 1.0], [-1.0, 1.0, 1.0], [-1.0, 1.0, -1.0]]),
    ([0.0, 1.0, 0.0], [[-1.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, -1.0], [-1.0, 1.0, -1.0]]),
    ([0.0, -1.0, 0.0], [[-1.0, -1.0, -1.0], [1.0, -1.0, -1.0], [1.0, -1.0, 1.0], [-1.0, -1.0, 1.0]]),
];

pub struct Avatar3d {
    colors: [Color32; 6],
    height: f32,
    width: f32,
    yaw: f32,
    pitch: f32,
    last_drag: f64,
}

impl Avatar3d {

    pub fn classic() -> Self {
        Self::from_ids([24, 23, 24, 24, 37, 37], 1.0, 1.0)
    }

    pub fn from_ids(ids: [u32; 6], height: f32, width: f32) -> Self {
        let mut a = Self { colors: [Color32::GRAY; 6], height: 1.0, width: 1.0, yaw: 0.6, pitch: 0.12, last_drag: -10.0 };
        a.set_look(ids, height, width);
        a
    }

    pub fn set_look(&mut self, ids: [u32; 6], height: f32, width: f32) {
        for (c, id) in self.colors.iter_mut().zip(ids) {
            *c = brick_color(id);
        }
        self.height = height.clamp(0.9, 1.05);
        self.width = width.clamp(0.7, 1.0);
    }

    pub fn show(&mut self, ui: &mut Ui, size: Vec2) {
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let now = ui.input(|i| i.time);
        if resp.dragged() {
            let d = resp.drag_delta();
            self.yaw += d.x * 0.012;
            self.pitch = (self.pitch + d.y * 0.008).clamp(-0.5, 0.7);
            self.last_drag = now;
        } else if now - self.last_drag > 1.5 {
            self.yaw += ui.input(|i| i.stable_dt).min(0.05) * 0.5;
        }
        if resp.double_clicked() {
            self.yaw = 0.0;
            self.pitch = 0.12;
        }
        ui.ctx().request_repaint();

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, Rounding::same(20.0), SURF_1);

        let dist = 18.0_f32;
        let px_per_stud = size.y * 0.80 / 5.8;
        let focal = dist * px_per_stud;
        let origin = pos2(rect.center().x, rect.center().y + size.y * 0.04);
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        let rot = |p: V3| -> V3 {
            let (x, y, z) = (p[0], p[1] - 2.9, p[2]);
            let (x1, z1) = (x * cy + z * sy, -x * sy + z * cy);
            [x1, y * cp - z1 * sp, y * sp + z1 * cp]
        };
        let project = |p: V3| -> Pos2 {
            let s = focal / (dist - p[2]);
            pos2(origin.x + p[0] * s, origin.y - p[1] * s)
        };


        let g = rot([0.0, 0.0, 0.0]);
        let gs = focal / (dist - g[2]);
        painter.add(Shape::ellipse_filled(project(g), Vec2::new(1.9 * gs, 0.55 * gs * (0.35 + self.pitch.abs())), Color32::from_black_alpha(70)));

        struct Quad {
            depth: f32,
            pts: [Pos2; 4],
            fill: Color32,
            head_front: Option<[V3; 4]>,
        }
        let light = {
            let l = [-0.35_f32, 0.65, 0.68];
            let n = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt();
            [l[0] / n, l[1] / n, l[2] / n]
        };
        let mut quads: Vec<Quad> = Vec::new();
        for (pi, part) in PARTS.iter().enumerate() {
            let base = self.colors[part.color];
            for (fi, (normal, corners)) in FACES.iter().enumerate() {
                let n = rot_dir(*normal, cy, sy, cp, sp);
                let world: [V3; 4] = corners.map(|c| {
                    rot([
                        (part.center[0] + c[0] * part.half[0]) * self.width,
                        (part.center[1] + c[1] * part.half[1]) * self.height,
                        (part.center[2] + c[2] * part.half[2]) * self.width,
                    ])
                });
                let centre = [
                    world.iter().map(|v| v[0]).sum::<f32>() / 4.0,
                    world.iter().map(|v| v[1]).sum::<f32>() / 4.0,
                    world.iter().map(|v| v[2]).sum::<f32>() / 4.0,
                ];
                let to_cam = [-centre[0], -centre[1], dist - centre[2]];
                if n[0] * to_cam[0] + n[1] * to_cam[1] + n[2] * to_cam[2] <= 0.0 {
                    continue;
                }
                let lit = 0.58 + 0.42 * (n[0] * light[0] + n[1] * light[1] + n[2] * light[2]).max(0.0);
                let fill = Color32::from_rgb(
                    (base.r() as f32 * lit) as u8,
                    (base.g() as f32 * lit) as u8,
                    (base.b() as f32 * lit) as u8,
                );
                quads.push(Quad {
                    depth: centre[2],
                    pts: world.map(project),
                    fill,
                    head_front: (pi == 0 && fi == 0).then_some(world),
                });
            }
        }
        quads.sort_by(|a, b| a.depth.partial_cmp(&b.depth).unwrap_or(std::cmp::Ordering::Equal));
        for q in &quads {
            painter.add(Shape::convex_polygon(q.pts.to_vec(), q.fill, Stroke::new(1.0_f32, q.fill)));
            if let Some(w) = q.head_front {
                draw_face(&painter, &w, &project);
            }
        }
    }
}

fn rot_dir(n: V3, cy: f32, sy: f32, cp: f32, sp: f32) -> V3 {
    let (x1, z1) = (n[0] * cy + n[2] * sy, -n[0] * sy + n[2] * cy);
    [x1, n[1] * cp - z1 * sp, n[1] * sp + z1 * cp]
}


fn draw_face(painter: &egui::Painter, w: &[V3; 4], project: &impl Fn(V3) -> Pos2) {
    let at = |u: f32, v: f32| -> V3 {
        let bottom = [w[0][0] + (w[1][0] - w[0][0]) * u, w[0][1] + (w[1][1] - w[0][1]) * u, w[0][2] + (w[1][2] - w[0][2]) * u];
        let top = [w[3][0] + (w[2][0] - w[3][0]) * u, w[3][1] + (w[2][1] - w[3][1]) * u, w[3][2] + (w[2][2] - w[3][2]) * u];
        [bottom[0] + (top[0] - bottom[0]) * v, bottom[1] + (top[1] - bottom[1]) * v, bottom[2] + (top[2] - bottom[2]) * v]
    };
    let ink = Color32::from_rgb(34, 30, 28);
    for u in [0.30_f32, 0.70] {
        let quad = [(u - 0.05, 0.58), (u + 0.05, 0.58), (u + 0.05, 0.74), (u - 0.05, 0.74)];
        let pts: Vec<Pos2> = quad.iter().map(|(a, b)| project(at(*a, *b))).collect();
        painter.add(Shape::convex_polygon(pts, ink, Stroke::NONE));
    }
    let smile: Vec<Pos2> = (0..=8)
        .map(|i| {
            let t = i as f32 / 8.0;
            let u = 0.28 + 0.44 * t;
            let v = 0.38 - 0.10 * (1.0 - (2.0 * t - 1.0).powi(2));
            project(at(u, v))
        })
        .collect();
    painter.add(Shape::line(smile, Stroke::new(2.2_f32, ink)));
}
