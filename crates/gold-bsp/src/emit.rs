use crate::disp::{self, Disp};
use crate::faces::{Face, FaceSet};
use crate::map::Map;
use crate::portals::Portals;
use crate::tree::Tree;
use gold_common::bsp::*;
use gold_common::math::*;
use gold_common::winding::Winding;
use std::collections::HashMap;

pub struct Output {
    pub verts: Vec<[f32; 3]>,
    vert_hash: HashMap<(i64, i64, i64), Vec<usize>>,
    pub edges: Vec<DEdge>,
    edge_hash: HashMap<(u16, u16), usize>,
    edge_uses: Vec<u8>,
    edge_contents: Vec<i32>,
    pub surfedges: Vec<i32>,
    pub faces: Vec<DFace>,
    pub face_ids: Vec<u16>,
    pub face_side_ids: Vec<i32>,
    pub orig_faces: Vec<DFace>,
    orig_face_map: HashMap<(usize, usize), i32>,
    pub prims: Vec<DPrimitive>,
    pub prim_indices: Vec<u16>,
    pub nodes: Vec<DNode>,
    pub leafs: Vec<DLeaf>,
    pub leaf_faces: Vec<u16>,
    pub leaf_brushes: Vec<u16>,
    pub brushes: Vec<DBrush>,
    pub brush_sides: Vec<DBrushSide>,
    pub brush_map: HashMap<usize, u16>,
    pub models: Vec<DModel>,
    pub dispinfos: Vec<DDispInfo>,
    pub disp_verts: Vec<DDispVert>,
    pub disp_tris: Vec<u16>,
    pub disp_lm_samples: Vec<u8>,
    face_out: HashMap<usize, u16>,
    pub world_bounds: Aabb,
}

fn snap_coord(v: f64) -> f64 {
    let r = v.round();
    if (v - r).abs() < 0.01 { r } else { v }
}

fn clamp_i16(v: f64) -> i16 {
    v.clamp(-32768.0, 32767.0) as i16
}

impl Default for Output {
    fn default() -> Self {
        Self::new()
    }
}

impl Output {
    pub fn new() -> Self {
        let mut o = Output {
            verts: Vec::new(),
            vert_hash: HashMap::new(),
            edges: vec![DEdge { v: [0, 0] }],
            edge_hash: HashMap::new(),
            edge_uses: vec![0],
            edge_contents: vec![0],
            surfedges: Vec::new(),
            faces: Vec::new(),
            face_ids: Vec::new(),
            face_side_ids: Vec::new(),
            orig_faces: Vec::new(),
            orig_face_map: HashMap::new(),
            prims: Vec::new(),
            prim_indices: Vec::new(),
            nodes: Vec::new(),
            leafs: Vec::new(),
            leaf_faces: Vec::new(),
            leaf_brushes: Vec::new(),
            brushes: Vec::new(),
            brush_sides: Vec::new(),
            brush_map: HashMap::new(),
            models: Vec::new(),
            dispinfos: Vec::new(),
            disp_verts: Vec::new(),
            disp_tris: Vec::new(),
            disp_lm_samples: Vec::new(),
            face_out: HashMap::new(),
            world_bounds: Aabb::EMPTY,
        };
        o.leafs.push(DLeaf {
            contents: CONTENTS_SOLID,
            cluster: -1,
            leaf_water_data_id: -1,
            ..Default::default()
        });
        o
    }

