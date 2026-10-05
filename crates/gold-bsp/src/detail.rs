use crate::disp::Disp;
use crate::emit::Output;
use crate::map::Map;
use gold_common::fs::GameFs;
use gold_common::kv::{self, Kv};
use gold_common::math::*;
use std::collections::HashMap;

pub const DETAIL_PROP_VERSION: u16 = 4;

const TYPE_MODEL: u8 = 0;
const TYPE_SPRITE: u8 = 1;
const TYPE_SHAPE_CROSS: u8 = 2;
const TYPE_SHAPE_TRI: u8 = 3;

const MODELFLAG_UPRIGHT: i32 = 1;

#[derive(Clone, Debug)]
struct DetailModel {
    model: Option<String>,
    kind: u8,
    amount: f32,
    min_cos: f32,
    max_cos: f32,
    flags: i32,
    orientation: u8,
    pos: [[f32; 2]; 2],
    tex: [[f32; 2]; 2],
    random_scale: f32,
    shape_size: u8,
    shape_angle: u8,
    sway: u8,
}

#[derive(Clone, Debug)]
struct Group {
    alpha: f32,
    models: Vec<DetailModel>,
}

#[derive(Clone, Debug)]
struct DetailType {
    density: f32,
    groups: Vec<Group>,
}

struct CRand(u32);
impl CRand {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(214013).wrapping_add(2531011);
        ((self.0 >> 16) & 0x7fff) as f32 / 32767.0
    }
}

