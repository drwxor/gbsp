use crate::geom::Geometry;
use crate::lightmap::*;
use crate::trace::{Bvh, Hit, SurfKind};
use gold_common::math::*;
use rayon::prelude::*;

pub struct Patch {
    pub face: usize,
    pub luxels: Vec<u32>,
    pub reflectivity: Vec3,
    pub direct: Vec<Vec3>,
    pub indirect: Vec<Vec3>,
}

pub struct Scene<'a> {
    pub geo: &'a Geometry,
    pub bvh: &'a Bvh,
    pub faces: &'a [Option<FaceLight>],
    pub patches: &'a [Patch],
    pub sky_ambient: Vec3,
    pub bounce_scale: f64,
    pub active: &'a [usize],
}

pub enum RayResult {
    Surface {
        face: usize,
        luxel: usize,
        patch: usize,
        front: bool,
    },
    Sky,
    Nothing,
}

impl<'a> Scene<'a> {
    pub fn resolve_hit(&self, dir: Vec3, h: &Hit) -> RayResult {
        let r = self.bvh.refs[h.tri as usize];
        match r.kind {
            SurfKind::Sky => return RayResult::Sky,
            SurfKind::Blocker => return RayResult::Nothing,
            SurfKind::Opaque => {}
        }
        if r.face < 0 {
            return RayResult::Nothing;
        }
        let fi = r.face as usize;
        let Some(fl) = &self.faces[fi] else {
            return RayResult::Nothing;
        };
        let g = &self.geo.geoms[fi];
        let p = self.bvh.hit_point(h);
        let (s, t, n) = match g.disp {
            Some(di) => {
                let d = &self.geo.disps[di];
                let tri = d.tris[r.disp_tri as usize];
                let (u, v) = (h.u as f64, h.v as f64);
                let w0 = 1.0 - u - v;
                let l = |k: usize| d.luxel[tri[k]];
                let s = l(0).0 * w0 + l(1).0 * u + l(2).0 * v;
                let t = l(0).1 * w0 + l(1).1 * u + l(2).1 * v;
                let n = d.normals[tri[0]] * w0 + d.normals[tri[1]] * u + d.normals[tri[2]] * v;
                (s, t, n)
            }
            None => {
                let (s, t) = g.world_to_luxel(p);
                (s, t, g.normal)
            }
        };
        let luxel = fl.luxel_index(s, t);
        let patch = fl.patch[luxel] as usize;
        RayResult::Surface {
            face: fi,
            luxel,
            patch,
            front: n.dot(dir) < 0.0,
        }
    }

    pub fn trace(&self, org: Vec3, dir: Vec3) -> RayResult {
        match self.bvh.closest(org, dir, 0.01, 1.0e6, &blocks_light) {
            Some(h) => self.resolve_hit(dir, &h),
            None => RayResult::Nothing,
        }
    }

    pub fn exitance(
        &self,
        face: usize,
        luxel: usize,
        patch: usize,
        front: bool,
        slot: usize,
    ) -> Vec3 {
        if !front {
            return Vec3::ZERO;
        }
        let Some(fl) = &self.faces[face] else {
            return Vec3::ZERO;
        };
        let p = &self.patches[patch];
        (fl.direct_flat(slot, luxel) + p.indirect[slot]).mul_elem(p.reflectivity)
            * self.bounce_scale
    }

    pub fn radiance(&self, r: &RayResult, slot: usize) -> Vec3 {
        match *r {
            RayResult::Sky if slot == 0 => self.sky_ambient,
            RayResult::Surface {
                face,
                luxel,
                patch,
                front,
            } => self.exitance(face, luxel, patch, front, slot),
            _ => Vec3::ZERO,
        }
    }
}

pub fn build_patches(
    geo: &Geometry,
    faces: &mut [Option<FaceLight>],
    chop: f64,
    nslots: usize,
) -> Vec<Patch> {
    let mut patches = Vec::new();
    for fl in faces.iter_mut().flatten() {
        let g = &geo.geoms[fl.face];
        let luxel_size = g.lm_sdir.length().max(g.lm_tdir.length()).max(1e-3);
        let step = ((chop / luxel_size).round() as usize).max(1);
        let pw = fl.w.div_ceil(step);
        let ph = fl.h.div_ceil(step);
        let base = patches.len();
        for _ in 0..pw * ph {
            patches.push(Patch {
                face: fl.face,
                luxels: Vec::new(),
                reflectivity: g.reflectivity,
                direct: vec![Vec3::ZERO; nslots],
                indirect: vec![Vec3::ZERO; nslots],
            });
        }
        for j in 0..fl.h {
            for i in 0..fl.w {
                let pi = base + (j / step) * pw + (i / step);
                let li = j * fl.w + i;
                fl.patch[li] = pi as u32;
                patches[pi].luxels.push(li as u32);
            }
        }
    }
    for p in patches.iter_mut() {
        let fl = faces[p.face].as_ref().unwrap();
        for slot in 0..nslots {
            if fl.direct[slot].is_empty() {
                continue;
            }
            let mut sum = Vec3::ZERO;
            let mut wsum = 0.0;
            for &l in &p.luxels {
                let w = fl.samples[l as usize].coverage.max(0.05);
                sum += fl.direct[slot][l as usize][0] * w;
                wsum += w;
            }
            p.direct[slot] = if wsum > 0.0 { sum / wsum } else { Vec3::ZERO };
        }
    }
    patches
}

