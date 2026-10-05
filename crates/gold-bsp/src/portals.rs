use crate::map::{Map, PlaneSet};
use crate::tree::*;
use gold_common::bsp::*;
use gold_common::math::*;
use gold_common::winding::Winding;

const BASE_WINDING_EPSILON: f64 = 0.001;
const SPLIT_WINDING_EPSILON: f64 = 0.001;

#[derive(Clone, Debug)]
pub struct Portal {
    pub plane: Plane,
    pub onnode: Option<usize>,
    pub nodes: [usize; 2],
    pub winding: Winding,
    pub sidefound: bool,
    pub side: Option<(usize, usize)>,
    pub face: [Option<usize>; 2],
    pub removed: bool,
}

pub struct Portals {
    pub list: Vec<Portal>,
}

fn add_portal_to_nodes(
    tree: &mut Tree,
    portals: &mut Portals,
    p: usize,
    front: usize,
    back: usize,
) {
    portals.list[p].nodes = [front, back];
    tree.nodes[front].portals.push(p);
    tree.nodes[back].portals.push(p);
}

fn remove_portal_from_node(tree: &mut Tree, p: usize, node: usize) {
    let v = &mut tree.nodes[node].portals;
    if let Some(i) = v.iter().position(|&x| x == p) {
        v.swap_remove(i);
    }
}

fn make_headnode_portals(tree: &mut Tree, portals: &mut Portals) {
    let head = tree.head;
    let outside = tree.outside;
    let b = tree.nodes[head].bounds;
    let bounds = [
        b.mins - Vec3::splat(SIDESPACE),
        b.maxs + Vec3::splat(SIDESPACE),
    ];
    let mut bplanes = Vec::new();
    let mut ids = Vec::new();
    for i in 0..3 {
        for j in 0..2 {
            let mut normal = Vec3::ZERO;

            let dist = if j == 1 {
                normal[i] = -1.0;
                -bounds[j][i]
            } else {
                normal[i] = 1.0;
                bounds[j][i]
            };
            let pl = Plane::new(normal, dist);
            bplanes.push(pl);
            let p = portals.list.len();
            portals.list.push(Portal {
                plane: pl,
                onnode: None,
                nodes: [0, 0],
                winding: Winding::base_for_plane(&pl),
                sidefound: false,
                side: None,
                face: [None, None],
                removed: false,
            });
            add_portal_to_nodes(tree, portals, p, head, outside);
            ids.push(p);
        }
    }
    for i in 0..6 {
        let mut w = Some(portals.list[ids[i]].winding.clone());
        for j in 0..6 {
            if i == j {
                continue;
            }
            if let Some(ww) = w {
                w = ww.chop(bplanes[j].normal, bplanes[j].dist, 0.1);
            }
        }
        portals.list[ids[i]].winding = w.unwrap_or_default();
    }
}

fn base_winding_for_node(tree: &Tree, planes: &PlaneSet, node: usize) -> Option<Winding> {
    let pl = planes.get(tree.nodes[node].planenum.unwrap());
    let mut w = Some(Winding::base_for_plane(pl));
    let mut child = node;
    let mut n = tree.nodes[node].parent;
    while let (Some(par), Some(ww)) = (n, w.clone()) {
        let p = planes.get(tree.nodes[par].planenum.unwrap());
        w = if tree.nodes[par].children[0] == child {
            ww.chop(p.normal, p.dist, BASE_WINDING_EPSILON)
        } else {
            ww.chop(-p.normal, -p.dist, BASE_WINDING_EPSILON)
        };
        child = par;
        n = tree.nodes[par].parent;
    }
    w
}

fn make_node_portal(tree: &mut Tree, planes: &PlaneSet, portals: &mut Portals, node: usize) {
    let Some(mut w) = base_winding_for_node(tree, planes, node) else {
        return;
    };
    for &p in &tree.nodes[node].portals {
        let port = &portals.list[p];
        let (normal, dist) = if port.nodes[0] == node {
            (port.plane.normal, port.plane.dist)
        } else {
            (-port.plane.normal, -port.plane.dist)
        };
        match w.chop(normal, dist, 0.1) {
            Some(nw) => w = nw,
            None => return,
        }
    }
    if w.is_tiny() {
        return;
    }
    let pl = *planes.get(tree.nodes[node].planenum.unwrap());
    let p = portals.list.len();
    portals.list.push(Portal {
        plane: pl,
        onnode: Some(node),
        nodes: [0, 0],
        winding: w,
        sidefound: false,
        side: None,
        face: [None, None],
        removed: false,
    });
    let [c0, c1] = tree.nodes[node].children;
    add_portal_to_nodes(tree, portals, p, c0, c1);
}