struct Gauss(u64);
impl Gauss {
    fn uniform(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    }
    fn gaussian(&mut self, mean: f64, stddev: f64) -> f64 {
        let (u1, u2) = (self.uniform(), self.uniform());
        mean + stddev * (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
    }
}

fn kv_f32(k: &Kv, key: &str, default: f32) -> f32 {
    k.get(key)
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(default)
}

fn scan_floats(s: &str) -> Vec<f32> {
    s.split_whitespace().map_while(|t| t.parse().ok()).collect()
}

fn parse_group(group_kv: &Kv, groups: &mut Vec<Group>) {
    let alpha = kv_f32(group_kv, "alpha", 1.0);
    let mut i = groups.len() as isize - 1;
    while i >= 0 && alpha <= groups[i as usize].alpha {
        i -= 1;
    }
    let mut group = Group {
        alpha,
        models: Vec::new(),
    };
    let mut total = 0.0f32;
    for m in group_kv.children().iter().filter(|c| c.is_block()) {
        let mut dm = DetailModel {
            model: None,
            kind: TYPE_SPRITE,
            amount: 0.0,
            min_cos: 0.0,
            max_cos: 0.0,
            flags: 0,
            orientation: 0,
            pos: [[0.0; 2]; 2],
            tex: [[0.0; 2]; 2],
            random_scale: 0.0,
            shape_size: 0,
            shape_angle: 0,
            sway: 0,
        };
        if let Some(name) = m.get("model") {
            dm.model = Some(name.to_string());
            dm.kind = TYPE_MODEL;
        } else if let Some(sprite) = m.get("sprite") {
            dm.kind = match m.get("sprite_shape").map(|s| s.to_ascii_lowercase()) {
                Some(s) if s == "cross" => TYPE_SHAPE_CROSS,
                Some(s) if s == "tri" => TYPE_SHAPE_TRI,
                _ => TYPE_SPRITE,
            };
            let f = scan_floats(sprite);
            if f.len() != 5 || f[4] == 0.0 {
                eprintln!("Warning: bad detail sprite \"{sprite}\"; expected \"x y w h texsize\"");
                continue;
            }
            let (x, y, w, h, size) = (f[0], f[1], f[2], f[3], f[4]);
            dm.tex = [
                [(x + 0.5) / size, (y + 0.5) / size],
                [(x + w - 0.5) / size, (y + h - 0.5) / size],
            ];
            dm.pos = [[-w / 2.0, h], [w / 2.0, 0.0]];
            if let Some(ss) = m.get("spritesize") {
                let f = scan_floats(ss);
                if f.len() == 4 {
                    let (xf, yf, w, h) = (f[0], f[1], f[2], f[3]);
                    dm.pos = [[-xf * w, (1.0 - yf) * h], [(1.0 - xf) * w, -yf * h]];
                }
            }
            dm.random_scale = kv_f32(m, "spriterandomscale", 0.0);
            dm.sway = (255.0 * kv_f32(m, "sway", 0.0).clamp(0.0, 1.0)) as u8;
            dm.shape_angle = kv_f32(m, "shape_angle", 0.0) as i32 as u8;
            dm.shape_size = (255.0 * kv_f32(m, "shape_size", 0.0).clamp(0.0, 1.0)) as u8;
        }
        dm.amount = kv_f32(m, "amount", 1.0) + total;
        total = dm.amount;
        if kv_f32(m, "upright", 0.0) as i32 != 0 {
            dm.flags |= MODELFLAG_UPRIGHT;
        }
        let min_angle = kv_f32(m, "minAngle", 180.0);
        let max_angle = kv_f32(m, "maxAngle", 180.0);
        dm.min_cos = (min_angle * std::f32::consts::PI / 180.0).cos();
        dm.max_cos = (max_angle * std::f32::consts::PI / 180.0).cos();
        if dm.min_cos < dm.max_cos {
            dm.min_cos = dm.max_cos;
        }
        dm.orientation = kv_f32(m, "detailOrientation", 0.0) as i32 as u8;
        group.models.push(dm);
    }
    if total > 1.0 {
        for m in group.models.iter_mut() {
            m.amount /= total;
        }
    }
    groups.insert((i + 1) as usize, group);
}

fn load_dictionary(fs: &GameFs, file: &str) -> HashMap<String, DetailType> {
    let mut dict = HashMap::new();
    let Some(data) = fs.read(file) else {
        return dict;
    };
    let text = String::from_utf8_lossy(&data);
    for root in kv::parse(&text) {
        for t in root.children().iter().filter(|c| c.is_block()) {
            let mut dt = DetailType {
                density: kv_f32(t, "density", 0.0),
                groups: Vec::new(),
            };
            for g in t.children().iter().filter(|c| c.is_block()) {
                parse_group(g, &mut dt.groups);
            }
            dict.entry(t.key.to_ascii_lowercase()).or_insert(dt);
        }
    }
    dict
}

fn select_group(dt: &DetailType, alpha: f32, rng: &mut CRand) -> usize {
    let n = dt.groups.len();
    let mut start = 0;
    while start + 1 < n {
        if alpha < dt.groups[start + 1].alpha {
            break;
        }
        start += 1;
    }
    let end = (start + 1).min(n - 1);
    if start == end {
        return start;
    }
    let da = dt.groups[end].alpha - dt.groups[start].alpha;
    let dist = if da != 0.0 {
        (alpha - dt.groups[start].alpha) / da
    } else {
        0.0
    };
    if rng.next() > dist { start } else { end }
}

fn select_model(g: &Group, rng: &mut CRand) -> Option<usize> {
    let r = rng.next();
    g.models.iter().position(|m| r <= m.amount)
}

fn basis_angles(x: Vec3, y: Vec3, z: Vec3) -> Vec3 {
    let xy = (x.x * x.x + x.y * x.y).sqrt();
    if xy > 0.001 {
        v3(
            (-x.z).atan2(xy).to_degrees(),
            x.y.atan2(x.x).to_degrees(),
            y.z.atan2(z.z).to_degrees(),
        )
    } else {
        v3(
            (-x.z).atan2(xy).to_degrees(),
            (-y.x).atan2(y.y).to_degrees(),
            0.0,
        )
    }
}

#[derive(Clone, Copy)]
struct Object {
    origin: Vec3,
    angles: Vec3,
    dict: u16,
    leaf: u16,
    orientation: u8,
    kind: u8,
    scale: f32,
    sway: u8,
    shape_angle: u8,
    shape_size: u8,
}

struct Builder<'a> {
    fs: &'a GameFs,
    models: Vec<String>,
    sprites: Vec<[[f32; 2]; 4]>,
    objects: Vec<Object>,
    valid_models: HashMap<String, bool>,
    leaf_of: &'a dyn Fn(Vec3) -> u16,
    gauss: Gauss,
    overflow: usize,
}

