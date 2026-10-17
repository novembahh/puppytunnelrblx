


use super::assets::{asset_id, download};
use super::mesh;
use super::rbxm::{self, CFrame, Value};
use super::scene::{
    rounded_box, srgb_to_linear, Avatar, Kind, Part, Regions, Texture, LEFT_LIMB_REGIONS, RIGHT_LIMB_REGIONS, TORSO_REGIONS, V3,
};
use image::RgbaImage;
use std::sync::Arc;
use std::time::Duration;

const HEAD_CENTER: V3 = [0.0, 4.5, 0.0];
const TORSO_CENTER: V3 = [0.0, 3.0, 0.0];
const DEFAULT_FACE: u64 = 144075659;
const DEFAULT_SHIRT: u64 = 855766176;
const DEFAULT_PANTS: u64 = 855782781;
const BEVEL: f32 = 0.05;


const LIMBS: [(V3, V3, &Regions, usize); 5] = [
    ([0.0, 3.0, 0.0], [1.0, 1.0, 0.5], &TORSO_REGIONS, 1),
    ([1.5, 3.0, 0.0], [0.5, 1.0, 0.5], &RIGHT_LIMB_REGIONS, 2),
    ([-1.5, 3.0, 0.0], [0.5, 1.0, 0.5], &LEFT_LIMB_REGIONS, 3),
    ([0.5, 1.0, 0.0], [0.5, 1.0, 0.5], &RIGHT_LIMB_REGIONS, 4),
    ([-0.5, 1.0, 0.0], [0.5, 1.0, 0.5], &LEFT_LIMB_REGIONS, 5),
];

fn clothing_template(id: u64) -> Option<Arc<Texture>> {
    let m = rbxm::parse(&download(id).ok()?).ok()?;
    for inst in &m.instances {
        for key in ["ShirtTemplate", "PantsTemplate", "Graphic"] {
            if let Some(Value::Str(s)) = inst.props.get(key) {
                let img = resolve_image(asset_id(s)?, 0)?;
                return Some(Arc::new(Texture::from_image(&img)));
            }
        }
    }
    None
}


fn tshirt_part(tex: Arc<Texture>) -> Part {
    let (cols, rows) = (12usize, 12usize);
    let (w, h, cy) = (1.5f32, 1.5f32, 3.1f32);
    let mut pos = Vec::new();
    let mut uv = Vec::new();
    for r in 0..=rows {
        for c in 0..=cols {
            let (tu, tv) = (c as f32 / cols as f32, r as f32 / rows as f32);
            pos.push([(0.5 - tu) * w, cy + (0.5 - tv) * h, -0.5 - 0.03]);
            uv.push([tu, 1.0 - tv]);
        }
    }
    let mut tris = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            let a = (r * (cols + 1) + c) as u32;
            let d = a + (cols + 1) as u32;
            tris.push([a, a + 1, d + 1]);
            tris.push([a, d + 1, d]);
        }
    }
    let nrm = vec![[0.0, 0.0, -1.0]; pos.len()];
    Part { pos, nrm, uv, tris, texture: Some(tex), color: [1.0; 3], kind: Kind::Decal }
}

fn brick_color(id: u64) -> V3 {
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
        194 => (163, 162, 165),
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
        1017 => (255, 175, 0),
        _ => (163, 162, 165),
    };
    [srgb_to_linear(r as f32 / 255.0), srgb_to_linear(g as f32 / 255.0), srgb_to_linear(b as f32 / 255.0)]
}

fn smooth_normals(pos: &[V3], tris: &[[u32; 3]]) -> Vec<V3> {
    let mut n = vec![[0.0f32; 3]; pos.len()];
    for t in tris {
        let (a, b, c) = (pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]);
        let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
        let f = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        for &i in t {
            for k in 0..3 {
                n[i as usize][k] += f[k];
            }
        }
    }
    for v in n.iter_mut() {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if l > 1e-8 {
            *v = [v[0] / l, v[1] / l, v[2] / l];
        } else {
            *v = [0.0, 1.0, 0.0];
        }
    }
    n
}

fn sgn_pow(x: f32, e: f32) -> f32 {
    x.signum() * x.abs().powf(e)
}

