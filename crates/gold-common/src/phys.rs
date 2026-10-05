use crate::math::*;

pub const METERS_PER_INCH: f64 = 0.0254;
const IVP_COMPACT_SURFACE_ID: [u8; 4] = *b"IVPS";

#[inline]
pub fn to_ivp(p: Vec3) -> Vec3 {
    v3(
        p.x * METERS_PER_INCH,
        -p.z * METERS_PER_INCH,
        p.y * METERS_PER_INCH,
    )
}

#[inline]
pub fn from_ivp(p: Vec3) -> Vec3 {
    v3(
        p.x / METERS_PER_INCH,
        p.z / METERS_PER_INCH,
        -p.y / METERS_PER_INCH,
    )
}

#[derive(Clone, Debug, Default)]
pub struct ConvexHull {
    pub verts: Vec<Vec3>,
    pub tris: Vec<[u32; 3]>,
    pub materials: Vec<u8>,
}

impl ConvexHull {
    pub fn from_polygons(polys: &[(Vec<Vec3>, u8)], weld: f64) -> Option<ConvexHull> {
        let mut hull = ConvexHull::default();
        let mut idx_polys: Vec<(Vec<u32>, u8)> = Vec::new();
        for (poly, mat) in polys {
            let mut ids: Vec<u32> = Vec::new();
            for &p in poly {
                let id = match hull.verts.iter().position(|&v| (v - p).length() <= weld) {
                    Some(i) => i as u32,
                    None => {
                        hull.verts.push(p);
                        (hull.verts.len() - 1) as u32
                    }
                };
                if ids.last() != Some(&id) {
                    ids.push(id);
                }
            }
            while ids.len() > 1 && ids.first() == ids.last() {
                ids.pop();
            }
            if ids.len() >= 3 {
                idx_polys.push((ids, *mat));
            }
        }
        let n = idx_polys.len();
        for i in 0..n {
            let mut changed = true;
            while changed {
                changed = false;
                let ids = idx_polys[i].0.clone();
                'edges: for k in 0..ids.len() {
                    let a = ids[k];
                    let b = ids[(k + 1) % ids.len()];
                    let pa = hull.verts[a as usize];
                    let pb = hull.verts[b as usize];
                    let dir = pb - pa;
                    let len = dir.length();
                    if len < 1e-9 {
                        continue;
                    }
                    let d = dir / len;
                    for (vi, &v) in hull.verts.iter().enumerate() {
                        let vi = vi as u32;
                        if vi == a || vi == b {
                            continue;
                        }
                        let t = (v - pa).dot(d);
                        if t <= weld || t >= len - weld {
                            continue;
                        }
                        let off = (pa + d * t - v).length();
                        if off <= weld {
                            idx_polys[i].0.insert(k + 1, vi);
                            changed = true;
                            break 'edges;
                        }
                    }
                }
            }
        }
        for (ids, mat) in &idx_polys {
            let tris = triangulate_convex(&hull.verts, ids);
            for t in tris {
                hull.tris.push(t);
                hull.materials.push(*mat);
            }
        }
        let mut edges = std::collections::HashMap::new();
        for t in &hull.tris {
            for k in 0..3 {
                *edges.entry((t[k], t[(k + 1) % 3])).or_insert(0) += 1;
            }
        }
        for (&(a, b), &c) in &edges {
            if c != 1 || edges.get(&(b, a)).copied() != Some(1) {
                return None;
            }
        }
        if hull.tris.len() < 4 {
            return None;
        }
        Some(hull)
    }
}

fn triangulate_convex(verts: &[Vec3], ids: &[u32]) -> Vec<[u32; 3]> {
    let mut out = Vec::new();
    let mut poly: Vec<u32> = ids.to_vec();
    let area = |a: u32, b: u32, c: u32| -> f64 {
        let pa = verts[a as usize];
        (verts[b as usize] - pa)
            .cross(verts[c as usize] - pa)
            .length()
    };
    let mut guard = 0;
    while poly.len() > 3 && guard < 1000 {
        guard += 1;
        let n = poly.len();
        let mut clipped = false;
        for i in 0..n {
            let a = poly[(i + n - 1) % n];
            let b = poly[i];
            let c = poly[(i + 1) % n];
            if area(a, b, c) > 1e-6 {
                out.push([a, b, c]);
                poly.remove(i);
                clipped = true;
                break;
            }
        }
        if !clipped {
            break;
        }
    }
    if poly.len() == 3 {
        out.push([poly[0], poly[1], poly[2]]);
    }
    out
}

