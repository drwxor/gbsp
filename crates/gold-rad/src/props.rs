use crate::lightmap::*;
use crate::lights::Light;
use crate::radiosity::{RayResult, Scene};
use crate::trace::SurfKind;
use gold_common::bsp::*;
use gold_common::fs::GameFs;
use gold_common::math::*;
use gold_common::mdl;
use rayon::prelude::*;

const STATIC_PROP_NO_PER_VERTEX_LIGHTING: u8 = 0x40;

pub struct PropInfo {
    pub index: usize,
    pub model: String,
    pub origin: Vec3,
    pub angles: Vec3,
    pub flags: u8,
}

pub fn parse_static_props(bsp: &BspFile) -> Vec<PropInfo> {
    let Some(g) = bsp.game_lump(GAMELUMP_STATIC_PROPS) else {
        return Vec::new();
    };
    let d = &g.data;
    let rd_i32 = |o: usize| {
        d.get(o..o + 4)
            .map(|b| i32::from_le_bytes(b.try_into().unwrap()))
            .unwrap_or(0)
    };
    let rd_f32 = |o: usize| {
        d.get(o..o + 4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .unwrap_or(0.0) as f64
    };
    let ndict = rd_i32(0).max(0) as usize;
    let mut o = 4;
    let names: Vec<String> = (0..ndict)
        .map(|i| {
            let s = &d[o + i * 128..o + i * 128 + 128];
            String::from_utf8_lossy(&s[..s.iter().position(|&b| b == 0).unwrap_or(128)])
                .into_owned()
        })
        .collect();
    o += 128 * ndict;
    let nleaf = rd_i32(o).max(0) as usize;
    o += 4 + 2 * nleaf;
    let nprops = rd_i32(o).max(0) as usize;
    o += 4;
    if nprops == 0 {
        return Vec::new();
    }
    let stride = (d.len() - o) / nprops;
    (0..nprops)
        .map(|p| {
            let b = o + p * stride;
            let ty = u16::from_le_bytes([d[b + 24], d[b + 25]]) as usize;
            PropInfo {
                index: p,
                model: names.get(ty).cloned().unwrap_or_default(),
                origin: v3(rd_f32(b), rd_f32(b + 4), rd_f32(b + 8)),
                angles: v3(rd_f32(b + 12), rd_f32(b + 16), rd_f32(b + 20)),
                flags: d[b + 31],
            }
        })
        .collect()
}

fn gather(
    scene: &Scene,
    pos: Vec3,
    normal: Vec3,
    rays: usize,
    seed: u64,
    skip_prop: i32,
    scale: f64,
) -> Vec3 {
    let mut rng = Rng::new(seed);
    let filt = |r: &crate::trace::TriRef| {
        blocks_light(r)
            && !(r.kind == SurfKind::Blocker && r.disp_tri == skip_prop && skip_prop >= 0)
    };
    let mut acc = Vec3::ZERO;
    for k in 0..rays {
        let dir = cosine_dir(normal, k, rays, &mut rng);
        if let Some(h) = scene.bvh.closest(pos, dir, 0.01, 1.0e6, &filt) {
            acc += scene.radiance(&scene.resolve_hit(dir, &h), 0);
        }
    }
    acc * (scale / rays.max(1) as f64)
}

fn gather_lightmaps(
    scene: &Scene,
    pos: Vec3,
    normal: Vec3,
    rays: usize,
    seed: u64,
    nslots: usize,
) -> Vec<Vec3> {
    let mut rng = Rng::new(seed);
    let mut acc = vec![Vec3::ZERO; nslots];
    for k in 0..rays {
        let dir = cosine_dir(normal, k, rays, &mut rng);
        match scene.trace(pos, dir) {
            RayResult::Surface {
                face,
                luxel,
                patch,
                front: true,
            } => {
                let Some(fl) = scene.faces[face].as_ref() else {
                    continue;
                };
                for (slot, a) in acc.iter_mut().enumerate() {
                    *a += fl.direct_flat(slot, luxel) + scene.patches[patch].indirect[slot];
                }
            }
            RayResult::Sky => acc[0] += scene.sky_ambient,
            _ => {}
        }
    }
    acc.iter()
        .map(|a| *a * (DETAIL_LIGHT_SCALE / rays.max(1) as f64))
        .collect()
}

const DETAIL_LIGHT_SCALE: f64 = 1.15;

const PROP_INDIRECT_SCALE: f64 = 2.0;

fn unbury(scene: &Scene, pos: Vec3) -> Vec3 {
    let solid = |p: Vec3| scene.geo.leafs[scene.geo.point_leaf(p)].contents & CONTENTS_SOLID != 0;
    if !solid(pos) {
        return pos;
    }
    let mut p = pos;
    for _ in 0..64 {
        p.z += 8.0;
        if !solid(p) {
            return p;
        }
    }
    pos
}

fn encode_vertex(v: Vec3) -> [u8; 4] {
    let q = |c: f64| {
        (255.0 * (c.max(0.0) / 255.0).powf(1.0 / 2.2) * 0.5)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    [q(v.z), q(v.y), q(v.x), 255]
}

fn vhv_file(checksum: i32, meshes: &[(u32, Vec<[u8; 4]>)]) -> Vec<u8> {
    let total: usize = meshes.iter().map(|m| m.1.len()).sum();
    let header = 40 + meshes.len() * 28;
    let align = |x: usize| x.div_ceil(512) * 512;
    let mut out = Vec::new();
    let i = |o: &mut Vec<u8>, v: u32| o.extend_from_slice(&v.to_le_bytes());
    i(&mut out, 2);
    i(&mut out, checksum as u32);
    i(&mut out, 4);
    i(&mut out, 4);
    i(&mut out, total as u32);
    i(&mut out, meshes.len() as u32);
    for _ in 0..4 {
        i(&mut out, 0);
    }
    let mut ofs = align(header);
    for (lod, colors) in meshes {
        i(&mut out, *lod);
        i(&mut out, colors.len() as u32);
        i(&mut out, ofs as u32);
        for _ in 0..4 {
            i(&mut out, 0);
        }
        ofs += colors.len() * 4;
    }
    out.resize(align(out.len()), 0);
    for (_, colors) in meshes {
        for c in colors {
            out.extend_from_slice(c);
        }
    }
    out.resize(align(out.len()), 0);
    out
}

pub fn light_static_props(
    bsp: &BspFile,
    fs: &GameFs,
    scene: &Scene,
    lights: &[Light],
    rays: usize,
) -> Vec<(usize, Vec<u8>)> {
    let props = parse_static_props(bsp);
    if props.is_empty() {
        return Vec::new();
    }
    let mut models: std::collections::HashMap<String, Option<mdl::ModelMeshes>> =
        Default::default();
    for p in &props {
        models
            .entry(p.model.to_ascii_lowercase())
            .or_insert_with(|| mdl::load_model_meshes(fs, &p.model));
    }
    let nslots = scene
        .faces
        .iter()
        .flatten()
        .next()
        .map(|f| f.direct.len())
        .unwrap_or(1);
    let files: Vec<Option<(usize, Vec<u8>)>> = props
        .par_iter()
        .map(|p| {
            if p.flags & STATIC_PROP_NO_PER_VERTEX_LIGHTING != 0 {
                return None;
            }
            let m = models.get(&p.model.to_ascii_lowercase())?.as_ref()?;
            let rot = angle_matrix(p.angles);
            let mut rng = Rng::new(p.index as u64 * 7717 + 11);
            let mut acc = vec![[Vec3::ZERO; 4]; nslots];
            let mut meshes = Vec::new();
            for mesh in &m.meshes {
                let mut colors = Vec::with_capacity(mesh.positions.len());
                for (vi, (&lp, &ln)) in mesh.positions.iter().zip(&mesh.normals).enumerate() {
                    let n = mat_mul_vec(&rot, ln).normalize();
                    let pos = unbury(scene, p.origin + mat_mul_vec(&rot, lp) + n * 0.5);
                    let cluster = scene.geo.point_cluster(pos);
                    for a in acc.iter_mut() {
                        *a = [Vec3::ZERO; 4];
                    }
                    direct_light(
                        scene.bvh,
                        scene.geo,
                        lights,
                        pos,
                        n,
                        None,
                        cluster,
                        &mut rng,
                        &mut acc,
                        p.index as i32,
                    );
                    let seed = (p.index as u64) << 32 | (mesh.lod as u64) << 24 | vi as u64;
                    let light = acc[0][0]
                        + gather(
                            scene,
                            pos,
                            n,
                            rays,
                            seed,
                            p.index as i32,
                            PROP_INDIRECT_SCALE,
                        );
                    colors.push(encode_vertex(light));
                }
                meshes.push((mesh.lod, colors));
            }
            Some((p.index, vhv_file(m.checksum, &meshes)))
        })
        .collect();
    files.into_iter().flatten().collect()
}

pub fn light_detail_props(
    bsp: &BspFile,
    scene: &Scene,
    styles: &[i32],
) -> Option<(u16, Vec<u8>, Vec<u8>)> {
    let g = bsp.game_lump(GAMELUMP_DETAIL_PROPS)?;
    let mut d = g.data.clone();
    let version = g.version;
    let rd_i32 = |d: &[u8], o: usize| i32::from_le_bytes(d[o..o + 4].try_into().unwrap());
    if d.len() < 12 {
        return None;
    }
    let ndict = rd_i32(&d, 0).max(0) as usize;
    let mut o = 4 + ndict * 128;
    let nsprites = rd_i32(&d, o).max(0) as usize;
    o += 4 + nsprites * 32;
    let nobj = rd_i32(&d, o).max(0) as usize;
    o += 4;
    if nobj == 0 {
        return None;
    }
    let stride = (d.len() - o) / nobj;
    let nslots = styles.len();
    let origins: Vec<Vec3> = (0..nobj)
        .map(|k| {
            let b = o + k * stride;
            let f = |x: usize| f32::from_le_bytes(d[b + x..b + x + 4].try_into().unwrap()) as f64;
            v3(f(0), f(4), f(8))
        })
        .collect();
    let lit: Vec<Vec<Vec3>> = origins
        .par_iter()
        .enumerate()
        .map(|(k, &org)| {
            let pos = org + v3(0.0, 0.0, 4.0);
            gather_lightmaps(
                scene,
                pos,
                v3(0.0, 0.0, -1.0),
                32,
                k as u64 ^ 0xdeadbeef,
                nslots,
            )
        })
        .collect();
    let mut style_lump: Vec<u8> = Vec::new();
    let mut nstyles = 0u32;
    for (k, l) in lit.iter().enumerate() {
        let b = o + k * stride;
        d[b + 28..b + 32]
            .copy_from_slice(bytemuck::bytes_of(&vector_to_color_rgbexp32(l[0].to_f32())));
        let first = nstyles;
        let mut count = 0u8;
        for (slot, v) in l.iter().enumerate().skip(1) {
            if v.max_elem() > 0.5 && count < 255 {
                style_lump
                    .extend_from_slice(bytemuck::bytes_of(&vector_to_color_rgbexp32(v.to_f32())));
                style_lump.push(styles[slot] as u8);
                nstyles += 1;
                count += 1;
            }
        }
        d[b + 32..b + 36].copy_from_slice(&first.to_le_bytes());
        d[b + 36] = count;
    }
    let mut sl = nstyles.to_le_bytes().to_vec();
    sl.extend(style_lump);
    Some((version, d, sl))
}
