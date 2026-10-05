#![allow(clippy::needless_range_loop, clippy::too_many_arguments)]

pub mod cubemap;
pub mod detail;
pub mod disp;
pub mod emit;
pub mod faces;
pub mod map;
pub mod occluders;
pub mod overlays;
pub mod physdisp;
pub mod physics;
pub mod portals;
pub mod props;
pub mod tree;
pub mod water;

use emit::Output;
use faces::FaceSet;
use gold_common::bsp::*;
use gold_common::fs::GameFs;
use gold_common::material::MaterialCache;
use gold_common::math::*;
use gold_common::vmf::Vmf;
use gold_common::zip::ZipArchive;
use map::Map;
use portals::Portals;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tree::Tree;

pub struct Options {
    pub input: PathBuf,
    pub output: PathBuf,
    pub game_dir: Option<PathBuf>,
    pub no_blocks: bool,
    pub no_tjunc: bool,
    pub no_merge: bool,
    pub leak_only: bool,
}

fn usage() {
    eprintln!("usage: gbsp [options] <mapfile.vmf>");
    eprintln!("  -game <dir>      game directory containing gameinfo.txt");
    eprintln!("  -o <file.bsp>    output file (default: next to the .vmf)");
    eprintln!("  -noblocks        don't split the world on the 1024 unit block grid");
    eprintln!("  -notjunc         don't fix T-junctions");
    eprintln!("  -leaktest        stop after the leak test");
}

pub fn parse_args(args: &[String]) -> Option<Options> {
    let mut input = None;
    let mut output = None;
    let mut game_dir = None;
    let mut no_blocks = false;
    let mut no_tjunc = false;
    let mut leak_only = false;
    let mut i = 1;
    while i < args.len() {
        let a = args[i].as_str();
        match a.to_ascii_lowercase().as_str() {
            "-game" | "-vproject" => {
                i += 1;
                game_dir = args.get(i).map(PathBuf::from);
            }
            "-o" => {
                i += 1;
                output = args.get(i).map(PathBuf::from);
            }
            "-noblocks" => no_blocks = true,
            "-notjunc" => no_tjunc = true,
            "-leaktest" => leak_only = true,
            _ if a.starts_with('-') => eprintln!("ignoring unknown option {a}"),
            _ => input = Some(PathBuf::from(a)),
        }
        i += 1;
    }
    let mut input = input?;
    if input.extension().is_none() {
        input.set_extension("vmf");
    }
    let output = output.unwrap_or_else(|| input.with_extension("bsp"));
    Some(Options {
        input,
        output,
        game_dir,
        no_blocks,
        no_tjunc,
        no_merge: false,
        leak_only,
    })
}

pub fn main_with_args(args: Vec<String>) -> i32 {
    let Some(opts) = parse_args(&args) else {
        usage();
        return 1;
    };
    match compile(&opts) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("Error: {e}");
            1
        }
    }
}

struct ModelBuild {
    tree: Tree,
    portals: Portals,
    brushes: Vec<usize>,
    entity: usize,
}

