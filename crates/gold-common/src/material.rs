use crate::bsp::*;
use crate::fs::GameFs;
use crate::kv::{self, Kv, KvValue};
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Clone, Debug)]
pub struct VtfInfo {
    pub width: i32,
    pub height: i32,
    pub reflectivity: [f32; 3],
}

pub fn parse_vtf_header(d: &[u8]) -> Option<VtfInfo> {
    if d.len() < 48 || &d[0..4] != b"VTF\0" {
        return None;
    }
    let w = u16::from_le_bytes([d[16], d[17]]) as i32;
    let h = u16::from_le_bytes([d[18], d[19]]) as i32;
    let rf = |o: usize| f32::from_le_bytes(d[o..o + 4].try_into().unwrap());
    Some(VtfInfo {
        width: w,
        height: h,
        reflectivity: [rf(32), rf(36), rf(40)],
    })
}

#[derive(Clone, Debug)]
pub struct Material {
    pub name: String,
    pub found: bool,
    pub shader: String,
    pub params: Vec<(String, String)>,
    pub width: i32,
    pub height: i32,
    pub reflectivity: [f32; 3],
    pub contents: i32,
    pub surf_flags: i32,
}

impl Material {
    pub fn param(&self, key: &str) -> Option<&str> {
        let k = key.to_ascii_lowercase();
        self.params
            .iter()
            .find(|(pk, _)| *pk == k)
            .map(|(_, v)| v.as_str())
    }
    pub fn flag(&self, key: &str) -> bool {
        self.param(key)
            .map(|v| {
                let v = v.trim();
                v != "0" && !v.is_empty() && v.parse::<f32>().map(|f| f != 0.0).unwrap_or(true)
            })
            .unwrap_or(false)
    }
    pub fn is_wvt(&self) -> bool {
        self.shader.eq_ignore_ascii_case("worldvertextransition")
    }
    pub fn surfaceprop(&self) -> &str {
        self.param("$surfaceprop").unwrap_or("default")
    }
}

pub struct MaterialCache {
    cache: Mutex<HashMap<String, Material>>,
}

impl Default for MaterialCache {
    fn default() -> Self {
        Self::new()
    }
}

fn flatten(kvs: &[Kv], out: &mut Vec<(String, String)>) {
    for kv in kvs {
        match &kv.value {
            KvValue::Str(s) => {
                let k = kv.key.to_ascii_lowercase();
                if let Some(p) = out.iter_mut().find(|(pk, _)| *pk == k) {
                    p.1 = s.clone();
                } else {
                    out.push((k, s.clone()));
                }
            }
            KvValue::Block(_) => {}
        }
    }
}

pub fn load_vmt(fs: &GameFs, name: &str, depth: usize) -> Option<(String, Vec<(String, String)>)> {
    if depth > 8 {
        return None;
    }
    let mut path = name.replace('\\', "/").to_ascii_lowercase();
    if !path.starts_with("materials/") {
        path = format!("materials/{path}");
    }
    if !path.ends_with(".vmt") {
        path.push_str(".vmt");
    }
    let data = fs.read(&path)?;
    let text = String::from_utf8_lossy(&data);
    let top = kv::parse(&text);
    let root = top.into_iter().find(|k| k.is_block())?;
    if root.key.eq_ignore_ascii_case("patch") {
        let include = root.get("include")?.to_string();
        let (shader, mut params) = load_vmt(fs, &include, depth + 1)?;
        for sub in ["replace", "insert"] {
            if let Some(b) = root.block(sub) {
                flatten(b.children(), &mut params);
            }
        }
        return Some((shader, params));
    }
    let mut params = Vec::new();
    flatten(root.children(), &mut params);
    Some((root.key.clone(), params))
}