pub fn active_slots(patches: &[Patch], nslots: usize) -> Vec<usize> {
    (0..nslots)
        .filter(|&s| s == 0 || patches.iter().any(|p| p.direct[s].max_elem() > 1e-3))
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub fn solve(
    geo: &Geometry,
    bvh: &Bvh,
    faces: &[Option<FaceLight>],
    patches: &mut [Patch],
    sky_ambient: Vec3,
    rays: usize,
    max_bounce: usize,
    scale: f64,
    active: &[usize],
) -> usize {
    let transfers: Vec<(Vec<(u32, f32)>, f64)> = {
        let scene = Scene {
            geo,
            bvh,
            faces,
            patches,
            sky_ambient,
            bounce_scale: scale,
            active,
        };
        (0..patches.len())
            .into_par_iter()
            .map(|pi| {
                let p = &scene.patches[pi];
                let fl = faces[p.face].as_ref().unwrap();
                let mut rng = Rng::new(pi as u64 * 7919 + 17);
                let mut hits: std::collections::HashMap<u32, u32> =
                    std::collections::HashMap::new();
                let mut sky = 0usize;
                let cand: Vec<u32> = {
                    let c: Vec<u32> = p
                        .luxels
                        .iter()
                        .copied()
                        .filter(|&l| fl.samples[l as usize].coverage > 0.25)
                        .collect();
                    if c.is_empty() { p.luxels.clone() } else { c }
                };
                for k in 0..rays {
                    let l = cand[(rng.next_u64() as usize) % cand.len()] as usize;
                    let s = &fl.samples[l];
                    let dir = cosine_dir(s.normal, k, rays, &mut rng);
                    match scene.trace(s.pos, dir) {
                        RayResult::Sky => sky += 1,
                        RayResult::Surface {
                            patch, front: true, ..
                        } => *hits.entry(patch as u32).or_insert(0) += 1,
                        _ => {}
                    }
                }
                let inv = 1.0 / rays as f32;
                (
                    hits.into_iter().map(|(k, c)| (k, c as f32 * inv)).collect(),
                    sky as f64 / rays as f64,
                )
            })
            .collect()
    };
    let n = patches.len();
    let mut most = 0;
    for &slot in active {
        let mut e: Vec<Vec3> = patches.iter().map(|p| p.direct[slot]).collect();
        let base: Vec<Vec3> = patches
            .iter()
            .zip(&transfers)
            .map(|(p, t)| {
                p.direct[slot]
                    + if slot == 0 {
                        sky_ambient * t.1
                    } else {
                        Vec3::ZERO
                    }
            })
            .collect();
        for b in 0..max_bounce {
            let next: Vec<Vec3> = (0..n)
                .into_par_iter()
                .map(|i| {
                    let mut acc = base[i];
                    for &(j, w) in &transfers[i].0 {
                        acc += e[j as usize].mul_elem(patches[j as usize].reflectivity)
                            * (w as f64 * scale);
                    }
                    acc
                })
                .collect();
            let mut maxd: f64 = 0.0;
            for i in 0..n {
                maxd = maxd.max((next[i] - e[i]).abs().max_elem());
            }
            e = next;
            most = most.max(b + 1);
            if maxd < 0.1 {
                break;
            }
        }
        for (p, ei) in patches.iter_mut().zip(e) {
            p.indirect[slot] = ei - p.direct[slot];
        }
    }
    most
}

pub fn final_gather(scene: &Scene, fl: &FaceLight, rays: usize) -> Vec<Vec<[Vec3; 4]>> {
    let g = &scene.geo.geoms[fl.face];
    let nslots = fl.direct.len();
    let mut out: Vec<Vec<[Vec3; 4]>> = (0..nslots)
        .map(|s| {
            if scene.active.contains(&s) {
                vec![[Vec3::ZERO; 4]; fl.samples.len()]
            } else {
                Vec::new()
            }
        })
        .collect();
    let inv = 1.0 / rays as f64;
    for (li, s) in fl.samples.iter().enumerate() {
        let mut rng = Rng::new((fl.face as u64) << 20 | li as u64);
        for k in 0..rays {
            let dir = cosine_dir(s.normal, k, rays, &mut rng);
            let hit = scene.trace(s.pos, dir);
            let bw = if g.bumped {
                let cosn = dir.dot(s.normal).max(1e-3);
                [0, 1, 2].map(|b| (dir.dot(s.bump[b]).max(0.0) / cosn).min(3.0))
            } else {
                [0.0; 3]
            };
            for &slot in scene.active {
                let l = scene.radiance(&hit, slot);
                if l.max_elem() <= 0.0 {
                    continue;
                }
                let acc = &mut out[slot][li];
                acc[0] += l * inv;
                if g.bumped {
                    for b in 0..3 {
                        acc[b + 1] += l * (bw[b] * inv);
                    }
                }
            }
        }
    }
    out
}

pub fn smooth(fl: &FaceLight, data: &mut [[Vec3; 4]], passes: usize) {
    let (w, h) = (fl.w, fl.h);
    if data.len() != w * h {
        return;
    }
    for _ in 0..passes {
        for pass in 0..2 {
            let src = data.to_vec();
            for j in 0..h {
                for i in 0..w {
                    let idx = j * w + i;
                    let (a, b) = if pass == 0 {
                        (
                            if i > 0 { idx - 1 } else { idx },
                            if i + 1 < w { idx + 1 } else { idx },
                        )
                    } else {
                        (
                            if j > 0 { idx - w } else { idx },
                            if j + 1 < h { idx + w } else { idx },
                        )
                    };
                    for p in 0..4 {
                        data[idx][p] = (src[a][p] + src[idx][p] * 2.0 + src[b][p]) * 0.25;
                    }
                }
            }
        }
    }
}
