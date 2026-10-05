use crate::map::{Map, PlaneSet};
use crate::portals::{Portals, visible_contents};
use crate::tree::*;
use gold_common::bsp::*;
use gold_common::math::*;
use gold_common::winding::Winding;

const CONTINUOUS_EPSILON: f64 = 0.005;

#[derive(Clone, Debug, Default)]
pub struct Face {
    pub planenum: usize,
    pub texinfo: i32,
    pub winding: Winding,
    pub contents: i32,
    pub merged: Option<usize>,
    pub split: Vec<usize>,
    pub side: Option<(usize, usize)>,
    pub dispinfo: Option<usize>,
    pub on_node: bool,
    pub orig_face: i32,
    pub detail: bool,
    pub prims: Option<Vec<u16>>,
    pub fog_volume: i32,
}

impl Face {
    pub fn is_final(&self) -> bool {
        self.merged.is_none() && self.split.is_empty()
    }
}

pub struct FaceSet {
    pub faces: Vec<Face>,
}

impl FaceSet {
    pub fn resolve(&self, f: usize, out: &mut Vec<usize>) {
        let face = &self.faces[f];
        if let Some(m) = face.merged {
            self.resolve(m, out);
            return;
        }
        if !face.split.is_empty() {
            for &s in &face.split {
                self.resolve(s, out);
            }
            return;
        }
        if !out.contains(&f) {
            out.push(f);
        }
    }
}

fn face_from_portal(
    tree: &Tree,
    map: &Map,
    portals: &Portals,
    p: usize,
    pside: usize,
) -> Option<Face> {
    let port = &portals.list[p];
    let (b, s) = port.side?;
    let side = &map.brushes[b].sides[s];
    let c_this = tree.nodes[port.nodes[pside]].contents;
    let c_other = tree.nodes[port.nodes[1 - pside]].contents;
    if c_this & CONTENTS_WINDOW != 0 && visible_contents(c_other ^ c_this) == CONTENTS_WINDOW {
        return None;
    }
    let winding = if pside == 1 {
        port.winding.reverse()
    } else {
        port.winding.clone()
    };
    Some(Face {
        planenum: (side.planenum & !1) | pside,
        texinfo: side.texinfo,
        winding,
        contents: c_this,
        side: Some((b, s)),
        on_node: true,
        orig_face: -1,
        fog_volume: -1,
        ..Default::default()
    })
}

fn try_merge_winding(f1: &Winding, f2: &Winding, normal: Vec3) -> Option<Winding> {
    let n1 = f1.len();
    let n2 = f2.len();
    let eq = |a: Vec3, b: Vec3| (a - b).abs().max_elem() < 0.001;
    for i in 0..n1 {
        let p1 = f1.p[i];
        let p2 = f1.p[(i + 1) % n1];
        for j in 0..n2 {
            let p3 = f2.p[j];
            let p4 = f2.p[(j + 1) % n2];
            if !(eq(p1, p4) && eq(p2, p3)) {
                continue;
            }
            let back = f1.p[(i + n1 - 1) % n1];
            let delta = p1 - back;
            let en = normal.cross(delta).normalize();
            let back2 = f2.p[(j + 2) % n2];
            let dot = (back2 - p1).dot(en);
            if dot > CONTINUOUS_EPSILON {
                return None;
            }
            let keep1 = dot < -CONTINUOUS_EPSILON;

            let back = f1.p[(i + 2) % n1];
            let delta = back - p2;
            let en = normal.cross(delta).normalize();
            let back2 = f2.p[(j + n2 - 1) % n2];
            let dot = (back2 - p2).dot(en);
            if dot > CONTINUOUS_EPSILON {
                return None;
            }
            let keep2 = dot < -CONTINUOUS_EPSILON;

            let mut out = Vec::new();
            let mut k = (i + 1) % n1;
            while k != i {
                if !(k == (i + 1) % n1 && !keep2) {
                    out.push(f1.p[k]);
                }
                k = (k + 1) % n1;
            }
            let mut l = (j + 1) % n2;
            while l != j {
                if !(l == (j + 1) % n2 && !keep1) {
                    out.push(f2.p[l]);
                }
                l = (l + 1) % n2;
            }
            return Some(Winding::new(out));
        }
    }
    None
}