fn split_node_portals(tree: &mut Tree, planes: &PlaneSet, portals: &mut Portals, node: usize) {
    let plane = *planes.get(tree.nodes[node].planenum.unwrap());
    let [f, b] = tree.nodes[node].children;
    let plist = std::mem::take(&mut tree.nodes[node].portals);
    for p in plist {
        let side = if portals.list[p].nodes[0] == node {
            0
        } else {
            1
        };
        let other = portals.list[p].nodes[1 - side];
        remove_portal_from_node(tree, p, other);
        let (mut fw, mut bw) =
            portals.list[p]
                .winding
                .clip_epsilon(plane.normal, plane.dist, SPLIT_WINDING_EPSILON);
        if fw.as_ref().map(|w| w.is_tiny()).unwrap_or(false) {
            fw = None;
        }
        if bw.as_ref().map(|w| w.is_tiny()).unwrap_or(false) {
            bw = None;
        }
        match (fw, bw) {
            (None, None) => {
                portals.list[p].removed = true;
            }
            (None, Some(_)) => {
                if side == 0 {
                    add_portal_to_nodes(tree, portals, p, b, other);
                } else {
                    add_portal_to_nodes(tree, portals, p, other, b);
                }
            }
            (Some(_), None) => {
                if side == 0 {
                    add_portal_to_nodes(tree, portals, p, f, other);
                } else {
                    add_portal_to_nodes(tree, portals, p, other, f);
                }
            }
            (Some(fw), Some(bw)) => {
                let mut np = portals.list[p].clone();
                np.winding = bw;
                portals.list[p].winding = fw;
                let npi = portals.list.len();
                portals.list.push(np);
                if side == 0 {
                    add_portal_to_nodes(tree, portals, p, f, other);
                    add_portal_to_nodes(tree, portals, npi, b, other);
                } else {
                    add_portal_to_nodes(tree, portals, p, other, f);
                    add_portal_to_nodes(tree, portals, npi, other, b);
                }
            }
        }
    }
}

pub fn make_tree_portals(tree: &mut Tree, planes: &PlaneSet) -> Portals {
    let mut portals = Portals { list: Vec::new() };
    make_headnode_portals(tree, &mut portals);
    let mut stack = vec![tree.head];
    while let Some(n) = stack.pop() {
        if tree.nodes[n].is_leaf() {
            continue;
        }
        make_node_portal(tree, planes, &mut portals, n);
        split_node_portals(tree, planes, &mut portals, n);
        let [c0, c1] = tree.nodes[n].children;
        stack.push(c1);
        stack.push(c0);
    }
    portals
}

fn portal_entity_flood(tree: &Tree, p: &Portal) -> bool {
    tree.nodes[p.nodes[0]].contents & CONTENTS_SOLID == 0
        && tree.nodes[p.nodes[1]].contents & CONTENTS_SOLID == 0
}

pub struct LeakInfo {
    pub entity: usize,
    pub path: Vec<Vec3>,
}

