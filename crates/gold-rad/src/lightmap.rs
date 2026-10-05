use crate::geom::{FaceGeom, Geometry};
use crate::lights::Light;
use crate::trace::{Bvh, SurfKind, TriRef};
use gold_common::bsp::*;
use gold_common::math::*;

pub const SAMPLE_NUDGE: f64 = 0.25;

pub const BUMP_BASIS: [[f64; 3]; 3] = [
    [0.816_496_580_927_726, 0.0, 0.577_350_269_189_626],
    [
        -0.408_248_290_463_863,
        std::f64::consts::FRAC_1_SQRT_2,
        0.577_350_269_189_626,
    ],
    [
        -0.408_248_290_463_863,
        -std::f64::consts::FRAC_1_SQRT_2,
        0.577_350_269_189_626,
    ],
];

#[derive(Clone, Copy, Debug, Default)]
pub struct Sample {
    pub pos: Vec3,
    pub normal: Vec3,
    pub coverage: f64,
    pub cluster: i32,
    pub bump: [Vec3; 3],
}

pub struct FaceLight {
    pub face: usize,
    pub w: usize,
    pub h: usize,
    pub pages: usize,
    pub samples: Vec<Sample>,
    pub direct: Vec<Vec<[Vec3; 4]>>,
    pub indirect: Vec<Vec<[Vec3; 4]>>,
    pub patch: Vec<u32>,
}

impl FaceLight {
    pub fn direct_flat(&self, slot: usize, luxel: usize) -> Vec3 {
        self.direct
            .get(slot)
            .and_then(|d| d.get(luxel))
            .map(|v| v[0])
            .unwrap_or(Vec3::ZERO)
    }
    pub fn total(&self, slot: usize, luxel: usize, page: usize) -> Vec3 {
        let d = self
            .direct
            .get(slot)
            .and_then(|d| d.get(luxel))
            .map(|v| v[page])
            .unwrap_or(Vec3::ZERO);
        let i = self
            .indirect
            .get(slot)
            .and_then(|d| d.get(luxel))
            .map(|v| v[page])
            .unwrap_or(Vec3::ZERO);
        d + i
    }
    pub fn luxel_index(&self, s: f64, t: f64) -> usize {
        let i = (s.round().max(0.0) as usize).min(self.w - 1);
        let j = (t.round().max(0.0) as usize).min(self.h - 1);
        j * self.w + i
    }
}

fn polygon_area_2d(p: &[(f64, f64)]) -> f64 {
    let mut a = 0.0;
    for i in 0..p.len() {
        let (x0, y0) = p[i];
        let (x1, y1) = p[(i + 1) % p.len()];
        a += x0 * y1 - x1 * y0;
    }
    a * 0.5
}

fn clip_2d(poly: &[(f64, f64)], a: f64, b: f64, c: f64) -> Vec<(f64, f64)> {
    let mut out = Vec::with_capacity(poly.len() + 2);
    let n = poly.len();
    for i in 0..n {
        let p = poly[i];
        let q = poly[(i + 1) % n];
        let dp = a * p.0 + b * p.1 + c;
        let dq = a * q.0 + b * q.1 + c;
        if dp >= 0.0 {
            out.push(p);
        }
        if (dp >= 0.0) != (dq >= 0.0) {
            let t = dp / (dp - dq);
            out.push((p.0 + (q.0 - p.0) * t, p.1 + (q.1 - p.1) * t));
        }
    }
    out
}

fn centroid_2d(p: &[(f64, f64)]) -> (f64, f64) {
    let a = polygon_area_2d(p);
    if a.abs() < 1e-12 {
        let n = p.len().max(1) as f64;
        return (
            p.iter().map(|q| q.0).sum::<f64>() / n,
            p.iter().map(|q| q.1).sum::<f64>() / n,
        );
    }
    let mut cx = 0.0;
    let mut cy = 0.0;
    for i in 0..p.len() {
        let (x0, y0) = p[i];
        let (x1, y1) = p[(i + 1) % p.len()];
        let cr = x0 * y1 - x1 * y0;
        cx += (x0 + x1) * cr;
        cy += (y0 + y1) * cr;
    }
    (cx / (6.0 * a), cy / (6.0 * a))
}

