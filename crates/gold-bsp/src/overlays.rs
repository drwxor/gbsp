use crate::map::Map;
use gold_common::bsp::*;
use gold_common::fs::GameFs;
use gold_common::material::MaterialCache;
use gold_common::math::*;
use gold_common::winding::Winding;
use std::collections::HashMap;

pub const OVERLAY_BSP_FACE_COUNT: usize = 64;
pub const OVERLAY_SIZE: usize = 352;

pub struct Overlay {
    pub texinfo: i32,
    pub render_order: u16,
    pub sides: Vec<i32>,
    pub u: [f32; 2],
    pub v: [f32; 2],
    pub uv_points: [Vec3; 4],
    pub basis_u: Vec3,
    pub basis_v: Vec3,
    pub origin: Vec3,
    pub normal: Vec3,
    pub fade_min: f32,
    pub fade_max: f32,
}

fn fkey(get: &dyn Fn(&str) -> Option<String>, k: &str, def: f64) -> f64 {
    get(k).and_then(|s| s.trim().parse().ok()).unwrap_or(def)
}

fn make_overlay(
    map: &mut Map,
    fs: &GameFs,
    mats: &MaterialCache,
    get: &dyn Fn(&str) -> Option<String>,
) -> Option<Overlay> {
    let material = get("material").unwrap_or_default();
    if material.is_empty() {
        return None;
    }
    let sides: Vec<i32> = get("sides")
        .map(|s| {
            s.split_whitespace()
                .filter_map(|t| t.parse().ok())
                .collect()
        })
        .unwrap_or_default();
    let vec = |k: &str| get(k).as_deref().and_then(parse_vec3).unwrap_or_default();
    let mut uv = [Vec3::ZERO; 4];
    for (i, u) in uv.iter_mut().enumerate() {
        *u = vec(&format!("uv{i}"));
    }
    let mat = mats.get(fs, &material);
    if !mat.found {
        eprintln!("Warning: overlay material \"{material}\" not found");
    }
    let td = map.texdata_for(&material, &mat);
    let t = TexInfo {
        texture_vecs: [[0.0, 0.0, 0.0, -99999.0]; 2],
        lightmap_vecs: [[0.0, 0.0, 0.0, -99999.0]; 2],
        flags: 0,
        texdata: td,
    };
    Some(Overlay {
        texinfo: map.add_texinfo(t),
        render_order: fkey(get, "RenderOrder", 0.0).clamp(0.0, 3.0) as u16,
        sides,
        u: [
            fkey(get, "StartU", 0.0) as f32,
            fkey(get, "EndU", 1.0) as f32,
        ],
        v: [
            fkey(get, "StartV", 0.0) as f32,
            fkey(get, "EndV", 1.0) as f32,
        ],
        uv_points: uv,
        basis_u: vec("BasisU"),
        basis_v: vec("BasisV"),
        origin: vec("BasisOrigin"),
        normal: vec("BasisNormal"),
        fade_min: fkey(get, "fademindist", -1.0) as f32,
        fade_max: fkey(get, "fademaxdist", 0.0) as f32,
    })
}

pub fn collect(map: &mut Map, fs: &GameFs, mats: &MaterialCache) -> Vec<Overlay> {
    let mut out = Vec::new();
    for ei in 0..map.entities.len() {
        if !map.entities[ei]
            .classname()
            .eq_ignore_ascii_case("info_overlay")
        {
            continue;
        }
        map.entities[ei].removed = true;
        let pairs = map.entities[ei].pairs.clone();
        let get = |k: &str| {
            pairs
                .iter()
                .find(|(pk, _)| pk.eq_ignore_ascii_case(k))
                .map(|(_, v)| v.clone())
        };
        if let Some(o) = make_overlay(map, fs, mats, &get) {
            out.push(o);
        }
    }
    out.sort_by_key(|o| o.sides.is_empty());
    out
}

pub fn collect_water(map: &mut Map, fs: &GameFs, mats: &MaterialCache) -> Vec<Overlay> {
    let mut out = Vec::new();
    for ei in 0..map.entities.len() {
        if !map.entities[ei]
            .classname()
            .eq_ignore_ascii_case("info_overlay_transition")
        {
            continue;
        }
        map.entities[ei].removed = true;
        for piece in map.entities[ei].overlay_transitions.clone() {
            let get = |k: &str| {
                piece
                    .iter()
                    .find(|(pk, _)| pk.eq_ignore_ascii_case(k))
                    .map(|(_, v)| v.clone())
            };
            if let Some(mut o) = make_overlay(map, fs, mats, &get) {
                o.render_order = 0;
                out.push(o);
            }
        }
    }
    out
}

