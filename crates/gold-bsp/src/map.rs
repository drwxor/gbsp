use gold_common::bsp::*;
use gold_common::fs::GameFs;
use gold_common::material::{Material, MaterialCache};
use gold_common::math::*;
use gold_common::vmf::{Vmf, VmfDisp, VmfEntity, VmfSide};
use gold_common::winding::Winding;
use std::collections::HashMap;

pub const NORMAL_EPSILON: f64 = 0.00001;
pub const DIST_EPSILON: f64 = 0.01;

#[derive(Default)]
pub struct PlaneSet {
    pub planes: Vec<Plane>,
    pub types: Vec<i32>,
    hash: HashMap<i64, Vec<usize>>,
}

fn hash_key(dist: f64) -> i64 {
    (dist.abs() / 8.0).floor() as i64
}

impl PlaneSet {
    fn plane_equal(p: &Plane, normal: Vec3, dist: f64) -> bool {
        (p.normal.x - normal.x).abs() < NORMAL_EPSILON
            && (p.normal.y - normal.y).abs() < NORMAL_EPSILON
            && (p.normal.z - normal.z).abs() < NORMAL_EPSILON
            && (p.dist - dist).abs() < DIST_EPSILON
    }

    fn snap(normal: &mut Vec3, dist: &mut f64) {
        for i in 0..3 {
            if (normal[i] - 1.0).abs() < NORMAL_EPSILON {
                *normal = Vec3::ZERO;
                normal[i] = 1.0;
                break;
            }
            if (normal[i] + 1.0).abs() < NORMAL_EPSILON {
                *normal = Vec3::ZERO;
                normal[i] = -1.0;
                break;
            }
        }
        if (*dist - dist.round()).abs() < DIST_EPSILON {
            *dist = dist.round();
        }
    }

    pub fn find(&mut self, normal: Vec3, dist: f64) -> usize {
        let mut normal = normal;
        let mut dist = dist;
        Self::snap(&mut normal, &mut dist);
        let key = hash_key(dist);
        for k in [key - 1, key, key + 1] {
            if let Some(list) = self.hash.get(&k) {
                for &i in list {
                    if Self::plane_equal(&self.planes[i], normal, dist) {
                        return i;
                    }
                }
            }
        }
        let p = Plane::new(normal, dist);
        let flip = p.flip();
        let n = self.planes.len();
        let positive_first =
            !(p.plane_type() < 3 && (normal.x < 0.0 || normal.y < 0.0 || normal.z < 0.0));
        let (a, b) = if positive_first { (p, flip) } else { (flip, p) };
        self.planes.push(a);
        self.types.push(a.plane_type());
        self.planes.push(b);
        self.types.push(b.plane_type());
        self.hash.entry(hash_key(a.dist)).or_default().push(n);
        self.hash.entry(hash_key(b.dist)).or_default().push(n + 1);
        if positive_first { n } else { n + 1 }
    }

    pub fn get(&self, n: usize) -> &Plane {
        &self.planes[n]
    }
}

#[derive(Clone, Debug)]
pub struct MapSide {
    pub planenum: usize,
    pub texinfo: i32,
    pub contents: i32,
    pub surf_flags: i32,
    pub winding: Option<Winding>,
    pub visible: bool,
    pub bevel: bool,
    pub tested: bool,
    pub disp: Option<usize>,
    pub side_id: i32,
    pub smoothing_groups: u32,
    pub material: usize,
}

#[derive(Clone, Debug)]
pub struct MapBrush {
    pub entity: usize,
    pub id: i32,
    pub contents: i32,
    pub sides: Vec<MapSide>,
    pub bounds: Aabb,
    pub is_disp: bool,
    pub areaportal: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct MapDisp {
    pub brush: usize,
    pub side: usize,
    pub info: VmfDisp,
    pub entity: usize,
}

#[derive(Clone, Debug)]
pub struct MapEntity {
    pub pairs: Vec<(String, String)>,
    pub brushes: Vec<usize>,
    pub origin: Vec3,
    pub removed: bool,
    pub overlay_transitions: Vec<Vec<(String, String)>>,
}

impl MapEntity {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.pairs
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }
    pub fn classname(&self) -> &str {
        self.get("classname").unwrap_or("")
    }
    pub fn set(&mut self, key: &str, value: String) {
        if let Some(p) = self
            .pairs
            .iter_mut()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
        {
            p.1 = value;
        } else {
            self.pairs.push((key.to_string(), value));
        }
    }
}

