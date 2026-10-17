

use super::scene::{linear_to_srgb, Avatar, Kind, Part, V3};

struct Vtx {
    x: f32,
    y: f32,
    iw: f32,

    u: f32,
    v: f32,
    n: V3,
}

struct Buffers {
    w: usize,
    h: usize,

    color: Vec<[f32; 4]>,

    depth: Vec<f32>,
}

#[derive(Clone, Copy)]
struct Cam {
    cx: f32,
    cy: f32,
    focal: f32,
    dist: f32,
    cy_rot: f32,
    sy_rot: f32,
    cp: f32,
    sp: f32,
}

impl Cam {
    fn rotate(&self, p: V3) -> V3 {
        let (x1, z1) = (p[0] * self.cy_rot + p[2] * self.sy_rot, -p[0] * self.sy_rot + p[2] * self.cy_rot);
        [x1, p[1] * self.cp - z1 * self.sp, p[1] * self.sp + z1 * self.cp]
    }
}

fn normalize(v: V3) -> V3 {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-8);
    [v[0] / l, v[1] / l, v[2] / l]
}

fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn shade(albedo: V3, n: V3, to_cam: V3) -> V3 {
    let key = normalize([0.55, 0.62, -0.62]);
    let fill = normalize([-0.7, 0.1, -0.55]);
    let ndl = (dot(n, key) + 0.25) / 1.25;
    let diffuse = ndl.clamp(0.0, 1.0);
    let fill_d = ((dot(n, fill) + 0.5) / 1.5).clamp(0.0, 1.0);
    let hemi = n[1] * 0.5 + 0.5;
    let sky = [0.50, 0.55, 0.64];
    let ground = [0.22, 0.20, 0.20];
    let rim = (1.0 - dot(n, to_cam).clamp(0.0, 1.0)).powi(3) * 0.10;
    let mut out = [0.0f32; 3];
    for k in 0..3 {
        let ambient = ground[k] + (sky[k] - ground[k]) * hemi;
        let key_c = [1.0, 0.97, 0.93][k];
        let lit = ambient * 0.62 + key_c * diffuse * 0.78 + 0.16 * fill_d * [0.8, 0.88, 1.0][k];
        out[k] = albedo[k] * lit + rim * [0.8, 0.9, 1.0][k];
    }
    out
}

fn soft_clip(c: f32) -> f32 {
    if c < 0.8 { c } else { 0.8 + 0.2 * ((c - 0.8) / 0.2).tanh() }
}

fn raster(part: &Part, cam: &Cam, b: &mut Buffers) {
    let verts: Vec<Vtx> = part
        .pos
        .iter()
        .zip(&part.nrm)
        .zip(&part.uv)
        .map(|((p, n), uv)| {
            let q = cam.rotate(*p);
            let d = q[2] + cam.dist;
            let iw = 1.0 / d.max(0.05);
            let nr = cam.rotate(*n);
            Vtx { x: cam.cx - cam.focal * q[0] * iw, y: cam.cy - cam.focal * q[1] * iw, iw, u: uv[0] * iw, v: uv[1] * iw, n: [nr[0] * iw, nr[1] * iw, nr[2] * iw] }
        })
        .collect();

    let tex_size = part.texture.as_ref().map(|t| t.base_size());
    let decal = part.kind == Kind::Decal;
    let bias = if decal { 1.004 } else { 1.0 };

    for t in &part.tris {
        let (a, bv, c) = (&verts[t[0] as usize], &verts[t[1] as usize], &verts[t[2] as usize]);
        let area = (c.x - bv.x) * (a.y - bv.y) - (c.y - bv.y) * (a.x - bv.x);
        if area.abs() < 1e-5 {
            continue;
        }
        let min_x = a.x.min(bv.x).min(c.x).floor().max(0.0) as usize;
        let max_x = (a.x.max(bv.x).max(c.x).ceil().min(b.w as f32 - 1.0)).max(0.0) as usize;
        let min_y = a.y.min(bv.y).min(c.y).floor().max(0.0) as usize;
        let max_y = (a.y.max(bv.y).max(c.y).ceil().min(b.h as f32 - 1.0)).max(0.0) as usize;
        if a.x.max(bv.x).max(c.x) < 0.0 || a.y.max(bv.y).max(c.y) < 0.0 || a.x.min(bv.x).min(c.x) > b.w as f32 || a.y.min(bv.y).min(c.y) > b.h as f32 {
            continue;
        }


        let lod = match tex_size {
            Some((tw, th)) => {
                let (u0, v0) = (a.u / a.iw, a.v / a.iw);
                let (u1, v1) = (bv.u / bv.iw, bv.v / bv.iw);
                let (u2, v2) = (c.u / c.iw, c.v / c.iw);
                let texels = (((u1 - u0) * (v2 - v0) - (u2 - u0) * (v1 - v0)).abs() * 0.5) * (tw * th) as f32;
                let pixels = area.abs() * 0.5;
                if texels > 0.0 { (0.5 * (texels / pixels).max(1e-6).log2()).max(0.0) } else { 0.0 }
            }
            None => 0.0,
        };
        let inv_area = 1.0 / area;

        for py in min_y..=max_y {
            let sy = py as f32 + 0.5;
            for px in min_x..=max_x {
                let sx = px as f32 + 0.5;
                let w0 = ((c.x - bv.x) * (sy - bv.y) - (c.y - bv.y) * (sx - bv.x)) * inv_area;
                let w1 = ((a.x - c.x) * (sy - c.y) - (a.y - c.y) * (sx - c.x)) * inv_area;
                let w2 = 1.0 - w0 - w1;
                const E: f32 = -1e-4;
                if w0 < E || w1 < E || w2 < E {
                    continue;
                }
                let iw = w0 * a.iw + w1 * bv.iw + w2 * c.iw;
                let idx = py * b.w + px;
                if iw * bias < b.depth[idx] {
                    continue;
                }
                let inv = 1.0 / iw;
                let u = (w0 * a.u + w1 * bv.u + w2 * c.u) * inv;
                let v = (w0 * a.v + w1 * bv.v + w2 * c.v) * inv;
                let n = normalize([
                    (w0 * a.n[0] + w1 * bv.n[0] + w2 * c.n[0]) * inv,
                    (w0 * a.n[1] + w1 * bv.n[1] + w2 * c.n[1]) * inv,
                    (w0 * a.n[2] + w1 * bv.n[2] + w2 * c.n[2]) * inv,
                ]);

                let (albedo, alpha) = match &part.texture {
                    Some(tex) => {
                        let s = tex.sample(u, v, lod);
                        let al = s[3];
                        if al < 0.004 {
                            continue;
                        }
                        let rgb = [s[0] / al, s[1] / al, s[2] / al];
                        (rgb, al)
                    }
                    None => (part.color, 1.0),
                };
                if !decal && alpha < 0.4 {
                    continue;
                }

                let d = 1.0 / iw;
                let pos_view = [(cam.cx - sx) / cam.focal * d, (cam.cy - sy) / cam.focal * d, d - cam.dist];
                let to_cam = normalize([-pos_view[0], -pos_view[1], -cam.dist - pos_view[2]]);
                let nn = if dot(n, to_cam) < 0.0 { [-n[0], -n[1], -n[2]] } else { n };
                let lit = shade(albedo, nn, to_cam);
                let rgb = [soft_clip(lit[0]), soft_clip(lit[1]), soft_clip(lit[2])];

                if decal {
                    let dst = &mut b.color[idx];
                    for k in 0..3 {
                        dst[k] = rgb[k] * alpha + dst[k] * (1.0 - alpha);
                    }
                    dst[3] = alpha + dst[3] * (1.0 - alpha);
                } else {
                    b.color[idx] = [rgb[0], rgb[1], rgb[2], 1.0];
                    b.depth[idx] = iw;
                }
            }
        }
    }
}

