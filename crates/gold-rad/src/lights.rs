use crate::geom::Geometry;
use gold_common::bsp::*;
use gold_common::math::*;

#[derive(Clone, Debug)]
pub struct Light {
    pub kind: i32,
    pub origin: Vec3,
    pub intensity: Vec3,
    pub normal: Vec3,
    pub stopdot: f64,
    pub stopdot2: f64,
    pub exponent: f64,
    pub radius: f64,
    pub constant: f64,
    pub linear: f64,
    pub quadratic: f64,
    pub cluster: i32,
    pub style: i32,
    pub sun_spread: f64,
    pub start_fade: f64,
    pub end_fade: f64,
    pub cap_dist: f64,
    pub slot: usize,
}

pub fn light_for_string(s: &str) -> Vec3 {
    let f: Vec<f64> = s
        .split_whitespace()
        .filter_map(|t| t.parse().ok())
        .collect();
    let (mut r, mut g, mut b, mut scaler) = (0.0, 0.0, 0.0, 1.0);
    match f.len() {
        0 => {}
        1 => {
            r = f[0];
            g = f[0];
            b = f[0];
        }
        2 => {
            r = f[0];
            g = f[0];
            b = f[0];
            scaler = f[1];
        }
        3 => {
            r = f[0];
            g = f[1];
            b = f[2];
        }
        _ => {
            r = f[0];
            g = f[1];
            b = f[2];
            scaler = f[3];
        }
    }
    let conv = |c: f64| (c / 255.0).max(0.0).powf(2.2) * 255.0;
    v3(conv(r), conv(g), conv(b)) * (scaler / 255.0)
}

fn setup_light_normal(angles: Vec3, angle: f64, pitch: f64) -> Vec3 {
    let mut out;
    if angle == -1.0 {
        out = v3(0.0, 0.0, 1.0);
    } else if angle == -2.0 {
        out = v3(0.0, 0.0, -1.0);
    } else {
        let a = if angle == 0.0 { angles.y } else { angle };
        out = v3(a.to_radians().cos(), a.to_radians().sin(), 0.0);
    }
    let p = if pitch == 0.0 { angles.x } else { pitch };
    out.z = p.to_radians().sin();
    out.x *= p.to_radians().cos();
    out.y *= p.to_radians().cos();
    out.normalize()
}

fn fkey(e: &Entity, k: &str) -> f64 {
    e.get(k)
        .and_then(|v| v.split_whitespace().next().and_then(|t| t.parse().ok()))
        .unwrap_or(0.0)
}

fn set_falloff(e: &Entity, l: &mut Light) {
    let d50 = fkey(e, "_fifty_percent_distance");
    if d50 > 0.0 {
        let mut d0 = fkey(e, "_zero_percent_distance");
        if d0 < d50 {
            d0 = 2.0 * d50;
        }
        l.constant = 0.0;
        l.linear = 0.0;
        l.quadratic = 2.0 / (d50 * d50);
        if fkey(e, "_hardfalloff") != 0.0 {
            l.end_fade = d0;
            l.start_fade = 0.75 * d0 + 0.25 * d50;
            l.cap_dist = d0;
        }
        return;
    }
    l.constant = fkey(e, "_constant_attn").max(0.0);
    l.linear = fkey(e, "_linear_attn").max(0.0);
    l.quadratic = fkey(e, "_quadratic_attn").max(0.0);
    l.radius = fkey(e, "_distance").max(0.0);
    if l.constant < 1e-4 && l.linear < 1e-4 && l.quadratic < 1e-4 {
        l.constant = 1.0;
    }
    let ratio = l.constant + 100.0 * l.linear + 100.0 * 100.0 * l.quadratic;
    if ratio > 0.0 {
        l.intensity *= ratio;
    }
}

fn light_style(e: &mut Entity, names: &mut Vec<String>) -> i32 {
    if let Some(name) = e
        .get("targetname")
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
    {
        let idx = match names.iter().position(|n| *n == name) {
            Some(i) => i,
            None => {
                names.push(name);
                names.len() - 1
            }
        };
        let style = 32 + idx as i32;
        e.set("style", &style.to_string());
        return style;
    }
    fkey(e, "style") as i32
}