impl MaterialCache {
    pub fn new() -> Self {
        MaterialCache {
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn get(&self, fs: &GameFs, name: &str) -> Material {
        let key = name.replace('\\', "/").to_ascii_lowercase();
        if let Some(m) = self.cache.lock().unwrap().get(&key) {
            return m.clone();
        }
        let m = Self::load(fs, name);
        self.cache.lock().unwrap().insert(key, m.clone());
        m
    }

    fn load(fs: &GameFs, name: &str) -> Material {
        let (found, shader, params) = match load_vmt(fs, name, 0) {
            Some((s, p)) => (true, s, p),
            None => (false, String::from("LightmappedGeneric"), Vec::new()),
        };
        let mut m = Material {
            name: name.to_string(),
            found,
            shader,
            params,
            width: 64,
            height: 64,
            reflectivity: [0.5, 0.5, 0.5],
            contents: CONTENTS_SOLID,
            surf_flags: 0,
        };
        let lname = name.to_ascii_lowercase();
        let tex = m
            .param("$basetexture")
            .map(|s| s.to_string())
            .or_else(|| m.param("$envmapmask").map(|s| s.to_string()))
            .or_else(|| m.param("$normalmap").map(|s| s.to_string()))
            .or_else(|| m.param("$bumpmap").map(|s| s.to_string()))
            .unwrap_or_else(|| lname.clone());
        let tpath = format!(
            "materials/{}.vtf",
            tex.replace('\\', "/")
                .to_ascii_lowercase()
                .trim_end_matches(".vtf")
        );
        if let Some(info) = fs.read(&tpath).as_deref().and_then(parse_vtf_header) {
            m.width = info.width;
            m.height = info.height;
            m.reflectivity = info.reflectivity;
        }
        if let Some(r) = m.param("$reflectivity") {
            let f: Vec<f32> = r
                .split(|c: char| c.is_whitespace() || c == '[' || c == ']' || c == '{' || c == '}')
                .filter_map(|t| t.parse().ok())
                .collect();
            if f.len() >= 3 {
                m.reflectivity = [f[0], f[1], f[2]];
            }
        }
        m.compute_flags();
        m
    }
}

impl Material {
    fn compute_flags(&mut self) {
        let shader = self.shader.to_ascii_lowercase();
        let mut contents = CONTENTS_SOLID;
        let mut flags = 0;

        if self.flag("%compilesky") {
            flags |= SURF_SKY | SURF_NOLIGHT;
        }
        if self.flag("%compile2dsky") {
            flags |= SURF_SKY | SURF_SKY2D | SURF_NOLIGHT;
        }
        if self.flag("%compilehint") {
            flags |= SURF_NODRAW | SURF_NOLIGHT | SURF_HINT;
            contents = 0;
        }
        if self.flag("%compileskip") {
            flags |= SURF_NODRAW | SURF_NOLIGHT | SURF_SKIP;
            contents = 0;
        }
        if self.flag("%compilenodraw") {
            flags |= SURF_NODRAW | SURF_NOLIGHT;
        }
        if self.flag("%compilenolight") {
            flags |= SURF_NOLIGHT;
        }
        if self.flag("%compiletrigger") {
            flags |= SURF_TRIGGER | SURF_NODRAW | SURF_NOLIGHT;
        }
        if self.flag("%compilenochop") {
            flags |= SURF_NOCHOP;
        }
        if self.flag("%noportal") {
            flags |= SURF_NOPORTAL;
        }
        if self.flag("%compileclip") {
            contents = CONTENTS_PLAYERCLIP | CONTENTS_MONSTERCLIP;
            flags |= SURF_NODRAW | SURF_NOLIGHT;
        }
        if self.flag("%compileplayerclip") || self.flag("%playerclip") {
            contents = CONTENTS_PLAYERCLIP;
            flags |= SURF_NODRAW | SURF_NOLIGHT;
        }
        if self.flag("%compilenpcclip") {
            contents = CONTENTS_MONSTERCLIP;
            flags |= SURF_NODRAW | SURF_NOLIGHT;
        }
        if self.flag("%compileorigin") {
            contents = CONTENTS_ORIGIN;
            flags |= SURF_NODRAW | SURF_NOLIGHT;
        }
        if self.flag("%compileareaportal") {
            contents = CONTENTS_AREAPORTAL;
            flags |= SURF_NODRAW | SURF_NOLIGHT;
        }
        if self.flag("%compilenonsolid") {
            contents = 0;
        }
        if self.flag("%compileladder") {
            contents |= CONTENTS_LADDER;
        }
        if self.flag("%compileblocklos") {
            contents = CONTENTS_BLOCKLOS;
            flags |= SURF_NODRAW | SURF_NOLIGHT;
        }
        if self.flag("%compilewater") || shader == "water" {
            contents = CONTENTS_WATER;
        }
        if self.flag("%compileslime") {
            contents = CONTENTS_SLIME;
        }
        if self.flag("%compiledetail") {
            contents |= CONTENTS_DETAIL;
        }
        let translucent = self.flag("$translucent")
            || self.flag("$additive")
            || self
                .param("$alpha")
                .and_then(|a| a.trim().parse::<f32>().ok())
                .map(|a| a < 1.0)
                .unwrap_or(false);
        if translucent {
            flags |= SURF_TRANS;
        }
        if self.flag("$alphatest") && flags & SURF_NODRAW == 0 && contents == CONTENTS_SOLID {
            contents = CONTENTS_GRATE | CONTENTS_TRANSLUCENT;
            flags |= SURF_TRANS;
        }
        let lightmapped = matches!(
            shader.as_str(),
            "lightmappedgeneric"
                | "worldvertextransition"
                | "lightmapped_4wayblend"
                | "lightmappedreflective"
                | "worldtwotextureblend"
                | "lightmappedgeneric_dx9"
                | "worldvertextransition_dx9"
        );
        if !lightmapped && self.found {
            flags |= SURF_NOLIGHT;
        }
        if lightmapped
            && (self.param("$bumpmap").is_some() || self.param("$normalmap").is_some())
            && flags & SURF_NOLIGHT == 0
        {
            flags |= SURF_BUMPLIGHT;
        }
        if contents & CONTENTS_WATER != 0 {
            flags |= SURF_NOLIGHT;
            flags &= !SURF_BUMPLIGHT;
        }
        self.contents = contents;
        self.surf_flags = flags;
    }
}