pub fn flood_entities(
    tree: &mut Tree,
    planes: &PlaneSet,
    portals: &Portals,
    map: &Map,
) -> Result<bool, LeakInfo> {
    let mut inside = false;
    let mut parent: Vec<Option<(usize, usize)>> = vec![None; tree.nodes.len()];
    let mut first_entity = vec![usize::MAX; tree.nodes.len()];
    for (ei, e) in map.entities.iter().enumerate().skip(1) {
        let Some(o) = e.get("origin").and_then(parse_vec3) else {
            continue;
        };
        if o == Vec3::ZERO {
            continue;
        }
        let class = e.classname().to_ascii_lowercase();
        let mut placed = false;
        let offsets: &[Vec3] = if class == "info_player_start" {
            &[
                v3(0.0, 0.0, 1.0),
                v3(-16.0, -16.0, 1.0),
                v3(16.0, -16.0, 1.0),
                v3(-16.0, 16.0, 1.0),
                v3(16.0, 16.0, 1.0),
                v3(0.0, 0.0, 16.0),
            ]
        } else {
            &[v3(0.0, 0.0, 1.0)]
        };
        for off in offsets {
            let leaf = tree.point_leaf(planes, o + *off);
            if tree.nodes[leaf].contents & CONTENTS_SOLID != 0 {
                continue;
            }
            placed = true;
            inside = true;
            if tree.nodes[leaf].occupied != 0 {
                break;
            }
            tree.nodes[leaf].occupant = Some(ei);
            tree.nodes[leaf].occupied = 1;
            first_entity[leaf] = ei;
            let mut queue = std::collections::VecDeque::new();
            queue.push_back(leaf);
            while let Some(n) = queue.pop_front() {
                let dist = tree.nodes[n].occupied;
                let plist = tree.nodes[n].portals.clone();
                for p in plist {
                    let port = &portals.list[p];
                    if port.removed {
                        continue;
                    }
                    let s = if port.nodes[1] == n { 1 } else { 0 };
                    let other = port.nodes[1 - s];
                    if tree.nodes[other].occupied != 0 {
                        continue;
                    }
                    if !portal_entity_flood(tree, port) {
                        continue;
                    }
                    tree.nodes[other].occupied = dist + 1;
                    tree.nodes[other].occupant = Some(ei);
                    parent[other] = Some((n, p));
                    first_entity[other] = ei;
                    queue.push_back(other);
                }
            }
            break;
        }
        let _ = placed;
    }
    if tree.nodes[tree.outside].occupied != 0 {
        let mut path = Vec::new();
        let mut n = tree.outside;
        let ent = first_entity[n];
        let mut guard = 0;
        while let Some((prev, p)) = parent[n] {
            path.push(portals.list[p].winding.center());
            n = prev;
            guard += 1;
            if guard > 100000 {
                break;
            }
        }
        if let Some(o) = map
            .entities
            .get(ent)
            .and_then(|e| e.get("origin"))
            .and_then(parse_vec3)
        {
            path.push(o);
        }
        return Err(LeakInfo { entity: ent, path });
    }
    Ok(inside)
}

pub fn fill_outside(tree: &mut Tree) -> (usize, usize, usize) {
    let (mut outside, mut inside, mut solid) = (0, 0, 0);
    for l in tree.leafs() {
        if tree.nodes[l].occupied == 0 {
            if tree.nodes[l].contents & CONTENTS_SOLID == 0 {
                outside += 1;
                tree.nodes[l].contents = CONTENTS_SOLID;
            } else {
                solid += 1;
            }
        } else {
            inside += 1;
        }
    }
    (outside, inside, solid)
}

pub fn visible_contents(contents: i32) -> i32 {
    let mut i = 1;
    while i <= LAST_VISIBLE_CONTENTS {
        if contents & i != 0 {
            return i;
        }
        i <<= 1;
    }
    0
}

fn find_portal_side(
    tree: &Tree,
    planes: &PlaneSet,
    map: &Map,
    p: &Portal,
) -> Option<(usize, usize)> {
    let c0 = tree.nodes[p.nodes[0]].contents;
    let c1 = tree.nodes[p.nodes[1]].contents;
    let viscontents = visible_contents(c0 ^ c1);
    if viscontents == 0 {
        return None;
    }
    let onnode = p.onnode?;
    let planenum = tree.nodes[onnode].planenum?;
    let p1 = planes.get(planenum);
    let center = p.winding.center();
    let mut best: Option<(usize, usize)> = None;
    let mut bestdot = 0.0;
    let mut exact: Vec<(usize, usize)> = Vec::new();
    for j in 0..2 {
        let n = p.nodes[j];
        for bb in &tree.nodes[n].brushes {
            let mb = &map.brushes[bb.original];
            if mb.contents & viscontents == 0 {
                continue;
            }
            for (si, side) in mb.sides.iter().enumerate() {
                if side.bevel {
                    continue;
                }
                if (side.planenum & !1) == planenum {
                    exact.push((bb.original, si));
                    continue;
                }
                let p2 = planes.get(side.planenum & !1);
                let dot = p1.normal.dot(p2.normal);
                if dot > bestdot {
                    bestdot = dot;
                    best = Some((bb.original, si));
                }
            }
        }
    }
    if !exact.is_empty() {
        for &(b, s) in &exact {
            if let Some(w) = &map.brushes[b].sides[s].winding
                && point_in_winding(w, center, 0.5)
            {
                return Some((b, s));
            }
        }
        for &(b, s) in &exact {
            if map.brushes[b].sides[s].surf_flags & SURF_NODRAW == 0 {
                return Some((b, s));
            }
        }
        return Some(exact[0]);
    }
    best
}

