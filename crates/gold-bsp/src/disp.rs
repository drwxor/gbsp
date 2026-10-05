use crate::map::Map;
use gold_common::bsp::*;
pub use gold_common::disp::*;
use gold_common::math::*;
use gold_common::winding::Winding;

#[derive(Clone, Debug)]
pub struct Disp {
    pub map_disp: usize,
    pub brush: usize,
    pub side: usize,
    pub entity: usize,
    pub power: i32,
    pub face: Winding,
    pub corners: [Vec3; 4],
    pub start_position: Vec3,
    pub flat: Vec<Vec3>,
    pub positions: Vec<Vec3>,
    pub verts: Vec<DDispVert>,
    pub tri_tags: Vec<u16>,
    pub texinfo: i32,
    pub planenum: usize,
    pub bounds: Aabb,
    pub allowed_verts: [u32; 10],
    pub flags: i32,
    pub edge_neighbors: [DispNeighbor; 4],
    pub corner_neighbors: [DispCornerNeighbors; 4],
}

impl Disp {
    pub fn size(&self) -> usize {
        (1usize << self.power) + 1
    }
}

pub fn build_disps(map: &Map) -> Vec<Disp> {
    let mut out = Vec::new();
    for (di, md) in map.disps.iter().enumerate() {
        let brush = &map.brushes[md.brush];
        let side = &brush.sides[md.side];
        let Some(w) = &side.winding else { continue };
        let w = w.remove_colinear(0.01);
        if w.len() != 4 {
            eprintln!(
                "Warning: displacement on brush {} side {} does not have 4 corners ({}), skipped",
                brush.id,
                side.side_id,
                w.len()
            );
            continue;
        }
        let info = &md.info;
        let size = info.size();
        let corners = corners_from_face(&w.p, info.start_position);
        let plane_normal = map.planes.get(side.planenum).normal;
        let mut flat = Vec::with_capacity(size * size);
        let mut positions = Vec::with_capacity(size * size);
        let mut verts = Vec::with_capacity(size * size);
        let mut bounds = Aabb::EMPTY;
        for i in 0..size {
            for j in 0..size {
                let idx = i * size + j;
                let f = flat_position(&corners, size, i, j);
                let n = info.normals.get(idx).copied().unwrap_or(Vec3::ZERO);
                let d = info.distances.get(idx).copied().unwrap_or(0.0);
                let off = info.offsets.get(idx).copied().unwrap_or(Vec3::ZERO);
                let _offn = info
                    .offset_normals
                    .get(idx)
                    .copied()
                    .unwrap_or(plane_normal);
                let delta = n * d + off + plane_normal * info.elevation;
                let pos = f + delta;
                let len = delta.length();
                let dir = if len > 1e-6 {
                    delta / len
                } else {
                    plane_normal
                };
                flat.push(f);
                positions.push(pos);
                bounds.add_point(pos);
                verts.push(DDispVert {
                    vector: dir.to_f32(),
                    dist: len as f32,
                    alpha: info.alphas.get(idx).copied().unwrap_or(0.0) as f32,
                });
            }
        }
        let tris = disp_tri_indices(info.power);
        let mut tri_tags = Vec::with_capacity(tris.len());
        for t in &tris {
            let a = positions[t[0]];
            let mut n = (positions[t[1]] - a).cross(positions[t[2]] - a).normalize();
            if n.dot(plane_normal) < 0.0 {
                n = -n;
            }
            let nz = n.z;
            let mut tag = 0u16;
            if nz >= 0.7 {
                tag |= DISPTRI_TAG_WALKABLE;
            }
            if nz >= 0.8 {
                tag |= DISPTRI_TAG_BUILDABLE;
            }
            tri_tags.push(tag);
        }
        let mut allowed = [u32::MAX; 10];
        for (k, v) in info.allowed_verts.iter().take(10).enumerate() {
            allowed[k] = *v as u32;
        }
        out.push(Disp {
            map_disp: di,
            brush: md.brush,
            side: md.side,
            entity: md.entity,
            power: info.power,
            face: w.clone(),
            corners,
            start_position: info.start_position,
            flat,
            positions,
            verts,
            tri_tags,
            texinfo: side.texinfo,
            planenum: side.planenum,
            bounds,
            allowed_verts: allowed,
            flags: info.flags,
            edge_neighbors: [DispNeighbor {
                sub: [DispSubNeighbor::NONE; 2],
            }; 4],
            corner_neighbors: [DispCornerNeighbors::default(); 4],
        });
    }
    find_neighbors(&mut out);
    out
}