const HEAD_A: f32 = 0.62;
const HEAD_H: f32 = 0.62;
const HEAD_N: f32 = 3.0;

fn head_ring_scale(v: f32) -> f32 {
    (1.0 - v.abs().powf(3.2)).max(0.0).powf(1.0 / 3.2)
}

fn head_part(color: V3) -> Part {
    let (rings, segs) = (24usize, 56usize);
    let mut pos = Vec::new();
    for r in 0..=rings {
        let v = -1.0 + 2.0 * r as f32 / rings as f32;
        let s = head_ring_scale(v);
        for k in 0..segs {
            let th = k as f32 / segs as f32 * std::f32::consts::TAU;
            let (sn, cs) = th.sin_cos();
            pos.push([
                HEAD_CENTER[0] + HEAD_A * s * sgn_pow(cs, 2.0 / HEAD_N),
                HEAD_CENTER[1] + v * HEAD_H,
                HEAD_CENTER[2] + HEAD_A * s * sgn_pow(sn, 2.0 / HEAD_N),
            ]);
        }
    }
    let mut tris = Vec::new();
    for r in 0..rings {
        for k in 0..segs {
            let (a, b) = ((r * segs + k) as u32, (r * segs + (k + 1) % segs) as u32);
            let (c, d) = (((r + 1) * segs + k) as u32, ((r + 1) * segs + (k + 1) % segs) as u32);
            tris.push([a, b, d]);
            tris.push([a, d, c]);
        }
    }
    let nrm = smooth_normals(&pos, &tris);
    let uv = vec![[0.0, 0.0]; pos.len()];
    Part { pos, nrm, uv, tris, texture: None, color, kind: Kind::Solid }
}


fn face_part(texture: Arc<Texture>) -> Part {
    let (cols, rows) = (32usize, 16usize);
    let mut pos = Vec::new();
    let mut uv = Vec::new();
    for r in 0..=rows {
        let tv = r as f32 / rows as f32;
        let y = 0.52 - tv * 1.04;
        let s = head_ring_scale(y / HEAD_H);
        let radius = HEAD_A * s;
        for c in 0..=cols {
            let tu = c as f32 / cols as f32;
            let x = (tu - 0.5) * 1.08;
            let xr = (x.abs() / radius.max(1e-3)).min(1.0);
            let z = radius * (1.0 - xr.powf(HEAD_N)).max(0.0).powf(1.0 / HEAD_N);
            pos.push([HEAD_CENTER[0] + x, HEAD_CENTER[1] + y, HEAD_CENTER[2] - z - 0.006]);
            uv.push([tu, 1.0 - tv]);
        }
    }
    let mut tris = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            let a = (r * (cols + 1) + c) as u32;
            let b = a + 1;
            let d = a + (cols + 1) as u32;
            tris.push([a, b, d + 1]);
            tris.push([a, d + 1, d]);
        }
    }
    let nrm = smooth_normals(&pos, &tris);
    Part { pos, nrm, uv, tris, texture: Some(texture), color: [1.0; 3], kind: Kind::Decal }
}

fn resolve_image(id: u64, depth: u8) -> Option<RgbaImage> {
    let bytes = download(id).ok()?;
    if let Ok(img) = image::load_from_memory(&bytes) {
        return Some(img.to_rgba8());
    }
    if depth >= 3 {
        return None;
    }
    let m = rbxm::parse(&bytes).ok()?;
    for inst in &m.instances {
        for key in ["Texture", "TextureId", "TextureID", "ColorMap"] {
            if let Some(Value::Str(s)) = inst.props.get(key) {
                if let Some(next) = asset_id(s) {
                    if next != id {
                        return resolve_image(next, depth + 1);
                    }
                }
            }
        }
    }
    None
}

fn head_attachment(name: &str) -> Option<CFrame> {
    Some(match name {
        "HatAttachment" | "HairAttachment" => CFrame::at([0.0, 0.6, 0.0]),
        "FaceFrontAttachment" => CFrame::at([0.0, 0.0, -0.6]),
        "FaceCenterAttachment" => CFrame::IDENTITY,
        _ => return None,
    })
}

