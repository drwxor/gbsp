use crate::map::Map;
use gold_common::bsp::*;
use gold_common::phys::{self, ConvexHull, PhysModel};

const MASK_SOLID: i32 =
    CONTENTS_SOLID | CONTENTS_MOVEABLE | CONTENTS_WINDOW | CONTENTS_MONSTER | CONTENTS_GRATE;

struct MaterialTable {
    names: Vec<String>,
}

impl MaterialTable {
    fn index(&mut self, name: &str) -> u8 {
        let n = name.to_ascii_lowercase();
        if let Some(i) = self.names.iter().position(|x| *x == n) {
            return (i + 1) as u8;
        }
        self.names.push(n);
        self.names.len() as u8
    }
    fn keydata(&self) -> String {
        let mut s = String::from("materialtable {\n");
        for (i, n) in self.names.iter().enumerate() {
            s.push_str(&format!("\"{}\" \"{}\"\n", n, i + 1));
        }
        s.push_str("}\n");
        s
    }
}

fn brush_hull(map: &Map, bi: usize, mats: &mut MaterialTable) -> Option<ConvexHull> {
    let b = &map.brushes[bi];
    let mut polys = Vec::new();
    for s in &b.sides {
        if s.bevel {
            continue;
        }
        let Some(w) = &s.winding else { continue };
        let w = w.remove_colinear(0.01);
        if w.len() < 3 {
            continue;
        }
        let surfprop = map
            .materials
            .get(s.material)
            .map(|m| m.surfaceprop().to_string())
            .unwrap_or_else(|| "default".into());
        let mi = mats.index(&surfprop);
        let mut p = w.p.clone();
        p.reverse();
        polys.push((p, mi));
    }
    ConvexHull::from_polygons(&polys, 0.05)
}

pub fn build_physcollide(
    map: &Map,
    models: &[(usize, Vec<usize>)],
    has_world_disps: bool,
) -> Vec<u8> {
    let mut out = Vec::new();
    for (mi, brushes) in models {
        let mut mats = MaterialTable { names: Vec::new() };
        let mut keydata = String::new();
        let mut solids = Vec::new();
        let groups: &[(i32, i32)] = if *mi == 0 {
            &[
                (MASK_SOLID, MASK_SOLID),
                (CONTENTS_PLAYERCLIP, CONTENTS_PLAYERCLIP),
                (CONTENTS_MONSTERCLIP, CONTENTS_MONSTERCLIP),
            ]
        } else {
            &[(
                MASK_SOLID | CONTENTS_PLAYERCLIP | CONTENTS_MONSTERCLIP,
                MASK_SOLID,
            )]
        };
        let mut used = std::collections::HashSet::new();
        for &(select, contents_out) in groups {
            let mut hulls = Vec::new();
            for &bi in brushes {
                let b = &map.brushes[bi];
                if b.is_disp || used.contains(&bi) || b.contents & select == 0 {
                    continue;
                }
                if b.contents & (MASK_SOLID | CONTENTS_PLAYERCLIP | CONTENTS_MONSTERCLIP) == 0 {
                    continue;
                }
                used.insert(bi);
                match brush_hull(map, bi, &mut mats) {
                    Some(h) => hulls.push(h),
                    None => eprintln!("Warning: could not build collision hull for brush {}", b.id),
                }
            }
            if hulls.is_empty() {
                continue;
            }
            let surface = phys::build_compact_surface(&hulls);
            let index = solids.len();
            solids.push(phys::wrap_collide(&surface));
            if *mi == 0 {
                keydata.push_str(&format!(
                    "staticsolid {{\n\"index\" \"{index}\"\n\"contents\" \"{contents_out}\"\n}}\n"
                ));
            } else {
                keydata.push_str(&format!(
                    "solid {{\n\"index\" \"{index}\"\n\"mass\" \"1.000000\"\n\"surfaceprop\" \"default\"\n\"damping\" \"0.000000\"\n\"rotdamping\" \"0.000000\"\n\"inertia\" \"1.000000\"\n\"volume\" \"1.000000\"\n}}\n"
                ));
            }
        }
        if *mi == 0 && has_world_disps {
            keydata.push_str("virtualterrain {}\n");
        }
        if solids.is_empty() && !(*mi == 0 && has_world_disps) {
            continue;
        }
        keydata.push_str(&mats.keydata());
        out.push(PhysModel {
            model_index: *mi as i32,
            solids,
            keydata,
        });
    }
    phys::build_physcollide_lump(&out)
}
