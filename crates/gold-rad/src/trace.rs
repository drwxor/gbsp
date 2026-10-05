use gold_common::math::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfKind {
    Opaque,
    Sky,
    Blocker,
}

#[derive(Clone, Copy, Debug)]
pub struct TriRef {
    pub kind: SurfKind,
    pub face: i32,
    pub disp_tri: i32,
}

#[derive(Clone, Copy)]
struct Tri {
    v0: [f32; 3],
    e1: [f32; 3],
    e2: [f32; 3],
}

#[derive(Clone, Copy, Default)]
struct Node {
    min: [f32; 3],
    max: [f32; 3],
    index: u32,
    count: u32,
    axis: u32,
}

pub struct Bvh {
    nodes: Vec<Node>,
    tris: Vec<Tri>,
    pub refs: Vec<TriRef>,
    pub verts: Vec<[Vec3; 3]>,
}

#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub t: f32,
    pub tri: u32,
    pub u: f32,
    pub v: f32,
}

#[inline]
fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline]
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
#[inline]
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub struct BvhBuilder {
    tris: Vec<[Vec3; 3]>,
    refs: Vec<TriRef>,
}

impl Default for BvhBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl BvhBuilder {
    pub fn new() -> Self {
        BvhBuilder {
            tris: Vec::new(),
            refs: Vec::new(),
        }
    }
    pub fn add(&mut self, a: Vec3, b: Vec3, c: Vec3, r: TriRef) {
        if (b - a).cross(c - a).length_sq() < 1e-10 {
            return;
        }
        self.tris.push([a, b, c]);
        self.refs.push(r);
    }
    pub fn len(&self) -> usize {
        self.tris.len()
    }
    pub fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }

    pub fn build(self) -> Bvh {
        let n = self.tris.len();
        let mut centroids = Vec::with_capacity(n);
        let mut boxes = Vec::with_capacity(n);
        for t in &self.tris {
            let mut b = Aabb::EMPTY;
            for p in t {
                b.add_point(*p);
            }
            centroids.push(b.center());
            boxes.push(b);
        }
        let mut idx: Vec<u32> = (0..n as u32).collect();
        let mut nodes = Vec::with_capacity(2 * n.max(1));
        nodes.push(Node::default());
        if n > 0 {
            build_rec(&mut nodes, 0, &mut idx, 0, n, &centroids, &boxes);
        }
        let tris = idx
            .iter()
            .map(|&i| {
                let t = &self.tris[i as usize];
                let v0 = t[0].to_f32();
                let v1 = t[1].to_f32();
                let v2 = t[2].to_f32();
                Tri {
                    v0,
                    e1: sub(v1, v0),
                    e2: sub(v2, v0),
                }
            })
            .collect();
        let refs = idx.iter().map(|&i| self.refs[i as usize]).collect();
        let verts = idx.iter().map(|&i| self.tris[i as usize]).collect();
        Bvh {
            nodes,
            tris,
            refs,
            verts,
        }
    }
}

fn build_rec(
    nodes: &mut Vec<Node>,
    ni: usize,
    idx: &mut [u32],
    start: usize,
    end: usize,
    cent: &[Vec3],
    boxes: &[Aabb],
) {
    let mut b = Aabb::EMPTY;
    let mut cb = Aabb::EMPTY;
    for &i in &idx[start..end] {
        b.add_box(&boxes[i as usize]);
        cb.add_point(cent[i as usize]);
    }
    nodes[ni].min = b.mins.to_f32();
    nodes[ni].max = b.maxs.to_f32();
    let count = end - start;
    if count <= 4 {
        nodes[ni].index = start as u32;
        nodes[ni].count = count as u32;
        return;
    }
    const BINS: usize = 16;
    let ext = cb.size();
    let mut best = (f64::MAX, 0usize, 0usize);
    for axis in 0..3 {
        if ext[axis] < 1e-9 {
            continue;
        }
        let mut bin_box = [Aabb::EMPTY; BINS];
        let mut bin_cnt = [0usize; BINS];
        for &i in &idx[start..end] {
            let c = cent[i as usize][axis];
            let k =
                (((c - cb.mins[axis]) / ext[axis]) * BINS as f64).min(BINS as f64 - 1.0) as usize;
            bin_cnt[k] += 1;
            bin_box[k].add_box(&boxes[i as usize]);
        }
        let area = |b: &Aabb| {
            if b.is_empty() {
                0.0
            } else {
                let s = b.size();
                s.x * s.y + s.y * s.z + s.z * s.x
            }
        };
        for split in 1..BINS {
            let mut lb = Aabb::EMPTY;
            let mut rb = Aabb::EMPTY;
            let mut lc = 0;
            let mut rc = 0;
            for k in 0..split {
                lb.add_box(&bin_box[k]);
                lc += bin_cnt[k];
            }
            for k in split..BINS {
                rb.add_box(&bin_box[k]);
                rc += bin_cnt[k];
            }
            if lc == 0 || rc == 0 {
                continue;
            }
            let cost = area(&lb) * lc as f64 + area(&rb) * rc as f64;
            if cost < best.0 {
                best = (cost, axis, split);
            }
        }
    }
    let mut mid;
    if best.0 < f64::MAX {
        let (_, axis, split) = best;
        let pivot = cb.mins[axis] + ext[axis] * split as f64 / BINS as f64;
        let slice = &mut idx[start..end];
        let mut l = 0;
        for k in 0..slice.len() {
            if cent[slice[k] as usize][axis] < pivot {
                slice.swap(l, k);
                l += 1;
            }
        }
        mid = start + l;
        nodes[ni].axis = axis as u32;
    } else {
        mid = start + count / 2;
    }
    if mid == start || mid == end {
        mid = start + count / 2;
    }
    let left = nodes.len();
    nodes.push(Node::default());
    let right = nodes.len();
    nodes.push(Node::default());
    nodes[ni].index = left as u32;
    nodes[ni].count = 0;
    build_rec(nodes, left, idx, start, mid, cent, boxes);
    build_rec(nodes, right, idx, mid, end, cent, boxes);
    let _ = right;
}

