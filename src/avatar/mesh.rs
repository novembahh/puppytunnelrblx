
#[derive(Debug, Default, Clone)]
pub struct Mesh {
    pub pos: Vec<[f32; 3]>,
    pub nrm: Vec<[f32; 3]>,
    pub uv: Vec<[f32; 2]>,
    pub tris: Vec<[u32; 3]>,
}

impl Mesh {
    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for p in &self.pos {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        (lo, hi)
    }
}

fn rd_u16(d: &[u8], o: usize) -> Result<u16, String> {
    d.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]])).ok_or_else(|| "mesh truncated".to_string())
}
fn rd_u32(d: &[u8], o: usize) -> Result<u32, String> {
    d.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok_or_else(|| "mesh truncated".to_string())
}
fn rd_f32(d: &[u8], o: usize) -> Result<f32, String> {
    rd_u32(d, o).map(f32::from_bits)
}

pub fn parse(data: &[u8]) -> Result<Mesh, String> {
    if !data.starts_with(b"version ") {
        return Err("not a Roblox mesh".to_string());
    }
    let nl = data.iter().position(|&b| b == b'\n').ok_or("bad mesh header")?;
    let version = String::from_utf8_lossy(&data[8..nl]).trim().to_string();
    let body = &data[nl + 1..];
    match version.split('.').next().unwrap_or("") {
        "1" => parse_v1(body, version == "1.00"),
        "2" => parse_binary(body, 2),
        "3" => parse_binary(body, 3),
        "4" => parse_binary(body, 4),
        v => Err(format!("unsupported mesh version {}", v)),
    }
}

fn parse_v1(body: &[u8], halve: bool) -> Result<Mesh, String> {
    let text = String::from_utf8_lossy(body);
    let mut lines = text.lines();
    let faces: usize = lines.next().ok_or("empty mesh")?.trim().parse().map_err(|_| "bad face count")?;
    let rest: String = lines.collect::<Vec<_>>().join("");
    let nums: Vec<f32> = rest
        .split(|c| c == '[' || c == ']' || c == ',')
        .filter(|s| !s.trim().is_empty())
        .filter_map(|s| s.trim().parse().ok())
        .collect();

    if nums.len() < faces * 3 * 9 {
        return Err("mesh data too short".to_string());
    }
    let s = if halve { 0.5 } else { 1.0 };
    let mut m = Mesh::default();
    for i in 0..faces * 3 {
        let v = &nums[i * 9..i * 9 + 9];
        m.pos.push([v[0] * s, v[1] * s, v[2] * s]);
        m.nrm.push([v[3], v[4], v[5]]);
        m.uv.push([v[6], v[7]]);
    }
    for f in 0..faces as u32 {
        m.tris.push([f * 3, f * 3 + 1, f * 3 + 2]);
    }
    Ok(m)
}

fn parse_binary(b: &[u8], major: u32) -> Result<Mesh, String> {
    let (header_len, vert_size, num_verts, num_faces, lod_count) = match major {
        2 => {
            let hl = rd_u16(b, 0)? as usize;
            (hl, *b.get(2).ok_or("truncated")? as usize, rd_u32(b, 4)? as usize, rd_u32(b, 8)? as usize, 0usize)
        }
        3 => {
            let hl = rd_u16(b, 0)? as usize;
            (hl, *b.get(2).ok_or("truncated")? as usize, rd_u32(b, 8)? as usize, rd_u32(b, 12)? as usize, rd_u16(b, 6)? as usize)
        }
        _ => {
            let hl = rd_u16(b, 0)? as usize;
            (hl, 40usize, rd_u32(b, 4)? as usize, rd_u32(b, 8)? as usize, rd_u16(b, 12)? as usize)
        }
    };
    let num_bones = if major >= 4 { rd_u16(b, 14)? as usize } else { 0 };
    if vert_size < 32 {
        return Err("unexpected vertex size".to_string());
    }
    let mut o = header_len;
    let mut m = Mesh::default();
    for i in 0..num_verts {
        let v = o + i * vert_size;
        m.pos.push([rd_f32(b, v)?, rd_f32(b, v + 4)?, rd_f32(b, v + 8)?]);
        m.nrm.push([rd_f32(b, v + 12)?, rd_f32(b, v + 16)?, rd_f32(b, v + 20)?]);
        m.uv.push([rd_f32(b, v + 24)?, rd_f32(b, v + 28)?]);
    }
    o += num_verts * vert_size;
    if num_bones > 0 {
        o += num_verts * 8;
    }
    let mut faces = Vec::with_capacity(num_faces);
    for i in 0..num_faces {
        let f = o + i * 12;
        faces.push([rd_u32(b, f)?, rd_u32(b, f + 4)?, rd_u32(b, f + 8)?]);
    }
    o += num_faces * 12;


    let mut keep = faces.len();
    let mut start = 0usize;
    if lod_count >= 2 && major >= 3 {
        let a = rd_u32(b, o)? as usize;
        let c = rd_u32(b, o + 4)? as usize;
        if c >= a && c <= faces.len() {
            start = a;
            keep = c;
        }
    }
    m.tris = faces[start.min(keep)..keep].iter().copied().filter(|t| t.iter().all(|&i| (i as usize) < num_verts)).collect();
    Ok(m)
}