fn put_f32(out: &mut Vec<u8>, v: f64) {
    out.extend_from_slice(&(v as f32).to_le_bytes());
}
fn put_i32(out: &mut Vec<u8>, v: i32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

struct TreeNode {
    mins: Vec3,
    maxs: Vec3,
    center: Vec3,
    radius: f64,
    ledge: Option<usize>,
    left: Option<Box<TreeNode>>,
    right: Option<Box<TreeNode>>,
}

fn build_tree(items: &mut [(usize, Vec3, Vec3)]) -> TreeNode {
    let mut mins = Vec3::splat(1e30);
    let mut maxs = Vec3::splat(-1e30);
    for (_, a, b) in items.iter() {
        mins = mins.min(*a);
        maxs = maxs.max(*b);
    }
    let center = (mins + maxs) * 0.5;
    if items.len() == 1 {
        let radius = (maxs - mins).length() * 0.5;
        return TreeNode {
            mins,
            maxs,
            center,
            radius,
            ledge: Some(items[0].0),
            left: None,
            right: None,
        };
    }
    let ext = maxs - mins;
    let axis = ext.major_axis();
    items.sort_by(|a, b| {
        let ca = (a.1[axis] + a.2[axis]) * 0.5;
        let cb = (b.1[axis] + b.2[axis]) * 0.5;
        ca.partial_cmp(&cb).unwrap()
    });
    let mid = items.len() / 2;
    let (l, r) = items.split_at_mut(mid);
    let left = build_tree(l);
    let right = build_tree(r);
    let mut radius = (maxs - mins).length() * 0.5;
    for c in [&left, &right] {
        radius = radius.max(c.center.dist(center) + c.radius);
    }
    TreeNode {
        mins,
        maxs,
        center,
        radius,
        ledge: None,
        left: Some(Box::new(left)),
        right: Some(Box::new(right)),
    }
}

fn flatten_tree<'a>(n: &'a TreeNode, out: &mut Vec<&'a TreeNode>) {
    out.push(n);
    if let Some(l) = &n.left {
        flatten_tree(l, out);
    }
    if let Some(r) = &n.right {
        flatten_tree(r, out);
    }
}

fn subtree_size(n: &TreeNode) -> usize {
    1 + n.left.as_ref().map(|l| subtree_size(l)).unwrap_or(0)
        + n.right.as_ref().map(|r| subtree_size(r)).unwrap_or(0)
}