pub fn compile(opts: &Options) -> Result<(), String> {
    let start = Instant::now();
    println!("gbsp: {}", opts.input.display());
    let vmf =
        Vmf::load(&opts.input).map_err(|e| format!("can't read {}: {e}", opts.input.display()))?;
    let game_dir = opts.game_dir.clone().or_else(GameFs::default_game_dir);
    let fs = match &game_dir {
        Some(d) => {
            println!("game directory: {}", d.display());
            GameFs::open(d)
        }
        None => {
            eprintln!("Warning: no game directory found; materials and models can't be resolved");
            GameFs::empty()
        }
    };
    let mats = MaterialCache::new();
    let map_name = opts
        .input
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut map = map::load_map(
        &vmf,
        &map_name,
        &map::LoadOptions {
            fs: &fs,
            mats: &mats,
        },
    );
    println!(
        "{} entities, {} brushes, {} displacements, {} planes, {} texinfos",
        map.entities.len(),
        map.brushes.len(),
        map.disps.len(),
        map.planes.planes.len(),
        map.texinfos.len()
    );
    let props = props::collect_static_props(&mut map, &fs);
    let overlay_list = overlays::collect(&mut map, &fs, &mats);
    let water_overlay_list = overlays::collect_water(&mut map, &fs, &mats);
    let occluder_list = occluders::collect(&mut map);
    let detail_entities = detail::collect_entities(&mut map);
    let mut overlay_texinfos: Vec<i32> = overlay_list
        .iter()
        .chain(&water_overlay_list)
        .map(|o| o.texinfo)
        .collect();
    println!("{} static props", props.len());

    let mut cubemaps = Vec::new();
    for e in map.entities.iter_mut() {
        let c = e.classname().to_ascii_lowercase();
        if c == "env_cubemap" {
            let o = e.get("origin").and_then(parse_vec3).unwrap_or_default();
            let size = e
                .get("cubemapsize")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let sides = e
                .get("sides")
                .map(|s| {
                    s.split_whitespace()
                        .filter_map(|t| t.parse().ok())
                        .collect()
                })
                .unwrap_or_default();
            cubemaps.push(cubemap::Cubemap {
                origin: [o.x as i32, o.y as i32, o.z as i32],
                size,
                sides,
            });
            e.removed = true;
        } else if c == "func_viscluster" {
            e.removed = true;
            e.brushes.clear();
        }
    }

    let mut fs_faces = FaceSet { faces: Vec::new() };
    let mut builds: Vec<ModelBuild> = Vec::new();

    let world_brushes = map.entities[0].brushes.clone();
    let mut structural = Vec::new();
    let mut detail = Vec::new();
    for &b in &world_brushes {
        let mb = &map.brushes[b];
        if mb.is_disp {
            continue;
        }
        if mb.areaportal.is_some() {
            structural.push(b);
            continue;
        }
        let is_hint = mb.sides.iter().any(|s| s.surf_flags & SURF_HINT != 0);
        if is_hint {
            map.brushes[b].contents = 0;
            structural.push(b);
        } else if mb.contents & CONTENTS_DETAIL != 0 || mb.contents & ALL_VISIBLE_CONTENTS == 0 {
            detail.push(b);
        } else {
            structural.push(b);
        }
    }
    let mut bounds = Aabb::EMPTY;
    for &b in &structural {
        bounds.add_box(&map.brushes[b].bounds);
    }
    if bounds.is_empty() {
        return Err("map has no structural world brushes".into());
    }
    let t = Instant::now();
    let mut pass = 0;
    let (mut world_tree, mut world_portals, flood) = loop {
        let brush_list = tree::make_bsp_brush_list(&map, &structural, &bounds.expand(1.0));
        let mut wt = tree::brush_bsp(&mut map.planes, brush_list, bounds, !opts.no_blocks);
        let nleafs = wt.leafs().len();
        println!(
            "BrushBSP pass {}: {} nodes, {} leafs ({:.2}s)",
            pass + 1,
            wt.nodes.len() - nleafs,
            nleafs,
            t.elapsed().as_secs_f64()
        );
        let mut wp = portals::make_tree_portals(&mut wt, &map.planes);
        let flood = portals::flood_entities(&mut wt, &map.planes, &wp, &map);
        if pass == 1 || !matches!(flood, Ok(true)) || opts.leak_only {
            break (wt, wp, flood);
        }
        portals::fill_outside(&mut wt);
        portals::mark_visible_sides(&wt, &mut wp, &mut map, &structural);
        pass += 1;
    };
    println!(
        "{} portals",
        world_portals.list.iter().filter(|p| !p.removed).count()
    );
    let mut leaked = false;
    match flood {
        Ok(true) => {
            let (o, i, s) = portals::fill_outside(&mut world_tree);
            println!(
                "FloodEntities: {} outside leafs filled, {} inside, {} solid",
                o, i, s
            );
        }
        Ok(false) => {
            eprintln!("Warning: no entities in empty space -- no filling performed");
        }
        Err(leak) => {
            leaked = true;
            let e = map.entities.get(leak.entity);
            eprintln!("**** LEAKED ****");
            if let Some(e) = e {
                eprintln!(
                    "Entity {} ({}) reached from outside -- no filling performed",
                    e.classname(),
                    e.get("origin").unwrap_or("")
                );
            }
            let lin = opts.output.with_extension("lin");
            let mut s = String::new();
            for p in leak.path.iter().rev() {
                s.push_str(&format!("{} {} {}\n", p.x, p.y, p.z));
            }
            let _ = std::fs::write(&lin, s);
            eprintln!("leak path written to {}", lin.display());
        }
    }
    if opts.leak_only {
        return if leaked {
            Err("map leaked".into())
        } else {
            Ok(())
        };
    }
    portals::mark_visible_sides(&world_tree, &mut world_portals, &mut map, &structural);
    let area_info = portals::flood_areas(&mut world_tree, &world_portals, &map);
    let (area_lump, areaportal_lump, clip_verts) =
        portals::build_area_lumps(&world_tree, &world_portals, &mut map, &area_info);
    if area_info.num_areas > 2 {
        println!(
            "{} areas, {} area portals",
            area_info.num_areas - 1,
            area_info.portal_areas.len()
        );
    }
    faces::make_faces(
        &mut world_tree,
        &map.planes,
        &mut world_portals,
        &map,
        &mut fs_faces,
    );
    let ndetail = faces::make_detail_faces(&mut world_tree, &mut map, &mut fs_faces, &detail);
    let nbottom = water::apply_bottom_materials(&mut fs_faces, &mut map, &fs, &mats);
    let water_data = water::water_volumes(&mut world_tree, &map);
    water::assign_fog_volumes(&world_tree, &world_portals, &mut fs_faces);
    if !water_data.is_empty() {
        println!(
            "{} water volumes, {} underwater faces",
            water_data.len(),
            nbottom
        );
    }
    faces::filter_brushes_into_tree(&mut world_tree, &map.planes, &map, &detail);
    let pruned = tree::prune_nodes(&mut world_tree);
    println!("PruneNodes: {} nodes pruned", pruned);
    let mut world_face_list = collect_final_faces(&world_tree, &fs_faces, &map);
    if !opts.no_tjunc {
        let added = faces::fix_tjuncs(&mut fs_faces, &world_face_list);
        println!("FixTjuncs: {} vertices added", added);
    }
    world_face_list.clear();
    println!("{} detail faces", ndetail);
    builds.push(ModelBuild {
        tree: world_tree,
        portals: world_portals,
        brushes: world_brushes.clone(),
        entity: 0,
    });

    for ei in 1..map.entities.len() {
        if map.entities[ei].removed || map.entities[ei].brushes.is_empty() {
            continue;
        }
        let ids: Vec<usize> = map.entities[ei]
            .brushes
            .iter()
            .copied()
            .filter(|&b| !map.brushes[b].is_disp)
            .collect();
        let mut b = Aabb::EMPTY;
        for &id in &ids {
            b.add_box(&map.brushes[id].bounds);
        }
        if b.is_empty() {
            for d in map.disps.iter().filter(|d| d.entity == ei) {
                if let Some(w) = &map.brushes[d.brush].sides[d.side].winding {
                    b.add_box(&w.bounds());
                }
            }
        }
        if b.is_empty() {
            continue;
        }
        let list = tree::make_bsp_brush_list(&map, &ids, &b.expand(1.0));
        let mut t = tree::brush_bsp(&mut map.planes, list, b, false);
        let mut p = portals::make_tree_portals(&mut t, &map.planes);
        portals::mark_visible_sides(&t, &mut p, &mut map, &ids);
        faces::make_faces(&mut t, &map.planes, &mut p, &map, &mut fs_faces);
        let fl = collect_final_faces(&t, &fs_faces, &map);
        if !opts.no_tjunc {
            faces::fix_tjuncs(&mut fs_faces, &fl);
        }
        let brushes = map.entities[ei].brushes.clone();
        builds.push(ModelBuild {
            tree: t,
            portals: p,
            brushes,
            entity: ei,
        });
    }

    let mut disps = disp::build_disps(&map);
    let mut pak = ZipArchive::default();
    {
        let mut all = Vec::new();
        for b in &builds {
            all.extend(collect_final_faces(&b.tree, &fs_faces, &map));
        }
        let n = cubemap::patch_cubemaps(
            &mut map,
            &mut fs_faces,
            &all,
            &mut disps,
            &cubemaps,
            &mut pak,
        );
        if n > 0 {
            println!("{} faces use {} env_cubemap sample(s)", n, cubemaps.len());
        }
    }

    let mut num_clusters = 0i32;
    {
        let wt = &mut builds[0].tree;
        for l in wt.leafs() {
            if wt.nodes[l].contents & CONTENTS_SOLID == 0 {
                wt.nodes[l].cluster = num_clusters;
                num_clusters += 1;
            } else {
                wt.nodes[l].cluster = -1;
            }
        }
    }
    write_portal_file(
        &opts.output.with_extension("prt"),
        &builds[0].tree,
        &builds[0].portals,
        num_clusters,
    )
    .map_err(|e| format!("can't write portal file: {e}"))?;
    println!("{} clusters", num_clusters);

    let mut out = Output::new();
    let mut model_entities: Vec<(usize, usize)> = Vec::new();
    let mut phys_models: Vec<(usize, Vec<usize>)> = Vec::new();
    let mut world_leaf_map: HashMap<usize, i32> = HashMap::new();
    let mut face_disp: HashMap<usize, usize> = HashMap::new();
    for (mi, mb) in builds.iter().enumerate() {
        let first_face = out.faces.len() as i32;
        out.emit_brushes(&map, &mb.brushes);
        let node_faces = out.emit_node_faces(&map, &fs_faces, &mb.tree);
        out.emit_detail_faces(&map, &fs_faces, &mb.tree);
        for (di, d) in disps
            .iter()
            .enumerate()
            .filter(|(_, d)| d.entity == mb.entity)
        {
            face_disp.insert(out.faces.len(), di);
            out.emit_disp(&map, d);
        }
        let mut leaf_map = HashMap::new();
        let head = out.emit_tree(
            &mb.tree,
            &fs_faces,
            Some(&mb.portals),
            &node_faces,
            mi == 0,
            &mut leaf_map,
        );
        let mut mbounds = Aabb::EMPTY;
        for &b in &mb.brushes {
            if !map.brushes[b].is_disp {
                mbounds.add_box(&map.brushes[b].bounds);
            }
        }
        for d in disps.iter().filter(|d| d.entity == mb.entity) {
            mbounds.add_box(&d.bounds);
        }
        if mbounds.is_empty() {
            mbounds = Aabb::new(Vec3::ZERO, Vec3::ZERO);
        }
        out.models.push(DModel {
            mins: mbounds.mins.to_f32(),
            maxs: mbounds.maxs.to_f32(),
            origin: [0.0; 3],
            headnode: head,
            firstface: first_face,
            numfaces: out.faces.len() as i32 - first_face,
        });
        if mi == 0 {
            world_leaf_map = leaf_map;
        }
        model_entities.push((mb.entity, mi));
        phys_models.push((mi, mb.brushes.clone()));
    }
    println!(
        "{} models, {} nodes, {} leafs, {} faces, {} brushes, {} vertices",
        out.models.len(),
        out.nodes.len(),
        out.leafs.len(),
        out.faces.len(),
        out.brushes.len(),
        out.verts.len()
    );

    let has_sky = out
        .faces
        .iter()
        .any(|f| map.texinfos[f.texinfo as usize].flags & SURF_SKY != 0);
    let has_sky2d = out
        .faces
        .iter()
        .any(|f| map.texinfos[f.texinfo as usize].flags & SURF_SKY2D != 0);
    for l in out.leafs.iter_mut() {
        let area = l.area();
        let mut flags = 0;
        if has_sky {
            flags |= LEAF_FLAGS_SKY;
        }
        if has_sky2d {
            flags |= LEAF_FLAGS_SKY2D;
        }
        l.set_area_flags(area, flags);
    }

    let detail_lump = {
        let wt = &builds[0].tree;
        let planes = &map.planes;
        let leaf_of = |p: Vec3| {
            world_leaf_map
                .get(&wt.point_leaf(planes, p))
                .copied()
                .unwrap_or(0)
                .max(0) as u16
        };
        detail::build_lump(
            &map,
            &fs,
            &out,
            &disps,
            &face_disp,
            &detail_entities,
            &leaf_of,
        )
    };

    let physdisp_lump = {
        let mut order: Vec<(&usize, &usize)> = face_disp.iter().collect();
        order.sort();
        physdisp::build_lump(&order.iter().map(|(_, di)| &disps[**di]).collect::<Vec<_>>())
    };
    let has_world_disps = disps.iter().any(|d| d.entity == 0);
    let physcollide = physics::build_physcollide(&map, &phys_models, has_world_disps);

    let mut water_data = water_data;
    let (texinfos, texdata, names) = compact_textures(
        &mut map,
        &mut out,
        &fs,
        &mut pak,
        &mut water_data,
        &mut overlay_texinfos,
    );

    let mut ents: Vec<Entity> = Vec::new();
    for (ei, e) in map.entities.iter().enumerate() {
        if e.removed {
            continue;
        }
        let class = e.classname().to_ascii_lowercase();
        if class == "func_detail" || class == "func_ladder" || class == "prop_static" {
            continue;
        }
        let mut ent = Entity { pairs: Vec::new() };
        if ei == 0 {
            let wb = if out.world_bounds.is_empty() {
                Aabb::new(Vec3::ZERO, Vec3::ZERO)
            } else {
                out.world_bounds
            };
            ent.pairs.push((
                "world_maxs".into(),
                format!(
                    "{} {} {}",
                    wb.maxs.x.round(),
                    wb.maxs.y.round(),
                    wb.maxs.z.round()
                ),
            ));
            ent.pairs.push((
                "world_mins".into(),
                format!(
                    "{} {} {}",
                    wb.mins.x.round(),
                    wb.mins.y.round(),
                    wb.mins.z.round()
                ),
            ));
        }
        for (k, v) in &e.pairs {
            ent.pairs.push((k.clone(), v.clone()));
        }
        if ei != 0 {
            if let Some(&(_, mi)) = model_entities.iter().find(|(me, _)| *me == ei) {
                ent.set("model", &format!("*{mi}"));
            } else if !e.brushes.is_empty() {
                continue;
            }
        }
        ents.push(ent);
    }

    let mut bsp = BspFile::new();
    bsp.map_revision = map.map_revision;
    bsp.set_entity_string(&write_entities(&ents));
    let dplanes: Vec<DPlane> = map
        .planes
        .planes
        .iter()
        .zip(map.planes.types.iter())
        .map(|(p, &t)| DPlane {
            normal: p.normal.to_f32(),
            dist: p.dist as f32,
            type_: t,
        })
        .collect();
    if dplanes.len() > 65536 {
        return Err("too many planes".into());
    }
    bsp.set(LUMP_PLANES, &dplanes);
    bsp.set(LUMP_TEXDATA, &texdata);
    bsp.set_texdata_names(&names);
    bsp.set(LUMP_TEXINFO, &texinfos);
    {
        let wt = &builds[0].tree;
        let planes = &map.planes;
        let area_of = |p: Vec3| wt.nodes[wt.point_leaf(planes, p)].area;
        bsp.lumps[LUMP_OCCLUSION] = occluders::build_lump(&occluder_list, &mut out, &area_of);
        if !occluder_list.is_empty() {
            println!("{} occluders", occluder_list.len());
        }
    }
    bsp.set(LUMP_VERTEXES, &out.verts);
    bsp.set(LUMP_NODES, &out.nodes);
    bsp.set(LUMP_FACES, &out.faces);
    bsp.set(LUMP_ORIGINALFACES, &out.orig_faces);
    bsp.set(LUMP_FACEIDS, &out.face_ids);
    bsp.set(LUMP_LEAFS, &out.leafs);
    bsp.set(LUMP_EDGES, &out.edges);
    bsp.set(LUMP_SURFEDGES, &out.surfedges);
    bsp.set(LUMP_MODELS, &out.models);
    bsp.set(LUMP_LEAFFACES, &out.leaf_faces);
    bsp.set(LUMP_LEAFBRUSHES, &out.leaf_brushes);
    bsp.set(LUMP_BRUSHES, &out.brushes);
    bsp.set(LUMP_BRUSHSIDES, &out.brush_sides);
    bsp.set(LUMP_AREAS, &area_lump);
    bsp.set(LUMP_AREAPORTALS, &areaportal_lump);
    bsp.set(LUMP_CLIPPORTALVERTS, &clip_verts);
    bsp.set(LUMP_DISPINFO, &out.dispinfos);
    bsp.set(LUMP_DISP_VERTS, &out.disp_verts);
    bsp.set(LUMP_DISP_TRIS, &out.disp_tris);
    bsp.lumps[LUMP_DISP_LIGHTMAP_SAMPLE_POSITIONS] = out.disp_lm_samples.clone();
    bsp.lumps[LUMP_PHYSCOLLIDE] = physcollide;
    bsp.lumps[LUMP_PHYSDISP] = physdisp_lump;
    bsp.set(LUMP_PRIMITIVES, &out.prims);
    bsp.set(LUMP_PRIMINDICES, &out.prim_indices);
    bsp.set(
        LUMP_CUBEMAPS,
        &cubemaps.iter().map(|c| c.sample()).collect::<Vec<_>>(),
    );
    bsp.set(LUMP_LEAFWATERDATA, &water_data);
    if !overlay_list.is_empty() || !water_overlay_list.is_empty() {
        let face_geo: Vec<(i32, usize, gold_common::winding::Winding)> = out
            .faces
            .iter()
            .zip(out.face_side_ids.iter())
            .map(|(f, &sid)| {
                let pts = (0..f.numedges as i32)
                    .map(|k| {
                        let se = out.surfedges[(f.firstedge + k) as usize];
                        let e = out.edges[se.unsigned_abs() as usize];
                        let vi = if se >= 0 { e.v[0] } else { e.v[1] };
                        Vec3::from_f32(out.verts[vi as usize])
                    })
                    .collect();
                (
                    sid,
                    f.planenum as usize,
                    gold_common::winding::Winding::new(pts),
                )
            })
            .collect();
        let mut side_faces: HashMap<i32, Vec<u32>> = HashMap::new();
        for o in overlay_list.iter().chain(&water_overlay_list) {
            for &s in &o.sides {
                side_faces
                    .entry(s)
                    .or_insert_with(|| overlays::faces_for_side(&map, s, &face_geo));
            }
        }
        let mut ovs = overlay_list;
        let mut wovs = water_overlay_list;
        for (o, &t) in ovs
            .iter_mut()
            .chain(wovs.iter_mut())
            .zip(overlay_texinfos.iter())
        {
            o.texinfo = t;
        }
        if !ovs.is_empty() {
            let (od, of) = overlays::build_lumps(&ovs, &side_faces);
            bsp.lumps[LUMP_OVERLAYS] = od;
            bsp.lumps[LUMP_OVERLAY_FADES] = of;
            println!("{} overlays", ovs.len());
        }
        if !wovs.is_empty() {
            bsp.lumps[LUMP_WATEROVERLAYS] = overlays::build_water_lump(&wovs, &side_faces);
            println!("{} water overlays", wovs.len());
        }
    }
    bsp.set(LUMP_LEAFMINDISTTOWATER, &vec![65535u16; out.leafs.len()]);
    bsp.set(
        LUMP_FACE_MACRO_TEXTURE_INFO,
        &vec![0xFFFFu16; out.faces.len()],
    );
    bsp.lumps[LUMP_MAP_FLAGS] = vec![0u8; 4];
    write_flat_vertnormals(&mut bsp, &out, &dplanes);
    bsp.lumps[LUMP_PAKFILE] = pak.to_bytes();
    let leaf_index = |n: usize| -> i32 { world_leaf_map.get(&n).copied().unwrap_or(-1) };
    let sprp = props::build_static_prop_lump(&props, &builds[0].tree, &map.planes, &leaf_index);
    bsp.set_game_lump(GAMELUMP_STATIC_PROPS, props::STATIC_PROP_VERSION, sprp);
    bsp.set_game_lump(
        GAMELUMP_DETAIL_PROPS,
        detail::DETAIL_PROP_VERSION,
        detail_lump,
    );

    bsp.write(&opts.output)
        .map_err(|e| format!("can't write {}: {e}", opts.output.display()))?;
    println!(
        "wrote {} ({:.2}s)",
        opts.output.display(),
        start.elapsed().as_secs_f64()
    );
    if leaked {
        eprintln!("Warning: the map leaked; vis and lighting will be slow and inaccurate");
    }
    Ok(())
}