fn corner_of(d: &Disp, c: usize) -> Vec3 {
    d.corners[c]
}

fn edge_points(d: &Disp, e: usize) -> (Vec3, Vec3) {
    (corner_of(d, e), corner_of(d, (e + 1) % 4))
}

const CORNER_TO_CORNER: u8 = 0;
const CORNER_TO_MIDPOINT: u8 = 1;
const MIDPOINT_TO_CORNER: u8 = 2;

fn veq(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 0.1
}

fn find_neighbors(disps: &mut [Disp]) {
    let n = disps.len();
    let mut edges = vec![
        [DispNeighbor {
            sub: [DispSubNeighbor::NONE; 2]
        }; 4];
        n
    ];
    for i in 0..n {
        for e in 0..4 {
            let (a, b) = edge_points(&disps[i], e);
            let mid = (a + b) * 0.5;
            let mut subs: Vec<DispSubNeighbor> = Vec::new();
            for j in 0..n {
                if i == j || disps[i].entity != disps[j].entity && false {
                    continue;
                }
                for ne in 0..4 {
                    let (c, d) = edge_points(&disps[j], ne);
                    let nmid = (c + d) * 0.5;
                    let orientation = ((ne + 4 - ((e + 2) % 4)) % 4) as u8;
                    if veq(a, d) && veq(b, c) {
                        subs.push(DispSubNeighbor {
                            neighbor: j as u16,
                            neighbor_orientation: orientation,
                            span: CORNER_TO_CORNER,
                            neighbor_span: CORNER_TO_CORNER,
                            pad: 0,
                        });
                    } else if veq(a, d) && veq(mid, c) || veq(mid, d) && veq(b, c) {
                        let sub = DispSubNeighbor {
                            neighbor: j as u16,
                            neighbor_orientation: orientation,
                            span: CORNER_TO_MIDPOINT,
                            neighbor_span: MIDPOINT_TO_CORNER,
                            pad: 0,
                        };
                        if veq(a, d) {
                            subs.insert(0, sub);
                        } else {
                            subs.push(sub);
                        }
                    } else if veq(a, nmid) && veq(b, c) || veq(a, d) && veq(b, nmid) {
                        subs.push(DispSubNeighbor {
                            neighbor: j as u16,
                            neighbor_orientation: orientation,
                            span: CORNER_TO_CORNER,
                            neighbor_span: if veq(a, d) {
                                CORNER_TO_MIDPOINT
                            } else {
                                MIDPOINT_TO_CORNER
                            },
                            pad: 0,
                        });
                    }
                }
            }
            for (k, s) in subs.into_iter().take(2).enumerate() {
                edges[i][e].sub[k] = s;
            }
        }
    }
    let mut corners = vec![[DispCornerNeighbors::default(); 4]; n];
    for i in 0..n {
        for c in 0..4 {
            let p = corner_of(&disps[i], c);
            let mut list = Vec::new();
            for j in 0..n {
                if i == j {
                    continue;
                }
                if edges[i].iter().any(|en| {
                    en.sub
                        .iter()
                        .any(|s| s.is_valid() && s.neighbor as usize == j)
                }) {
                    continue;
                }
                if (0..4).any(|k| veq(corner_of(&disps[j], k), p)) {
                    list.push(j as u16);
                }
            }
            let cn = &mut corners[i][c];
            for (k, &j) in list.iter().take(4).enumerate() {
                cn.neighbors[k] = j;
            }
            cn.num_neighbors = list.len().min(4) as u8;
        }
    }
    for i in 0..n {
        disps[i].edge_neighbors = edges[i];
        disps[i].corner_neighbors = corners[i];
    }
}