fn merge_node_faces(
    fs: &mut FaceSet,
    planes: &PlaneSet,
    node_faces: &mut Vec<usize>,
    node_plane: usize,
) {
    let normal = planes.get(node_plane).normal;
    let mut i = 0;
    while i < node_faces.len() {
        let f1 = node_faces[i];
        if !fs.faces[f1].is_final() {
            i += 1;
            continue;
        }
        let mut merged_any = false;
        for j in 0..i {
            let f2 = node_faces[j];
            if !fs.faces[f2].is_final() {
                continue;
            }
            let (a, b) = (&fs.faces[f1], &fs.faces[f2]);
            if a.texinfo != b.texinfo || a.planenum != b.planenum || a.contents != b.contents {
                continue;
            }
            let n = if a.planenum & 1 == 1 { -normal } else { normal };
            if let Some(w) = try_merge_winding(&a.winding, &b.winding, n) {
                let mut nf = a.clone();
                nf.winding = w;
                nf.merged = None;
                nf.split.clear();
                let ni = fs.faces.len();
                fs.faces.push(nf);
                fs.faces[f1].merged = Some(ni);
                fs.faces[f2].merged = Some(ni);
                node_faces.push(ni);
                merged_any = true;
                break;
            }
        }
        let _ = merged_any;
        i += 1;
    }
    node_faces.retain(|&f| fs.faces[f].is_final());
}

pub fn subdivide_face(fs: &mut FaceSet, map: &Map, f: usize, out: &mut Vec<usize>) {
    let ti = fs.faces[f].texinfo;
    if ti < 0 {
        out.push(f);
        return;
    }
    let tex = &map.texinfos[ti as usize];
    if tex.flags & (SURF_NOCHOP | SURF_NODRAW | SURF_NOLIGHT | SURF_SKY) != 0
        || fs.faces[f].dispinfo.is_some()
    {
        out.push(f);
        return;
    }
    for axis in 0..2 {
        let lv = tex.lightmap_vecs[axis];
        let vec = v3(lv[0] as f64, lv[1] as f64, lv[2] as f64);
        let len = vec.length();
        if len < 1e-9 {
            continue;
        }
        let off = lv[3] as f64;
        let w = &fs.faces[f].winding;
        let mut mins: f64 = 1e30;
        let mut maxs: f64 = -1e30;
        for p in &w.p {
            let v = p.dot(vec) + off;
            mins = mins.min(v);
            maxs = maxs.max(v);
        }
        let extent = maxs.ceil() - mins.floor();
        if extent <= MAX_BRUSH_LIGHTMAP_DIM_WITHOUT_BORDER as f64 {
            continue;
        }
        let split_s = mins.floor() + (MAX_BRUSH_LIGHTMAP_DIM_WITHOUT_BORDER - 1) as f64;
        let normal = vec / len;
        let dist = (split_s - off) / len;
        let (front, back) = w.clip_epsilon(normal, dist, 0.1);
        match (front, back) {
            (Some(fw), Some(bw)) => {
                let base = fs.faces[f].clone();
                let mut ids = Vec::new();
                for nw in [bw, fw] {
                    let mut nf = base.clone();
                    nf.winding = nw;
                    nf.split.clear();
                    nf.merged = None;
                    let ni = fs.faces.len();
                    fs.faces.push(nf);
                    ids.push(ni);
                }
                fs.faces[f].split = ids.clone();
                for ni in ids {
                    subdivide_face(fs, map, ni, out);
                }
                return;
            }
            _ => continue,
        }
    }
    out.push(f);
}

pub fn make_faces(
    tree: &mut Tree,
    planes: &PlaneSet,
    portals: &mut Portals,
    map: &Map,
    fs: &mut FaceSet,
) {
    for l in tree.leafs() {
        if tree.nodes[l].contents & CONTENTS_SOLID != 0 {
            continue;
        }
        let plist = tree.nodes[l].portals.clone();
        for p in plist {
            if portals.list[p].removed || portals.list[p].onnode.is_none() {
                continue;
            }
            let s = if portals.list[p].nodes[1] == l { 1 } else { 0 };
            if let Some(face) = face_from_portal(tree, map, portals, p, s) {
                let fi = fs.faces.len();
                fs.faces.push(face);
                portals.list[p].face[s] = Some(fi);
                let on = portals.list[p].onnode.unwrap();
                tree.nodes[on].faces.push(fi);
            }
        }
    }
    let mut stack = vec![tree.head];
    while let Some(n) = stack.pop() {
        let Some(pn) = tree.nodes[n].planenum else {
            continue;
        };
        let mut nf = std::mem::take(&mut tree.nodes[n].faces);
        merge_node_faces(fs, planes, &mut nf, pn);
        let mut out = Vec::new();
        for f in nf {
            subdivide_face(fs, map, f, &mut out);
        }
        tree.nodes[n].faces = out;
        let [c0, c1] = tree.nodes[n].children;
        stack.push(c0);
        stack.push(c1);
    }
}