fn collect_final_faces(tree: &Tree, fs: &FaceSet, map: &Map) -> Vec<usize> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for n in 0..tree.nodes.len() {
        for &f in tree.nodes[n]
            .faces
            .iter()
            .chain(tree.nodes[n].leaf_faces.iter())
        {
            let mut finals = Vec::new();
            fs.resolve(f, &mut finals);
            for ff in finals {
                let ti = fs.faces[ff].texinfo;
                if ti < 0
                    || map.texinfos[ti as usize].flags & (SURF_NODRAW | SURF_SKIP | SURF_HINT) != 0
                {
                    continue;
                }
                if seen.insert(ff) {
                    out.push(ff);
                }
            }
        }
    }
    out
}

fn write_flat_vertnormals(bsp: &mut BspFile, out: &Output, planes: &[DPlane]) {
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u16> = Vec::new();
    for f in &out.faces {
        let n = planes[f.planenum as usize].normal;
        let ni = normals.len() as u16;
        normals.push(n);
        for _ in 0..f.numedges {
            indices.push(ni);
        }
    }
    bsp.set(LUMP_VERTNORMALS, &normals);
    bsp.set(LUMP_VERTNORMALINDICES, &indices);
}

fn compact_textures(
    map: &mut Map,
    out: &mut Output,
    fs: &GameFs,
    pak: &mut ZipArchive,
    water: &mut [DLeafWaterData],
    extra: &mut [i32],
) -> (Vec<TexInfo>, Vec<DTexData>, Vec<String>) {
    let mut used = vec![false; map.texinfos.len()];
    for f in &out.faces {
        used[f.texinfo as usize] = true;
    }
    for s in &out.brush_sides {
        used[s.texinfo as usize] = true;
    }
    let mut patched: HashMap<i32, i32> = HashMap::new();
    let brush_face_texinfos: std::collections::HashSet<i16> = out
        .faces
        .iter()
        .filter(|f| f.dispinfo < 0)
        .map(|f| f.texinfo)
        .collect();
    for &ti in &brush_face_texinfos {
        let td = map.texinfos[ti as usize].texdata;
        if patched.contains_key(&td) {
            continue;
        }
        let mat = &map.materials[td as usize];
        if !mat.is_wvt() {
            continue;
        }
        let name = map.texdata_names[td as usize].clone();
        let patch_name = format!(
            "maps/{}/{}_wvt_patch",
            map.map_name,
            name.to_ascii_lowercase()
        );
        let mut vmt = String::from("\"LightmappedGeneric\"\r\n{\r\n");
        for (k, v) in &mat.params {
            if k == "$basetexture2" || k == "$bumpmap2" || k == "$surfaceprop2" {
                continue;
            }
            vmt.push_str(&format!("\t\"{k}\"\t\t\"{v}\"\r\n"));
        }
        vmt.push_str("}\r\n");
        pak.add(&format!("materials/{patch_name}.vmt"), vmt.into_bytes());
        let mut m = mat.clone();
        m.shader = "LightmappedGeneric".into();
        let ntd = map.texdata_for(&patch_name, &m);
        patched.insert(td, ntd);
    }
    let _ = fs;
    let mut face_remap: HashMap<i16, i16> = HashMap::new();
    for &ti in &brush_face_texinfos {
        let td = map.texinfos[ti as usize].texdata;
        if let Some(&ntd) = patched.get(&td) {
            let mut t = map.texinfos[ti as usize];
            t.texdata = ntd;
            let nti = map.add_texinfo(t);
            if nti as usize >= used.len() {
                used.resize(nti as usize + 1, false);
            }
            used[nti as usize] = true;
            face_remap.insert(ti, nti as i16);
        }
    }
    for f in out.faces.iter_mut() {
        if f.dispinfo < 0
            && let Some(&n) = face_remap.get(&f.texinfo)
        {
            f.texinfo = n;
        }
    }
    used.resize(map.texinfos.len(), false);
    let mut used2 = vec![false; map.texinfos.len()];
    for f in &out.faces {
        used2[f.texinfo as usize] = true;
    }
    for w in water.iter() {
        if w.surface_texinfo_id >= 0 {
            used2[w.surface_texinfo_id as usize] = true;
        }
    }
    for &t in extra.iter() {
        used2[t as usize] = true;
    }
    for s in &out.brush_sides {
        used2[s.texinfo as usize] = true;
    }
    let mut ti_map = vec![-1i16; map.texinfos.len()];
    let mut td_map: HashMap<i32, i32> = HashMap::new();
    let mut texinfos = Vec::new();
    let mut texdata = Vec::new();
    let mut names = Vec::new();
    for (i, t) in map.texinfos.iter().enumerate() {
        if !used2[i] {
            continue;
        }
        let ntd = *td_map.entry(t.texdata).or_insert_with(|| {
            let mut d = map.texdata[t.texdata as usize];
            d.name_string_table_id = names.len() as i32;
            names.push(map.texdata_names[t.texdata as usize].clone());
            texdata.push(d);
            (texdata.len() - 1) as i32
        });
        let mut nt = *t;
        nt.texdata = ntd;
        ti_map[i] = texinfos.len() as i16;
        texinfos.push(nt);
    }
    for f in out.faces.iter_mut() {
        f.texinfo = ti_map[f.texinfo as usize];
    }
    for f in out.orig_faces.iter_mut() {
        if f.texinfo >= 0 {
            f.texinfo = ti_map[f.texinfo as usize].max(0);
        }
    }
    for s in out.brush_sides.iter_mut() {
        s.texinfo = ti_map[s.texinfo as usize];
    }
    for w in water.iter_mut() {
        if w.surface_texinfo_id >= 0 {
            w.surface_texinfo_id = ti_map
                .get(w.surface_texinfo_id as usize)
                .copied()
                .unwrap_or(-1);
        }
    }
    for t in extra.iter_mut() {
        *t = ti_map[*t as usize] as i32;
    }
    (texinfos, texdata, names)
}