pub fn build_compact_surface(hulls: &[ConvexHull]) -> Vec<u8> {
    assert!(!hulls.is_empty());
    let mut point_base = Vec::new();
    let mut points: Vec<Vec3> = Vec::new();
    for h in hulls {
        point_base.push(points.len());
        for &v in &h.verts {
            points.push(to_ivp(v));
        }
    }
    let ledges_size: usize = hulls.iter().map(|h| 16 + 16 * h.tris.len()).sum();
    let points_ofs = 48 + ledges_size;
    let tree_ofs = points_ofs + 16 * points.len();

    let mut all_min = Vec3::splat(1e30);
    let mut all_max = Vec3::splat(-1e30);
    for p in &points {
        all_min = all_min.min(*p);
        all_max = all_max.max(*p);
    }
    let mut vol_total = 0.0;
    let mut centroid = Vec3::ZERO;
    for (hi, h) in hulls.iter().enumerate() {
        let base = point_base[hi];
        let o = points[base];
        for t in &h.tris {
            let a = points[base + t[0] as usize] - o;
            let b = points[base + t[1] as usize] - o;
            let c = points[base + t[2] as usize] - o;
            let v = a.dot(b.cross(c)) / 6.0;
            vol_total += v;
            centroid += (o * 4.0 + a + b + c) * (v / 4.0);
        }
    }
    let mass_center = if vol_total.abs() > 1e-12 {
        centroid / vol_total
    } else {
        (all_min + all_max) * 0.5
    };
    let mut upper_radius: f64 = 0.0;
    for p in &points {
        upper_radius = upper_radius.max(p.dist(mass_center));
    }
    let ext = all_max - all_min;
    let inertia_k = (ext.length_sq() / 12.0).max(1e-6);

    let mut items: Vec<(usize, Vec3, Vec3)> = hulls
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let mut mn = Vec3::splat(1e30);
            let mut mx = Vec3::splat(-1e30);
            for &v in &h.verts {
                let p = to_ivp(v);
                mn = mn.min(p);
                mx = mx.max(p);
            }
            (i, mn, mx)
        })
        .collect();
    let tree = build_tree(&mut items);
    let mut nodes = Vec::new();
    flatten_tree(&tree, &mut nodes);

    let mut ledge_offsets = Vec::new();
    {
        let mut o = 48;
        for h in hulls {
            ledge_offsets.push(o);
            o += 16 + 16 * h.tris.len();
        }
    }

    let total = tree_ofs + 28 * nodes.len();
    let mut out = Vec::with_capacity(total);
    put_f32(&mut out, mass_center.x);
    put_f32(&mut out, mass_center.y);
    put_f32(&mut out, mass_center.z);
    put_f32(&mut out, inertia_k);
    put_f32(&mut out, inertia_k);
    put_f32(&mut out, inertia_k);
    put_f32(&mut out, upper_radius);
    put_u32(&mut out, 245 | ((total as u32) << 8));
    put_i32(&mut out, (tree_ofs) as i32);
    put_i32(&mut out, 0);
    put_i32(&mut out, 0);
    out.extend_from_slice(&IVP_COMPACT_SURFACE_ID);

    for (hi, h) in hulls.iter().enumerate() {
        let ledge_ofs = ledge_offsets[hi];
        debug_assert_eq!(out.len(), ledge_ofs);
        let ntri = h.tris.len();
        put_i32(&mut out, (points_ofs as i64 - ledge_ofs as i64) as i32);
        put_i32(&mut out, hi as i32);
        let size_div_16 = (16 + 16 * ntri + 16 * h.verts.len()) / 16;
        put_u32(&mut out, (1 << 2) | ((size_div_16 as u32) << 8));
        out.extend_from_slice(&(ntri as i16).to_le_bytes());
        out.extend_from_slice(&0i16.to_le_bytes());

        let mut edge_map = std::collections::HashMap::new();
        for (ti, t) in h.tris.iter().enumerate() {
            for k in 0..3 {
                edge_map.insert((t[k], t[(k + 1) % 3]), (ti, k));
            }
        }
        let normals: Vec<Vec3> = h
            .tris
            .iter()
            .map(|t| {
                let a = h.verts[t[0] as usize];
                (h.verts[t[1] as usize] - a)
                    .cross(h.verts[t[2] as usize] - a)
                    .normalize()
            })
            .collect();
        let base = point_base[hi] as u32;
        for (ti, t) in h.tris.iter().enumerate() {
            let mut pierce = 0usize;
            let mut best = 2.0;
            for (tj, n) in normals.iter().enumerate() {
                let d = n.dot(normals[ti]);
                if d < best {
                    best = d;
                    pierce = tj;
                }
            }
            let mat = h.materials.get(ti).copied().unwrap_or(1) as u32 & 0x7f;
            put_u32(
                &mut out,
                (ti as u32 & 0xfff) | ((pierce as u32 & 0xfff) << 12) | (mat << 24),
            );
            for k in 0..3 {
                let a = t[k];
                let b = t[(k + 1) % 3];
                let (oti, ok) = edge_map.get(&(b, a)).copied().unwrap_or((ti, k));
                let this_unit = (ti * 4 + 1 + k) as i32;
                let opp_unit = (oti * 4 + 1 + ok) as i32;
                let rel = (opp_unit - this_unit) & 0x7fff;
                put_u32(&mut out, ((base + a) & 0xffff) | ((rel as u32) << 16));
            }
        }
    }
    for p in &points {
        put_f32(&mut out, p.x);
        put_f32(&mut out, p.y);
        put_f32(&mut out, p.z);
        put_f32(&mut out, 0.0);
    }
    for (ni, n) in nodes.iter().enumerate() {
        let node_ofs = tree_ofs + ni * 28;
        let right = match &n.left {
            Some(l) => (1 + subtree_size(l)) as i32 * 28,
            None => 0,
        };
        put_i32(&mut out, right);
        let ledge = match n.ledge {
            Some(li) => ledge_offsets[li] as i32 - node_ofs as i32,
            None => 0,
        };
        put_i32(&mut out, ledge);
        put_f32(&mut out, n.center.x);
        put_f32(&mut out, n.center.y);
        put_f32(&mut out, n.center.z);
        put_f32(&mut out, n.radius);
        let half = (n.maxs - n.mins) * 0.5;
        for a in 0..3 {
            let b = if n.radius > 1e-9 {
                (half[a] / n.radius * 250.0).floor() + 1.0
            } else {
                1.0
            };
            out.push(b.clamp(0.0, 255.0) as u8);
        }
        out.push(0);
    }
    debug_assert_eq!(out.len(), total);
    out
}