fn filter_face_into_tree(
    tree: &Tree,
    planes: &PlaneSet,
    w: Winding,
    normal: Vec3,
) -> Vec<(usize, Winding)> {
    let mut out = Vec::new();
    let mut stack = vec![(tree.head, w)];
    while let Some((n, w)) = stack.pop() {
        match tree.nodes[n].planenum {
            None => {
                if tree.nodes[n].contents & CONTENTS_SOLID == 0 {
                    out.push((n, w));
                }
            }
            Some(pn) => {
                let pl = planes.get(pn);
                let cls = w.classify(pl.normal, pl.dist, 0.1);
                let [c0, c1] = tree.nodes[n].children;
                if cls == gold_common::winding::SIDE_ON {
                    if normal.dot(pl.normal) > 0.0 {
                        stack.push((c0, w));
                    } else {
                        stack.push((c1, w));
                    }
                    continue;
                }
                let (f, b) = w.clip_epsilon(pl.normal, pl.dist, 0.1);
                if let Some(f) = f {
                    stack.push((c0, f));
                }
                if let Some(b) = b {
                    stack.push((c1, b));
                }
            }
        }
    }
    out
}

pub fn filter_brushes_into_tree(tree: &mut Tree, planes: &PlaneSet, map: &Map, ids: &[usize]) {
    for &bi in ids {
        let mb = &map.brushes[bi];
        let mut stack = vec![tree.head];
        while let Some(n) = stack.pop() {
            match tree.nodes[n].planenum {
                None => {
                    if !tree.nodes[n].extra_brushes.contains(&bi) {
                        tree.nodes[n].extra_brushes.push(bi);
                    }
                }
                Some(pn) => {
                    let pl = planes.get(pn);
                    let mut front = false;
                    let mut back = false;
                    for s in &mb.sides {
                        if let Some(w) = &s.winding {
                            for p in &w.p {
                                let d = p.dot(pl.normal) - pl.dist;
                                if d > 0.1 {
                                    front = true;
                                }
                                if d < -0.1 {
                                    back = true;
                                }
                            }
                        }
                    }
                    let [c0, c1] = tree.nodes[n].children;
                    if front {
                        stack.push(c0);
                    }
                    if back {
                        stack.push(c1);
                    }
                }
            }
        }
    }
}

pub fn make_detail_faces(
    tree: &mut Tree,
    map: &mut Map,
    fs: &mut FaceSet,
    detail: &[usize],
) -> usize {
    let planes_owned = std::mem::take(&mut map.planes);
    let planes = &planes_owned;
    let count = make_detail_faces_inner(tree, planes, map, fs, detail);
    map.planes = planes_owned;
    count
}