fn ground_shadow(cam: &Cam, center_y: f32, b: &mut Buffers) {
    let (cp, sp) = (cam.cp, cam.sp);
    for py in 0..b.h {
        let yn = (cam.cy - (py as f32 + 0.5)) / cam.focal;
        let denom = yn * cp + sp;
        if denom.abs() < 1e-4 {
            continue;
        }
        let d = (cam.dist * sp - center_y) / denom;
        if d <= 0.1 {
            continue;
        }
        for px in 0..b.w {
            let xn = (cam.cx - (px as f32 + 0.5)) / cam.focal;
            let q = [d * xn, d * yn, d - cam.dist];

            let z1 = -q[1] * sp + q[2] * cp;
            let r2 = q[0] * q[0] + z1 * z1;
            let a = 0.42 * (-(r2) / 1.35).exp();
            if a > 0.004 {
                b.color[py * b.w + px] = [0.0, 0.0, 0.0, a];
            }
        }
    }
}


pub fn render(avatar: &Avatar, w: usize, h: usize, yaw: f32, pitch: f32) -> Vec<u8> {
    let ss = if w * h * 9 <= 2_000_000 { 3 } else { 2 };
    let (bw, bh) = (w * ss, h * ss);
    let mut b = Buffers { w: bw, h: bh, color: vec![[0.0; 4]; bw * bh], depth: vec![0.0; bw * bh] };

    let dist = avatar.height * 3.4;
    let ppu = 0.80 * bh as f32 / avatar.height;
    let (sy, cy) = yaw.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    let cam = Cam { cx: bw as f32 * 0.5, cy: bh as f32 * 0.5, focal: ppu * dist, dist, cy_rot: cy, sy_rot: sy, cp, sp };

    ground_shadow(&cam, avatar.center[1], &mut b);

    let solid = avatar.parts.iter().filter(|p| p.kind == Kind::Solid);
    let decals = avatar.parts.iter().filter(|p| p.kind == Kind::Decal);
    let recentered = |p: &Part| -> Part {
        Part {
            pos: p.pos.iter().map(|v| [v[0] - avatar.center[0], v[1] - avatar.center[1], v[2] - avatar.center[2]]).collect(),
            nrm: p.nrm.clone(),
            uv: p.uv.clone(),
            tris: p.tris.clone(),
            texture: p.texture.clone(),
            color: p.color,
            kind: p.kind,
        }
    };
    for p in solid {
        raster(&recentered(p), &cam, &mut b);
    }
    for p in decals {
        raster(&recentered(p), &cam, &mut b);
    }

    let mut out = vec![0u8; w * h * 4];
    let inv = 1.0 / (ss * ss) as f32;
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0.0f32; 4];
            for sy in 0..ss {
                for sx in 0..ss {
                    let p = b.color[(y * ss + sy) * bw + x * ss + sx];
                    for k in 0..4 {
                        acc[k] += p[k];
                    }
                }
            }
            let a = acc[3] * inv;
            let o = (y * w + x) * 4;
            if a > 0.0005 {
                for k in 0..3 {
                    let lin = (acc[k] * inv / a).min(1.0);
                    out[o + k] = (linear_to_srgb(lin) * a * 255.0 + 0.5) as u8;
                }
                out[o + 3] = (a.min(1.0) * 255.0 + 0.5) as u8;
            }
        }
    }
    out
}