pub fn wrap_collide(surface: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(surface.len() + 32);
    put_i32(&mut out, (surface.len() + 28) as i32);
    out.extend_from_slice(b"VPHY");
    out.extend_from_slice(&0x0100u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    put_i32(&mut out, surface.len() as i32);
    put_f32(&mut out, 1.0);
    put_f32(&mut out, 1.0);
    put_f32(&mut out, 1.0);
    put_i32(&mut out, 0);
    out.extend_from_slice(surface);
    out
}

pub struct PhysModel {
    pub model_index: i32,
    pub solids: Vec<Vec<u8>>,
    pub keydata: String,
}

pub fn build_physcollide_lump(models: &[PhysModel]) -> Vec<u8> {
    let mut out = Vec::new();
    for m in models {
        let data_size: usize = m.solids.iter().map(|s| s.len()).sum();
        let mut kd = m.keydata.as_bytes().to_vec();
        kd.push(0);
        put_i32(&mut out, m.model_index);
        put_i32(&mut out, data_size as i32);
        put_i32(&mut out, kd.len() as i32);
        put_i32(&mut out, m.solids.len() as i32);
        for s in &m.solids {
            out.extend_from_slice(s);
        }
        out.extend_from_slice(&kd);
    }
    put_i32(&mut out, -1);
    put_i32(&mut out, -1);
    put_i32(&mut out, 0);
    put_i32(&mut out, 0);
    out
}

fn rd_i32(d: &[u8], o: usize) -> Option<i32> {
    d.get(o..o + 4)
        .map(|b| i32::from_le_bytes(b.try_into().unwrap()))
}
fn rd_f32(d: &[u8], o: usize) -> Option<f32> {
    d.get(o..o + 4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
}

pub fn read_compact_surface(d: &[u8], surf: usize) -> Option<Vec<Vec<[Vec3; 3]>>> {
    let tree_root = rd_i32(d, surf + 32)? as i64;
    let mut ledges = Vec::new();
    let mut stack = vec![surf as i64 + tree_root];
    let mut guard = 0;
    while let Some(node) = stack.pop() {
        guard += 1;
        if guard > 1_000_000 || node < 0 {
            return None;
        }
        let node = node as usize;
        let right = rd_i32(d, node)? as i64;
        let ledge = rd_i32(d, node + 4)? as i64;
        if ledge != 0 {
            ledges.push((node as i64 + ledge) as usize);
        }
        if right != 0 {
            stack.push(node as i64 + 28);
            stack.push(node as i64 + right);
        }
    }
    let mut out = Vec::new();
    for l in ledges {
        let pt_ofs = rd_i32(d, l)? as i64;
        let ntri = i16::from_le_bytes(d.get(l + 12..l + 14)?.try_into().unwrap()) as usize;
        let pbase = (l as i64 + pt_ofs) as usize;
        let mut tris = Vec::with_capacity(ntri);
        for t in 0..ntri {
            let tb = l + 16 + t * 16;
            let mut pts = [Vec3::ZERO; 3];
            for k in 0..3 {
                let e =
                    u32::from_le_bytes(d.get(tb + 4 + k * 4..tb + 8 + k * 4)?.try_into().unwrap());
                let pi = (e & 0xffff) as usize;
                let po = pbase + pi * 16;
                let p = v3(
                    rd_f32(d, po)? as f64,
                    rd_f32(d, po + 4)? as f64,
                    rd_f32(d, po + 8)? as f64,
                );
                pts[k] = from_ivp(p);
            }
            tris.push(pts);
        }
        out.push(tris);
    }
    Some(out)
}

pub fn read_collide_solid(d: &[u8]) -> Option<Vec<Vec<[Vec3; 3]>>> {
    if d.len() >= 8 && &d[4..8] == b"VPHY" {
        let model_type = u16::from_le_bytes([d[10], d[11]]);
        if model_type != 0 {
            return None;
        }
        read_compact_surface(d, 4 + 28)
    } else {
        read_compact_surface(d, 4)
    }
}

pub fn read_phy_file(d: &[u8]) -> Vec<Vec<[Vec3; 3]>> {
    let mut out = Vec::new();
    let Some(header_size) = rd_i32(d, 0) else {
        return out;
    };
    let solid_count = rd_i32(d, 8).unwrap_or(0);
    let mut pos = header_size as usize;
    for _ in 0..solid_count {
        let Some(size) = rd_i32(d, pos) else { break };
        let end = pos + 4 + size as usize;
        if end > d.len() {
            break;
        }
        if let Some(mut hulls) = read_collide_solid(&d[pos..end]) {
            out.append(&mut hulls);
        }
        pos = end;
    }
    out
}