pub fn point_in_winding(w: &Winding, p: Vec3, eps: f64) -> bool {
    let n = w.len();
    if n < 3 {
        return false;
    }
    let normal = w.plane().normal;
    let c = w.center();
    for i in 0..n {
        let a = w.p[i];
        let b = w.p[(i + 1) % n];
        let e = b - a;
        let inward = normal.cross(e);
        let sgn = if inward.dot(c - a) >= 0.0 { 1.0 } else { -1.0 };
        let d = inward.normalize().dot(p - a) * sgn;
        if d < -eps {
            return false;
        }
    }
    true
}

pub fn mark_visible_sides(tree: &Tree, portals: &mut Portals, map: &mut Map, brush_ids: &[usize]) {
    for &b in brush_ids {
        for s in map.brushes[b].sides.iter_mut() {
            s.visible = false;
        }
    }
    for l in tree.leafs() {
        if tree.nodes[l].contents == 0 {
            continue;
        }
        for &p in &tree.nodes[l].portals {
            if portals.list[p].removed || portals.list[p].onnode.is_none() {
                continue;
            }
            if !portals.list[p].sidefound {
                let s = find_portal_side(tree, &map.planes, map, &portals.list[p]);
                portals.list[p].side = s;
                portals.list[p].sidefound = true;
            }
            if let Some((b, s)) = portals.list[p].side {
                map.brushes[b].sides[s].visible = true;
            }
        }
    }
}

pub struct AreaInfo {
    pub num_areas: usize,
    pub portal_areas: std::collections::BTreeMap<usize, Vec<i32>>,
}

fn leaf_areaportal(tree: &Tree, map: &Map, n: usize) -> Option<usize> {
    if tree.nodes[n].contents & CONTENTS_AREAPORTAL == 0 {
        return None;
    }
    tree.nodes[n]
        .brushes
        .iter()
        .find_map(|b| map.brushes[b.original].areaportal)
}

pub fn flood_areas(tree: &mut Tree, portals: &Portals, map: &Map) -> AreaInfo {
    let mut info = AreaInfo {
        num_areas: 1,
        portal_areas: Default::default(),
    };
    for l in tree.leafs() {
        tree.nodes[l].area = 0;
    }
    let mut next_area = 0;
    for start in tree.leafs() {
        let n = &tree.nodes[start];
        if n.area != 0
            || n.contents & CONTENTS_SOLID != 0
            || leaf_areaportal(tree, map, start).is_some()
        {
            continue;
        }
        next_area += 1;
        let area = next_area;
        tree.nodes[start].area = area;
        let mut stack = vec![start];
        while let Some(cur) = stack.pop() {
            let plist = tree.nodes[cur].portals.clone();
            for p in plist {
                let port = &portals.list[p];
                if port.removed || port.onnode.is_none() {
                    continue;
                }
                let s = if port.nodes[1] == cur { 1 } else { 0 };
                let other = port.nodes[1 - s];
                if !portal_entity_flood(tree, port) {
                    continue;
                }
                if let Some(e) = leaf_areaportal(tree, map, other) {
                    let v = info.portal_areas.entry(e).or_default();
                    if !v.contains(&area) {
                        v.push(area);
                    }
                    continue;
                }
                if tree.nodes[other].area == 0 {
                    tree.nodes[other].area = area;
                    stack.push(other);
                }
            }
        }
    }
    for l in tree.leafs() {
        if let Some(e) = leaf_areaportal(tree, map, l) {
            tree.nodes[l].area = info
                .portal_areas
                .get(&e)
                .and_then(|v| v.first().copied())
                .unwrap_or(0);
        }
    }
    for (e, areas) in &info.portal_areas {
        if areas.len() != 2 {
            let ent = &map.entities[*e];
            eprintln!(
                "Warning: {} {} touches {} area(s); it must separate exactly two areas",
                ent.classname(),
                ent.get("targetname").or(ent.get("hammerid")).unwrap_or(""),
                areas.len()
            );
        }
    }
    info.num_areas = next_area as usize + 1;
    info
}

