

use std::collections::HashMap;

#[derive(Clone, Debug)]
pub enum Value {
    Str(String),
    Bool(bool),
    Int(i32),
    Float(f32),
    Vec3([f32; 3]),
    Color3([f32; 3]),
    CFrame(CFrame),
    Enum(u32),
}

#[derive(Clone, Copy, Debug)]
pub struct CFrame {

    pub r: [[f32; 3]; 3],
    pub p: [f32; 3],
}

impl CFrame {
    pub const IDENTITY: CFrame = CFrame { r: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], p: [0.0; 3] };

    pub fn at(p: [f32; 3]) -> Self {
        Self { r: Self::IDENTITY.r, p }
    }

    pub fn apply(&self, v: [f32; 3]) -> [f32; 3] {
        let r = &self.r;
        [
            r[0][0] * v[0] + r[0][1] * v[1] + r[0][2] * v[2] + self.p[0],
            r[1][0] * v[0] + r[1][1] * v[1] + r[1][2] * v[2] + self.p[1],
            r[2][0] * v[0] + r[2][1] * v[1] + r[2][2] * v[2] + self.p[2],
        ]
    }

    pub fn mul(&self, o: &CFrame) -> CFrame {
        let mut r = [[0.0f32; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                r[i][j] = (0..3).map(|k| self.r[i][k] * o.r[k][j]).sum();
            }
        }
        CFrame { r, p: self.apply(o.p) }
    }

    pub fn inverse(&self) -> CFrame {
        let mut t = [[0.0f32; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                t[i][j] = self.r[j][i];
            }
        }
        let inv = CFrame { r: t, p: [0.0; 3] };
        let p = inv.apply([-self.p[0], -self.p[1], -self.p[2]]);
        CFrame { r: t, p }
    }
}

#[derive(Debug, Default, Clone)]
pub struct Instance {
    pub class: String,
    pub props: HashMap<String, Value>,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
}

#[derive(Debug, Default)]
pub struct Model {
    pub instances: Vec<Instance>,
}

impl Model {
    pub fn find_child(&self, parent: usize, name: &str) -> Option<usize> {
        self.instances[parent]
            .children
            .iter()
            .copied()
            .find(|&c| matches!(self.instances[c].props.get("Name"), Some(Value::Str(n)) if n == name))
    }

    pub fn children_of_class<'a>(&'a self, parent: usize, class: &'a str) -> impl Iterator<Item = usize> + 'a {
        self.instances[parent].children.iter().copied().filter(move |&c| self.instances[c].class == class)
    }

    pub fn first_of_class(&self, class: &str) -> Option<usize> {
        self.instances.iter().position(|i| i.class == class)
    }

    pub fn str_prop(&self, idx: usize, name: &str) -> Option<&str> {
        match self.instances[idx].props.get(name) {
            Some(Value::Str(s)) => Some(s),
            _ => None,
        }
    }

    pub fn vec3_prop(&self, idx: usize, name: &str) -> Option<[f32; 3]> {
        match self.instances[idx].props.get(name) {
            Some(Value::Vec3(v)) => Some(*v),
            _ => None,
        }
    }

    pub fn cframe_prop(&self, idx: usize, name: &str) -> Option<CFrame> {
        match self.instances[idx].props.get(name) {
            Some(Value::CFrame(c)) => Some(*c),
            _ => None,
        }
    }

    pub fn color_prop(&self, idx: usize, name: &str) -> Option<[f32; 3]> {
        match self.instances[idx].props.get(name) {
            Some(Value::Color3(c)) => Some(*c),
            _ => None,
        }
    }

    pub fn enum_prop(&self, idx: usize, name: &str) -> Option<u32> {
        match self.instances[idx].props.get(name) {
            Some(Value::Enum(v)) => Some(*v),
            _ => None,
        }
    }
}