impl Builder<'_> {
    fn model_index(&mut self, name: &str) -> Option<u16> {
        let fs = self.fs;
        let valid = *self
            .valid_models
            .entry(name.to_ascii_lowercase())
            .or_insert_with(|| {
                let ok = fs.exists(&name.replace('\\', "/"));
                if !ok {
                    eprintln!("Warning: detail prop model {name} not found");
                }
                ok
            });
        if !valid {
            return None;
        }
        let i = self
            .models
            .iter()
            .position(|m| m == name)
            .unwrap_or_else(|| {
                self.models.push(name.to_string());
                self.models.len() - 1
            });
        Some(i as u16)
    }

    fn sprite_index(&mut self, pos: [[f32; 2]; 2], tex: [[f32; 2]; 2]) -> u16 {
        let key = [pos[0], pos[1], tex[0], tex[1]];
        let i = self
            .sprites
            .iter()
            .position(|s| *s == key)
            .unwrap_or_else(|| {
                self.sprites.push(key);
                self.sprites.len() - 1
            });
        i as u16
    }

    fn push(&mut self, o: Object) {
        if self.objects.len() >= 65535 {
            self.overflow += 1;
            return;
        }
        self.objects.push(o);
    }

    fn place(&mut self, m: &DetailModel, pt: Vec3, normal: Vec3, rng: &mut CRand) {
        let cos_angle = normal.z as f32;
        if cos_angle < m.max_cos {
            return;
        }
        if cos_angle < m.min_cos {
            let p = (cos_angle - m.max_cos) / (m.min_cos - m.max_cos);
            if rng.next() > p {
                return;
            }
        }
        let angles = if m.flags & MODELFLAG_UPRIGHT != 0 {
            v3(0.0, 360.0 * rng.next() as f64, 0.0)
        } else {
            let z = normal;
            let mut x = v3(1.0, 0.0, 0.0);
            if x.dot(z).abs() - 1.0 > -1e-3 {
                x = v3(0.0, 1.0, 0.0);
            }
            let y = z.cross(x).normalize();
            let x = y.cross(z).normalize();
            let rot = (360.0 * rng.next() as f64).to_radians();
            let (s, c) = rot.sin_cos();
            basis_angles(x * c + y * s, y * c - x * s, z)
        };
        let leaf = (self.leaf_of)(pt);
        match m.kind {
            TYPE_MODEL => {
                let Some(dict) = self.model_index(m.model.as_deref().unwrap_or("")) else {
                    return;
                };
                self.push(Object {
                    origin: pt,
                    angles,
                    dict,
                    leaf,
                    orientation: m.orientation,
                    kind: TYPE_MODEL,
                    scale: 1.0,
                    sway: 0,
                    shape_angle: 0,
                    shape_size: 0,
                });
            }
            _ => {
                let scale = if m.random_scale != 0.0 {
                    self.gauss.gaussian(1.0, m.random_scale as f64).abs() as f32
                } else {
                    1.0
                };
                let dict = self.sprite_index(m.pos, m.tex);
                self.push(Object {
                    origin: pt,
                    angles,
                    dict,
                    leaf,
                    orientation: m.orientation,
                    kind: m.kind,
                    scale,
                    sway: m.sway,
                    shape_angle: m.shape_angle,
                    shape_size: m.shape_size,
                });
            }
        }
    }
}

pub struct DetailEntity {
    kind: u8,
    origin: Vec3,
    angles: Vec3,
    orientation: u8,
    model: String,
    pos: [[f32; 2]; 2],
    tex: [[f32; 2]; 2],
}