fn torso_attachment(name: &str) -> Option<CFrame> {
    let p = match name {
        "NeckAttachment" => [0.0, 1.0, 0.0],
        "BodyFrontAttachment" => [0.0, 0.0, -0.5],
        "BodyBackAttachment" => [0.0, 0.0, 0.5],
        "LeftCollarAttachment" => [-1.0, 1.0, 0.0],
        "RightCollarAttachment" => [1.0, 1.0, 0.0],
        "LeftShoulderAttachment" => [-1.5, 1.0, 0.0],
        "RightShoulderAttachment" => [1.5, 1.0, 0.0],
        "WaistFrontAttachment" => [0.0, -1.0, -0.5],
        "WaistBackAttachment" => [0.0, -1.0, 0.5],
        "WaistCenterAttachment" => [0.0, -1.0, 0.0],
        _ => return None,
    };
    Some(CFrame::at(p))
}

fn accessory_part(id: u64) -> Result<Part, String> {
    let bytes = download(id)?;
    let model = rbxm::parse(&bytes)?;
    let acc = model.first_of_class("Accessory").ok_or("no accessory in asset")?;
    let handle = model
        .instances[acc]
        .children
        .iter()
        .copied()
        .find(|&c| matches!(model.instances[c].class.as_str(), "Part" | "MeshPart") && model.str_prop(c, "Name") == Some("Handle"))
        .ok_or("accessory has no handle")?;


    let mut placement = None;
    for att in model.children_of_class(handle, "Attachment") {
        let name = model.str_prop(att, "Name").unwrap_or_default().to_string();
        let local = model.cframe_prop(att, "CFrame").unwrap_or(CFrame::IDENTITY);
        if let Some(on_head) = head_attachment(&name) {
            placement = Some(CFrame::at(HEAD_CENTER).mul(&on_head).mul(&local.inverse()));
            break;
        }
        if let Some(on_torso) = torso_attachment(&name) {
            placement = Some(CFrame::at(TORSO_CENTER).mul(&on_torso).mul(&local.inverse()));
            break;
        }
    }
    let world = placement.unwrap_or_else(|| {
        let ap = model.cframe_prop(acc, "AttachmentPoint").unwrap_or(CFrame::IDENTITY);
        CFrame::at([HEAD_CENTER[0], HEAD_CENTER[1] + 0.5, HEAD_CENTER[2]]).mul(&ap.inverse())
    });


    let (mesh_url, tex_url, scale, offset, size) = if model.instances[handle].class == "MeshPart" {
        (
            model.str_prop(handle, "MeshId").map(str::to_string),
            model.str_prop(handle, "TextureID").map(str::to_string),
            [1.0; 3],
            [0.0; 3],
            model.vec3_prop(handle, "Size"),
        )
    } else {
        let sm = model.children_of_class(handle, "SpecialMesh").next().ok_or("handle has no mesh")?;
        (
            model.str_prop(sm, "MeshId").map(str::to_string),
            model.str_prop(sm, "TextureId").map(str::to_string),
            model.vec3_prop(sm, "Scale").unwrap_or([1.0; 3]),
            model.vec3_prop(sm, "Offset").unwrap_or([0.0; 3]),
            None,
        )
    };
    let mesh_id = mesh_url.as_deref().and_then(asset_id).ok_or("handle has no mesh id")?;
    let m = mesh::parse(&download(mesh_id)?)?;

    let part_scale = match size {
        Some(sz) => {
            let (lo, hi) = m.bounds();
            let ext = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
            [0, 1, 2].map(|k| if ext[k] > 1e-6 { sz[k] / ext[k] } else { 1.0 })
        }
        None => scale,
    };
    let pos: Vec<V3> = m
        .pos
        .iter()
        .map(|v| world.apply([v[0] * part_scale[0] + offset[0], v[1] * part_scale[1] + offset[1], v[2] * part_scale[2] + offset[2]]))
        .collect();
    let nrm: Vec<V3> = if m.nrm.iter().any(|n| n.iter().any(|c| *c != 0.0)) {
        m.nrm
            .iter()
            .map(|n| {
                let r = &world.r;
                let v = [
                    r[0][0] * n[0] + r[0][1] * n[1] + r[0][2] * n[2],
                    r[1][0] * n[0] + r[1][1] * n[1] + r[1][2] * n[2],
                    r[2][0] * n[0] + r[2][1] * n[1] + r[2][2] * n[2],
                ];
                let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-8);
                [v[0] / l, v[1] / l, v[2] / l]
            })
            .collect()
    } else {
        smooth_normals(&pos, &m.tris)
    };

    let texture = tex_url
        .as_deref()
        .and_then(asset_id)
        .and_then(|t| resolve_image(t, 0))
        .map(|img| Arc::new(Texture::from_image(&img)));
    let color = model
        .color_prop(handle, "Color3uint8")
        .map(|c| [srgb_to_linear(c[0]), srgb_to_linear(c[1]), srgb_to_linear(c[2])])
        .unwrap_or([0.63, 0.63, 0.63]);
    Ok(Part { pos, nrm, uv: m.uv, tris: m.tris, texture, color, kind: Kind::Solid })
}