struct Reader<'a> {
    d: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    fn new(d: &'a [u8]) -> Self {
        Self { d, p: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        if self.p + n > self.d.len() {
            return Err("unexpected end of data".to_string());
        }
        let s = &self.d[self.p..self.p + n];
        self.p += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn string(&mut self) -> Result<String, String> {
        let n = self.u32()? as usize;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }

    fn interleaved_u32(&mut self, n: usize) -> Result<Vec<u32>, String> {
        let raw = self.take(n * 4)?;
        Ok((0..n)
            .map(|i| u32::from_be_bytes([raw[i], raw[n + i], raw[2 * n + i], raw[3 * n + i]]))
            .collect())
    }
    fn interleaved_i32(&mut self, n: usize) -> Result<Vec<i32>, String> {
        Ok(self.interleaved_u32(n)?.into_iter().map(|v| ((v >> 1) as i32) ^ -((v & 1) as i32)).collect())
    }
    fn interleaved_f32(&mut self, n: usize) -> Result<Vec<f32>, String> {
        Ok(self.interleaved_u32(n)?.into_iter().map(|v| f32::from_bits((v >> 1) | (v << 31))).collect())
    }
    fn referents(&mut self, n: usize) -> Result<Vec<i32>, String> {
        let mut out = self.interleaved_i32(n)?;
        for i in 1..n {
            out[i] += out[i - 1];
        }
        Ok(out)
    }
}

const ROTATIONS: [(u8, [[f32; 3]; 3]); 24] = [
    (0x02, [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]]),
    (0x03, [[1., 0., 0.], [0., 0., -1.], [0., 1., 0.]]),
    (0x05, [[1., 0., 0.], [0., -1., 0.], [0., 0., -1.]]),
    (0x06, [[1., 0., 0.], [0., 0., 1.], [0., -1., 0.]]),
    (0x07, [[0., 1., 0.], [1., 0., 0.], [0., 0., -1.]]),
    (0x09, [[0., 0., 1.], [1., 0., 0.], [0., 1., 0.]]),
    (0x0a, [[0., -1., 0.], [1., 0., 0.], [0., 0., 1.]]),
    (0x0c, [[0., 0., -1.], [1., 0., 0.], [0., -1., 0.]]),
    (0x0d, [[0., 1., 0.], [0., 0., 1.], [1., 0., 0.]]),
    (0x0e, [[0., 0., -1.], [0., 1., 0.], [1., 0., 0.]]),
    (0x10, [[0., -1., 0.], [0., 0., -1.], [1., 0., 0.]]),
    (0x11, [[0., 0., 1.], [0., -1., 0.], [1., 0., 0.]]),
    (0x14, [[-1., 0., 0.], [0., 1., 0.], [0., 0., -1.]]),
    (0x15, [[-1., 0., 0.], [0., 0., 1.], [0., 1., 0.]]),
    (0x17, [[-1., 0., 0.], [0., -1., 0.], [0., 0., 1.]]),
    (0x18, [[-1., 0., 0.], [0., 0., -1.], [0., -1., 0.]]),
    (0x19, [[0., 1., 0.], [-1., 0., 0.], [0., 0., 1.]]),
    (0x1b, [[0., 0., -1.], [-1., 0., 0.], [0., 1., 0.]]),
    (0x1c, [[0., -1., 0.], [-1., 0., 0.], [0., 0., -1.]]),
    (0x1d, [[0., 0., 1.], [-1., 0., 0.], [0., -1., 0.]]),
    (0x1f, [[0., 1., 0.], [0., 0., -1.], [-1., 0., 0.]]),
    (0x20, [[0., 0., 1.], [0., 1., 0.], [-1., 0., 0.]]),
    (0x22, [[0., -1., 0.], [0., 0., 1.], [-1., 0., 0.]]),
    (0x23, [[0., 0., -1.], [0., -1., 0.], [-1., 0., 0.]]),
];


pub fn parse(data: &[u8]) -> Result<Model, String> {
    if data.starts_with(b"<roblox!") {
        parse_binary(data)
    } else if data.starts_with(b"<roblox") {
        parse_xml(data)
    } else {
        Err("not a Roblox model".to_string())
    }
}