fn closest_on_segment(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a.0 + dx * t, a.1 + dy * t)
}

pub fn bump_normals(tex: &TexInfo, flat: Vec3, phong: Vec3) -> [Vec3; 3] {
    let s = v3(
        tex.texture_vecs[0][0] as f64,
        tex.texture_vecs[0][1] as f64,
        tex.texture_vecs[0][2] as f64,
    );
    let t = v3(
        tex.texture_vecs[1][0] as f64,
        tex.texture_vecs[1][1] as f64,
        tex.texture_vecs[1][2] as f64,
    );
    let left_handed = s.cross(t).dot(flat) < 0.0;
    let mut b1 = phong.cross(s).normalize();
    let b0 = b1.cross(phong).normalize();
    let b2 = phong;
    if left_handed {
        b1 = -b1;
    }
    let mut out = [Vec3::ZERO; 3];
    for (k, lb) in BUMP_BASIS.iter().enumerate() {
        out[k] = (b0 * lb[0] + b1 * lb[1] + b2 * lb[2]).normalize();
    }
    out
}

pub fn build_face_samples(
    geo: &Geometry,
    g: &FaceGeom,
    vertex_normals: Option<&[Vec3]>,
) -> Vec<Sample> {
    let w = g.width();
    let h = g.height();
    let poly: Vec<(f64, f64)> = g.points.iter().map(|&p| g.world_to_luxel(p)).collect();
    let area = polygon_area_2d(&poly);
    let sign = if area >= 0.0 { 1.0 } else { -1.0 };
    let pc = centroid_2d(&poly);
    let mut out = Vec::with_capacity(w * h);
    for j in 0..h {
        for i in 0..w {
            let (x, y) = (i as f64, j as f64);
            let mut sq = vec![
                (x - 0.5, y - 0.5),
                (x + 0.5, y - 0.5),
                (x + 0.5, y + 0.5),
                (x - 0.5, y + 0.5),
            ];
            for k in 0..poly.len() {
                let a = poly[k];
                let b = poly[(k + 1) % poly.len()];
                let (nx, ny) = (-(b.1 - a.1) * sign, (b.0 - a.0) * sign);
                let c = -(nx * a.0 + ny * a.1);
                sq = clip_2d(&sq, nx, ny, c);
                if sq.len() < 3 {
                    break;
                }
            }
            let cov = if sq.len() >= 3 {
                polygon_area_2d(&sq).abs()
            } else {
                0.0
            };
            let (s, t) = if cov > 1e-3 {
                centroid_2d(&sq)
            } else {
                let mut best = (0.0, 0.0);
                let mut bd = f64::MAX;
                for k in 0..poly.len() {
                    let c = closest_on_segment((x, y), poly[k], poly[(k + 1) % poly.len()]);
                    let d = (c.0 - x).powi(2) + (c.1 - y).powi(2);
                    if d < bd {
                        bd = d;
                        best = c;
                    }
                }
                (
                    best.0 + (pc.0 - best.0) * 0.01,
                    best.1 + (pc.1 - best.1) * 0.01,
                )
            };
            let on_plane = g.luxel_to_world(s, t);
            let normal = match vertex_normals {
                Some(vn) => phong_normal(g, vn, on_plane),
                None => g.normal,
            };
            let pos = on_plane + g.normal * SAMPLE_NUDGE;
            let bump = if g.bumped {
                bump_normals(&g.texinfo, g.normal, normal)
            } else {
                [Vec3::ZERO; 3]
            };
            out.push(Sample {
                pos,
                normal,
                coverage: cov.min(1.0),
                cluster: geo.point_cluster(pos),
                bump,
            });
        }
    }
    out
}

