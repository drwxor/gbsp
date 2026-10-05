#![allow(clippy::needless_range_loop, clippy::too_many_arguments)]

pub mod ambient;
pub mod geom;
pub mod lightmap;
pub mod lights;
pub mod props;
pub mod radiosity;
pub mod trace;

use geom::Geometry;
use gold_common::bsp::*;
use gold_common::fs::GameFs;
use gold_common::math::*;
use gold_common::mdl;
use lightmap::*;
use rayon::prelude::*;
use std::path::PathBuf;
use std::time::Instant;
use trace::{Bvh, BvhBuilder, SurfKind, TriRef};

pub struct Options {
    pub bsp: PathBuf,
    pub game_dir: Option<PathBuf>,
    pub bounces: usize,
    pub chop: f64,
    pub transfer_rays: usize,
    pub gather_rays: usize,
    pub ambient_rays: usize,
    pub supersample: bool,
    pub ldr: bool,
    pub hdr: bool,
    pub prop_shadows: bool,
    pub bounce_scale: f64,
    pub lights_file: Option<PathBuf>,
    pub prop_rays: usize,
    pub prop_lighting: bool,
}

fn usage() {
    eprintln!("usage: grad [options] <mapfile>");
    eprintln!("  -game <dir>      game directory (for static prop collision models)");
    eprintln!("  -bounce <n>      maximum radiosity bounces (default 100)");
    eprintln!("  -chop <n>        patch size in world units (default 64)");
    eprintln!("  -fast            quick, noisy preview lighting");
    eprintln!("  -final           higher quality indirect lighting");
    eprintln!("  -noextra         disable shadow edge supersampling");
    eprintln!("  -ldr | -hdr | -both   which lighting data to write (default both)");
    eprintln!("  -nopropshadows   static props don't cast shadows");
    eprintln!("  -bouncescale <f> scale reflected light (default 1.0)");
    eprintln!("  -lights <file>   extra texture light (.rad) file");
    eprintln!("  -nostaticproplighting  don.t bake per-vertex static prop lighting");
}

pub fn main_with_args(args: Vec<String>) -> i32 {
    let mut o = Options {
        bsp: PathBuf::new(),
        game_dir: None,
        bounces: 100,
        chop: 64.0,
        transfer_rays: 256,
        gather_rays: 144,
        ambient_rays: 256,
        supersample: true,
        ldr: true,
        hdr: true,
        prop_shadows: true,
        bounce_scale: 1.0,
        lights_file: None,
        prop_rays: 64,
        prop_lighting: true,
    };
    let mut input = None;
    let mut i = 1;
    while i < args.len() {
        let a = args[i].as_str();
        match a.to_ascii_lowercase().as_str() {
            "-game" | "-vproject" => {
                i += 1;
                o.game_dir = args.get(i).map(PathBuf::from);
            }
            "-bounce" => {
                i += 1;
                o.bounces = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(100);
            }
            "-chop" => {
                i += 1;
                o.chop = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(64.0);
            }
            "-threads" => {
                i += 1;
                if let Some(n) = args.get(i).and_then(|s| s.parse::<usize>().ok()) {
                    let _ = rayon::ThreadPoolBuilder::new()
                        .num_threads(n)
                        .build_global();
                }
            }
            "-fast" => {
                o.transfer_rays = 64;
                o.gather_rays = 36;
                o.ambient_rays = 64;
                o.supersample = false;
            }
            "-final" => {
                o.transfer_rays = 1024;
                o.gather_rays = 400;
                o.ambient_rays = 1024;
            }
            "-noextra" => o.supersample = false,
            "-extra" => o.supersample = true,
            "-ldr" => {
                o.ldr = true;
                o.hdr = false;
            }
            "-hdr" => {
                o.ldr = false;
                o.hdr = true;
            }
            "-both" => {
                o.ldr = true;
                o.hdr = true;
            }
            "-nopropshadows" => o.prop_shadows = false,
            "-nostaticproplighting" => o.prop_lighting = false,
            "-lights" => {
                i += 1;
                o.lights_file = args.get(i).map(PathBuf::from);
            }
            "-bouncescale" => {
                i += 1;
                o.bounce_scale = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(1.0);
            }
            _ if a.starts_with('-') => eprintln!("ignoring unknown option {a}"),
            _ => input = Some(PathBuf::from(a)),
        }
        i += 1;
    }
    let Some(input) = input else {
        usage();
        return 1;
    };
    o.bsp = input.with_extension("bsp");
    match run(&o) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("Error: {e}");
            1
        }
    }
}

