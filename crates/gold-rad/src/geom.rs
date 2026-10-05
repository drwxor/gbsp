use gold_common::bsp::*;
use gold_common::math::*;

pub struct DispSurf {
    pub face: usize,
    pub power: i32,
    pub size: usize,
    pub corners: [Vec3; 4],
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub luxel: Vec<(f64, f64)>,
    pub tris: Vec<[usize; 3]>,
}

pub struct FaceGeom {
    pub index: usize,
    pub points: Vec<Vec3>,
    pub normal: Vec3,
    pub dist: f64,
    pub texinfo: TexInfo,
    pub flags: i32,
    pub mins: [i32; 2],
    pub size: [i32; 2],
    pub lm_origin: Vec3,
    pub lm_sdir: Vec3,
    pub lm_tdir: Vec3,
    pub disp: Option<usize>,
    pub reflectivity: Vec3,
    pub model: usize,
    pub lit: bool,
    pub bumped: bool,
}

impl FaceGeom {
    pub fn luxel_to_world(&self, s: f64, t: f64) -> Vec3 {
        self.lm_origin + self.lm_sdir * s + self.lm_tdir * t
    }
    pub fn world_to_luxel(&self, p: Vec3) -> (f64, f64) {
        let lv = &self.texinfo.lightmap_vecs;
        let s =
            p.x * lv[0][0] as f64 + p.y * lv[0][1] as f64 + p.z * lv[0][2] as f64 + lv[0][3] as f64
                - self.mins[0] as f64;
        let t =
            p.x * lv[1][0] as f64 + p.y * lv[1][1] as f64 + p.z * lv[1][2] as f64 + lv[1][3] as f64
                - self.mins[1] as f64;
        (s, t)
    }
    pub fn width(&self) -> usize {
        (self.size[0] + 1) as usize
    }
    pub fn height(&self) -> usize {
        (self.size[1] + 1) as usize
    }
}

pub struct Geometry {
    pub planes: Vec<DPlane>,
    pub texinfo: Vec<TexInfo>,
    pub texdata: Vec<DTexData>,
    pub faces: Vec<DFace>,
    pub geoms: Vec<FaceGeom>,
    pub disps: Vec<DispSurf>,
    pub nodes: Vec<DNode>,
    pub leafs: Vec<DLeaf>,
    pub models: Vec<DModel>,
    pub num_clusters: usize,
    pub pvs: Vec<Vec<u8>>,
}

