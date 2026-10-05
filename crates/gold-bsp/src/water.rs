use crate::faces::FaceSet;
use crate::map::Map;
use crate::portals::Portals;
use crate::tree::Tree;
use gold_common::bsp::*;
use gold_common::fs::GameFs;
use gold_common::material::MaterialCache;
use std::collections::HashMap;

pub fn apply_bottom_materials(
    fs: &mut FaceSet,
    map: &mut Map,
    gfs: &GameFs,
    mats: &MaterialCache,
) -> usize {
    let mut cache: HashMap<i32, Option<i32>> = HashMap::new();
    let mut count = 0;
    for fi in 0..fs.faces.len() {
        let f = &fs.faces[fi];
        if !f.is_final() || f.texinfo < 0 || f.contents & (CONTENTS_WATER | CONTENTS_SLIME) == 0 {
            continue;
        }
        let ti = f.texinfo;
        let nti = *cache.entry(ti).or_insert_with(|| {
            let td = map.texinfos[ti as usize].texdata as usize;
            let bottom = map.materials[td].param("$bottommaterial")?.to_string();
            let mat = mats.get(gfs, &bottom);
            if !mat.found {
                eprintln!("Warning: water bottom material \"{bottom}\" not found");
            }
            let ntd = map.texdata_for(&bottom, &mat);
            let mut t = map.texinfos[ti as usize];
            t.texdata = ntd;
            t.flags = (t.flags & !(SURF_BUMPLIGHT)) | mat.surf_flags;
            Some(map.add_texinfo(t))
        });
        if let Some(n) = nti {
            fs.faces[fi].texinfo = n;
            count += 1;
        }
    }
    count
}

pub fn water_volumes(tree: &mut Tree, map: &Map) -> Vec<DLeafWaterData> {
    let mut out: Vec<DLeafWaterData> = Vec::new();
    let mut keys: Vec<(i32, i32)> = Vec::new();
    for l in tree.leafs() {
        let n = &tree.nodes[l];
        if n.contents & (CONTENTS_WATER | CONTENTS_SLIME) == 0 || n.contents & CONTENTS_SOLID != 0 {
            continue;
        }
        let mut top = f64::MIN;
        let mut bottom = f64::MAX;
        let mut texinfo = -1;
        for b in &n.brushes {
            let mb = &map.brushes[b.original];
            if mb.contents & (CONTENTS_WATER | CONTENTS_SLIME) == 0 {
                continue;
            }
            top = top.max(mb.bounds.maxs.z);
            bottom = bottom.min(mb.bounds.mins.z);
            for s in &mb.sides {
                if s.bevel || s.contents & (CONTENTS_WATER | CONTENTS_SLIME) == 0 {
                    continue;
                }
                if map.planes.get(s.planenum).normal.z > 0.7 {
                    texinfo = s.texinfo;
                }
            }
            if texinfo < 0 {
                texinfo = mb
                    .sides
                    .iter()
                    .find(|s| s.contents & (CONTENTS_WATER | CONTENTS_SLIME) != 0)
                    .map(|s| s.texinfo)
                    .unwrap_or(-1);
            }
        }
        if top == f64::MIN {
            continue;
        }
        let key = (top.round() as i32, texinfo);
        let id = match keys.iter().position(|k| *k == key) {
            Some(i) => {
                out[i].min_z = out[i].min_z.min(bottom as f32);
                i
            }
            None => {
                keys.push(key);
                out.push(DLeafWaterData {
                    surface_z: top as f32,
                    min_z: bottom as f32,
                    surface_texinfo_id: texinfo as i16,
                    pad: 0,
                });
                out.len() - 1
            }
        };
        tree.nodes[l].water_id = id as i32;
    }
    out
}

pub fn assign_fog_volumes(tree: &Tree, portals: &Portals, fs: &mut FaceSet) {
    for l in tree.leafs() {
        let n = &tree.nodes[l];
        if n.water_id < 0 {
            continue;
        }
        let mut faces = Vec::new();
        for &p in &n.portals {
            let port = &portals.list[p];
            let s = if port.nodes[1] == l { 1 } else { 0 };
            if let Some(f) = port.face[s] {
                fs.resolve(f, &mut faces);
            }
        }
        for &f in &n.leaf_faces {
            fs.resolve(f, &mut faces);
        }
        for f in faces {
            fs.faces[f].fog_volume = n.water_id;
        }
    }
}