fn load_static_prop_hulls(bsp: &BspFile, fs: &GameFs, builder: &mut BvhBuilder) -> usize {
    let Some(g) = bsp.game_lump(GAMELUMP_STATIC_PROPS) else {
        return 0;
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
    let mut names = Vec::new();
    for i in 0..ndict {
        let s = &d[o + i * 128..o + i * 128 + 128];
        names.push(
            String::from_utf8_lossy(&s[..s.iter().position(|&b| b == 0).unwrap_or(128)])
                .into_owned(),
        );
    }
    o += 128 * ndict;
    let nleaf = rd_i32(o).max(0) as usize;
    o += 4 + 2 * nleaf;
    let nprops = rd_i32(o).max(0) as usize;
    o += 4;
    if nprops == 0 {
        return 0;
    }
    let stride = (d.len() - o) / nprops;
    let mut models: Vec<Option<mdl::ModelInfo>> =
        names.iter().map(|n| mdl::load_model(fs, n)).collect();
    let mut count = 0;
    for p in 0..nprops {
        let b = o + p * stride;
        let origin = v3(rd_f32(b), rd_f32(b + 4), rd_f32(b + 8));
        let angles = v3(rd_f32(b + 12), rd_f32(b + 16), rd_f32(b + 20));
        let ty = u16::from_le_bytes([d[b + 24], d[b + 25]]) as usize;
        let flags = d[b + 31];
        if flags & 0x10 != 0 {
            continue;
        }
        let Some(Some(m)) = models.get_mut(ty) else {
            continue;
        };
        let rot = angle_matrix(angles);
        for hull in &m.collision {
            for t in hull {
                let w: Vec<Vec3> = t.iter().map(|&v| origin + mat_mul_vec(&rot, v)).collect();
                builder.add(
                    w[0],
                    w[1],
                    w[2],
                    TriRef {
                        kind: SurfKind::Blocker,
                        face: -1,
                        disp_tri: p as i32,
                    },
                );
            }
        }
        count += 1;
    }
    count
}

fn build_bvh(
    geo: &Geometry,
    ents: &[Entity],
    bsp: &BspFile,
    fs: &GameFs,
    prop_shadows: bool,
) -> (Bvh, usize) {
    let mut b = BvhBuilder::new();
    let mut shadow_models = vec![false; geo.models.len()];
    shadow_models[0] = true;
    for e in ents {
        if e.get("vrad_brush_cast_shadows")
            .map(|v| v.trim() != "0")
            .unwrap_or(false)
            && let Some(m) = e
                .get("model")
                .and_then(|m| m.strip_prefix('*'))
                .and_then(|m| m.parse::<usize>().ok())
            && m < shadow_models.len()
        {
            shadow_models[m] = true;
        }
    }
    for g in &geo.geoms {
        if !shadow_models[g.model] {
            continue;
        }
        if g.flags & (SURF_TRANS | SURF_WARP | SURF_NODRAW) != 0 {
            continue;
        }
        let kind = if g.flags & SURF_SKY != 0 {
            SurfKind::Sky
        } else {
            SurfKind::Opaque
        };
        if let Some(di) = g.disp {
            let d = &geo.disps[di];
            for (ti, t) in d.tris.iter().enumerate() {
                b.add(
                    d.positions[t[0]],
                    d.positions[t[1]],
                    d.positions[t[2]],
                    TriRef {
                        kind,
                        face: g.index as i32,
                        disp_tri: ti as i32,
                    },
                );
            }
        } else {
            for k in 1..g.points.len().saturating_sub(1) {
                b.add(
                    g.points[0],
                    g.points[k],
                    g.points[k + 1],
                    TriRef {
                        kind,
                        face: g.index as i32,
                        disp_tri: -1,
                    },
                );
            }
        }
    }
    let props = if prop_shadows {
        load_static_prop_hulls(bsp, fs, &mut b)
    } else {
        0
    };
    (b.build(), props)
}

fn vertex_normals(geo: &Geometry) -> Vec<Vec<Vec3>> {
    use std::collections::HashMap;
    let key = |p: Vec3| {
        (
            (p.x * 8.0).round() as i64,
            (p.y * 8.0).round() as i64,
            (p.z * 8.0).round() as i64,
        )
    };
    let mut by_pos: HashMap<(i64, i64, i64), Vec<usize>> = HashMap::new();
    for (fi, g) in geo.geoms.iter().enumerate() {
        if geo.faces[fi].smoothing_groups == 0 {
            continue;
        }
        for p in &g.points {
            by_pos.entry(key(*p)).or_default().push(fi);
        }
    }
    geo.geoms
        .iter()
        .enumerate()
        .map(|(fi, g)| {
            let groups = geo.faces[fi].smoothing_groups;
            g.points
                .iter()
                .map(|p| {
                    if groups == 0 {
                        return g.normal;
                    }
                    let mut n = Vec3::ZERO;
                    let mut seen = Vec::new();
                    for &of in by_pos.get(&key(*p)).map(|v| v.as_slice()).unwrap_or(&[]) {
                        if geo.faces[of].smoothing_groups & groups != 0 && !seen.contains(&of) {
                            seen.push(of);
                            n += geo.geoms[of].normal * geo.faces[of].area.max(1.0) as f64;
                        }
                    }
                    if n.length() < 1e-6 {
                        g.normal
                    } else {
                        n.normalize()
                    }
                })
                .collect()
        })
        .collect()
}

fn luminance(v: Vec3) -> f64 {
    v.x * 0.299 + v.y * 0.587 + v.z * 0.114
}

fn supersample_face(
    geo: &Geometry,
    bvh: &Bvh,
    lights: &[lights::Light],
    fl: &mut FaceLight,
    nslots: usize,
) -> usize {
    let g = &geo.geoms[fl.face];
    let (w, h) = (fl.w, fl.h);
    let lum: Vec<f64> = (0..w * h)
        .map(|l| (0..nslots).map(|s| luminance(fl.direct_flat(s, l))).sum())
        .collect();
    let mut todo = Vec::new();
    for j in 0..h {
        for i in 0..w {
            let idx = j * w + i;
            let c = lum[idx];
            let mut maxd: f64 = 0.0;
            let mut maxv = c;
            for (di, dj) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1)] {
                let (ni, nj) = (i as i64 + di, j as i64 + dj);
                if ni < 0 || nj < 0 || ni >= w as i64 || nj >= h as i64 {
                    continue;
                }
                let o = lum[nj as usize * w + ni as usize];
                maxd = maxd.max((o - c).abs());
                maxv = maxv.max(o);
            }
            if maxd > 1.0 + 0.05 * maxv {
                todo.push(idx);
            }
        }
    }
    let poly: Vec<(f64, f64)> = g.points.iter().map(|&p| g.world_to_luxel(p)).collect();
    let inside = |s: f64, t: f64| -> bool {
        let mut sign = 0.0;
        for k in 0..poly.len() {
            let a = poly[k];
            let b = poly[(k + 1) % poly.len()];
            let cr = (b.0 - a.0) * (t - a.1) - (b.1 - a.1) * (s - a.0);
            if cr.abs() < 1e-9 {
                continue;
            }
            if sign == 0.0 {
                sign = cr.signum();
            } else if cr.signum() != sign {
                return false;
            }
        }
        true
    };
    let n = todo.len();
    let mut acc = vec![[Vec3::ZERO; 4]; nslots];
    for idx in todo {
        let (i, j) = ((idx % w) as f64, (idx / w) as f64);
        let mut rng = Rng::new((fl.face as u64) << 24 | idx as u64 | 1 << 60);
        for a in acc.iter_mut() {
            *a = [Vec3::ZERO; 4];
        }
        let mut cnt = 0;
        for a in 0..3 {
            for b in 0..3 {
                let s = i + (a as f64 - 1.0) / 3.0;
                let t = j + (b as f64 - 1.0) / 3.0;
                let (pos, normal) = match g.disp {
                    Some(di) => {
                        let (p, n) = disp_point(geo, di, s, t);
                        (p + n * SAMPLE_NUDGE, n)
                    }
                    None => {
                        if !inside(s, t) {
                            continue;
                        }
                        (
                            g.luxel_to_world(s, t) + g.normal * SAMPLE_NUDGE,
                            fl.samples[idx].normal,
                        )
                    }
                };
                let bump = if g.bumped {
                    Some(fl.samples[idx].bump)
                } else {
                    None
                };
                let cluster = geo.point_cluster(pos);
                direct_light(
                    bvh,
                    geo,
                    lights,
                    pos,
                    normal,
                    bump.as_ref(),
                    cluster,
                    &mut rng,
                    &mut acc,
                    -1,
                );
                cnt += 1;
            }
        }
        if cnt > 0 {
            for (slot, a) in acc.iter().enumerate() {
                if fl.direct[slot].is_empty() {
                    if a.iter().all(|v| v.max_elem() <= 0.0) {
                        continue;
                    }
                    fl.direct[slot] = vec![[Vec3::ZERO; 4]; w * h];
                }
                for p in 0..4 {
                    fl.direct[slot][idx][p] = a[p] / cnt as f64;
                }
            }
        }
    }
    n
}

