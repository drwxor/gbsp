use crate::disp::Disp;
use gold_common::math::*;

const DISP_NO_PHYSICS_COLL: i32 = 1 << 1;

const EPS: f64 = 0.01;

fn convex_hull(p: &[Vec3], eps: f64) -> Option<Vec<[usize; 3]>> {
    let n = p.len();
    if n < 3 {
        return None;
    }
    let (mut a, mut b) = (0, 0);
    for i in 0..n {
        if p[i].x < p[a].x || (p[i].x == p[a].x && p[i].y < p[a].y) {
            a = i;
        }
    }
    let mut best = -1.0;
    for i in 0..n {
        let d = (p[i] - p[a]).length();
        if d > best {
            best = d;
            b = i;
        }
    }
    let ab = (p[b] - p[a]).normalize();
    let mut c = usize::MAX;
    best = eps;
    for i in 0..n {
        let d = (p[i] - p[a]).cross(ab).length();
        if d > best {
            best = d;
            c = i;
        }
    }
    if c == usize::MAX {
        return None;
    }
    let plane_n = (p[b] - p[a]).cross(p[c] - p[a]).normalize();
    let mut d = usize::MAX;
    best = eps;
    for i in 0..n {
        let h = (p[i] - p[a]).dot(plane_n).abs();
        if h > best {
            best = h;
            d = i;
        }
    }
    if d == usize::MAX {
        return Some(flat_hull(p, plane_n, eps));
    }

    let normal = |t: &[usize; 3]| (p[t[1]] - p[t[0]]).cross(p[t[2]] - p[t[0]]).normalize();
    let mut tris: Vec<[usize; 3]> = vec![[a, b, c], [a, c, d], [a, d, b], [b, d, c]];
    let inside = (p[a] + p[b] + p[c] + p[d]) * 0.25;
    for t in tris.iter_mut() {
        if normal(t).dot(p[t[0]] - inside) < 0.0 {
            t.swap(1, 2);
        }
    }
    let centre = p.iter().fold(Vec3::ZERO, |s, &x| s + x) / n as f64;
    let mut order: Vec<usize> = (0..n).filter(|i| ![a, b, c, d].contains(i)).collect();
    order.sort_by(|&i, &j| {
        (p[j] - centre)
            .length()
            .partial_cmp(&(p[i] - centre).length())
            .unwrap()
    });
    for i in order {
        let visible: Vec<bool> = tris
            .iter()
            .map(|t| normal(t).dot(p[i] - p[t[0]]) > eps)
            .collect();
        if !visible.iter().any(|&v| v) {
            continue;
        }
        let mut horizon = Vec::new();
        for (t, _) in tris.iter().zip(&visible).filter(|(_, v)| **v) {
            for k in 0..3 {
                let (u, v) = (t[k], t[(k + 1) % 3]);
                let shared = tris
                    .iter()
                    .zip(&visible)
                    .any(|(o, &ov)| ov && (0..3).any(|m| o[m] == v && o[(m + 1) % 3] == u));
                if !shared {
                    horizon.push((u, v));
                }
            }
        }
        let mut kept: Vec<[usize; 3]> = tris
            .iter()
            .zip(&visible)
            .filter(|(_, v)| !**v)
            .map(|(t, _)| *t)
            .collect();
        for (u, v) in horizon {
            kept.push([u, v, i]);
        }
        tris = kept;
    }
    Some(tris)
}

fn flat_hull(p: &[Vec3], n: Vec3, eps: f64) -> Vec<[usize; 3]> {
    let u = if n.z.abs() < 0.9 {
        v3(0.0, 0.0, 1.0).cross(n).normalize()
    } else {
        v3(1.0, 0.0, 0.0).cross(n).normalize()
    };
    let w = n.cross(u);
    let q: Vec<(f64, f64)> = p.iter().map(|x| (x.dot(u), x.dot(w))).collect();
    let mut idx: Vec<usize> = (0..p.len()).collect();
    idx.sort_by(|&i, &j| q[i].partial_cmp(&q[j]).unwrap());
    let cross = |o: usize, a: usize, b: usize| {
        (q[a].0 - q[o].0) * (q[b].1 - q[o].1) - (q[a].1 - q[o].1) * (q[b].0 - q[o].0)
    };
    let mut hull: Vec<usize> = Vec::new();
    for pass in 0..2 {
        let start = hull.len();
        let it: Box<dyn Iterator<Item = &usize>> = if pass == 0 {
            Box::new(idx.iter())
        } else {
            Box::new(idx.iter().rev())
        };
        for &i in it {
            while hull.len() >= start + 2
                && cross(hull[hull.len() - 2], hull[hull.len() - 1], i) <= eps
            {
                hull.pop();
            }
            hull.push(i);
        }
        hull.pop();
    }
    let m = hull.len();
    let mut tris = Vec::new();
    for k in 1..m.saturating_sub(1) {
        tris.push([hull[0], hull[k], hull[k + 1]]);
    }
    for k in 1..m.saturating_sub(1) {
        let (a, b, c) = (hull[1], hull[(1 + k) % m], hull[(2 + k) % m]);
        tris.push([a, c, b]);
    }
    tris
}