fn make_detail_faces_inner(
    tree: &mut Tree,
    planes: &PlaneSet,
    map: &mut Map,
    fs: &mut FaceSet,
    detail: &[usize],
) -> usize {
    let mut count = 0;
    let huge = Aabb::new(Vec3::splat(-1e9), Vec3::splat(1e9));
    let list = crate::tree::make_bsp_brush_list(map, detail, &huge);
    let chopped = crate::tree::chop_brushes(planes, list);
    let mut per_side: std::collections::BTreeMap<(usize, usize), Vec<Winding>> =
        std::collections::BTreeMap::new();
    for b in &chopped {
        for s in &b.sides {
            if let (Some(orig), Some(w)) = (s.original, &s.winding) {
                per_side.entry(orig).or_default().push(w.clone());
            }
        }
    }
    for ((bi, si), fragments) in per_side {
        {
            let side = &map.brushes[bi].sides[si];
            if side.bevel || side.disp.is_some() {
                continue;
            }
            if map.brushes[bi].contents & ALL_VISIBLE_CONTENTS == 0 {
                continue;
            }
            let ti = side.texinfo;
            if map.texinfos[ti as usize].flags & (SURF_NODRAW | SURF_SKIP | SURF_HINT) != 0 {
                continue;
            }
            let normal = planes.get(side.planenum).normal;
            let planenum = side.planenum;
            let mut in_empty = Vec::new();
            for f in fragments {
                for (_, fw) in filter_face_into_tree(tree, planes, f, normal) {
                    in_empty.push(fw);
                }
            }
            {
                for &bj in detail {
                    if bj == bi {
                        continue;
                    }
                    let other = &map.brushes[bj];
                    if other.contents & CONTENTS_SOLID == 0
                        || !other.bounds.expand(0.1).intersects(&map.brushes[bi].bounds)
                    {
                        continue;
                    }
                    let Some(cw) = other
                        .sides
                        .iter()
                        .find(|s| !s.bevel && s.planenum == (planenum ^ 1))
                        .and_then(|s| s.winding.as_ref())
                    else {
                        continue;
                    };
                    in_empty = in_empty
                        .into_iter()
                        .flat_map(|f| subtract_polygon(&f, cw))
                        .collect();
                }
            }
            let pieces = merge_windings(in_empty, normal);
            map.brushes[bi].sides[si].visible = true;
            for p in pieces {
                if p.is_tiny() {
                    continue;
                }
                let mut leafs: Vec<usize> = filter_face_into_tree(tree, planes, p.clone(), normal)
                    .into_iter()
                    .map(|(l, _)| l)
                    .collect();
                leafs.sort();
                leafs.dedup();
                if leafs.is_empty() {
                    continue;
                }
                {
                    let face = Face {
                        planenum,
                        texinfo: ti,
                        winding: p,
                        contents: tree.nodes[leafs[0]].contents,
                        side: Some((bi, si)),
                        on_node: false,
                        orig_face: -1,
                        fog_volume: -1,
                        detail: true,
                        ..Default::default()
                    };
                    let fi = fs.faces.len();
                    fs.faces.push(face);
                    let mut out = Vec::new();
                    subdivide_face(fs, map, fi, &mut out);
                    for f in out {
                        let fw = fs.faces[f].winding.clone();
                        for (leaf, _) in filter_face_into_tree(tree, planes, fw, normal) {
                            if !tree.nodes[leaf].leaf_faces.contains(&f) {
                                tree.nodes[leaf].leaf_faces.push(f);
                            }
                        }
                        count += 1;
                    }
                }
            }
        }
    }
    count
}

pub fn fix_tjuncs(fs: &mut FaceSet, faces: &[usize]) -> usize {
    let snap = |v: Vec3| -> Vec3 {
        let mut o = v;
        for i in 0..3 {
            let r = o[i].round();
            if (o[i] - r).abs() < 0.01 {
                o[i] = r;
            }
        }
        o
    };
    let mut verts: Vec<Vec3> = Vec::new();
    let mut grid: std::collections::HashMap<(i64, i64, i64), Vec<usize>> =
        std::collections::HashMap::new();
    let cell = 64.0;
    let key = |v: Vec3| {
        (
            (v.x / cell).floor() as i64,
            (v.y / cell).floor() as i64,
            (v.z / cell).floor() as i64,
        )
    };
    for &f in faces {
        for p in fs.faces[f].winding.p.iter_mut() {
            *p = snap(*p);
        }
        for &p in &fs.faces[f].winding.p {
            let k = key(p);
            let mut found = false;
            'o: for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        if let Some(l) = grid.get(&(k.0 + dx, k.1 + dy, k.2 + dz)) {
                            for &vi in l {
                                if (verts[vi] - p).length() < 0.01 {
                                    found = true;
                                    break 'o;
                                }
                            }
                        }
                    }
                }
            }
            if !found {
                grid.entry(k).or_default().push(verts.len());
                verts.push(p);
            }
        }
    }
    let mut added = 0;
    for &f in faces {
        let w = fs.faces[f].winding.clone();
        let n = w.len();
        let mut out = Vec::with_capacity(n * 2);
        let mut face_added = false;
        for i in 0..n {
            let a = w.p[i];
            let b = w.p[(i + 1) % n];
            out.push(a);
            let e = b - a;
            let len = e.length();
            if len < 0.1 {
                continue;
            }
            let dir = e / len;
            let mn = a.min(b) - Vec3::splat(0.1);
            let mx = a.max(b) + Vec3::splat(0.1);
            let k0 = key(mn);
            let k1 = key(mx);
            let mut on_edge: Vec<(f64, Vec3)> = Vec::new();
            for x in k0.0..=k1.0 {
                for y in k0.1..=k1.1 {
                    for z in k0.2..=k1.2 {
                        if let Some(l) = grid.get(&(x, y, z)) {
                            for &vi in l {
                                let v = verts[vi];
                                let t = (v - a).dot(dir);
                                if t <= 0.1 || t >= len - 0.1 {
                                    continue;
                                }
                                let off = (a + dir * t - v).length();
                                if off < 0.05 {
                                    on_edge.push((t, v));
                                }
                            }
                        }
                    }
                }
            }
            if !on_edge.is_empty() {
                on_edge.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
                on_edge.dedup_by(|x, y| (x.0 - y.0).abs() < 0.01);
                for (_, v) in on_edge {
                    out.push(v);
                    added += 1;
                    face_added = true;
                }
            }
        }
        if face_added {
            fs.faces[f].winding = Winding::new(out);
        }
        let w = &fs.faces[f].winding;
        if has_colinear(w) {
            fs.faces[f].prims = Some(triangulate_polygon(w));
        } else {
            fs.faces[f].prims = None;
        }
    }
    added
}