fn face_styles(fl: &FaceLight, styles: &[i32], face_center: Vec3) -> Vec<usize> {
    let mut cands: Vec<(usize, f64)> = Vec::new();
    for slot in 1..styles.len() {
        let mut max: f64 = 0.0;
        for l in 0..fl.samples.len() {
            max = max.max(luminance(fl.total(slot, l, 0)));
        }
        if max > 0.5 {
            cands.push((slot, max));
        }
    }
    cands.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    if cands.len() > MAXLIGHTMAPS - 1 {
        eprintln!(
            "Warning: too many light styles on a face at {:?}; dropping the weakest",
            face_center
        );
        cands.truncate(MAXLIGHTMAPS - 1);
    }
    std::iter::once(0)
        .chain(cands.into_iter().map(|c| c.0))
        .collect()
}

struct PassOutput {
    lighting: Vec<u8>,
    faces: Vec<DFace>,
    ambient: ambient::LeafAmbient,
    worldlights: Vec<DWorldLight>,
    vhv: Vec<(usize, Vec<u8>)>,
    detail: Option<(u16, Vec<u8>, Vec<u8>)>,
    entities: Vec<Entity>,
}

struct Shared<'a> {
    o: &'a Options,
    bsp: &'a BspFile,
    fs: &'a GameFs,
    geo: &'a Geometry,
    bvh: &'a Bvh,
    vnormals: &'a [Vec<Vec3>],
}