    pub fn vertex(&mut self, p: Vec3) -> u16 {
        let v = v3(snap_coord(p.x), snap_coord(p.y), snap_coord(p.z));
        let key = |x: f64| (x / 4.0).floor() as i64;
        let k = (key(v.x), key(v.y), key(v.z));
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(l) = self.vert_hash.get(&(k.0 + dx, k.1 + dy, k.2 + dz)) {
                        for &i in l {
                            let q = self.verts[i];
                            if (q[0] as f64 - v.x).abs() < 0.01
                                && (q[1] as f64 - v.y).abs() < 0.01
                                && (q[2] as f64 - v.z).abs() < 0.01
                            {
                                return i as u16;
                            }
                        }
                    }
                }
            }
        }
        let i = self.verts.len();
        self.verts.push(v.to_f32());
        self.vert_hash.entry(k).or_default().push(i);
        i as u16
    }

    fn edge(&mut self, v1: u16, v2: u16, contents: i32) -> i32 {
        if let Some(&e) = self.edge_hash.get(&(v2, v1))
            && self.edge_uses[e] == 1
            && self.edge_contents[e] == contents
        {
            self.edge_uses[e] += 1;
            return -(e as i32);
        }
        let e = self.edges.len();
        self.edges.push(DEdge { v: [v1, v2] });
        self.edge_uses.push(1);
        self.edge_contents.push(contents);
        self.edge_hash.insert((v1, v2), e);
        e as i32
    }

    fn add_winding_edges(&mut self, w: &Winding, contents: i32) -> (i32, i16) {
        let first = self.surfedges.len() as i32;
        let ids: Vec<u16> = w.p.iter().map(|&p| self.vertex(p)).collect();
        let n = ids.len();
        for i in 0..n {
            let e = self.edge(ids[i], ids[(i + 1) % n], contents);
            self.surfedges.push(e);
        }
        (first, n as i16)
    }

    pub fn lightmap_extents(w: &Winding, tex: &TexInfo) -> ([i32; 2], [i32; 2]) {
        let mut mins = [0i32; 2];
        let mut size = [0i32; 2];
        for axis in 0..2 {
            let lv = tex.lightmap_vecs[axis];
            let mut lo = f64::MAX;
            let mut hi = f64::MIN;
            for p in &w.p {
                let pf = p.to_f32();
                let v = pf[0] as f64 * lv[0] as f64
                    + pf[1] as f64 * lv[1] as f64
                    + pf[2] as f64 * lv[2] as f64
                    + lv[3] as f64;
                lo = lo.min(v);
                hi = hi.max(v);
            }
            mins[axis] = lo.floor() as i32;
            size[axis] = (hi.ceil() - lo.floor()) as i32;
        }
        (mins, size)
    }

    fn orig_face(&mut self, map: &Map, side: (usize, usize)) -> i32 {
        if let Some(&i) = self.orig_face_map.get(&side) {
            return i;
        }
        let s = &map.brushes[side.0].sides[side.1];
        let Some(w) = &s.winding else { return -1 };
        let (firstedge, numedges) = self.add_winding_edges(w, -1);
        let i = self.orig_faces.len() as i32;
        self.orig_faces.push(DFace {
            planenum: s.planenum as u16,
            side: (s.planenum & 1) as u8,
            on_node: 1,
            firstedge,
            numedges,
            texinfo: s.texinfo as i16,
            dispinfo: -1,
            surface_fog_volume_id: 0,
            orig_face: -1,
            ..Default::default()
        });
        self.orig_face_map.insert(side, i);
        i
    }

    pub fn emit_face(&mut self, map: &Map, fs: &FaceSet, fi: usize) -> Option<u16> {
        if let Some(&o) = self.face_out.get(&fi) {
            return Some(o);
        }
        let f: &Face = &fs.faces[fi];
        if f.texinfo < 0 {
            return None;
        }
        let tex = map.texinfos[f.texinfo as usize];
        if tex.flags & (SURF_NODRAW | SURF_SKIP | SURF_HINT) != 0 {
            return None;
        }
        if f.winding.len() < 3 {
            return None;
        }
        let orig = match f.side {
            Some(s) => self.orig_face(map, s),
            None => -1,
        };
        let (firstedge, numedges) = self.add_winding_edges(&f.winding, f.contents);
        let (mins, size) = if tex.flags & SURF_NOLIGHT != 0 {
            ([0, 0], [0, 0])
        } else {
            Self::lightmap_extents(&f.winding, &tex)
        };
        if tex.flags & SURF_NOLIGHT == 0
            && (size[0] > MAX_BRUSH_LIGHTMAP_DIM_WITHOUT_BORDER
                || size[1] > MAX_BRUSH_LIGHTMAP_DIM_WITHOUT_BORDER)
        {
            eprintln!(
                "Warning: face with lightmap extents {}x{} exceeds the limit (at {:?})",
                size[0],
                size[1],
                f.winding.center()
            );
        }
        let (num_prims, first_prim) = match &f.prims {
            Some(idx) if !idx.is_empty() => {
                let first = self.prims.len() as u16;
                self.prims.push(DPrimitive {
                    type_: 0,
                    pad: 0,
                    first_index: self.prim_indices.len() as u16,
                    index_count: idx.len() as u16,
                    first_vert: 0,
                    vert_count: 0,
                });
                self.prim_indices.extend_from_slice(idx);
                (1u16, first)
            }
            _ => (0, 0),
        };
        let (side_id, smoothing) = match f.side {
            Some((b, s)) => (
                map.brushes[b].sides[s].side_id,
                map.brushes[b].sides[s].smoothing_groups,
            ),
            None => (0, 0),
        };
        let o = self.faces.len() as u16;
        self.faces.push(DFace {
            planenum: f.planenum as u16,
            side: (f.planenum & 1) as u8,
            on_node: if f.on_node { 1 } else { 0 },
            firstedge,
            numedges,
            texinfo: f.texinfo as i16,
            dispinfo: -1,
            surface_fog_volume_id: f.fog_volume as i16,
            styles: [0, 255, 255, 255],
            lightofs: -1,
            area: f.winding.area() as f32,
            lightmap_texture_mins_in_luxels: mins,
            lightmap_texture_size_in_luxels: size,
            orig_face: orig,
            num_prims,
            first_prim_id: first_prim,
            smoothing_groups: smoothing,
        });
        self.face_ids.push(side_id as u16);
        self.face_side_ids.push(side_id);
        self.face_out.insert(fi, o);
        if tex.flags & SURF_SKY == 0 {
            for p in &f.winding.p {
                self.world_bounds.add_point(*p);
            }
        }
        Some(o)
    }

    pub fn emit_disp(&mut self, map: &Map, d: &Disp) -> u16 {
        let tex = map.texinfos[d.texinfo as usize];
        let (firstedge, numedges) = self.add_winding_edges(&d.face, 0);
        let (mins, size) = if tex.flags & SURF_NOLIGHT != 0 {
            ([0, 0], [0, 0])
        } else {
            (
                Self::lightmap_extents(&d.face, &tex).0,
                disp::disp_lightmap_size(&d.corners, &tex.lightmap_vecs),
            )
        };
        if size[0] > MAX_DISP_LIGHTMAP_DIM_WITHOUT_BORDER
            || size[1] > MAX_DISP_LIGHTMAP_DIM_WITHOUT_BORDER
        {
            eprintln!(
                "Warning: displacement lightmap too large ({}x{}); increase its lightmap scale",
                size[0], size[1]
            );
        }
        let orig = self.orig_face(map, (d.brush, d.side));
        let side = &map.brushes[d.brush].sides[d.side];
        let fi = self.faces.len() as u16;
        let di = self.dispinfos.len();
        self.faces.push(DFace {
            planenum: d.planenum as u16,
            side: (d.planenum & 1) as u8,
            on_node: 0,
            firstedge,
            numedges,
            texinfo: d.texinfo as i16,
            dispinfo: di as i16,
            surface_fog_volume_id: -1,
            styles: [0, 255, 255, 255],
            lightofs: -1,
            area: d.face.area() as f32,
            lightmap_texture_mins_in_luxels: mins,
            lightmap_texture_size_in_luxels: size,
            orig_face: orig,
            num_prims: 0,
            first_prim_id: 0,
            smoothing_groups: side.smoothing_groups,
        });
        self.face_ids.push(side.side_id as u16);
        self.face_side_ids.push(side.side_id);
        let lm_start = self.disp_lm_samples.len() as i32;
        if tex.flags & SURF_NOLIGHT == 0 {
            let samples = disp::lightmap_sample_positions(d.power, size);
            self.disp_lm_samples.extend(samples);
        }
        self.dispinfos.push(DDispInfo {
            start_position: d.start_position.to_f32(),
            disp_vert_start: self.disp_verts.len() as i32,
            disp_tri_start: self.disp_tris.len() as i32,
            power: d.power,
            min_tess: (0x8000_0000u32 as i32) | d.flags,
            smoothing_angle: 0.0,
            contents: CONTENTS_SOLID,
            map_face: fi,
            pad0: 0,
            lightmap_alpha_start: 0,
            lightmap_sample_position_start: lm_start,
            edge_neighbors: d.edge_neighbors,
            corner_neighbors: d.corner_neighbors,
            allowed_verts: d.allowed_verts,
        });
        self.disp_verts.extend_from_slice(&d.verts);
        self.disp_tris.extend_from_slice(&d.tri_tags);
        for p in &d.positions {
            self.world_bounds.add_point(*p);
        }
        fi
    }

    pub fn emit_brushes(&mut self, map: &Map, ids: &[usize]) {
        for &bi in ids {
            let b = &map.brushes[bi];
            if b.is_disp || b.areaportal.is_some() || self.brush_map.contains_key(&bi) {
                continue;
            }
            let idx = self.brushes.len() as u16;
            let first = self.brush_sides.len() as i32;
            for s in &b.sides {
                self.brush_sides.push(DBrushSide {
                    planenum: s.planenum as u16,
                    texinfo: s.texinfo as i16,
                    dispinfo: 0,
                    bevel: if s.bevel { 1 } else { 0 },
                });
            }
            self.brushes.push(DBrush {
                firstside: first,
                numsides: b.sides.len() as i32,
                contents: b.contents,
            });
            self.brush_map.insert(bi, idx);
        }
    }

    pub fn emit_node_faces(
        &mut self,
        map: &Map,
        fs: &FaceSet,
        tree: &Tree,
    ) -> HashMap<usize, (u16, u16)> {
        let mut out = HashMap::new();
        let mut stack = vec![tree.head];
        while let Some(n) = stack.pop() {
            if tree.nodes[n].is_leaf() {
                continue;
            }
            let first = self.faces.len() as u16;
            for &f in &tree.nodes[n].faces {
                self.emit_face(map, fs, f);
            }
            out.insert(n, (first, self.faces.len() as u16 - first));
            let [c0, c1] = tree.nodes[n].children;
            stack.push(c1);
            stack.push(c0);
        }
        out
    }

    pub fn emit_detail_faces(&mut self, map: &Map, fs: &FaceSet, tree: &Tree) {
        for l in tree.leafs() {
            for &f in &tree.nodes[l].leaf_faces {
                self.emit_face(map, fs, f);
            }
        }
    }

    fn emit_leaf(
        &mut self,
        tree: &Tree,
        fs: &FaceSet,
        portals: Option<&Portals>,
        n: usize,
        is_world: bool,
    ) -> i32 {
        let node = &tree.nodes[n];
        let mut leaf = DLeaf {
            contents: node.contents,
            cluster: if is_world { node.cluster as i16 } else { -1 },
            leaf_water_data_id: node.water_id as i16,
            ..Default::default()
        };
        leaf.set_area_flags(node.area, 0);
        let b = if node.bounds.is_empty() {
            Aabb::new(Vec3::ZERO, Vec3::ZERO)
        } else {
            node.bounds
        };
        leaf.mins = [
            clamp_i16(b.mins.x.floor()),
            clamp_i16(b.mins.y.floor()),
            clamp_i16(b.mins.z.floor()),
        ];
        leaf.maxs = [
            clamp_i16(b.maxs.x.ceil()),
            clamp_i16(b.maxs.y.ceil()),
            clamp_i16(b.maxs.z.ceil()),
        ];
        leaf.firstleafface = self.leaf_faces.len() as u16;
        if node.contents & CONTENTS_SOLID == 0 {
            let mut list: Vec<u16> = Vec::new();
            if let Some(portals) = portals {
                for &p in &node.portals {
                    let port = &portals.list[p];
                    let s = if port.nodes[1] == n { 1 } else { 0 };
                    if let Some(f) = port.face[s] {
                        let mut finals = Vec::new();
                        fs.resolve(f, &mut finals);
                        for ff in finals {
                            if let Some(&o) = self.face_out.get(&ff)
                                && !list.contains(&o)
                            {
                                list.push(o);
                            }
                        }
                    }
                }
            }
            for &f in &node.leaf_faces {
                if let Some(&o) = self.face_out.get(&f)
                    && !list.contains(&o)
                {
                    list.push(o);
                }
            }
            self.leaf_faces.extend(list);
        }
        leaf.numleaffaces = self.leaf_faces.len() as u16 - leaf.firstleafface;
        leaf.firstleafbrush = self.leaf_brushes.len() as u16;
        let mut bl: Vec<u16> = Vec::new();
        for b in &node.brushes {
            if let Some(&o) = self.brush_map.get(&b.original)
                && !bl.contains(&o)
            {
                bl.push(o);
            }
        }
        for b in &node.extra_brushes {
            if let Some(&o) = self.brush_map.get(b)
                && !bl.contains(&o)
            {
                bl.push(o);
            }
        }
        bl.sort();
        self.leaf_brushes.extend(bl);
        leaf.numleafbrushes = self.leaf_brushes.len() as u16 - leaf.firstleafbrush;
        let i = self.leafs.len() as i32;
        self.leafs.push(leaf);
        i
    }

    pub fn emit_tree(
        &mut self,
        tree: &Tree,
        fs: &FaceSet,
        portals: Option<&Portals>,
        node_faces: &HashMap<usize, (u16, u16)>,
        is_world: bool,
        leaf_map: &mut HashMap<usize, i32>,
    ) -> i32 {
        if tree.nodes[tree.head].is_leaf() {
            let l = self.emit_leaf(tree, fs, portals, tree.head, is_world);
            leaf_map.insert(tree.head, l);
            return -(l + 1);
        }
        self.emit_node_r(tree, fs, portals, node_faces, is_world, tree.head, leaf_map)
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_node_r(
        &mut self,
        tree: &Tree,
        fs: &FaceSet,
        portals: Option<&Portals>,
        node_faces: &HashMap<usize, (u16, u16)>,
        is_world: bool,
        n: usize,
        leaf_map: &mut HashMap<usize, i32>,
    ) -> i32 {
        let node = &tree.nodes[n];
        let idx = self.nodes.len();
        let b = node.bounds;
        let (first, count) = node_faces.get(&n).copied().unwrap_or((0, 0));
        self.nodes.push(DNode {
            planenum: node.planenum.unwrap() as i32,
            children: [0, 0],
            mins: [
                clamp_i16(b.mins.x.floor()),
                clamp_i16(b.mins.y.floor()),
                clamp_i16(b.mins.z.floor()),
            ],
            maxs: [
                clamp_i16(b.maxs.x.ceil()),
                clamp_i16(b.maxs.y.ceil()),
                clamp_i16(b.maxs.z.ceil()),
            ],
            firstface: first,
            numfaces: count,
            area: 0,
            padding: 0,
        });
        for i in 0..2 {
            let c = node.children[i];
            let r = if tree.nodes[c].is_leaf() {
                let l = self.emit_leaf(tree, fs, portals, c, is_world);
                leaf_map.insert(c, l);
                -(l + 1)
            } else {
                self.emit_node_r(tree, fs, portals, node_faces, is_world, c, leaf_map)
            };
            self.nodes[idx].children[i] = r;
        }
        idx as i32
    }
}