pub fn phong_normal(g: &FaceGeom, vn: &[Vec3], p: Vec3) -> Vec3 {
    let n = g.points.len();
    let mut best = g.normal;
    let mut best_out = f64::MAX;
    for k in 1..n.saturating_sub(1) {
        let (a, b, c) = (g.points[0], g.points[k], g.points[k + 1]);
        let v0 = b - a;
        let v1 = c - a;
        let v2 = p - a;
        let d00 = v0.dot(v0);
        let d01 = v0.dot(v1);
        let d11 = v1.dot(v1);
        let d20 = v2.dot(v0);
        let d21 = v2.dot(v1);
        let den = d00 * d11 - d01 * d01;
        if den.abs() < 1e-12 {
            continue;
        }
        let v = (d11 * d20 - d01 * d21) / den;
        let w = (d00 * d21 - d01 * d20) / den;
        let u = 1.0 - v - w;
        let outside = (-u).max(0.0) + (-v).max(0.0) + (-w).max(0.0);
        if outside < best_out {
            best_out = outside;
            let (u, v, w) = (u.max(0.0), v.max(0.0), w.max(0.0));
            let s = u + v + w;
            best = ((vn[0] * u + vn[k] * v + vn[k + 1] * w) / s.max(1e-9)).normalize();
        }
    }
    if best.length() < 0.5 { g.normal } else { best }
}

pub fn disp_locate(geo: &Geometry, di: usize, s: f64, t: f64) -> (usize, [f64; 3]) {
    let d = &geo.disps[di];
    let g = &geo.geoms[d.face];
    let fj = s / g.size[0].max(1) as f64;
    let fi = t / g.size[1].max(1) as f64;
    gold_common::disp::disp_param_locate(d.power, fi, fj)
}

pub fn disp_point(geo: &Geometry, di: usize, s: f64, t: f64) -> (Vec3, Vec3) {
    let d = &geo.disps[di];
    let (ti, b) = disp_locate(geo, di, s, t);
    let tri = d.tris[ti];
    let p = d.positions[tri[0]] * b[0] + d.positions[tri[1]] * b[1] + d.positions[tri[2]] * b[2];
    let n = (d.normals[tri[0]] * b[0] + d.normals[tri[1]] * b[1] + d.normals[tri[2]] * b[2])
        .normalize();
    (p, n)
}

pub fn build_disp_samples(geo: &Geometry, g: &FaceGeom) -> Vec<Sample> {
    let di = g.disp.unwrap();
    let w = g.width();
    let h = g.height();
    let mut out = Vec::with_capacity(w * h);
    for j in 0..h {
        for i in 0..w {
            let (p, n) = disp_point(geo, di, i as f64, j as f64);
            let pos = p + n * SAMPLE_NUDGE;
            let bump = if g.bumped {
                bump_normals(&g.texinfo, g.normal, n)
            } else {
                [Vec3::ZERO; 3]
            };
            out.push(Sample {
                pos,
                normal: n,
                coverage: 1.0,
                cluster: geo.point_cluster(pos),
                bump,
            });
        }
    }
    out
}

pub fn blocks_light(r: &TriRef) -> bool {
    matches!(r.kind, SurfKind::Opaque | SurfKind::Sky | SurfKind::Blocker)
}

pub fn direct_light(
    bvh: &Bvh,
    geo: &Geometry,
    lights: &[Light],
    pos: Vec3,
    normal: Vec3,
    bump: Option<&[Vec3; 3]>,
    cluster: i32,
    rng: &mut Rng,
    out: &mut [[Vec3; 4]],
    skip_prop: i32,
) {
    let filt = |r: &TriRef| {
        blocks_light(r)
            && !(r.kind == SurfKind::Blocker && r.disp_tri == skip_prop && skip_prop >= 0)
    };
    for l in lights {
        match l.kind {
            EMIT_POINT | EMIT_SPOTLIGHT | EMIT_SURFACE => {
                if !geo.cluster_visible(cluster, l.cluster) {
                    continue;
                }
                let delta = l.origin - pos;
                let dist = delta.length().max(1.0);
                let dir = delta / dist;
                let ndl = normal.dot(dir);
                let bump_any = bump
                    .map(|b| b.iter().any(|bn| bn.dot(dir) > 0.0))
                    .unwrap_or(false);
                if ndl <= 0.0 && !bump_any {
                    continue;
                }
                let atten = l.attenuation(dist, -dir);
                if atten <= 0.0 || l.intensity.max_elem() * atten < 1e-4 {
                    continue;
                }
                if bvh.occluded(pos, dir, 0.0, (dist - 0.5).max(0.0) as f32, &filt) {
                    continue;
                }
                let c = l.intensity * atten;
                let o = &mut out[l.slot];
                o[0] += c * ndl.max(0.0);
                if let Some(b) = bump {
                    for k in 0..3 {
                        o[k + 1] += c * b[k].dot(dir).max(0.0);
                    }
                }
            }
            EMIT_SKYLIGHT => {
                let dir = -l.normal;
                let ndl = normal.dot(dir);
                let bump_any = bump
                    .map(|b| b.iter().any(|bn| bn.dot(dir) > 0.0))
                    .unwrap_or(false);
                if ndl <= 0.0 && !bump_any {
                    continue;
                }
                let samples = if l.sun_spread < 1.0 { 16 } else { 1 };
                let mut frac = 0.0;
                for k in 0..samples {
                    let d = if samples > 1 {
                        jitter_cone(dir, l.sun_spread, k, samples, rng)
                    } else {
                        dir
                    };
                    if sees_sky_filtered(bvh, pos, d, &filt) {
                        frac += 1.0;
                    }
                }
                frac /= samples as f64;
                if frac <= 0.0 {
                    continue;
                }
                let c = l.intensity * frac;
                let o = &mut out[l.slot];
                o[0] += c * ndl.max(0.0);
                if let Some(b) = bump {
                    for k in 0..3 {
                        o[k + 1] += c * b[k].dot(dir).max(0.0);
                    }
                }
            }
            _ => {}
        }
    }
}