fn inverse3(m: [[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    Some([
        [
            (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * inv,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv,
        ],
        [
            (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * inv,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv,
        ],
        [
            (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * inv,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv,
        ],
    ])
}

fn mul3(m: &[[f64; 3]; 3], v: Vec3) -> Vec3 {
    v3(
        m[0][0] * v.x + m[0][1] * v.y + m[0][2] * v.z,
        m[1][0] * v.x + m[1][1] * v.y + m[1][2] * v.z,
        m[2][0] * v.x + m[2][1] * v.y + m[2][2] * v.z,
    )
}

impl Geometry {
    pub fn load(bsp: &BspFile) -> Geometry {
        let planes: Vec<DPlane> = bsp.get(LUMP_PLANES);
        let texinfo: Vec<TexInfo> = bsp.get(LUMP_TEXINFO);
        let texdata: Vec<DTexData> = bsp.get(LUMP_TEXDATA);
        let faces: Vec<DFace> = bsp.get(LUMP_FACES);
        let verts: Vec<[f32; 3]> = bsp.get(LUMP_VERTEXES);
        let edges: Vec<DEdge> = bsp.get(LUMP_EDGES);
        let surfedges: Vec<i32> = bsp.get(LUMP_SURFEDGES);
        let dispinfo: Vec<DDispInfo> = bsp.get(LUMP_DISPINFO);
        let dispverts: Vec<DDispVert> = bsp.get(LUMP_DISP_VERTS);
        let nodes: Vec<DNode> = bsp.get(LUMP_NODES);
        let leafs: Vec<DLeaf> = bsp.get(LUMP_LEAFS);
        let models: Vec<DModel> = bsp.get(LUMP_MODELS);

        let mut face_model = vec![0usize; faces.len()];
        for (mi, m) in models.iter().enumerate() {
            for f in m.firstface..m.firstface + m.numfaces {
                if (f as usize) < faces.len() {
                    face_model[f as usize] = mi;
                }
            }
        }

        let mut geoms = Vec::with_capacity(faces.len());
        for (fi, f) in faces.iter().enumerate() {
            let mut points = Vec::with_capacity(f.numedges as usize);
            for k in 0..f.numedges as i32 {
                let se = surfedges[(f.firstedge + k) as usize];
                let vi = if se >= 0 {
                    edges[se as usize].v[0]
                } else {
                    edges[(-se) as usize].v[1]
                };
                points.push(Vec3::from_f32(verts[vi as usize]));
            }
            let pl = planes[f.planenum as usize];
            let normal = Vec3::from_f32(pl.normal);
            let tex = texinfo[f.texinfo as usize];
            let lit = tex.flags & SURF_NOLIGHT == 0 && f.lightofs != -2;
            let bumped = tex.flags & SURF_BUMPLIGHT != 0;
            let sv = v3(
                tex.lightmap_vecs[0][0] as f64,
                tex.lightmap_vecs[0][1] as f64,
                tex.lightmap_vecs[0][2] as f64,
            );
            let tv = v3(
                tex.lightmap_vecs[1][0] as f64,
                tex.lightmap_vecs[1][1] as f64,
                tex.lightmap_vecs[1][2] as f64,
            );
            let mins = f.lightmap_texture_mins_in_luxels;
            let so = tex.lightmap_vecs[0][3] as f64 - mins[0] as f64;
            let to = tex.lightmap_vecs[1][3] as f64 - mins[1] as f64;
            let dist = pl.dist as f64;
            let m = [
                [sv.x, sv.y, sv.z],
                [tv.x, tv.y, tv.z],
                [normal.x, normal.y, normal.z],
            ];
            let (lm_origin, lm_sdir, lm_tdir) = match inverse3(m) {
                Some(inv) => {
                    let origin = mul3(&inv, v3(-so, -to, dist));
                    let p1 = mul3(&inv, v3(1.0 - so, -to, dist));
                    let p2 = mul3(&inv, v3(-so, 1.0 - to, dist));
                    (origin, p1 - origin, p2 - origin)
                }
                None => (Vec3::ZERO, Vec3::ZERO, Vec3::ZERO),
            };
            let td = texdata
                .get(tex.texdata as usize)
                .copied()
                .unwrap_or_default();
            geoms.push(FaceGeom {
                index: fi,
                points,
                normal,
                dist,
                texinfo: tex,
                flags: tex.flags,
                mins,
                size: f.lightmap_texture_size_in_luxels,
                lm_origin,
                lm_sdir,
                lm_tdir,
                disp: if f.dispinfo >= 0 {
                    Some(f.dispinfo as usize)
                } else {
                    None
                },
                reflectivity: Vec3::from_f32(td.reflectivity),
                model: face_model[fi],
                lit,
                bumped,
            });
        }

        let mut disps = Vec::with_capacity(dispinfo.len());
        for (di, d) in dispinfo.iter().enumerate() {
            let fi = d.map_face as usize;
            let g = &geoms[fi];
            let size = (1usize << d.power) + 1;
            let start = Vec3::from_f32(d.start_position);
            let mut corners = [Vec3::ZERO; 4];
            if g.points.len() == 4 {
                let mut best = 0;
                let mut bd = f64::MAX;
                for (i, p) in g.points.iter().enumerate() {
                    let dd = (*p - start).length_sq();
                    if dd < bd {
                        bd = dd;
                        best = i;
                    }
                }
                for k in 0..4 {
                    corners[k] = g.points[(best + k) % 4];
                }
            }
            let interval = (size - 1) as f64;
            let mut positions = Vec::with_capacity(size * size);
            let mut luxel = Vec::with_capacity(size * size);
            for i in 0..size {
                for j in 0..size {
                    let fi_ = i as f64 / interval;
                    let fj = j as f64 / interval;
                    let e0 = corners[0] + (corners[1] - corners[0]) * fi_;
                    let e1 = corners[3] + (corners[2] - corners[3]) * fi_;
                    let flat = e0 + (e1 - e0) * fj;
                    let dv = dispverts[d.disp_vert_start as usize + i * size + j];
                    positions.push(flat + Vec3::from_f32(dv.vector) * dv.dist as f64);
                    luxel.push(gold_common::disp::disp_vertex_luxel(d.power, g.size, i, j));
                }
            }
            let tris = disp_tri_indices(d.power);
            disps.push(DispSurf {
                face: fi,
                power: d.power,
                size,
                corners,
                positions,
                normals: Vec::new(),
                luxel,
                tris,
            });
            let _ = di;
        }
        compute_disp_normals(&mut disps, &geoms);

        let vis = &bsp.lumps[LUMP_VISIBILITY];
        let mut num_clusters = 0;
        let mut pvs = Vec::new();
        if vis.len() >= 4 {
            num_clusters = i32::from_le_bytes(vis[0..4].try_into().unwrap()) as usize;
            let row = num_clusters.div_ceil(8);
            for c in 0..num_clusters {
                let ofs =
                    i32::from_le_bytes(vis[4 + c * 8..8 + c * 8].try_into().unwrap()) as usize;
                pvs.push(decompress_vis(vis, ofs, row));
            }
        }
        Geometry {
            planes,
            texinfo,
            texdata,
            faces,
            geoms,
            disps,
            nodes,
            leafs,
            models,
            num_clusters,
            pvs,
        }
    }

    pub fn point_leaf(&self, p: Vec3) -> usize {
        if self.nodes.is_empty() {
            return 0;
        }
        let mut n: i32 = self.models.first().map(|m| m.headnode).unwrap_or(0);
        while n >= 0 {
            let node = &self.nodes[n as usize];
            let pl = &self.planes[node.planenum as usize];
            let d = p.dot(Vec3::from_f32(pl.normal)) - pl.dist as f64;
            n = if d >= 0.0 {
                node.children[0]
            } else {
                node.children[1]
            };
        }
        (-(n + 1)) as usize
    }

    pub fn point_cluster(&self, p: Vec3) -> i32 {
        self.leafs[self.point_leaf(p)].cluster as i32
    }

    pub fn cluster_visible(&self, a: i32, b: i32) -> bool {
        if a < 0 || b < 0 || self.pvs.is_empty() {
            return true;
        }
        let row = &self.pvs[a as usize];
        row[(b as usize) >> 3] & (1 << (b & 7)) != 0
    }
}

pub use gold_common::disp::disp_tri_indices;

fn compute_disp_normals(disps: &mut [DispSurf], geoms: &[FaceGeom]) {
    use std::collections::HashMap;
    let key = |p: Vec3| {
        (
            (p.x * 2.0).round() as i64,
            (p.y * 2.0).round() as i64,
            (p.z * 2.0).round() as i64,
        )
    };
    let mut acc: HashMap<(i64, i64, i64), Vec3> = HashMap::new();
    let mut per_disp: Vec<Vec<Vec3>> = Vec::new();
    for d in disps.iter() {
        let face_n = geoms[d.face].normal;
        let mut vn = vec![Vec3::ZERO; d.positions.len()];
        for t in &d.tris {
            let a = d.positions[t[0]];
            let mut n = (d.positions[t[1]] - a).cross(d.positions[t[2]] - a);
            if n.dot(face_n) < 0.0 {
                n = -n;
            }
            for &k in t {
                vn[k] += n;
            }
        }
        for (k, n) in vn.iter().enumerate() {
            *acc.entry(key(d.positions[k])).or_insert(Vec3::ZERO) += n.normalize();
        }
        per_disp.push(vn);
    }
    for (d, vn) in disps.iter_mut().zip(per_disp) {
        let face_n = geoms[d.face].normal;
        d.normals = d
            .positions
            .iter()
            .zip(vn)
            .map(|(p, own)| {
                let shared = acc.get(&key(*p)).copied().unwrap_or(own);
                let n = if shared.length() > 1e-6 {
                    shared.normalize()
                } else {
                    own.normalize()
                };
                if n.length() < 0.5 { face_n } else { n }
            })
            .collect();
    }
}