pub struct Map {
    pub planes: PlaneSet,
    pub brushes: Vec<MapBrush>,
    pub entities: Vec<MapEntity>,
    pub disps: Vec<MapDisp>,
    pub texinfos: Vec<TexInfo>,
    pub texdata: Vec<DTexData>,
    pub texdata_names: Vec<String>,
    pub materials: Vec<Material>,
    texinfo_map: HashMap<Vec<u32>, i32>,
    texdata_map: HashMap<String, i32>,
    pub map_name: String,
    pub map_revision: i32,
    pub wvt_patches: Vec<(String, String)>,
}

pub fn texinfo_key(t: &TexInfo) -> Vec<u32> {
    let mut k = Vec::with_capacity(18);
    for a in t.texture_vecs.iter().chain(t.lightmap_vecs.iter()) {
        for f in a {
            k.push(f.to_bits());
        }
    }
    k.push(t.flags as u32);
    k.push(t.texdata as u32);
    k
}

impl Map {
    pub fn new(map_name: &str) -> Map {
        Map {
            planes: PlaneSet::default(),
            brushes: Vec::new(),
            entities: Vec::new(),
            disps: Vec::new(),
            texinfos: Vec::new(),
            texdata: Vec::new(),
            texdata_names: Vec::new(),
            materials: Vec::new(),
            texinfo_map: HashMap::new(),
            texdata_map: HashMap::new(),
            map_name: map_name.to_string(),
            map_revision: 0,
            wvt_patches: Vec::new(),
        }
    }

    pub fn texdata_for(&mut self, name: &str, mat: &Material) -> i32 {
        let key = name.to_ascii_lowercase();
        if let Some(&i) = self.texdata_map.get(&key) {
            return i;
        }
        let i = self.texdata.len() as i32;
        self.texdata.push(DTexData {
            reflectivity: mat.reflectivity,
            name_string_table_id: i,
            width: mat.width,
            height: mat.height,
            view_width: mat.width,
            view_height: mat.height,
        });
        self.texdata_names.push(name.to_string());
        self.materials.push(mat.clone());
        self.texdata_map.insert(key, i);
        i
    }

    pub fn add_texinfo(&mut self, t: TexInfo) -> i32 {
        let key = texinfo_key(&t);
        if let Some(&i) = self.texinfo_map.get(&key) {
            return i;
        }
        let i = self.texinfos.len() as i32;
        self.texinfos.push(t);
        self.texinfo_map.insert(key, i);
        i
    }

    pub fn material_for_texinfo(&self, ti: i32) -> Option<&Material> {
        if ti < 0 {
            return None;
        }
        self.materials
            .get(self.texinfos[ti as usize].texdata as usize)
    }
}

fn texinfo_for_side(side: &VmfSide, origin: Vec3, texdata: i32, flags: i32) -> TexInfo {
    let mut t = TexInfo::default();
    let lscale = if side.lightmap_scale <= 0.0 {
        16.0
    } else {
        side.lightmap_scale
    };
    let axes = [&side.uaxis, &side.vaxis];
    for (i, ax) in axes.iter().enumerate() {
        let tv = ax.axis / ax.scale;
        let lv = ax.axis / lscale;
        t.texture_vecs[i] = [
            tv.x as f32,
            tv.y as f32,
            tv.z as f32,
            (ax.shift + origin.dot(tv)) as f32,
        ];
        t.lightmap_vecs[i] = [
            lv.x as f32,
            lv.y as f32,
            lv.z as f32,
            (ax.shift * ax.scale / lscale + origin.dot(lv)) as f32,
        ];
    }
    t.flags = flags;
    t.texdata = texdata;
    t
}

pub fn make_brush_windings(planes: &PlaneSet, b: &mut MapBrush) -> bool {
    let mut bounds = Aabb::EMPTY;
    let n = b.sides.len();
    for i in 0..n {
        if b.sides[i].bevel {
            continue;
        }
        let p = *planes.get(b.sides[i].planenum);
        let mut w = Some(Winding::base_for_plane(&p));
        for j in 0..n {
            if i == j || b.sides[j].bevel {
                continue;
            }
            if b.sides[j].planenum == (b.sides[i].planenum ^ 1) {
                continue;
            }
            if b.sides[j].planenum == b.sides[i].planenum {
                continue;
            }
            let pj = planes.get(b.sides[j].planenum ^ 1);
            if let Some(ww) = w {
                w = ww.chop(pj.normal, pj.dist, 0.0);
            }
        }
        if let Some(w) = &w {
            for pt in &w.p {
                bounds.add_point(*pt);
            }
        }
        b.sides[i].winding = w;
    }
    b.bounds = bounds;
    if bounds.is_empty() {
        return false;
    }
    for i in 0..3 {
        if bounds.mins[i] < -MAX_MAP_COORD || bounds.maxs[i] > MAX_MAP_COORD {
            return false;
        }
    }
    true
}