pub fn sees_sky(bvh: &Bvh, pos: Vec3, dir: Vec3) -> bool {
    sees_sky_filtered(bvh, pos, dir, &blocks_light)
}

pub fn sees_sky_filtered(bvh: &Bvh, pos: Vec3, dir: Vec3, filt: &dyn Fn(&TriRef) -> bool) -> bool {
    match bvh.closest(pos, dir, 0.0, 1.0e6, filt) {
        Some(h) => bvh.refs[h.tri as usize].kind == SurfKind::Sky,
        None => false,
    }
}

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    pub fn f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }
}

pub fn tangent_frame(n: Vec3) -> (Vec3, Vec3) {
    let a = if n.x.abs() > 0.9 {
        v3(0.0, 1.0, 0.0)
    } else {
        v3(1.0, 0.0, 0.0)
    };
    let t = n.cross(a).normalize();
    let b = n.cross(t);
    (t, b)
}

pub fn cosine_dir(n: Vec3, k: usize, count: usize, rng: &mut Rng) -> Vec3 {
    let side = (count as f64).sqrt().max(1.0) as usize;
    let (i, j) = (k % side, (k / side) % side);
    let u1 = (i as f64 + rng.f64()) / side as f64;
    let u2 = (j as f64 + rng.f64()) / side as f64;
    let r = u1.sqrt();
    let phi = 2.0 * std::f64::consts::PI * u2;
    let (t, b) = tangent_frame(n);
    (t * (r * phi.cos()) + b * (r * phi.sin()) + n * (1.0 - u1).max(0.0).sqrt()).normalize()
}

pub fn sphere_dir(k: usize, count: usize, rng: &mut Rng) -> Vec3 {
    let side = (count as f64).sqrt().max(1.0) as usize;
    let (i, j) = (k % side, (k / side) % side);
    let u1 = (i as f64 + rng.f64()) / side as f64;
    let u2 = (j as f64 + rng.f64()) / side as f64;
    let z = 1.0 - 2.0 * u1;
    let r = (1.0 - z * z).max(0.0).sqrt();
    let phi = 2.0 * std::f64::consts::PI * u2;
    v3(r * phi.cos(), r * phi.sin(), z)
}

fn jitter_cone(dir: Vec3, cos_max: f64, k: usize, count: usize, rng: &mut Rng) -> Vec3 {
    let side = (count as f64).sqrt().max(1.0) as usize;
    let (i, j) = (k % side, (k / side) % side);
    let u1 = (i as f64 + rng.f64()) / side as f64;
    let u2 = (j as f64 + rng.f64()) / side as f64;
    let cos_t = 1.0 - u1 * (1.0 - cos_max);
    let sin_t = (1.0 - cos_t * cos_t).max(0.0).sqrt();
    let phi = 2.0 * std::f64::consts::PI * u2;
    let (t, b) = tangent_frame(dir);
    (t * (sin_t * phi.cos()) + b * (sin_t * phi.sin()) + dir * cos_t).normalize()
}