fn vec2(s: Option<&str>) -> [f32; 2] {
    let f = s.map(scan_floats).unwrap_or_default();
    [
        f.first().copied().unwrap_or(0.0),
        f.get(1).copied().unwrap_or(0.0),
    ]
}

pub fn collect_entities(map: &mut Map) -> Vec<DetailEntity> {
    let mut out = Vec::new();
    for e in map.entities.iter_mut() {
        let class = e.classname().to_ascii_lowercase();
        let origin = e.get("origin").and_then(parse_vec3).unwrap_or_default();
        let angles = e.get("angles").and_then(parse_vec3).unwrap_or_default();
        let orientation = e
            .get("detailOrientation")
            .and_then(|s| s.trim().parse::<i32>().ok())
            .unwrap_or(0) as u8;
        if class == "detail_prop" || class == "prop_detail" {
            let model = e.get("model").unwrap_or("").to_string();
            out.push(DetailEntity {
                kind: TYPE_MODEL,
                origin,
                angles,
                orientation,
                model,
                pos: [[0.0; 2]; 2],
                tex: [[0.0; 2]; 2],
            });
            e.removed = true;
        } else if class == "prop_detail_sprite" {
            let pos = [vec2(e.get("position_ul")), vec2(e.get("position_lr"))];
            let mut tex = [vec2(e.get("tex_ul")), vec2(e.get("tex_size"))];
            let size: f32 = e
                .get("tex_size")
                .and_then(|s| scan_floats(s).first().copied())
                .unwrap_or(1.0);
            tex[1][0] += tex[0][0] - 0.5;
            tex[1][1] += tex[0][1] - 0.5;
            tex[0][0] += 0.5;
            tex[0][1] += 0.5;
            if size != 0.0 {
                for t in tex.iter_mut() {
                    t[0] /= size;
                    t[1] /= size;
                }
            }
            out.push(DetailEntity {
                kind: TYPE_SPRITE,
                origin,
                angles,
                orientation,
                model: String::new(),
                pos,
                tex,
            });
            e.removed = true;
        }
    }
    out
}

fn disp_vertex_normals(d: &Disp, plane_n: Vec3) -> Vec<Vec3> {
    let size = d.size();
    let mut n = vec![Vec3::ZERO; size * size];
    for t in gold_common::disp::disp_tri_indices(d.power) {
        let a = d.positions[t[0]];
        let mut tn = (d.positions[t[1]] - a).cross(d.positions[t[2]] - a);
        if tn.dot(plane_n) < 0.0 {
            tn = -tn;
        }
        for &k in &t {
            n[k] += tn;
        }
    }
    n.into_iter()
        .map(|v| {
            if v.length() > 1e-9 {
                v.normalize()
            } else {
                plane_n
            }
        })
        .collect()
}