#[inline]
fn ray_box(min: [f32; 3], max: [f32; 3], org: [f32; 3], inv: [f32; 3], tmax: f32) -> Option<f32> {
    let mut t0 = 0.0f32;
    let mut t1 = tmax;
    for a in 0..3 {
        let ta = (min[a] - org[a]) * inv[a];
        let tb = (max[a] - org[a]) * inv[a];
        let (lo, hi) = if ta < tb { (ta, tb) } else { (tb, ta) };
        t0 = t0.max(lo);
        t1 = t1.min(hi);
        if t0 > t1 {
            return None;
        }
    }
    Some(t0)
}

impl Bvh {
    #[inline]
    fn intersect_tri(
        &self,
        i: usize,
        org: [f32; 3],
        dir: [f32; 3],
        tmin: f32,
        tmax: f32,
    ) -> Option<(f32, f32, f32)> {
        let t = &self.tris[i];
        let p = cross(dir, t.e2);
        let det = dot(t.e1, p);
        if det.abs() < 1e-12 {
            return None;
        }
        let inv = 1.0 / det;
        let s = sub(org, t.v0);
        let u = dot(s, p) * inv;
        if !(-1e-6..=1.000001).contains(&u) {
            return None;
        }
        let q = cross(s, t.e1);
        let v = dot(dir, q) * inv;
        if v < -1e-6 || u + v > 1.000001 {
            return None;
        }
        let tt = dot(t.e2, q) * inv;
        if tt <= tmin || tt >= tmax {
            return None;
        }
        Some((tt, u, v))
    }

    pub fn closest(
        &self,
        org: Vec3,
        dir: Vec3,
        tmin: f32,
        tmax: f32,
        filter: &dyn Fn(&TriRef) -> bool,
    ) -> Option<Hit> {
        if self.tris.is_empty() {
            return None;
        }
        let o = org.to_f32();
        let d = dir.to_f32();
        let inv = [1.0 / d[0], 1.0 / d[1], 1.0 / d[2]];
        let mut best: Option<Hit> = None;
        let mut tbest = tmax;
        let mut stack = [0u32; 128];
        let mut sp = 1;
        stack[0] = 0;
        while sp > 0 {
            sp -= 1;
            let node = &self.nodes[stack[sp] as usize];
            if ray_box(node.min, node.max, o, inv, tbest).is_none() {
                continue;
            }
            if node.count > 0 {
                let first = node.index as usize;
                for i in first..first + node.count as usize {
                    if let Some((t, u, v)) = self.intersect_tri(i, o, d, tmin, tbest)
                        && filter(&self.refs[i])
                    {
                        tbest = t;
                        best = Some(Hit {
                            t,
                            tri: i as u32,
                            u,
                            v,
                        });
                    }
                }
            } else {
                let l = node.index;
                let r = l + 1;
                let dneg = d[node.axis as usize] < 0.0;
                if sp + 2 > stack.len() {
                    continue;
                }
                if dneg {
                    stack[sp] = l;
                    stack[sp + 1] = r;
                } else {
                    stack[sp] = r;
                    stack[sp + 1] = l;
                }
                sp += 2;
            }
        }
        best
    }

    pub fn occluded(
        &self,
        org: Vec3,
        dir: Vec3,
        tmin: f32,
        tmax: f32,
        filter: &dyn Fn(&TriRef) -> bool,
    ) -> bool {
        if self.tris.is_empty() {
            return false;
        }
        let o = org.to_f32();
        let d = dir.to_f32();
        let inv = [1.0 / d[0], 1.0 / d[1], 1.0 / d[2]];
        let mut stack = [0u32; 128];
        let mut sp = 1;
        stack[0] = 0;
        while sp > 0 {
            sp -= 1;
            let node = &self.nodes[stack[sp] as usize];
            if ray_box(node.min, node.max, o, inv, tmax).is_none() {
                continue;
            }
            if node.count > 0 {
                let first = node.index as usize;
                for i in first..first + node.count as usize {
                    if self.intersect_tri(i, o, d, tmin, tmax).is_some() && filter(&self.refs[i]) {
                        return true;
                    }
                }
            } else if sp + 2 <= stack.len() {
                stack[sp] = node.index;
                stack[sp + 1] = node.index + 1;
                sp += 2;
            }
        }
        false
    }

    pub fn hit_point(&self, h: &Hit) -> Vec3 {
        let t = &self.verts[h.tri as usize];
        let (u, v) = (h.u as f64, h.v as f64);
        t[0] * (1.0 - u - v) + t[1] * u + t[2] * v
    }

    pub fn tri_normal(&self, tri: u32) -> Vec3 {
        let t = &self.verts[tri as usize];
        (t[1] - t[0]).cross(t[2] - t[0]).normalize()
    }
}