fn parse_xml(data: &[u8]) -> Result<Model, String> {
    let text = String::from_utf8_lossy(data);
    let doc = roxmltree::Document::parse(&text).map_err(|e| e.to_string())?;
    let mut model = Model::default();
    for item in doc.root_element().children().filter(|n| n.has_tag_name("Item")) {
        xml_item(&mut model, item, None);
    }
    Ok(model)
}

fn xml_item(model: &mut Model, node: roxmltree::Node, parent: Option<usize>) {
    let idx = model.instances.len();
    let mut inst = Instance { class: node.attribute("class").unwrap_or_default().to_string(), parent, ..Default::default() };
    if let Some(props) = node.children().find(|n| n.has_tag_name("Properties")) {
        for p in props.children().filter(|n| n.is_element()) {
            let Some(name) = p.attribute("name") else { continue };
            let child = |t: &str| -> f32 {
                p.children().find(|n| n.has_tag_name(t)).and_then(|n| n.text()).and_then(|v| v.trim().parse().ok()).unwrap_or(0.0)
            };
            let value = match p.tag_name().name() {
                "string" | "ProtectedString" => Some(Value::Str(p.text().unwrap_or_default().to_string())),
                "Content" => Some(Value::Str(
                    p.children().find(|n| n.has_tag_name("url")).and_then(|n| n.text()).unwrap_or_default().trim().to_string(),
                )),
                "bool" => Some(Value::Bool(p.text().map_or(false, |t| t.trim() == "true"))),
                "int" => p.text().and_then(|t| t.trim().parse().ok()).map(Value::Int),
                "float" | "double" => p.text().and_then(|t| t.trim().parse().ok()).map(Value::Float),
                "token" => p.text().and_then(|t| t.trim().parse().ok()).map(Value::Enum),
                "Vector3" => Some(Value::Vec3([child("X"), child("Y"), child("Z")])),
                "CoordinateFrame" => Some(Value::CFrame(CFrame {
                    r: [
                        [child("R00"), child("R01"), child("R02")],
                        [child("R10"), child("R11"), child("R12")],
                        [child("R20"), child("R21"), child("R22")],
                    ],
                    p: [child("X"), child("Y"), child("Z")],
                })),
                "Color3" | "Color3uint8" => p.text().and_then(|t| t.trim().parse::<u32>().ok()).map(|v| {
                    Value::Color3([((v >> 16) & 255) as f32 / 255.0, ((v >> 8) & 255) as f32 / 255.0, (v & 255) as f32 / 255.0])
                }),
                _ => None,
            };
            if let Some(v) = value {
                inst.props.insert(name.to_string(), v);
            }
        }
    }
    model.instances.push(inst);
    if let Some(pa) = parent {
        model.instances[pa].children.push(idx);
    }
    for c in node.children().filter(|n| n.has_tag_name("Item")) {
        xml_item(model, c, Some(idx));
    }
}