pub const MAX_MAP_COORD: f64 = 16384.0;

pub fn add_brush_bevels(planes: &mut PlaneSet, b: &mut MapBrush) {
    for axis in 0..3 {
        for dir in [-1.0f64, 1.0] {
            let mut normal = Vec3::ZERO;
            normal[axis] = dir;
            let exists = b.sides.iter().any(|s| {
                let p = planes.get(s.planenum);
                p.normal == normal
            });
            if !exists {
                let dist = if dir > 0.0 {
                    b.bounds.maxs[axis]
                } else {
                    -b.bounds.mins[axis]
                };
                let pn = planes.find(normal, dist);
                let texinfo = b.sides[0].texinfo;
                b.sides.push(MapSide {
                    planenum: pn,
                    texinfo,
                    contents: b.sides[0].contents,
                    surf_flags: 0,
                    winding: None,
                    visible: false,
                    bevel: true,
                    tested: false,
                    disp: None,
                    side_id: 0,
                    smoothing_groups: 0,
                    material: b.sides[0].material,
                });
            }
        }
    }
    if b.sides
        .iter()
        .filter(|s| !s.bevel)
        .all(|s| planes.get(s.planenum).plane_type() < 3)
    {
        return;
    }
    let nsides = b.sides.len();
    for i in 0..nsides {
        let Some(w) = b.sides[i].winding.clone() else {
            continue;
        };
        let n = w.p.len();
        for j in 0..n {
            let k = (j + 1) % n;
            let vec = w.p[j] - w.p[k];
            if vec.length() < 0.5 {
                continue;
            }
            let vec = vec.normalize();
            let mut vec = vec;
            for kk in 0..3 {
                if (vec[kk] - 1.0).abs() < 1e-3 {
                    vec = Vec3::ZERO;
                    vec[kk] = 1.0;
                } else if (vec[kk] + 1.0).abs() < 1e-3 {
                    vec = Vec3::ZERO;
                    vec[kk] = -1.0;
                }
            }
            for axis in 0..3 {
                for dir in [-1.0f64, 1.0] {
                    let mut vec2 = Vec3::ZERO;
                    vec2[axis] = dir;
                    let normal = vec.cross(vec2);
                    if normal.length() < 0.5 {
                        continue;
                    }
                    let normal = normal.normalize();
                    let dist = w.p[j].dot(normal);
                    let mut ok = true;
                    'outer: for s in &b.sides {
                        if let Some(w2) = &s.winding {
                            for p in &w2.p {
                                if p.dot(normal) - dist > 0.1 {
                                    ok = false;
                                    break 'outer;
                                }
                            }
                        }
                    }
                    if !ok {
                        continue;
                    }
                    let pn = planes.find(normal, dist);
                    if b.sides.iter().any(|s| s.planenum == pn) {
                        continue;
                    }
                    let texinfo = b.sides[0].texinfo;
                    b.sides.push(MapSide {
                        planenum: pn,
                        texinfo,
                        contents: b.sides[0].contents,
                        surf_flags: 0,
                        winding: None,
                        visible: false,
                        bevel: true,
                        tested: false,
                        disp: None,
                        side_id: 0,
                        smoothing_groups: 0,
                        material: b.sides[0].material,
                    });
                }
            }
        }
    }
}

fn brush_contents(sides: &[MapSide], texinfos: &[TexInfo]) -> i32 {
    let mut contents = sides[0].contents & !CONTENTS_LADDER;
    let mut union = 0;
    let mut trans = 0;
    for s in sides {
        union |= s.contents;
        trans |= texinfos[s.texinfo as usize].flags;
    }
    if union & CONTENTS_LADDER != 0 {
        contents |= CONTENTS_LADDER;
    }
    let liquid = union & (CONTENTS_WATER | CONTENTS_SLIME);
    if liquid != 0 {
        contents |= liquid | CONTENTS_TRANSLUCENT;
        contents &= !CONTENTS_SOLID;
    }
    if trans & (SURF_TRANS | SURF_WARP) != 0 && contents & CONTENTS_SOLID != 0 {
        contents &= !CONTENTS_SOLID;
        contents |= CONTENTS_WINDOW | CONTENTS_TRANSLUCENT;
    }
    if contents & ALL_VISIBLE_CONTENTS == 0 && contents & CONTENTS_AREAPORTAL == 0 && contents != 0
    {
        contents |= CONTENTS_DETAIL;
    }
    contents
}