fn write_portal_file(
    path: &Path,
    tree: &Tree,
    portals: &Portals,
    num_clusters: i32,
) -> std::io::Result<()> {
    let mut lines = Vec::new();
    for p in &portals.list {
        if p.removed || p.onnode.is_none() {
            continue;
        }
        let c0 = tree.nodes[p.nodes[0]].cluster;
        let c1 = tree.nodes[p.nodes[1]].cluster;
        if c0 < 0 || c1 < 0 || c0 == c1 {
            continue;
        }
        let w = &p.winding;
        if w.len() < 3 {
            continue;
        }
        let wp = w.plane();
        let (a, b) = if p.plane.normal.dot(wp.normal) < 0.99 {
            (c1, c0)
        } else {
            (c0, c1)
        };
        let mut s = format!("{} {} {} ", w.len(), a, b);
        for pt in &w.p {
            s.push_str(&format!(
                "({} {} {} ) ",
                fmt_num(pt.x),
                fmt_num(pt.y),
                fmt_num(pt.z)
            ));
        }
        lines.push(s);
    }
    let mut out = format!("PRT1\n{}\n{}\n", num_clusters, lines.len());
    for l in lines {
        out.push_str(&l);
        out.push('\n');
    }
    std::fs::write(path, out)
}

fn fmt_num(v: f64) -> String {
    format!("{:.6}", v)
}