fn light_pass(sh: &Shared, hdr: bool) -> PassOutput {
    let (o, geo, bvh) = (sh.o, sh.geo, sh.bvh);
    println!("---- {} lighting ----", if hdr { "HDR" } else { "LDR" });
    let mut ents = parse_entities(&sh.bsp.entity_string());
    let mut lights = lights::parse_lights(&mut ents, geo, hdr);
    let texlights = lights::load_texlights(sh.fs, &o.bsp, o.lights_file.as_deref(), hdr);
    let surface = lights::texture_lights(geo, &sh.bsp.texdata_names(), &texlights);
    if !surface.is_empty() {
        println!("{} texture light patches", surface.len());
    }
    lights.extend(surface);
    let mut styles: Vec<i32> = vec![0];
    for l in lights.iter_mut() {
        l.slot = match styles.iter().position(|&s| s == l.style) {
            Some(i) => i,
            None => {
                styles.push(l.style);
                styles.len() - 1
            }
        };
    }
    let nslots = styles.len();
    let sky_ambient = lights
        .iter()
        .filter(|l| l.kind == EMIT_SKYAMBIENT)
        .fold(Vec3::ZERO, |a, l| a + l.intensity);
    println!(
        "{} direct lights, {} light styles",
        lights.iter().filter(|l| l.kind != EMIT_SKYAMBIENT).count(),
        nslots
    );

    let t = Instant::now();
    let mut faces: Vec<Option<FaceLight>> = (0..geo.geoms.len())
        .into_par_iter()
        .map(|fi| {
            let g = &geo.geoms[fi];
            if !g.lit
                || g.size[0] < 0
                || g.size[1] < 0
                || (g.size[0] == 0 && g.size[1] == 0 && g.disp.is_none())
            {
                return None;
            }
            let smooth = geo.faces[fi].smoothing_groups != 0;
            let samples = match g.disp {
                Some(_) => build_disp_samples(geo, g),
                None => {
                    build_face_samples(geo, g, if smooth { Some(&sh.vnormals[fi]) } else { None })
                }
            };
            let mut rng = Rng::new(fi as u64 * 104729 + 3);
            let n = samples.len();
            let mut direct: Vec<Vec<[Vec3; 4]>> = vec![Vec::new(); nslots];
            direct[0] = vec![[Vec3::ZERO; 4]; n];
            let mut acc = vec![[Vec3::ZERO; 4]; nslots];
            for (li, s) in samples.iter().enumerate() {
                for a in acc.iter_mut() {
                    *a = [Vec3::ZERO; 4];
                }
                direct_light(
                    bvh,
                    geo,
                    &lights,
                    s.pos,
                    s.normal,
                    if g.bumped { Some(&s.bump) } else { None },
                    s.cluster,
                    &mut rng,
                    &mut acc,
                    -1,
                );
                for (slot, a) in acc.iter().enumerate() {
                    if slot != 0 && a.iter().all(|v| v.max_elem() <= 0.0) {
                        continue;
                    }
                    if direct[slot].is_empty() {
                        direct[slot] = vec![[Vec3::ZERO; 4]; n];
                    }
                    direct[slot][li] = *a;
                }
            }
            let mut fl = FaceLight {
                face: fi,
                w: g.width(),
                h: g.height(),
                pages: if g.bumped { 4 } else { 1 },
                samples,
                direct,
                indirect: vec![Vec::new(); nslots],
                patch: vec![0; n],
            };
            if o.supersample {
                supersample_face(geo, bvh, &lights, &mut fl, nslots);
            }
            Some(fl)
        })
        .collect();
    let nlit = faces.iter().flatten().count();
    let nluxels: usize = faces.iter().flatten().map(|f| f.samples.len()).sum();
    println!(
        "direct lighting: {} lit faces, {} luxels ({:.2}s)",
        nlit,
        nluxels,
        t.elapsed().as_secs_f64()
    );

    let t = Instant::now();
    let mut patches = radiosity::build_patches(geo, &mut faces, o.chop, nslots);
    let active = radiosity::active_slots(&patches, nslots);
    let bounce_scale = if o.bounces == 0 { 0.0 } else { o.bounce_scale };
    let bounces = if o.bounces > 0 {
        radiosity::solve(
            geo,
            bvh,
            &faces,
            &mut patches,
            sky_ambient,
            o.transfer_rays,
            o.bounces,
            o.bounce_scale,
            &active,
        )
    } else {
        0
    };
    println!(
        "radiosity: {} patches, {} styles, {} bounces ({:.2}s)",
        patches.len(),
        active.len(),
        bounces,
        t.elapsed().as_secs_f64()
    );

    let t = Instant::now();
    let gathered: Vec<Option<Vec<Vec<[Vec3; 4]>>>> = {
        let scene = radiosity::Scene {
            geo,
            bvh,
            faces: &faces,
            patches: &patches,
            sky_ambient,
            bounce_scale,
            active: &active,
        };
        faces
            .par_iter()
            .map(|f| {
                f.as_ref().map(|fl| {
                    let mut ind = radiosity::final_gather(&scene, fl, o.gather_rays);
                    for s in ind.iter_mut() {
                        radiosity::smooth(fl, s, 2);
                    }
                    ind
                })
            })
            .collect()
    };
    for (f, ind) in faces.iter_mut().zip(gathered) {
        if let (Some(fl), Some(ind)) = (f.as_mut(), ind) {
            fl.indirect = ind;
        }
    }
    println!("final gather ({:.2}s)", t.elapsed().as_secs_f64());

    let t = Instant::now();
    let scene = radiosity::Scene {
        geo,
        bvh,
        faces: &faces,
        patches: &patches,
        sky_ambient,
        bounce_scale,
        active: &active,
    };
    let amb = ambient::compute(&scene, o.ambient_rays);
    let vhv = if o.prop_lighting {
        props::light_static_props(sh.bsp, sh.fs, &scene, &lights, o.prop_rays)
    } else {
        Vec::new()
    };
    let detail = props::light_detail_props(sh.bsp, &scene, &styles);
    println!(
        "leaf ambient: {} samples, {} lit static props, {} ({:.2}s)",
        amb.samples.len(),
        vhv.len(),
        if detail.is_some() {
            "detail props lit"
        } else {
            "no detail props"
        },
        t.elapsed().as_secs_f64()
    );

    let mut lighting: Vec<u8> = Vec::new();
    let mut dfaces = geo.faces.clone();
    for (fi, df) in dfaces.iter_mut().enumerate() {
        match &faces[fi] {
            Some(fl) => {
                let slots = face_styles(
                    fl,
                    &styles,
                    geo.geoms[fi].points.first().copied().unwrap_or_default(),
                );
                for &slot in slots.iter().rev() {
                    let mut avg = Vec3::ZERO;
                    for l in 0..fl.samples.len() {
                        avg += fl.total(slot, l, 0);
                    }
                    avg = avg / fl.samples.len().max(1) as f64;
                    lighting.extend_from_slice(bytemuck::bytes_of(&vector_to_color_rgbexp32(
                        avg.to_f32(),
                    )));
                }
                df.lightofs = lighting.len() as i32;
                df.styles = [255; 4];
                for (k, &slot) in slots.iter().enumerate() {
                    df.styles[k] = styles[slot] as u8;
                    for p in 0..fl.pages {
                        for l in 0..fl.samples.len() {
                            let v = fl.total(slot, l, p).max(Vec3::ZERO);
                            lighting.extend_from_slice(bytemuck::bytes_of(
                                &vector_to_color_rgbexp32(v.to_f32()),
                            ));
                        }
                    }
                }
            }
            None => {
                df.lightofs = -1;
                df.styles = [255; 4];
            }
        }
    }
    PassOutput {
        lighting,
        faces: dfaces,
        ambient: amb,
        worldlights: lights::worldlights_lump(&lights),
        vhv,
        detail,
        entities: ents,
    }
}