pub struct LoadOptions<'a> {
    pub fs: &'a GameFs,
    pub mats: &'a MaterialCache,
}

pub fn load_map(vmf: &Vmf, map_name: &str, opts: &LoadOptions) -> Map {
    let mut map = Map::new(map_name);
    map.map_revision = vmf.map_version;

    for (ei, ve) in vmf.entities.iter().enumerate() {
        let mut ent = MapEntity {
            pairs: Vec::new(),
            brushes: Vec::new(),
            origin: Vec3::ZERO,
            removed: false,
            overlay_transitions: ve.overlay_transitions.clone(),
        };
        for (k, v) in &ve.pairs {
            if k.eq_ignore_ascii_case("id") {
                ent.pairs.push(("hammerid".into(), v.clone()));
            } else {
                ent.pairs.push((k.clone(), v.clone()));
            }
        }
        for (k, v) in &ve.connections {
            ent.pairs.push((k.clone(), v.clone()));
        }
        ent.origin = ve.get("origin").and_then(parse_vec3).unwrap_or_default();
        let entity_index = map.entities.len();
        map.entities.push(ent);
        load_entity_brushes(&mut map, ve, entity_index, ei == 0, opts);
    }

    let mut portal_number = 0;
    for ei in 1..map.entities.len() {
        let class = map.entities[ei].classname().to_ascii_lowercase();
        match class.as_str() {
            "func_detail" => {
                let bl = std::mem::take(&mut map.entities[ei].brushes);
                for &b in &bl {
                    map.brushes[b].contents |= CONTENTS_DETAIL;
                    map.brushes[b].entity = 0;
                }
                for d in map.disps.iter_mut().filter(|d| d.entity == ei) {
                    d.entity = 0;
                }
                map.entities[0].brushes.extend(bl);
                map.entities[ei].removed = true;
            }
            "func_areaportal" | "func_areaportalwindow" => {
                portal_number += 1;
                let bl = std::mem::take(&mut map.entities[ei].brushes);
                for &b in &bl {
                    map.brushes[b].contents = CONTENTS_AREAPORTAL;
                    map.brushes[b].entity = 0;
                    map.brushes[b].areaportal = Some(ei);
                }
                map.entities[0].brushes.extend(bl);
                map.entities[ei].set("portalnumber", portal_number.to_string());
            }
            "func_ladder" => {
                let bl = std::mem::take(&mut map.entities[ei].brushes);
                let mut b = Aabb::EMPTY;
                for &bi in &bl {
                    map.brushes[bi].contents |= CONTENTS_LADDER | CONTENTS_DETAIL;
                    map.brushes[bi].entity = 0;
                    b.add_box(&map.brushes[bi].bounds);
                }
                map.entities[0].brushes.extend(bl);
                if !b.is_empty() {
                    let id = map.entities[ei].get("hammerid").unwrap_or("0").to_string();
                    let e = &mut map.entities[ei];
                    e.pairs = vec![
                        ("classname".into(), "info_ladder".into()),
                        ("mins.x".into(), format!("{:.2}", b.mins.x)),
                        ("mins.y".into(), format!("{:.2}", b.mins.y)),
                        ("mins.z".into(), format!("{:.2}", b.mins.z)),
                        ("maxs.x".into(), format!("{:.2}", b.maxs.x)),
                        ("maxs.y".into(), format!("{:.2}", b.maxs.y)),
                        ("maxs.z".into(), format!("{:.2}", b.maxs.z)),
                        ("hammerid".into(), id),
                    ];
                } else {
                    map.entities[ei].removed = true;
                }
            }
            _ => {}
        }
    }
    map
}