fn has_colinear(w: &Winding) -> bool {
    let n = w.len();
    for i in 0..n {
        let a = w.p[(i + n - 1) % n];
        let b = w.p[i];
        let c = w.p[(i + 1) % n];
        let (e1, e2) = (b - a, c - b);
        let cr = e1.cross(e2).length();
        if cr < 1e-4 * e1.length() * e2.length() + 1e-9 {
            return true;
        }
    }
    false
}

pub fn triangulate_polygon(w: &Winding) -> Vec<u16> {
    let remaining_ok = |idx: &Vec<u16>, i: usize| -> bool {
        if idx.len() <= 3 {
            return true;
        }
        let mut r = idx.clone();
        r.remove(i);
        polygon_area(w, &r) > 1e-3
    };
    let mut idx: Vec<u16> = (0..w.len() as u16).collect();
    let mut out = Vec::new();
    let area = |a: u16, b: u16, c: u16| -> f64 {
        let pa = w.p[a as usize];
        (w.p[b as usize] - pa).cross(w.p[c as usize] - pa).length()
    };
    let mut guard = 0;
    while idx.len() > 3 && guard < 10000 {
        guard += 1;
        let n = idx.len();
        let mut best = None;
        let mut best_area = 1e-4;
        for i in 0..n {
            let a = idx[(i + n - 1) % n];
            let b = idx[i];
            let c = idx[(i + 1) % n];
            let ar = area(a, b, c);
            if ar > best_area && remaining_ok(&idx, i) {
                best_area = ar;
                best = Some(i);
            }
        }
        let Some(i) = best else { break };
        let a = idx[(i + n - 1) % n];
        let b = idx[i];
        let c = idx[(i + 1) % n];
        out.extend_from_slice(&[a, b, c]);
        idx.remove(i);
    }
    if idx.len() == 3 && area(idx[0], idx[1], idx[2]) > 1e-6 {
        out.extend_from_slice(&[idx[0], idx[1], idx[2]]);
    }
    out
}

fn polygon_area(w: &Winding, idx: &[u16]) -> f64 {
    let mut total = Vec3::ZERO;
    let p0 = w.p[idx[0] as usize];
    for k in 1..idx.len().saturating_sub(1) {
        total += (w.p[idx[k] as usize] - p0).cross(w.p[idx[k + 1] as usize] - p0);
    }
    total.length() * 0.5
}

fn merge_windings(mut pieces: Vec<Winding>, normal: Vec3) -> Vec<Winding> {
    let mut changed = true;
    while changed && pieces.len() > 1 {
        changed = false;
        'outer: for i in 0..pieces.len() {
            for j in 0..pieces.len() {
                if i == j {
                    continue;
                }
                if let Some(m) = try_merge_winding(&pieces[i], &pieces[j], -normal)
                    .or_else(|| try_merge_winding(&pieces[i], &pieces[j], normal))
                {
                    let (a, b) = if i > j { (i, j) } else { (j, i) };
                    pieces.remove(a);
                    pieces.remove(b);
                    pieces.push(m);
                    changed = true;
                    break 'outer;
                }
            }
        }
    }
    pieces
}

pub fn subtract_polygon(a: &Winding, b: &Winding) -> Vec<Winding> {
    let n = b.plane().normal;
    let c = b.center();
    let mut out = Vec::new();
    let mut inside = Some(a.clone());
    for i in 0..b.len() {
        let p0 = b.p[i];
        let p1 = b.p[(i + 1) % b.len()];
        let mut en = n.cross(p1 - p0).normalize();
        if en.length() < 0.5 {
            continue;
        }
        if en.dot(c - p0) > 0.0 {
            en = -en;
        }
        let Some(cur) = inside.take() else { break };
        let (front, back) = cur.clip_epsilon(en, en.dot(p0), 0.01);
        if let Some(f) = front {
            out.push(f);
        }
        inside = back;
    }
    if inside.is_none() {
        return if out.is_empty() { vec![a.clone()] } else { out };
    }
    out
}
