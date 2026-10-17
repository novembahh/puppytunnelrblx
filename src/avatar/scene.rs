
use image::RgbaImage;
use std::sync::Arc;

pub type V3 = [f32; 3];

pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

pub fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}


pub struct Texture {
    pub levels: Vec<(usize, usize, Vec<[f32; 4]>)>,
}

impl Texture {
    pub fn from_image(img: &RgbaImage) -> Self {
        let (mut w, mut h) = (img.width() as usize, img.height() as usize);
        let mut base = img.clone();
        if w.max(h) > 512 {
            let s = 512.0 / w.max(h) as f32;
            let (nw, nh) = (((w as f32 * s) as u32).max(1), ((h as f32 * s) as u32).max(1));
            base = image::imageops::resize(img, nw, nh, image::imageops::FilterType::Lanczos3);
            w = nw as usize;
            h = nh as usize;
        }
        let lut: Vec<f32> = (0..256).map(|i| srgb_to_linear(i as f32 / 255.0)).collect();
        let data: Vec<[f32; 4]> = base
            .pixels()
            .map(|p| {
                let a = p[3] as f32 / 255.0;
                [lut[p[0] as usize] * a, lut[p[1] as usize] * a, lut[p[2] as usize] * a, a]
            })
            .collect();
        let mut levels = vec![(w, h, data)];
        while w > 1 || h > 1 {
            let (pw, ph, prev) = levels.last().unwrap();
            let (nw, nh) = ((*pw / 2).max(1), (*ph / 2).max(1));
            let mut next = vec![[0.0f32; 4]; nw * nh];
            for y in 0..nh {
                for x in 0..nw {
                    let mut acc = [0.0f32; 4];
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let sx = (x * 2 + dx).min(pw - 1);
                        let sy = (y * 2 + dy).min(ph - 1);
                        let p = prev[sy * pw + sx];
                        for k in 0..4 {
                            acc[k] += p[k] * 0.25;
                        }
                    }
                    next[y * nw + x] = acc;
                }
            }
            levels.push((nw, nh, next));
            w = nw;
            h = nh;
        }
        Self { levels }
    }

    pub fn base_size(&self) -> (usize, usize) {
        (self.levels[0].0, self.levels[0].1)
    }

    fn bilinear(&self, level: usize, u: f32, v: f32) -> [f32; 4] {
        let (w, h, d) = &self.levels[level];
        let x = (u * *w as f32 - 0.5).clamp(0.0, (*w - 1) as f32);
        let y = (v * *h as f32 - 0.5).clamp(0.0, (*h - 1) as f32);
        let (x0, y0) = (x.floor() as usize, y.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(*w - 1), (y0 + 1).min(*h - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let mut out = [0.0f32; 4];
        for k in 0..4 {
            let top = d[y0 * w + x0][k] * (1.0 - fx) + d[y0 * w + x1][k] * fx;
            let bot = d[y1 * w + x0][k] * (1.0 - fx) + d[y1 * w + x1][k] * fx;
            out[k] = top * (1.0 - fy) + bot * fy;
        }
        out
    }


    pub fn sample(&self, u: f32, v: f32, lod: f32) -> [f32; 4] {
        let max = (self.levels.len() - 1) as f32;
        let lod = lod.clamp(0.0, max);
        let l0 = lod.floor() as usize;
        let l1 = (l0 + 1).min(self.levels.len() - 1);
        let t = lod - l0 as f32;
        let a = self.bilinear(l0, u, 1.0 - v);
        if t < 0.001 || l0 == l1 {
            return a;
        }
        let b = self.bilinear(l1, u, 1.0 - v);
        [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, a[3] + (b[3] - a[3]) * t]
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {

    Solid,

    Decal,
}

pub struct Part {
    pub pos: Vec<V3>,
    pub nrm: Vec<V3>,
    pub uv: Vec<[f32; 2]>,
    pub tris: Vec<[u32; 3]>,
    pub texture: Option<Arc<Texture>>,

    pub color: V3,
    pub kind: Kind,
}

pub struct Avatar {
    pub parts: Vec<Part>,

    pub center: V3,
    pub height: f32,
}



pub type Regions = [[f32; 4]; 6];
pub const TEMPLATE_W: f32 = 585.0;
pub const TEMPLATE_H: f32 = 559.0;

pub const TORSO_REGIONS: Regions = [
    [231.0, 74.0, 128.0, 128.0],
    [427.0, 74.0, 128.0, 128.0],
    [165.0, 74.0, 64.0, 128.0],
    [361.0, 74.0, 64.0, 128.0],
    [231.0, 8.0, 128.0, 64.0],
    [231.0, 204.0, 128.0, 64.0],
];

pub const RIGHT_LIMB_REGIONS: Regions = [
    [217.0, 355.0, 64.0, 128.0],
    [85.0, 355.0, 64.0, 128.0],
    [151.0, 355.0, 64.0, 128.0],
    [19.0, 355.0, 64.0, 128.0],
    [217.0, 289.0, 64.0, 64.0],
    [217.0, 485.0, 64.0, 64.0],
];
pub const LEFT_LIMB_REGIONS: Regions = [
    [308.0, 355.0, 64.0, 128.0],
    [440.0, 355.0, 64.0, 128.0],
    [506.0, 355.0, 64.0, 128.0],
    [374.0, 355.0, 64.0, 128.0],
    [308.0, 289.0, 64.0, 64.0],
    [308.0, 485.0, 64.0, 64.0],
];


pub fn rounded_box(center: V3, half: V3, radius: f32, color: V3, regions: Option<&Regions>) -> Part {
    let mut p = Part { pos: vec![], nrm: vec![], uv: vec![], tris: vec![], texture: None, color, kind: Kind::Solid };
    let n = 10usize;

    let faces: [(usize, f32, usize, usize); 6] = [(2, -1.0, 0, 1), (2, 1.0, 0, 1), (0, 1.0, 2, 1), (0, -1.0, 2, 1), (1, 1.0, 0, 2), (1, -1.0, 0, 2)];
    let inner_half = [half[0] - radius, half[1] - radius, half[2] - radius];
    for (fi, (na, sign, ua, va)) in faces.iter().enumerate() {
        let base = p.pos.len() as u32;
        for j in 0..=n {
            for i in 0..=n {
                let (su, sv) = (i as f32 / n as f32 * 2.0 - 1.0, j as f32 / n as f32 * 2.0 - 1.0);
                let mut d = [0.0f32; 3];
                d[*na] = sign * half[*na];
                d[*ua] = su * half[*ua];
                d[*va] = sv * half[*va];
                let mut inner = [0.0f32; 3];
                for k in 0..3 {
                    inner[k] = d[k].clamp(-inner_half[k], inner_half[k]);
                }
                let mut nr = [d[0] - inner[0], d[1] - inner[1], d[2] - inner[2]];
                let l = (nr[0] * nr[0] + nr[1] * nr[1] + nr[2] * nr[2]).sqrt();
                if l < 1e-6 {
                    nr = [0.0; 3];
                    nr[*na] = *sign;
                } else {
                    nr = [nr[0] / l, nr[1] / l, nr[2] / l];
                }
                p.pos.push([
                    center[0] + inner[0] + nr[0] * radius,
                    center[1] + inner[1] + nr[1] * radius,
                    center[2] + inner[2] + nr[2] * radius,
                ]);
                p.nrm.push(nr);

                let uv = match regions {
                    Some(r) => {
                        let (hx, hy, hz) = (half[0] * 2.0, half[1] * 2.0, half[2] * 2.0);
                        let (u, v) = match fi {
                            0 => (0.5 - d[0] / hx, 0.5 - d[1] / hy),
                            1 => (0.5 + d[0] / hx, 0.5 - d[1] / hy),
                            2 => (0.5 - d[2] / hz, 0.5 - d[1] / hy),
                            3 => (0.5 + d[2] / hz, 0.5 - d[1] / hy),
                            4 => (0.5 - d[0] / hx, 0.5 - d[2] / hz),
                            _ => (0.5 - d[0] / hx, 0.5 + d[2] / hz),
                        };
                        let [rx, ry, rw, rh] = r[fi];
                        [(rx + 0.5 + u * (rw - 1.0)) / TEMPLATE_W, 1.0 - (ry + 0.5 + v * (rh - 1.0)) / TEMPLATE_H]
                    }
                    None => [0.0, 0.0],
                };
                p.uv.push(uv);
            }
        }
        for j in 0..n {
            for i in 0..n {
                let a = base + (j * (n + 1) + i) as u32;
                let b = a + 1;
                let c = a + (n + 1) as u32;
                p.tris.push([a, b, c + 1]);
                p.tris.push([a, c + 1, c]);
            }
        }
    }
    p
}
