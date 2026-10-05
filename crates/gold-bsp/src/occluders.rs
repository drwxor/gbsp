use crate::emit::Output;
use crate::map::Map;
use gold_common::math::*;
use gold_common::winding::Winding;

pub const OCCLUDER_FLAGS_INACTIVE: i32 = 0x1;

pub struct Occluder {
    pub flags: i32,
    pub polys: Vec<(Winding, usize)>,
    pub bounds: Aabb,
}

fn is_occluder_material(name: &str) -> bool {
    name.to_ascii_lowercase()
        .replace('\\', "/")
        .ends_with("tools/toolsoccluder")
}

pub fn collect(map: &mut Map) -> Vec<Occluder> {
    let mut out = Vec::new();
    for ei in 0..map.entities.len() {
        if !map.entities[ei]
            .classname()
            .eq_ignore_ascii_case("func_occluder")
        {
            continue;
        }
        let brushes = std::mem::take(&mut map.entities[ei].brushes);
        let mut occ = Occluder {
            flags: 0,
            polys: Vec::new(),
            bounds: Aabb::EMPTY,
        };
        if map.entities[ei]
            .get("StartActive")
            .map(|v| v.trim() == "0")
            .unwrap_or(false)
        {
            occ.flags |= OCCLUDER_FLAGS_INACTIVE;
        }
        for b in brushes {
            let mb = &map.brushes[b];
            occ.bounds.add_box(&mb.bounds);
            for s in &mb.sides {
                if s.bevel || !is_occluder_material(&map.texdata_names[s.material]) {
                    continue;
                }
                if let Some(w) = &s.winding {
                    occ.polys.push((w.remove_colinear(0.01), s.planenum));
                }
            }
        }
        let e = &mut map.entities[ei];
        e.set("occludernumber", out.len().to_string());
        e.set("model", String::new());
        if occ.polys.is_empty() {
            eprintln!(
                "Warning: func_occluder {} has no sides textured tools/toolsoccluder",
                e.get("hammerid").unwrap_or("")
            );
        }
        out.push(occ);
    }
    out
}

pub fn build_lump(
    occluders: &[Occluder],
    out: &mut Output,
    area_of: &dyn Fn(Vec3) -> i32,
) -> Vec<u8> {
    let mut data = Vec::new();
    let mut polys = Vec::new();
    let mut indices: Vec<i32> = Vec::new();
    let i = |d: &mut Vec<u8>, v: i32| d.extend_from_slice(&v.to_le_bytes());
    let f = |d: &mut Vec<u8>, v: f32| d.extend_from_slice(&v.to_le_bytes());
    i(&mut data, occluders.len() as i32);
    for o in occluders {
        i(&mut data, o.flags);
        i(&mut data, (polys.len() / 3) as i32);
        i(&mut data, o.polys.len() as i32);
        let b = if o.bounds.is_empty() {
            Aabb::new(Vec3::ZERO, Vec3::ZERO)
        } else {
            o.bounds
        };
        for v in b.mins.to_f32().into_iter().chain(b.maxs.to_f32()) {
            f(&mut data, v);
        }
        i(&mut data, area_of(b.center()));
        for (w, planenum) in &o.polys {
            polys.push(indices.len() as i32);
            polys.push(w.len() as i32);
            polys.push(*planenum as i32);
            for p in &w.p {
                indices.push(out.vertex(*p) as i32);
            }
        }
    }
    i(&mut data, (polys.len() / 3) as i32);
    for v in polys {
        i(&mut data, v);
    }
    i(&mut data, indices.len() as i32);
    for v in indices {
        i(&mut data, v);
    }
    data
}