fn convex_hull_2d(pts: &[(f64, f64)]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..pts.len()).collect();
    idx.sort_by(|&a, &b| pts[a].partial_cmp(&pts[b]).unwrap());
    idx.dedup_by(|a, b| {
        (pts[*a].0 - pts[*b].0).abs() < 1e-6 && (pts[*a].1 - pts[*b].1).abs() < 1e-6
    });
    if idx.len() < 3 {
        return idx;
    }
    let cross = |o: usize, a: usize, b: usize| {
        (pts[a].0 - pts[o].0) * (pts[b].1 - pts[o].1)
            - (pts[a].1 - pts[o].1) * (pts[b].0 - pts[o].0)
    };
    let mut lower: Vec<usize> = Vec::new();
    for &i in &idx {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], i) <= 1e-9 {
            lower.pop();
        }
        lower.push(i);
    }
    let mut upper: Vec<usize> = Vec::new();
    for &i in idx.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], i) <= 1e-9 {
            upper.pop();
        }
        upper.push(i);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

pub fn build_area_lumps(
    tree: &Tree,
    portals: &Portals,
    map: &mut Map,
    info: &AreaInfo,
) -> (Vec<DArea>, Vec<DAreaPortal>, Vec<[f32; 3]>) {
    let mut geom: std::collections::BTreeMap<usize, (Vec<Vec3>, Plane, [i32; 2])> =
        Default::default();
    for (&e, areas) in &info.portal_areas {
        if areas.len() != 2 {
            continue;
        }
        let a0 = areas[0];
        let mut points = Vec::new();
        let mut best: Option<(f64, Plane, Vec3)> = None;
        for l in tree.leafs() {
            if leaf_areaportal(tree, map, l) != Some(e) {
                continue;
            }
            for &p in &tree.nodes[l].portals {
                let port = &portals.list[p];
                if port.removed || port.onnode.is_none() {
                    continue;
                }
                let other = if port.nodes[0] == l {
                    port.nodes[1]
                } else {
                    port.nodes[0]
                };
                if tree.nodes[other].area != a0
                    || leaf_areaportal(tree, map, other).is_some()
                    || tree.nodes[other].contents & CONTENTS_SOLID != 0
                {
                    continue;
                }
                points.extend(port.winding.p.iter().copied());
                let area = port.winding.area();
                if best.as_ref().map(|b| area > b.0).unwrap_or(true) {
                    best = Some((area, port.plane, tree.nodes[other].bounds.center()));
                }
            }
        }
        let Some((_, mut plane, a0_center)) = best else {
            continue;
        };
        if plane.distance(a0_center) < 0.0 {
            plane = plane.flip();
        }
        let n = plane.normal;
        let (u, v) = {
            let a = if n.x.abs() > 0.9 {
                v3(0.0, 1.0, 0.0)
            } else {
                v3(1.0, 0.0, 0.0)
            };
            let u = n.cross(a).normalize();
            (u, n.cross(u))
        };
        let p2: Vec<(f64, f64)> = points.iter().map(|p| (p.dot(u), p.dot(v))).collect();
        let hull: Vec<Vec3> = convex_hull_2d(&p2)
            .into_iter()
            .map(|i| {
                let p = points[i];
                p - n * (p.dot(n) - plane.dist)
            })
            .collect();
        if hull.len() < 3 {
            continue;
        }
        geom.insert(e, (hull, plane, [areas[0], areas[1]]));
    }
    let mut areas = vec![DArea {
        numareaportals: 0,
        firstareaportal: 0,
    }];
    let mut aps = vec![DAreaPortal::default()];
    let mut verts: Vec<[f32; 3]> = Vec::new();
    for a in 1..info.num_areas as i32 {
        let first = aps.len() as i32;
        for (&e, (hull, plane, pa)) in &geom {
            if pa[0] != a && pa[1] != a {
                continue;
            }
            let other = if pa[0] == a { pa[1] } else { pa[0] };
            let pl = if pa[0] == a { *plane } else { plane.flip() };
            let planenum = map.planes.find(pl.normal, pl.dist);
            let key = map.entities[e]
                .get("portalnumber")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0u16);
            aps.push(DAreaPortal {
                portal_key: key,
                otherarea: other as u16,
                first_clip_portal_vert: verts.len() as u16,
                num_clip_portal_verts: hull.len() as u16,
                planenum: planenum as i32,
            });
            verts.extend(hull.iter().map(|p| p.to_f32()));
        }
        areas.push(DArea {
            numareaportals: aps.len() as i32 - first,
            firstareaportal: first,
        });
    }
    (areas, aps, verts)
}