pub fn faces_for_side(map: &Map, side_id: i32, faces: &[(i32, usize, Winding)]) -> Vec<u32> {
    let mut out = Vec::new();
    let side = map
        .brushes
        .iter()
        .flat_map(|b| b.sides.iter())
        .find(|s| s.side_id == side_id);
    for (fi, (sid, planenum, w)) in faces.iter().enumerate() {
        if *sid == side_id {
            out.push(fi as u32);
            continue;
        }
        let Some(s) = side else { continue };
        let Some(sw) = &s.winding else { continue };
        if s.disp.is_some() {
            continue;
        }
        let sp = map.planes.get(s.planenum);
        let fp = map.planes.get(*planenum);
        if sp.normal.dot(fp.normal) < 0.999 || (sp.dist - fp.dist).abs() > 0.1 {
            continue;
        }
        if polygons_overlap(w, sw, sp.normal) {
            out.push(fi as u32);
        }
    }
    out
}

fn polygons_overlap(a: &Winding, b: &Winding, normal: Vec3) -> bool {
    let c = b.center();
    let mut cur = Some(a.clone());
    for i in 0..b.len() {
        let p0 = b.p[i];
        let p1 = b.p[(i + 1) % b.len()];
        let mut en = normal.cross(p1 - p0).normalize();
        if en.length() < 0.5 {
            continue;
        }
        if en.dot(c - p0) < 0.0 {
            en = -en;
        }
        let Some(w) = cur else { return false };
        cur = w.chop(en, en.dot(p0) + 0.1, 0.01);
    }
    cur.map(|w| w.area() > 1.0).unwrap_or(false)
}

pub fn build_lumps(
    overlays: &[Overlay],
    side_faces: &HashMap<i32, Vec<u32>>,
) -> (Vec<u8>, Vec<u8>) {
    write_overlays(overlays, side_faces, OVERLAY_BSP_FACE_COUNT, 0)
}

pub fn build_water_lump(overlays: &[Overlay], side_faces: &HashMap<i32, Vec<u32>>) -> Vec<u8> {
    write_overlays(
        overlays,
        side_faces,
        WATEROVERLAY_BSP_FACE_COUNT,
        MAX_MAP_OVERLAYS + 1,
    )
    .0
}

const WATEROVERLAY_BSP_FACE_COUNT: usize = 256;
const MAX_MAP_OVERLAYS: i32 = 512;

fn write_overlays(
    overlays: &[Overlay],
    side_faces: &HashMap<i32, Vec<u32>>,
    max_faces: usize,
    first_id: i32,
) -> (Vec<u8>, Vec<u8>) {
    let mut data = Vec::with_capacity(overlays.len() * OVERLAY_SIZE);
    let mut fades = Vec::with_capacity(overlays.len() * 8);
    let f = |d: &mut Vec<u8>, v: f32| d.extend_from_slice(&v.to_le_bytes());
    for (i, o) in overlays.iter().enumerate() {
        let mut faces: Vec<u32> = Vec::new();
        for s in &o.sides {
            if let Some(list) = side_faces.get(s) {
                for &fi in list {
                    if !faces.contains(&fi) {
                        faces.push(fi);
                    }
                }
            }
        }
        if faces.len() > max_faces {
            eprintln!(
                "Warning: overlay at {:?} touches {} faces, only {} are used",
                o.origin,
                faces.len(),
                max_faces
            );
            faces.truncate(max_faces);
        }
        data.extend_from_slice(&(first_id + i as i32).to_le_bytes());
        data.extend_from_slice(&(o.texinfo as i16).to_le_bytes());
        let fc = (faces.len() as u16 & 0x3fff) | (o.render_order << 14);
        data.extend_from_slice(&fc.to_le_bytes());
        for k in 0..max_faces {
            let v = faces.get(k).map(|&x| x as i32).unwrap_or(0);
            data.extend_from_slice(&v.to_le_bytes());
        }
        f(&mut data, o.u[0]);
        f(&mut data, o.u[1]);
        f(&mut data, o.v[0]);
        f(&mut data, o.v[1]);
        let flip = if o.normal.cross(o.basis_u).dot(o.basis_v) < 0.0 {
            1.0
        } else {
            0.0
        };
        let zs = [o.basis_u.x, o.basis_u.y, o.basis_u.z, flip];
        for k in 0..4 {
            f(&mut data, o.uv_points[k].x as f32);
            f(&mut data, o.uv_points[k].y as f32);
            f(&mut data, zs[k] as f32);
        }
        for v in o.origin.to_f32().into_iter().chain(o.normal.to_f32()) {
            f(&mut data, v);
        }
        let (mn, mx) = if o.fade_max > 0.0 {
            (o.fade_min.max(0.0), o.fade_max)
        } else {
            (0.0, 0.0)
        };
        f(&mut fades, mn * mn);
        f(&mut fades, mx * mx);
    }
    (data, fades)
}