pub fn parse_lights(ents: &mut [Entity], geo: &Geometry, hdr: bool) -> Vec<Light> {
    let mut out = Vec::new();
    let mut names: Vec<String> = Vec::new();
    for e in ents.iter_mut() {
        let class_l = e.classname().to_ascii_lowercase();
        let style = if class_l == "light" || class_l == "light_spot" {
            light_style(e, &mut names)
        } else {
            0
        };
        let e = &*e;
        let class = e.classname().to_ascii_lowercase();
        let origin = e.get("origin").and_then(parse_vec3).unwrap_or_default();
        let angles = e.get("angles").and_then(parse_vec3).unwrap_or_default();
        let base = Light {
            kind: EMIT_POINT,
            origin,
            intensity: Vec3::ZERO,
            normal: Vec3::ZERO,
            stopdot: 0.0,
            stopdot2: 0.0,
            exponent: 0.0,
            radius: 0.0,
            constant: 0.0,
            linear: 0.0,
            quadratic: 0.0,
            cluster: geo.point_cluster(origin),
            style: 0,
            sun_spread: 0.0,
            start_fade: 0.0,
            end_fade: 0.0,
            cap_dist: 0.0,
            slot: 0,
        };
        let light_key = |ldr: &str, hdr_key: &str| -> Vec3 {
            if hdr && let Some(v) = e.get(hdr_key) {
                let iv = light_for_string(v);
                if iv.x >= 0.0 && !v.trim().starts_with("-1") {
                    let scale = e
                        .get(&format!("{}scaleHDR", ldr))
                        .and_then(|s| s.trim().parse().ok())
                        .unwrap_or(1.0);
                    return iv * scale;
                }
            }
            let v = light_for_string(e.get(ldr).unwrap_or("0 0 0 0"));
            if hdr {
                let scale = e
                    .get(&format!("{}scaleHDR", ldr))
                    .and_then(|s| s.trim().parse().ok())
                    .unwrap_or(1.0);
                v * scale
            } else {
                v
            }
        };
        match class.as_str() {
            "light" => {
                let mut l = base.clone();
                l.intensity = light_key("_light", "_lightHDR");
                set_falloff(e, &mut l);
                l.style = style;
                out.push(l);
            }
            "light_spot" => {
                let mut l = base.clone();
                l.kind = EMIT_SPOTLIGHT;
                l.intensity = light_key("_light", "_lightHDR");
                set_falloff(e, &mut l);
                l.normal = setup_light_normal(angles, fkey(e, "angle"), fkey(e, "pitch"));
                let mut inner = fkey(e, "_inner_cone");
                let mut outer = fkey(e, "_cone");
                if outer <= 0.0 {
                    outer = 45.0;
                }
                if inner <= 0.0 || inner > outer {
                    inner = outer;
                }
                inner = inner.min(90.0);
                outer = outer.min(90.0);
                l.stopdot = inner.to_radians().cos();
                l.stopdot2 = outer.to_radians().cos();
                l.exponent = fkey(e, "_exponent");
                l.style = style;
                out.push(l);
            }
            "light_environment" => {
                if out.iter().any(|l: &Light| l.kind == EMIT_SKYLIGHT) {
                    eprintln!(
                        "Warning: multiple light_environment entities; only the first is used"
                    );
                    continue;
                }
                let dir = setup_light_normal(angles, fkey(e, "angle"), fkey(e, "pitch"));
                let mut sun = base.clone();
                sun.kind = EMIT_SKYLIGHT;
                sun.intensity = light_key("_light", "_lightHDR");
                sun.normal = dir;
                let spread = fkey(e, "SunSpreadAngle");
                sun.sun_spread = if spread > 0.0 {
                    (spread * 0.5).to_radians().cos()
                } else {
                    1.0
                };
                out.push(sun);
                let mut amb = base.clone();
                amb.kind = EMIT_SKYAMBIENT;
                amb.intensity = light_key("_ambient", "_ambientHDR");
                amb.normal = Vec3::ZERO;
                out.push(amb);
            }
            _ => {}
        }
    }
    out
}

pub fn worldlights_lump(lights: &[Light]) -> Vec<DWorldLight> {
    lights
        .iter()
        .map(|l| DWorldLight {
            origin: l.origin.to_f32(),
            intensity: (l.intensity / 255.0).to_f32(),
            normal: l.normal.to_f32(),
            cluster: l.cluster,
            type_: l.kind,
            style: l.style,
            stopdot: l.stopdot as f32,
            stopdot2: l.stopdot2 as f32,
            exponent: l.exponent as f32,
            radius: l.radius as f32,
            constant_attn: l.constant as f32,
            linear_attn: l.linear as f32,
            quadratic_attn: l.quadratic as f32,
            flags: 0,
            texinfo: 0,
            owner: 0,
        })
        .collect()
}

impl Light {
    pub fn attenuation(&self, dist: f64, dir_to_point: Vec3) -> f64 {
        if self.radius > 0.0 && dist > self.radius {
            return 0.0;
        }
        if self.cap_dist > 0.0 && dist > self.cap_dist {
            return 0.0;
        }
        let denom = self.constant + self.linear * dist + self.quadratic * dist * dist;
        let mut ratio = if denom > 0.0 { 1.0 / denom } else { 1.0 };
        if self.end_fade > self.start_fade && dist > self.start_fade {
            let t = ((dist - self.start_fade) / (self.end_fade - self.start_fade)).clamp(0.0, 1.0);
            ratio *= 1.0 - t * t * (3.0 - 2.0 * t);
        }
        if self.kind == EMIT_SURFACE {
            let cos_e = dir_to_point.dot(self.normal);
            if cos_e <= 0.001 {
                return 0.0;
            }
            return cos_e / (dist * dist);
        }
        if self.kind == EMIT_SPOTLIGHT {
            let dot2 = dir_to_point.dot(self.normal);
            if dot2 <= self.stopdot2 {
                return 0.0;
            }
            if self.exponent != 0.0 {
                ratio *= dot2.powf(self.exponent);
            }
            if dot2 <= self.stopdot && self.stopdot > self.stopdot2 {
                ratio *= (dot2 - self.stopdot2) / (self.stopdot - self.stopdot2);
            }
        }
        ratio
    }
}

