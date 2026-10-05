use crate::map::{Map, PlaneSet};
use crate::tree::Tree;
use gold_common::bsp::*;
use gold_common::fs::GameFs;
use gold_common::math::*;
use gold_common::mdl;

pub const STATIC_PROP_VERSION: u16 = 6;

pub const STATIC_PROP_FLAG_FADES: u8 = 0x1;
pub const STATIC_PROP_USE_LIGHTING_ORIGIN: u8 = 0x2;
pub const STATIC_PROP_IGNORE_NORMALS: u8 = 0x8;
pub const STATIC_PROP_NO_SHADOW: u8 = 0x10;
pub const STATIC_PROP_SCREEN_SPACE_FADE: u8 = 0x20;
pub const STATIC_PROP_NO_PER_VERTEX_LIGHTING: u8 = 0x40;
pub const STATIC_PROP_NO_SELF_SHADOWING: u8 = 0x80;

pub struct StaticProp {
    pub model: String,
    pub origin: Vec3,
    pub angles: Vec3,
    pub solid: u8,
    pub flags: u8,
    pub skin: i32,
    pub fade_min: f32,
    pub fade_max: f32,
    pub lighting_origin: Vec3,
    pub fade_scale: f32,
    pub bounds: Aabb,
}

fn entity_float(e: &crate::map::MapEntity, key: &str, def: f32) -> f32 {
    e.get(key)
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(def)
}

fn entity_bool(e: &crate::map::MapEntity, key: &str) -> bool {
    e.get(key)
        .map(|v| v.trim() != "0" && !v.trim().is_empty())
        .unwrap_or(false)
}

pub fn collect_static_props(map: &mut Map, fs: &GameFs) -> Vec<StaticProp> {
    let mut out = Vec::new();
    let names: Vec<(String, Vec3)> = map
        .entities
        .iter()
        .filter_map(|e| {
            Some((
                e.get("targetname")?.to_string(),
                e.get("origin").and_then(parse_vec3)?,
            ))
        })
        .collect();
    for ei in 0..map.entities.len() {
        let e = &map.entities[ei];
        if !e.classname().eq_ignore_ascii_case("prop_static") {
            continue;
        }
        let model = e.get("model").unwrap_or("").replace('\\', "/");
        let origin = e.get("origin").and_then(parse_vec3).unwrap_or_default();
        let angles = e.get("angles").and_then(parse_vec3).unwrap_or_default();
        let Some(info) = mdl::load_model(fs, &model) else {
            eprintln!("Warning: prop_static model \"{model}\" not found, prop removed");
            map.entities[ei].removed = true;
            continue;
        };
        if info.flags & 0x10 == 0 {
            eprintln!(
                "Error! prop_static using model \"{model}\", which must be used on a dynamic entity (i.e. prop_physics). Deleted."
            );
            map.entities[ei].removed = true;
            continue;
        }
        let mb = info.bounds();
        let m = angle_matrix(angles);
        let mut bounds = Aabb::EMPTY;
        for k in 0..8 {
            let c = v3(
                if k & 1 != 0 { mb.maxs.x } else { mb.mins.x },
                if k & 2 != 0 { mb.maxs.y } else { mb.mins.y },
                if k & 4 != 0 { mb.maxs.z } else { mb.mins.z },
            );
            bounds.add_point(origin + mat_mul_vec(&m, c));
        }
        let mut flags = 0u8;
        let fade_min = entity_float(e, "fademindist", -1.0);
        let fade_max = entity_float(e, "fademaxdist", 0.0);
        if fade_max > 0.0 && fade_min >= 0.0 {
            flags |= STATIC_PROP_FLAG_FADES;
        }
        if entity_bool(e, "disableshadows") {
            flags |= STATIC_PROP_NO_SHADOW;
        }
        if entity_bool(e, "screenspacefade") {
            flags |= STATIC_PROP_SCREEN_SPACE_FADE;
        }
        if entity_bool(e, "ignorenormals") {
            flags |= STATIC_PROP_IGNORE_NORMALS;
        }
        if entity_bool(e, "disablevertexlighting") {
            flags |= STATIC_PROP_NO_PER_VERTEX_LIGHTING;
        }
        if entity_bool(e, "disableselfshadowing") {
            flags |= STATIC_PROP_NO_SELF_SHADOWING;
        }
        let mut lighting_origin = bounds.center();
        if let Some(lo) = e.get("lightingorigin").filter(|s| !s.is_empty())
            && let Some((_, o)) = names.iter().find(|(n, _)| n.eq_ignore_ascii_case(lo))
        {
            lighting_origin = *o;
            flags |= STATIC_PROP_USE_LIGHTING_ORIGIN;
        }
        out.push(StaticProp {
            model,
            origin,
            angles,
            solid: e
                .get("solid")
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(6),
            flags,
            skin: e
                .get("skin")
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(0),
            fade_min,
            fade_max,
            lighting_origin,
            fade_scale: entity_float(e, "fadescale", 1.0),
            bounds,
        });
        map.entities[ei].removed = true;
    }
    out
}