fn pierce(p: &[Vec3], tris: &[[usize; 3]], t: usize) -> usize {
    let tri = |k: usize| (p[tris[k][0]], p[tris[k][1]], p[tris[k][2]]);
    let (a, b, c) = tri(t);
    let n = (b - a).cross(c - a).normalize();
    let org = (a + b + c) / 3.0;
    let dir = -n;
    let mut best = (f64::MAX, usize::MAX);
    for k in 0..tris.len() {
        if k == t {
            continue;
        }
        let (a, b, c) = tri(k);
        let nk = (b - a).cross(c - a);
        let len = nk.length();
        if len < 1e-12 {
            continue;
        }
        let nk = nk / len;
        let facing = nk.dot(dir);
        if facing <= 1e-9 {
            continue;
        }
        let dist = (a - org).dot(nk) / facing;
        if dist < -EPS {
            continue;
        }
        let hit = org + dir * dist;
        let inside = [(a, b), (b, c), (c, a)]
            .iter()
            .all(|&(e0, e1)| (e1 - e0).cross(hit - e0).dot(nk) >= -EPS * (e1 - e0).length());
        if inside && dist < best.0 {
            best = (dist, k);
        }
    }
    if best.1 != usize::MAX {
        best.1
    } else {
        let mut m = (f64::MAX, 0);
        for k in 0..tris.len() {
            if k == t {
                continue;
            }
            let (a, b, c) = tri(k);
            let d = (b - a).cross(c - a).normalize().dot(n);
            if d < m.0 {
                m = (d, k);
            }
        }
        m.1
    }
}

fn pack_hull(p: &[Vec3]) -> Option<Vec<u8>> {
    [EPS, 0.001, 0.05, 0.0001, 0.2]
        .iter()
        .find_map(|&eps| pack_hull_eps(p, eps))
}

fn pack_hull_eps(p: &[Vec3], eps: f64) -> Option<Vec<u8>> {
    let tris = convex_hull(p, eps)?;
    let mut edges: Vec<(usize, usize)> = Vec::new();
    let mut edge_of = std::collections::HashMap::new();
    let mut tri_edges = Vec::with_capacity(tris.len());
    for t in &tris {
        let mut te = [0usize; 3];
        for k in 0..3 {
            let (u, v) = (t[k], t[(k + 1) % 3]);
            let key = (u.min(v), u.max(v));
            te[k] = *edge_of.entry(key).or_insert_with(|| {
                edges.push((u, v));
                edges.len() - 1
            });
        }
        tri_edges.push(te);
    }
    let mut directed = std::collections::HashSet::new();
    for t in &tris {
        for k in 0..3 {
            if !directed.insert((t[k], t[(k + 1) % 3])) {
                return None;
            }
        }
    }
    if directed.iter().any(|&(u, v)| !directed.contains(&(v, u))) {
        return None;
    }
    let (nt, ne) = (tris.len(), edges.len());
    if nt > 255 || ne > 255 || edges.iter().any(|&(u, v)| u > 255 || v > 255) {
        return None;
    }
    let mut out = vec![1, 0, 0, 0, nt as u8, nt as u8, ne as u8, ne as u8, 0];
    for (k, te) in tri_edges.iter().enumerate() {
        out.extend(te.iter().map(|&e| e as u8));
        out.push(pierce(p, &tris, k) as u8);
    }
    for (u, v) in edges {
        out.push(u as u8);
        out.push(v as u8);
    }
    Some(out)
}

pub fn build_lump(disps: &[&Disp]) -> Vec<u8> {
    if disps.is_empty() {
        return Vec::new();
    }
    if disps.iter().any(|d| d.size() * d.size() > 256) {
        println!("power 4 displacements: displacement physics not precomputed");
        return Vec::new();
    }
    let hulls: Vec<Vec<u8>> = disps
        .iter()
        .map(|d| {
            if d.flags & DISP_NO_PHYSICS_COLL != 0 {
                return Vec::new();
            }
            pack_hull(&d.positions).unwrap_or_else(|| {
                eprintln!("Warning: couldn't build a physics hull for displacement {}; the engine will build it", d.map_disp);
                Vec::new()
            })
        })
        .collect();
    let mut out = Vec::new();
    out.extend_from_slice(&(hulls.len() as u16).to_le_bytes());
    for h in &hulls {
        out.extend_from_slice(&(h.len() as u16).to_le_bytes());
    }
    for h in hulls {
        out.extend(h);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(size: usize, height: impl Fn(f64, f64) -> f64) -> Vec<Vec3> {
        let mut p = Vec::new();
        for i in 0..size {
            for j in 0..size {
                let (x, y) = (j as f64 * 64.0, i as f64 * 64.0);
                p.push(v3(x, y, height(x, y)));
            }
        }
        p
    }

    fn check(p: &[Vec3]) -> (usize, usize) {
        let data = pack_hull(p).expect("hull");
        let (t, e) = (data[4] as usize, data[6] as usize);
        assert_eq!(data.len(), 9 + 4 * t + 2 * e);
        assert_eq!(2 * e, 3 * t, "closed triangle mesh");
        (t, e)
    }

    #[test]
    fn bowl_hull_drops_coplanar_points() {
        let p = grid(5, |x, y| {
            128.0 * (((x - 128.0).abs().max((y - 128.0).abs())) / 128.0)
        });
        assert_eq!(check(&p), (6, 9));
    }

    #[test]
    fn flat_hull_is_double_sided() {
        let p = grid(9, |_, _| 0.0);
        assert_eq!(check(&p), (4, 6));
    }
}
