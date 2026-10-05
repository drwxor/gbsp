use crate::lightmap::{Rng, sphere_dir};
use crate::radiosity::{RayResult, Scene};
use gold_common::bsp::*;
use gold_common::math::*;
use rayon::prelude::*;

const BOX_DIRS: [Vec3; 6] = [
    v3(1.0, 0.0, 0.0),
    v3(-1.0, 0.0, 0.0),
    v3(0.0, 1.0, 0.0),
    v3(0.0, -1.0, 0.0),
    v3(0.0, 0.0, 1.0),
    v3(0.0, 0.0, -1.0),
];

pub struct LeafAmbient {
    pub index: Vec<DLeafAmbientIndex>,
    pub samples: Vec<DLeafAmbientLighting>,
}

fn ambient_cube(scene: &Scene, pos: Vec3, rays: usize, seed: u64) -> [Vec3; 6] {
    let mut rng = Rng::new(seed);
    let mut acc = [Vec3::ZERO; 6];
    let mut wsum = [0.0f64; 6];
    for k in 0..rays {
        let dir = sphere_dir(k, rays, &mut rng);
        let l = scene.radiance(&scene.trace(pos, dir), 0);
        for (a, d) in BOX_DIRS.iter().enumerate() {
            let w = dir.dot(*d);
            if w > 0.0 {
                acc[a] += l * w;
                wsum[a] += w;
            }
        }
    }
    let mut out = [Vec3::ZERO; 6];
    for a in 0..6 {
        if wsum[a] > 0.0 {
            out[a] = acc[a] / wsum[a];
        }
    }
    out
}

fn is_buried(scene: &Scene, pos: Vec3) -> bool {
    let mut back = 0;
    for d in BOX_DIRS {
        if let RayResult::Surface { front: false, .. } = scene.trace(pos, d) {
            back += 1;
        }
    }
    back >= 2
        || matches!(
            scene.trace(pos, v3(0.0, 0.0, 1.0)),
            RayResult::Surface { front: false, .. }
        )
}

pub fn compute(scene: &Scene, rays: usize) -> LeafAmbient {
    let geo = scene.geo;
    let per_leaf: Vec<Vec<DLeafAmbientLighting>> = (0..geo.leafs.len())
        .into_par_iter()
        .map(|li| {
            let leaf = &geo.leafs[li];
            if leaf.contents & CONTENTS_SOLID != 0 || leaf.cluster < 0 {
                return Vec::new();
            }
            let mins = v3(
                leaf.mins[0] as f64,
                leaf.mins[1] as f64,
                leaf.mins[2] as f64,
            );
            let maxs = v3(
                leaf.maxs[0] as f64,
                leaf.maxs[1] as f64,
                leaf.maxs[2] as f64,
            );
            let size = maxs - mins;
            if size.x <= 0.0 || size.y <= 0.0 || size.z <= 0.0 {
                return Vec::new();
            }
            let volume = size.x * size.y * size.z;
            let want = ((volume / (256.0 * 256.0 * 256.0)).sqrt() as usize).clamp(1, 8);
            let mut rng = Rng::new(li as u64 * 31 + 5);
            let mut points = Vec::new();
            let mut attempts = 0;
            while points.len() < want && attempts < want * 12 {
                let p = if attempts == 0 {
                    (mins + maxs) * 0.5
                } else {
                    v3(
                        mins.x + size.x * rng.f64(),
                        mins.y + size.y * rng.f64(),
                        mins.z + size.z * rng.f64(),
                    )
                };
                attempts += 1;
                if geo.point_leaf(p) != li || is_buried(scene, p) {
                    continue;
                }
                points.push(p);
            }
            if points.is_empty() {
                points.push((mins + maxs) * 0.5);
            }
            points
                .iter()
                .enumerate()
                .map(|(k, &p)| {
                    let cube = ambient_cube(scene, p, rays, (li as u64) << 8 | k as u64);
                    let rel = (p - mins).mul_elem(v3(1.0 / size.x, 1.0 / size.y, 1.0 / size.z));
                    let q = |f: f64| (f * 255.0).round().clamp(0.0, 255.0) as u8;
                    let mut s = DLeafAmbientLighting {
                        x: q(rel.x),
                        y: q(rel.y),
                        z: q(rel.z),
                        ..Default::default()
                    };
                    for a in 0..6 {
                        s.cube[a] = vector_to_color_rgbexp32(cube[a].to_f32());
                    }
                    s
                })
                .collect()
        })
        .collect();
    let mut index = Vec::with_capacity(per_leaf.len());
    let mut samples = Vec::new();
    for l in per_leaf {
        index.push(DLeafAmbientIndex {
            ambient_sample_count: l.len() as u16,
            first_ambient_sample: samples.len() as u16,
        });
        samples.extend(l);
    }
    LeafAmbient { index, samples }
}