fn load_entity_brushes(
    map: &mut Map,
    ve: &VmfEntity,
    entity_index: usize,
    is_world: bool,
    opts: &LoadOptions,
) {
    let mut origin = Vec3::ZERO;
    let mut have_origin_brush = false;
    let class = ve.classname().to_ascii_lowercase();
    let merge_to_world = is_world || class == "func_detail" || class == "func_ladder";
    let mut solids = Vec::new();
    for solid in &ve.solids {
        let is_origin = solid
            .sides
            .iter()
            .all(|s| opts.mats.get(opts.fs, &s.material).contents & CONTENTS_ORIGIN != 0);
        if is_origin && !merge_to_world {
            let mut b = Aabb::EMPTY;
            for s in &solid.sides {
                for p in &s.points {
                    b.add_point(*p);
                }
            }
            let mut tmp_planes = PlaneSet::default();
            let mut mb = MapBrush {
                entity: entity_index,
                id: solid.id,
                contents: 0,
                sides: Vec::new(),
                bounds: Aabb::EMPTY,
                is_disp: false,
                areaportal: None,
            };
            for s in &solid.sides {
                if let Some(pl) = Plane::from_points(s.points[0], s.points[1], s.points[2]) {
                    let pn = tmp_planes.find(pl.normal, pl.dist);
                    mb.sides.push(MapSide {
                        planenum: pn,
                        texinfo: 0,
                        contents: 0,
                        surf_flags: 0,
                        winding: None,
                        visible: false,
                        bevel: false,
                        tested: false,
                        disp: None,
                        side_id: s.id,
                        smoothing_groups: 0,
                        material: 0,
                    });
                }
            }
            if make_brush_windings(&tmp_planes, &mut mb) {
                b = mb.bounds;
            }
            origin = b.center();
            have_origin_brush = true;
        } else {
            solids.push(solid);
        }
    }
    if have_origin_brush {
        map.entities[entity_index].origin = origin;
        map.entities[entity_index].set("origin", format!("{} {} {}", origin.x, origin.y, origin.z));
    }
    let tex_origin = if have_origin_brush {
        origin
    } else {
        Vec3::ZERO
    };

    for solid in solids {
        let mut mb = MapBrush {
            entity: entity_index,
            id: solid.id,
            contents: 0,
            sides: Vec::new(),
            bounds: Aabb::EMPTY,
            is_disp: false,
            areaportal: None,
        };
        let mut disp_sides = Vec::new();
        for (si, s) in solid.sides.iter().enumerate() {
            let pts = [
                s.points[0] - tex_origin,
                s.points[1] - tex_origin,
                s.points[2] - tex_origin,
            ];
            let Some(pl) = Plane::from_points(pts[0], pts[1], pts[2]) else {
                eprintln!(
                    "Warning: brush {} side {} has degenerate plane",
                    solid.id, s.id
                );
                continue;
            };
            let pn = map.planes.find(pl.normal, pl.dist);
            if mb.sides.iter().any(|o| o.planenum == pn) {
                eprintln!("Warning: brush {} has duplicate plane", solid.id);
                continue;
            }
            if mb.sides.iter().any(|o| o.planenum == (pn ^ 1)) {
                eprintln!("Warning: brush {} has mirrored plane", solid.id);
                continue;
            }
            let mat = opts.mats.get(opts.fs, &s.material);
            if !mat.found {
                eprintln!("Warning: material \"{}\" not found", s.material);
            }
            let texdata = map.texdata_for(&s.material, &mat);
            let ti = texinfo_for_side(s, tex_origin, texdata, mat.surf_flags);
            let texinfo = map.add_texinfo(ti);
            if s.disp.is_some() {
                disp_sides.push((mb.sides.len(), si));
            }
            mb.sides.push(MapSide {
                planenum: pn,
                texinfo,
                contents: mat.contents,
                surf_flags: mat.surf_flags,
                winding: None,
                visible: true,
                bevel: false,
                tested: false,
                disp: None,
                side_id: s.id,
                smoothing_groups: s.smoothing_groups,
                material: texdata as usize,
            });
        }
        if mb.sides.len() < 4 {
            eprintln!("Warning: brush {} has too few sides, removed", solid.id);
            continue;
        }
        mb.contents = brush_contents(&mb.sides, &map.texinfos);
        if !make_brush_windings(&map.planes, &mut mb) {
            eprintln!(
                "Warning: brush {} is invalid or outside the world, removed",
                solid.id
            );
            continue;
        }
        let bi = map.brushes.len();
        if !disp_sides.is_empty() {
            mb.is_disp = true;
            for (msi, vsi) in disp_sides {
                let di = map.disps.len();
                map.disps.push(MapDisp {
                    brush: bi,
                    side: msi,
                    info: solid.sides[vsi].disp.clone().unwrap(),
                    entity: entity_index,
                });
                mb.sides[msi].disp = Some(di);
            }
        } else {
            add_brush_bevels(&mut map.planes, &mut mb);
        }
        for s in mb.sides.iter_mut() {
            if s.surf_flags & (SURF_NODRAW | SURF_HINT | SURF_SKIP) != 0
                && s.surf_flags & SURF_HINT == 0
            {
                s.visible = false;
            }
        }
        map.brushes.push(mb);
        map.entities[entity_index].brushes.push(bi);
    }
}