fn disp_uv_to_surf(d: &Disp, normals: &[Vec3], u: f32, v: f32) -> (Vec3, Vec3, f32) {
    let w = d.size();
    let fu = u * (w as f32 - 1.000001);
    let fv = v * (w as f32 - 1.000001);
    let su = fu as usize;
    let sv = fv as usize;
    let nu = (su + 1).min(w - 1);
    let nv = (sv + 1).min(w - 1);
    let (xu, xv) = ((fu - su as f32) as f64, (fv - sv as f32) as f64);
    let idx = |uu: usize, vv: usize| vv * w + uu;
    let alpha = |i: usize| d.verts[i].alpha as f64;
    let lerp3 =
        |i: [usize; 3], base: usize, eu: (usize, usize), ev: (usize, usize), a: f64, b: f64| {
            let p = |k: usize| d.positions[i[k]];
            let n = |k: usize| normals[i[k]];
            let al = |k: usize| alpha(i[k]);
            let pos = p(base) + (p(eu.0) - p(eu.1)) * a + (p(ev.0) - p(ev.1)) * b;
            let nrm = n(base) + (n(eu.0) - n(eu.1)) * a + (n(ev.0) - n(ev.1)) * b;
            let alp = al(base) + (al(eu.0) - al(eu.1)) * a + (al(ev.0) - al(ev.1)) * b;
            (pos, nrm.normalize(), alp as f32)
        };
    let odd = (sv * w + su) % 2 == 1;
    if odd {
        if xu + xv >= 1.0 + 1e-3 {
            let i = [idx(su, nv), idx(nu, nv), idx(nu, sv)];
            lerp3(i, 1, (0, 1), (2, 1), 1.0 - xu, 1.0 - xv)
        } else {
            let i = [idx(su, sv), idx(su, nv), idx(nu, sv)];
            lerp3(i, 0, (2, 0), (1, 0), xu, xv)
        }
    } else if xu < xv {
        let i = [idx(su, sv), idx(su, nv), idx(nu, nv)];
        lerp3(i, 0, (2, 1), (1, 0), xu, xv)
    } else {
        let i = [idx(su, sv), idx(nu, nv), idx(nu, sv)];
        lerp3(i, 0, (2, 0), (1, 2), xu, xv)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn build_lump(
    map: &Map,
    fs: &GameFs,
    out: &Output,
    disps: &[Disp],
    face_disp: &HashMap<usize, usize>,
    entities: &[DetailEntity],
    leaf_of: &dyn Fn(Vec3) -> u16,
) -> Vec<u8> {
    let vbsp_file = map
        .entities
        .first()
        .and_then(|w| w.get("detailvbsp"))
        .filter(|s| !s.is_empty())
        .unwrap_or("detail.vbsp");
    let mut dict: Option<HashMap<String, DetailType>> = None;
    let mut b = Builder {
        fs,
        models: Vec::new(),
        sprites: Vec::new(),
        objects: Vec::new(),
        valid_models: HashMap::new(),
        leaf_of,
        gauss: Gauss(0x9e3779b97f4a7c15),
        overflow: 0,
    };
    let mut warned: std::collections::HashSet<String> = Default::default();
    for (fi, f) in out.faces.iter().enumerate() {
        let Some(mat) = map.material_for_texinfo(f.texinfo as i32) else {
            continue;
        };
        let Some(dtype) = mat.param("%detailtype") else {
            continue;
        };
        let dict = dict.get_or_insert_with(|| load_dictionary(fs, vbsp_file));
        let Some(dt) = dict.get(&dtype.to_ascii_lowercase()) else {
            if warned.insert(dtype.to_string()) {
                eprintln!(
                    "Warning: material {} uses unknown detail object type {}",
                    mat.name, dtype
                );
            }
            continue;
        };
        if dt.groups.is_empty() {
            continue;
        }
        let seed = out.face_ids.get(fi).copied().unwrap_or(0) as u32;
        let mut rng = CRand(seed.wrapping_add(1));
        if let Some(&di) = face_disp.get(&fi) {
            let d = &disps[di];
            let normals = disp_vertex_normals(d, map.planes.get(d.planenum).normal);
            let c = &d.corners;
            let area = 0.5 * (c[1] - c[0]).cross(c[2] - c[0]).length()
                + 0.5 * (c[2] - c[0]).cross(c[3] - c[0]).length();
            let samples = (area as f32 * dt.density * 0.000001) as i32;
            for _ in 0..samples {
                let u = rng.next();
                let v = rng.next();
                let (pt, n, alpha) = disp_uv_to_surf(d, &normals, u, v);
                let g = select_group(dt, alpha / 255.0, &mut rng);
                let Some(m) = select_model(&dt.groups[g], &mut rng) else {
                    continue;
                };
                b.place(&dt.groups[g].models[m], pt, n, &mut rng);
            }
        } else {
            let pts: Vec<Vec3> = (0..f.numedges)
                .map(|k| {
                    let se = out.surfedges[(f.firstedge + k as i32) as usize];
                    let e = out.edges[se.unsigned_abs() as usize];
                    Vec3::from_f32(out.verts[if se >= 0 { e.v[0] } else { e.v[1] } as usize])
                })
                .collect();
            if pts.len() < 3 {
                continue;
            }
            for i in 1..pts.len() - 1 {
                let e1 = pts[i] - pts[0];
                let e2 = pts[i + 1] - pts[0];
                let area_vec = e1.cross(e2);
                let len = area_vec.length();
                let samples = (0.5 * len as f32 * dt.density * 0.000001) as i32;
                if len <= 0.0 {
                    continue;
                }
                let normal = area_vec / -len;
                for _ in 0..samples {
                    let mut u = rng.next();
                    let mut v = rng.next();
                    if v > 1.0 - u {
                        u = 1.0 - u;
                        v = 1.0 - v;
                    }
                    let g = select_group(dt, 1.0, &mut rng);
                    let Some(m) = select_model(&dt.groups[g], &mut rng) else {
                        continue;
                    };
                    let pt = pts[0] + e1 * u as f64 + e2 * v as f64;
                    b.place(&dt.groups[g].models[m], pt, normal, &mut rng);
                }
            }
        }
    }
    for e in entities {
        let leaf = leaf_of(e.origin);
        if e.kind == TYPE_MODEL {
            let Some(dict) = b.model_index(&e.model) else {
                continue;
            };
            b.push(Object {
                origin: e.origin,
                angles: e.angles,
                dict,
                leaf,
                orientation: e.orientation,
                kind: TYPE_MODEL,
                scale: 1.0,
                sway: 0,
                shape_angle: 0,
                shape_size: 0,
            });
        } else {
            let dict = b.sprite_index(e.pos, e.tex);
            b.push(Object {
                origin: e.origin,
                angles: e.angles,
                dict,
                leaf,
                orientation: e.orientation,
                kind: TYPE_SPRITE,
                scale: 1.0,
                sway: 0,
                shape_angle: 0,
                shape_size: 0,
            });
        }
    }
    if b.overflow > 0 {
        eprintln!(
            "Warning: too many detail props; {} dropped (65535 max)",
            b.overflow
        );
    }
    b.objects.sort_by_key(|o| o.leaf);
    if !b.objects.is_empty() {
        println!("{} detail props", b.objects.len());
    }

    let mut d = Vec::new();
    d.extend_from_slice(&(b.models.len() as i32).to_le_bytes());
    for m in &b.models {
        let mut name = [0u8; 128];
        let bytes = m.as_bytes();
        let n = bytes.len().min(127);
        name[..n].copy_from_slice(&bytes[..n]);
        d.extend_from_slice(&name);
    }
    d.extend_from_slice(&(b.sprites.len() as i32).to_le_bytes());
    for s in &b.sprites {
        for p in s {
            d.extend_from_slice(&p[0].to_le_bytes());
            d.extend_from_slice(&p[1].to_le_bytes());
        }
    }
    d.extend_from_slice(&(b.objects.len() as i32).to_le_bytes());
    for o in &b.objects {
        for c in [
            o.origin.x, o.origin.y, o.origin.z, o.angles.x, o.angles.y, o.angles.z,
        ] {
            d.extend_from_slice(&(c as f32).to_le_bytes());
        }
        d.extend_from_slice(&o.dict.to_le_bytes());
        d.extend_from_slice(&o.leaf.to_le_bytes());
        d.extend_from_slice(&[255, 255, 255, 0]);
        d.extend_from_slice(&0u32.to_le_bytes());
        d.push(0);
        d.push(o.sway);
        d.push(o.shape_angle);
        d.push(o.shape_size);
        d.push(o.orientation);
        d.extend_from_slice(&[0; 3]);
        d.push(o.kind);
        d.extend_from_slice(&[0; 3]);
        d.extend_from_slice(&o.scale.to_le_bytes());
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crand_matches_msvc() {
        let mut r = CRand(1);
        let v: Vec<u32> = (0..3)
            .map(|_| (r.next() * 32767.0).round() as u32)
            .collect();
        assert_eq!(v, [41, 18467, 6334]);
    }
}