fn parse_binary(data: &[u8]) -> Result<Model, String> {
    if data.len() < 32 || &data[..8] != b"<roblox!" {
        return Err("not a binary Roblox model".to_string());
    }
    let mut r = Reader::new(data);
    r.take(8 + 6 + 2)?;
    let _classes = r.u32()?;
    let instance_count = r.u32()? as usize;
    r.take(8)?;

    let mut model = Model { instances: vec![Instance::default(); instance_count] };

    let mut classes: HashMap<u32, (String, Vec<i32>)> = HashMap::new();

    let mut by_ref: HashMap<i32, usize> = HashMap::new();
    let mut next_index = 0usize;

    loop {
        let name = match r.take(4) {
            Ok(n) => [n[0], n[1], n[2], n[3]],
            Err(_) => break,
        };
        let clen = r.u32()? as usize;
        let ulen = r.u32()? as usize;
        r.u32()?;
        let payload = if clen == 0 {
            r.take(ulen)?.to_vec()
        } else {
            let comp = r.take(clen)?;
            lz4_flex::block::decompress(comp, ulen).map_err(|e| format!("lz4: {}", e))?
        };
        match &name {
            b"END\0" => break,
            b"INST" => {
                let mut c = Reader::new(&payload);
                let class_id = c.u32()?;
                let class = c.string()?;
                let has_service = c.u8()?;
                let count = c.u32()? as usize;
                let refs = c.referents(count)?;
                let _ = has_service;
                for &rf in &refs {
                    if next_index < model.instances.len() {
                        model.instances[next_index].class = class.clone();
                        by_ref.insert(rf, next_index);
                        next_index += 1;
                    }
                }
                classes.insert(class_id, (class, refs));
            }
            b"PROP" => {
                let mut c = Reader::new(&payload);
                let class_id = c.u32()?;
                let prop = c.string()?;
                let ty = c.u8()?;
                let Some((_, refs)) = classes.get(&class_id) else { continue };
                let refs = refs.clone();
                let n = refs.len();
                let values = match read_values(&mut c, ty, n) {
                    Ok(Some(v)) => v,
                    _ => continue,
                };
                for (rf, v) in refs.iter().zip(values) {
                    if let Some(&idx) = by_ref.get(rf) {
                        model.instances[idx].props.insert(prop.clone(), v);
                    }
                }
            }
            b"PRNT" => {
                let mut c = Reader::new(&payload);
                c.u8()?;
                let count = c.u32()? as usize;
                let children = c.referents(count)?;
                let parents = c.referents(count)?;
                for (ch, pa) in children.iter().zip(parents.iter()) {
                    if let Some(&ci) = by_ref.get(ch) {
                        if let Some(&pi) = by_ref.get(pa) {
                            model.instances[ci].parent = Some(pi);
                            model.instances[pi].children.push(ci);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(model)
}

fn read_values(c: &mut Reader, ty: u8, n: usize) -> Result<Option<Vec<Value>>, String> {
    Ok(Some(match ty {
        0x01 => (0..n).map(|_| c.string().map(Value::Str)).collect::<Result<_, _>>()?,
        0x02 => (0..n).map(|_| c.u8().map(|b| Value::Bool(b != 0))).collect::<Result<_, _>>()?,
        0x03 => c.interleaved_i32(n)?.into_iter().map(Value::Int).collect(),
        0x04 => c.interleaved_f32(n)?.into_iter().map(Value::Float).collect(),
        0x0c => {
            let (r, g, b) = (c.interleaved_f32(n)?, c.interleaved_f32(n)?, c.interleaved_f32(n)?);
            (0..n).map(|i| Value::Color3([r[i], g[i], b[i]])).collect()
        }
        0x0e => {
            let (x, y, z) = (c.interleaved_f32(n)?, c.interleaved_f32(n)?, c.interleaved_f32(n)?);
            (0..n).map(|i| Value::Vec3([x[i], y[i], z[i]])).collect()
        }
        0x1a => {
            let raw = c.take(n * 3)?;
            (0..n).map(|i| Value::Color3([raw[i] as f32 / 255.0, raw[n + i] as f32 / 255.0, raw[2 * n + i] as f32 / 255.0])).collect()
        }
        0x12 => c.interleaved_u32(n)?.into_iter().map(Value::Enum).collect(),
        0x10 => {
            let mut rots = Vec::with_capacity(n);
            for _ in 0..n {
                let id = c.u8()?;
                if id == 0 {
                    let mut m = [[0.0f32; 3]; 3];
                    for row in m.iter_mut() {
                        for v in row.iter_mut() {
                            *v = c.f32()?;
                        }
                    }
                    rots.push(m);
                } else {
                    let m = ROTATIONS.iter().find(|(i, _)| *i == id).map(|(_, m)| *m).ok_or("unknown rotation id")?;
                    rots.push(m);
                }
            }
            let (x, y, z) = (c.interleaved_f32(n)?, c.interleaved_f32(n)?, c.interleaved_f32(n)?);
            (0..n).map(|i| Value::CFrame(CFrame { r: rots[i], p: [x[i], y[i], z[i]] })).collect()
        }
        _ => return Ok(None),
    }))
}