pub fn parse_rad(text: &str, out: &mut std::collections::HashMap<String, Vec3>, hdr: bool) {
    for line in text.lines() {
        let mut line = line.split("//").next().unwrap_or("").trim();
        if let Some(rest) = line.strip_prefix("ldr:") {
            if hdr {
                continue;
            }
            line = rest.trim();
        } else if let Some(rest) = line.strip_prefix("hdr:") {
            if !hdr {
                continue;
            }
            line = rest.trim();
        }
        let mut toks = line.split_whitespace();
        let Some(name) = toks.next() else { continue };
        let lname = name.to_ascii_lowercase();
        if lname == "noshadow" || lname == "forcetextureshadow" {
            continue;
        }
        let rest: Vec<&str> = toks.collect();
        if rest.len() < 3 {
            continue;
        }
        let ldr = rest[..rest.len().min(4)].join(" ");
        out.insert(lname.replace('\\', "/"), light_for_string(&ldr));
    }
}

pub fn load_texlights(
    fs: &gold_common::fs::GameFs,
    bsp_path: &std::path::Path,
    extra: Option<&std::path::Path>,
    hdr: bool,
) -> std::collections::HashMap<String, Vec3> {
    let mut out = std::collections::HashMap::new();
    let read =
        |text: Option<String>, what: &str, out: &mut std::collections::HashMap<String, Vec3>| {
            if let Some(t) = text {
                let before = out.len();
                parse_rad(&t, out, hdr);
                println!("{} texlights from {}", out.len() - before, what);
            }
        };
    read(
        fs.read("lights.rad")
            .map(|d| String::from_utf8_lossy(&d).into_owned()),
        "lights.rad",
        &mut out,
    );
    let map_rad = bsp_path.with_extension("rad");
    read(
        std::fs::read_to_string(&map_rad).ok(),
        &map_rad.display().to_string(),
        &mut out,
    );
    if let Some(p) = extra {
        read(
            std::fs::read_to_string(p).ok(),
            &p.display().to_string(),
            &mut out,
        );
    }
    out
}

fn split_emitter(
    w: gold_common::winding::Winding,
    chop: f64,
    out: &mut Vec<gold_common::winding::Winding>,
) {
    let b = w.bounds();
    let size = b.size();
    let axis = size.major_axis();
    if size[axis] < chop {
        out.push(w);
        return;
    }
    let mut n = Vec3::ZERO;
    n[axis] = 1.0;
    let mid = (b.mins[axis] + b.maxs[axis]) * 0.5;
    let (f, bk) = w.clip_epsilon(n, mid, 0.01);
    match (f, bk) {
        (Some(f), Some(bk)) => {
            split_emitter(f, chop, out);
            split_emitter(bk, chop, out);
        }
        (Some(x), None) | (None, Some(x)) => out.push(x),
        (None, None) => {}
    }
}

pub fn texture_lights(
    geo: &Geometry,
    texdata_names: &[String],
    rad: &std::collections::HashMap<String, Vec3>,
) -> Vec<Light> {
    let mut out = Vec::new();
    if rad.is_empty() {
        return out;
    }
    for g in &geo.geoms {
        if g.model != 0 || g.disp.is_some() || g.points.len() < 3 {
            continue;
        }
        let Some(td) = geo.texdata.get(g.texinfo.texdata as usize) else {
            continue;
        };
        let Some(name) = texdata_names.get(td.name_string_table_id as usize) else {
            continue;
        };
        let Some(&color) = rad.get(&name.to_ascii_lowercase().replace('\\', "/")) else {
            continue;
        };
        let tv = |k: usize| {
            v3(
                g.texinfo.texture_vecs[k][0] as f64,
                g.texinfo.texture_vecs[k][1] as f64,
                g.texinfo.texture_vecs[k][2] as f64,
            )
            .length()
        };
        let tile = (td.width.max(1) * td.height.max(1)) as f64;
        let mut pieces = Vec::new();
        split_emitter(
            gold_common::winding::Winding::new(g.points.clone()),
            64.0,
            &mut pieces,
        );
        for w in pieces {
            let area = w.area();
            if area < 1e-3 {
                continue;
            }
            let origin = w.center();
            out.push(Light {
                kind: EMIT_SURFACE,
                origin,
                intensity: color * (area * tv(0) * tv(1) / tile * 10000.0),
                normal: g.normal,
                stopdot: 0.0,
                stopdot2: 0.0,
                exponent: 0.0,
                radius: 0.0,
                constant: 0.0,
                linear: 0.0,
                quadratic: 0.0,
                cluster: geo.point_cluster(origin + g.normal),
                style: 0,
                sun_spread: 0.0,
                start_fade: 0.0,
                end_fade: 0.0,
                cap_dist: 0.0,
                slot: 0,
            });
        }
    }
    out
}