pub fn run(o: &Options) -> Result<(), String> {
    let start = Instant::now();
    println!("grad: {}", o.bsp.display());
    let mut bsp =
        BspFile::read(&o.bsp).map_err(|e| format!("can't read {}: {e}", o.bsp.display()))?;
    let ents = parse_entities(&bsp.entity_string());
    let game_dir = o.game_dir.clone().or_else(GameFs::default_game_dir);
    let fs = game_dir
        .as_deref()
        .map(GameFs::open)
        .unwrap_or_else(GameFs::empty);
    let geo = Geometry::load(&bsp);
    if geo.pvs.is_empty() {
        eprintln!("Warning: no visibility data; run gvis first for faster lighting");
    }
    let t = Instant::now();
    let (bvh, nprops) = build_bvh(&geo, &ents, &bsp, &fs, o.prop_shadows);
    println!(
        "{} faces, {} displacements, {} shadow-casting props, {} triangles in ray tracer ({:.2}s)",
        geo.faces.len(),
        geo.disps.len(),
        nprops,
        bvh.refs.len(),
        t.elapsed().as_secs_f64()
    );
    let vnormals = vertex_normals(&geo);

    let (ldr, hdr) = {
        let sh = Shared {
            o,
            bsp: &bsp,
            fs: &fs,
            geo: &geo,
            bvh: &bvh,
            vnormals: &vnormals,
        };
        let ldr = if o.ldr {
            Some(light_pass(&sh, false))
        } else {
            None
        };
        let hdr = if o.hdr {
            Some(light_pass(&sh, true))
        } else {
            None
        };
        (ldr, hdr)
    };

    let mut pak = gold_common::zip::ZipArchive::parse(&bsp.lumps[LUMP_PAKFILE]);
    pak.files
        .retain(|(n, _)| !(n.starts_with("sp_") && n.ends_with(".vhv")));
    let mut flags = 0u32;
    match &ldr {
        Some(p) => {
            bsp.lumps[LUMP_LIGHTING] = p.lighting.clone();
            bsp.set(LUMP_FACES, &p.faces);
            bsp.set(LUMP_WORLDLIGHTS, &p.worldlights);
            bsp.set(LUMP_LEAF_AMBIENT_INDEX, &p.ambient.index);
            bsp.set(LUMP_LEAF_AMBIENT_LIGHTING, &p.ambient.samples);
            for (i, d) in &p.vhv {
                pak.add(&format!("sp_{i}.vhv"), d.clone());
            }
            if !p.vhv.is_empty() {
                flags |= LVLFLAGS_BAKED_STATIC_PROP_LIGHTING_NONHDR;
            }
            if let Some((v, d, s)) = &p.detail {
                bsp.set_game_lump(GAMELUMP_DETAIL_PROPS, *v, d.clone());
                bsp.set_game_lump(GAMELUMP_DETAIL_PROP_LIGHTING, 0, s.clone());
            }
        }
        None => {
            bsp.lumps[LUMP_LIGHTING].clear();
            bsp.lumps[LUMP_WORLDLIGHTS].clear();
            let mut unlit = geo.faces.clone();
            for f in unlit.iter_mut() {
                f.lightofs = -1;
                f.styles = [255; 4];
            }
            bsp.set(LUMP_FACES, &unlit);
        }
    }
    if let Some(p) = &hdr {
        bsp.lumps[LUMP_LIGHTING_HDR] = p.lighting.clone();
        bsp.set(LUMP_FACES_HDR, &p.faces);
        bsp.set(LUMP_WORLDLIGHTS_HDR, &p.worldlights);
        bsp.set(LUMP_LEAF_AMBIENT_INDEX_HDR, &p.ambient.index);
        bsp.set(LUMP_LEAF_AMBIENT_LIGHTING_HDR, &p.ambient.samples);
        for (i, d) in &p.vhv {
            pak.add(&format!("sp_hdr_{i}.vhv"), d.clone());
        }
        if !p.vhv.is_empty() {
            flags |= LVLFLAGS_BAKED_STATIC_PROP_LIGHTING_HDR;
        }
        if let Some((v, d, s)) = &p.detail {
            if ldr.is_none() {
                bsp.set_game_lump(GAMELUMP_DETAIL_PROPS, *v, d.clone());
            }
            bsp.set_game_lump(GAMELUMP_DETAIL_PROP_LIGHTING_HDR, 0, s.clone());
        }
    }
    bsp.lumps[LUMP_PAKFILE] = pak.to_bytes();
    if let Some(p) = ldr.as_ref().or(hdr.as_ref()) {
        bsp.set_entity_string(&write_entities(&p.entities));
    }
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u16> = Vec::new();
    let mut map: std::collections::HashMap<[u32; 3], u16> = std::collections::HashMap::new();
    for (fi, vn) in vnormals.iter().enumerate() {
        for k in 0..geo.faces[fi].numedges as usize {
            let n = vn.get(k).copied().unwrap_or(geo.geoms[fi].normal).to_f32();
            let key = [n[0].to_bits(), n[1].to_bits(), n[2].to_bits()];
            let idx = *map.entry(key).or_insert_with(|| {
                normals.push(n);
                (normals.len() - 1) as u16
            });
            indices.push(idx);
        }
    }
    bsp.set(LUMP_VERTNORMALS, &normals);
    bsp.set(LUMP_VERTNORMALINDICES, &indices);
    bsp.lumps[LUMP_MAP_FLAGS] = flags.to_le_bytes().to_vec();
    for l in [
        LUMP_LIGHTING,
        LUMP_LIGHTING_HDR,
        LUMP_FACES,
        LUMP_FACES_HDR,
        LUMP_LEAF_AMBIENT_LIGHTING,
        LUMP_LEAF_AMBIENT_LIGHTING_HDR,
    ] {
        bsp.lump_versions[l] = 1;
    }
    bsp.write(&o.bsp)
        .map_err(|e| format!("can't write {}: {e}", o.bsp.display()))?;
    println!(
        "wrote {} ({:.2}s)",
        o.bsp.display(),
        start.elapsed().as_secs_f64()
    );
    Ok(())
}