fn box_leafs(
    tree: &Tree,
    planes: &PlaneSet,
    b: &Aabb,
    leaf_index: &dyn Fn(usize) -> i32,
) -> Vec<u16> {
    let mut out = Vec::new();
    let mut stack = vec![tree.head];
    while let Some(n) = stack.pop() {
        match tree.nodes[n].planenum {
            None => {
                if tree.nodes[n].contents & CONTENTS_SOLID == 0 {
                    let li = leaf_index(n);
                    if li >= 0 {
                        out.push(li as u16);
                    }
                }
            }
            Some(pn) => {
                let pl = planes.get(pn);
                let s = crate::tree::box_on_plane_side(b.mins, b.maxs, pl, planes.types[pn]);
                let [c0, c1] = tree.nodes[n].children;
                if s & crate::tree::PSIDE_FRONT != 0 {
                    stack.push(c0);
                }
                if s & crate::tree::PSIDE_BACK != 0 {
                    stack.push(c1);
                }
            }
        }
    }
    out.sort();
    out
}

pub fn build_static_prop_lump(
    props: &[StaticProp],
    tree: &Tree,
    planes: &PlaneSet,
    leaf_index: &dyn Fn(usize) -> i32,
) -> Vec<u8> {
    let mut dict: Vec<String> = Vec::new();
    for p in props {
        if !dict.iter().any(|d| d.eq_ignore_ascii_case(&p.model)) {
            dict.push(p.model.clone());
        }
    }
    let mut out = Vec::new();
    out.extend_from_slice(&(dict.len() as i32).to_le_bytes());
    for d in &dict {
        let mut name = [0u8; 128];
        let b = d.as_bytes();
        name[..b.len().min(127)].copy_from_slice(&b[..b.len().min(127)]);
        out.extend_from_slice(&name);
    }
    let mut leaves: Vec<u16> = Vec::new();
    let mut ranges = Vec::new();
    for p in props {
        let l = box_leafs(tree, planes, &p.bounds, leaf_index);
        ranges.push((leaves.len() as u16, l.len() as u16));
        leaves.extend(l);
    }
    out.extend_from_slice(&(leaves.len() as i32).to_le_bytes());
    for l in &leaves {
        out.extend_from_slice(&l.to_le_bytes());
    }
    out.extend_from_slice(&(props.len() as i32).to_le_bytes());
    let f = |out: &mut Vec<u8>, v: f32| out.extend_from_slice(&v.to_le_bytes());
    for (p, (first, count)) in props.iter().zip(ranges) {
        for v in p.origin.to_f32() {
            f(&mut out, v);
        }
        for v in p.angles.to_f32() {
            f(&mut out, v);
        }
        let ty = dict
            .iter()
            .position(|d| d.eq_ignore_ascii_case(&p.model))
            .unwrap() as u16;
        out.extend_from_slice(&ty.to_le_bytes());
        out.extend_from_slice(&first.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.push(p.solid);
        out.push(p.flags);
        out.extend_from_slice(&p.skin.to_le_bytes());
        f(&mut out, p.fade_min);
        f(&mut out, p.fade_max);
        for v in p.lighting_origin.to_f32() {
            f(&mut out, v);
        }
        f(&mut out, p.fade_scale);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    out
}