pub fn build(user_id: u64, progress: &dyn Fn(&str)) -> Result<Avatar, String> {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(10)).build();
    progress("Reading avatar");
    let v: serde_json::Value = agent
        .get(&format!("https://avatar.roblox.com/v1/users/{}/avatar", user_id))
        .call()
        .map_err(|e| format!("could not reach Roblox: {}", e))?
        .into_json()
        .map_err(|e| e.to_string())?;

    let get = |k: &str| v["bodyColors"][k].as_u64().unwrap_or(194);
    let colors = [
        get("headColorId"),
        get("torsoColorId"),
        get("rightArmColorId"),
        get("leftArmColorId"),
        get("rightLegColorId"),
        get("leftLegColorId"),
    ]
    .map(brick_color);
    let height_scale = v["scales"]["height"].as_f64().unwrap_or(1.0) as f32;
    let width_scale = v["scales"]["width"].as_f64().unwrap_or(1.0) as f32;

    let mut accessory_ids = Vec::new();
    let mut face_id = DEFAULT_FACE;
    let (mut shirt_id, mut pants_id, mut tshirt_id) = (None, None, None);
    if let Some(assets) = v["assets"].as_array() {
        for a in assets {
            let (id, ty) = (a["id"].as_u64().unwrap_or(0), a["assetType"]["id"].as_u64().unwrap_or(0));
            match ty {
                8 | 41..=47 => accessory_ids.push(id),
                18 => face_id = id,
                11 => shirt_id = Some(id),
                12 => pants_id = Some(id),
                2 => tshirt_id = Some(id),
                _ => {}
            }
        }
    }

    if shirt_id.is_none() && v["defaultShirtApplied"].as_bool().unwrap_or(false) {
        shirt_id = Some(DEFAULT_SHIRT);
    }
    if pants_id.is_none() && v["defaultPantsApplied"].as_bool().unwrap_or(false) {
        pants_id = Some(DEFAULT_PANTS);
    }

    progress("Downloading avatar assets");
    let (face_img, accessories, shirt, pants, tshirt) = std::thread::scope(|s| {
        let shirt = s.spawn(move || shirt_id.and_then(clothing_template));
        let pants = s.spawn(move || pants_id.and_then(clothing_template));
        let tshirt = s.spawn(move || tshirt_id.and_then(clothing_template));
        let face = s.spawn(move || {
            let bytes = download(face_id).ok()?;
            let m = rbxm::parse(&bytes).ok()?;
            let decal = m.first_of_class("Decal")?;
            let tid = asset_id(m.str_prop(decal, "Texture")?)?;
            resolve_image(tid, 0)
        });
        let handles: Vec<_> = accessory_ids.iter().map(|&id| s.spawn(move || accessory_part(id))).collect();
        let parts: Vec<Result<Part, String>> = handles.into_iter().map(|h| h.join().unwrap_or_else(|_| Err("worker panicked".into()))).collect();
        (
            face.join().ok().flatten(),
            parts,
            shirt.join().ok().flatten(),
            pants.join().ok().flatten(),
            tshirt.join().ok().flatten(),
        )
    });

    let mut parts = vec![head_part(colors[0])];
    for (center, half, _, ci) in LIMBS.iter() {
        parts.push(rounded_box(*center, *half, BEVEL, colors[*ci], None));
    }
    if let Some(img) = face_img {
        parts.push(face_part(Arc::new(Texture::from_image(&img))));
    }

    for (tex, grow) in [(pants, 0.012f32), (shirt, 0.022)] {
        if let Some(tex) = tex {
            for (center, half, regions, _) in LIMBS.iter() {
                let g = [half[0] + grow, half[1] + grow, half[2] + grow];
                let mut layer = rounded_box(*center, g, BEVEL + grow, [1.0; 3], Some(regions));
                layer.texture = Some(Arc::clone(&tex));
                layer.kind = Kind::Decal;
                parts.push(layer);
            }
        }
    }
    if let Some(tex) = tshirt {
        parts.push(tshirt_part(tex));
    }
    for a in accessories {
        match a {
            Ok(p) => parts.push(p),
            Err(e) => progress(&format!("skipped an item: {}", e)),
        }
    }

    for p in parts.iter_mut() {
        for q in p.pos.iter_mut() {
            q[0] *= width_scale;
            q[1] *= height_scale;
            q[2] *= width_scale;
        }
    }
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for p in &parts {
        for q in &p.pos {
            lo = lo.min(q[1]);
            hi = hi.max(q[1]);
        }
    }
    lo = lo.min(0.0);
    Ok(Avatar { parts, center: [0.0, (lo + hi) * 0.5, 0.0], height: hi - lo + 0.5 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore]
    fn render_user() {
        let id: u64 = std::env::var("USER_ID").ok().and_then(|s| s.parse().ok()).unwrap_or(156);
        let out = std::env::var("OUT_DIR_PNG").unwrap_or_else(|_| ".".to_string());
        let avatar = build(id, &|m| eprintln!("{}", m)).unwrap();
        eprintln!("{} parts, height {}", avatar.parts.len(), avatar.height);
        for (i, yaw) in [0.0f32, 0.9, 2.2, 3.6].iter().enumerate() {
            let (w, h) = (380, 540);
            let t0 = std::time::Instant::now();
            let px = crate::avatar::render::render(&avatar, w, h, *yaw, 0.1);
            eprintln!("frame {} in {:?}", i, t0.elapsed());
            let img = image::RgbaImage::from_raw(w as u32, h as u32, px).unwrap();

            let mut bg = image::RgbaImage::from_pixel(w as u32, h as u32, image::Rgba([37, 33, 32, 255]));
            for (x, y, p) in img.enumerate_pixels() {
                let a = p[3] as f32 / 255.0;
                let b = bg.get_pixel_mut(x, y);
                for k in 0..3 {
                    b[k] = (p[k] as f32 + b[k] as f32 * (1.0 - a)).min(255.0) as u8;
                }
            }
            bg.save(format!("{}/avatar_{}_{}.png", out, id, i)).unwrap();
        }
    }
}

#[cfg(test)]
mod template_tests {
    use super::*;

    #[test]
    #[ignore]
    fn dump_clothing() {
        let out = std::env::var("OUT_DIR_PNG").unwrap_or_else(|_| ".".to_string());
        for id in [855766176u64, 855782781u64, 1142116u64] {
            let bytes = download(id).unwrap();
            let m = rbxm::parse(&bytes).unwrap();
            for inst in &m.instances {
                eprintln!("{} {} {:?}", id, inst.class, inst.props.keys().collect::<Vec<_>>());
            }
            for key in ["ShirtTemplate", "PantsTemplate", "Graphic"] {
                for inst in &m.instances {
                    if let Some(Value::Str(s)) = inst.props.get(key) {
                        let tid = asset_id(s).unwrap();
                        if let Some(img) = resolve_image(tid, 0) {
                            eprintln!("{} -> image {}x{}", key, img.width(), img.height());
                            img.save(format!("{}/{}_{}.png", out, key, id)).unwrap();
                        }
                    }
                }
            }
        }
    }
}
