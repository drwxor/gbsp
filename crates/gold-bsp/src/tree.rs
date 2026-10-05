use crate::map::{Map, PlaneSet};
use gold_common::bsp::*;
use gold_common::math::*;
use gold_common::winding::Winding;

pub const SIDESPACE: f64 = 8.0;
pub const PLANESIDE_EPSILON: f64 = 0.001;
pub const BLOCK_SIZE: f64 = 1024.0;

pub const PSIDE_FRONT: i32 = 1;
pub const PSIDE_BACK: i32 = 2;
pub const PSIDE_BOTH: i32 = 3;
pub const PSIDE_FACING: i32 = 4;

#[derive(Clone, Debug)]
pub struct BspSide {
    pub planenum: usize,
    pub winding: Option<Winding>,
    pub original: Option<(usize, usize)>,
    pub visible: bool,
    pub tested: bool,
    pub bevel: bool,
    pub on_node: bool,
    pub surf_flags: i32,
}

#[derive(Clone, Debug)]
pub struct BspBrush {
    pub original: usize,
    pub contents: i32,
    pub sides: Vec<BspSide>,
    pub bounds: Aabb,
    pub side: i32,
    pub testside: i32,
}

impl BspBrush {
    pub fn bound(&mut self) {
        let mut b = Aabb::EMPTY;
        for s in &self.sides {
            if let Some(w) = &s.winding {
                for p in &w.p {
                    b.add_point(*p);
                }
            }
        }
        self.bounds = b;
    }

    pub fn volume(&self, planes: &PlaneSet) -> f64 {
        let Some(corner) = self
            .sides
            .iter()
            .find_map(|s| s.winding.as_ref().map(|w| w.p[0]))
        else {
            return 0.0;
        };
        let mut vol = 0.0;
        for s in &self.sides {
            let Some(w) = &s.winding else { continue };
            let p = planes.get(s.planenum);
            let d = -(corner.dot(p.normal) - p.dist);
            vol += d * w.area();
        }
        vol / 3.0
    }

    pub fn from_bounds(planes: &mut PlaneSet, mins: Vec3, maxs: Vec3) -> BspBrush {
        let mut b = BspBrush {
            original: usize::MAX,
            contents: 0,
            sides: Vec::new(),
            bounds: Aabb::EMPTY,
            side: 0,
            testside: 0,
        };
        for i in 0..3 {
            let mut n = Vec3::ZERO;
            n[i] = 1.0;
            let p = planes.find(n, maxs[i]);
            b.sides.push(BspSide {
                planenum: p,
                winding: None,
                original: None,
                visible: false,
                tested: false,
                bevel: false,
                on_node: false,
                surf_flags: 0,
            });
            let mut n = Vec3::ZERO;
            n[i] = -1.0;
            let p = planes.find(n, -mins[i]);
            b.sides.push(BspSide {
                planenum: p,
                winding: None,
                original: None,
                visible: false,
                tested: false,
                bevel: false,
                on_node: false,
                surf_flags: 0,
            });
        }
        create_brush_windings(planes, &mut b);
        b
    }
}

pub fn create_brush_windings(planes: &PlaneSet, b: &mut BspBrush) {
    let n = b.sides.len();
    for i in 0..n {
        let p = planes.get(b.sides[i].planenum);
        let mut w = Some(Winding::base_for_plane(p));
        for j in 0..n {
            if i == j || b.sides[j].bevel {
                continue;
            }
            let pj = planes.get(b.sides[j].planenum ^ 1);
            if let Some(ww) = w {
                w = ww.chop(pj.normal, pj.dist, 0.0);
            }
        }
        b.sides[i].winding = w;
    }
    b.bound();
}

pub fn box_on_plane_side(mins: Vec3, maxs: Vec3, plane: &Plane, ptype: i32) -> i32 {
    if ptype < 3 {
        let a = ptype as usize;
        let n = plane.normal[a];
        let (lo, hi) = if n > 0.0 {
            (mins[a], maxs[a])
        } else {
            (-maxs[a], -mins[a])
        };
        let dist = plane.dist;
        let mut side = 0;
        if hi > dist + PLANESIDE_EPSILON {
            side |= PSIDE_FRONT;
        }
        if lo < dist - PLANESIDE_EPSILON {
            side |= PSIDE_BACK;
        }
        return side;
    }
    let mut corners = [Vec3::ZERO; 2];
    for i in 0..3 {
        if plane.normal[i] < 0.0 {
            corners[0][i] = mins[i];
            corners[1][i] = maxs[i];
        } else {
            corners[1][i] = mins[i];
            corners[0][i] = maxs[i];
        }
    }
    let d1 = plane.normal.dot(corners[0]) - plane.dist;
    let d2 = plane.normal.dot(corners[1]) - plane.dist;
    let mut side = 0;
    if d1 >= PLANESIDE_EPSILON {
        side = PSIDE_FRONT;
    }
    if d2 < PLANESIDE_EPSILON {
        side |= PSIDE_BACK;
    }
    side
}

fn brush_mostly_on_side(planes: &PlaneSet, b: &BspBrush, planenum: usize) -> i32 {
    let p = planes.get(planenum);
    let mut max = 0.0;
    let mut side = PSIDE_FRONT;
    for s in &b.sides {
        let Some(w) = &s.winding else { continue };
        for pt in &w.p {
            let d = pt.dot(p.normal) - p.dist;
            if d > max {
                max = d;
                side = PSIDE_FRONT;
            }
            if -d > max {
                max = -d;
                side = PSIDE_BACK;
            }
        }
    }
    side
}

pub fn split_brush(
    planes: &PlaneSet,
    brush: &BspBrush,
    planenum: usize,
) -> (Option<BspBrush>, Option<BspBrush>) {
    let plane = *planes.get(planenum);
    let mut d_front: f64 = 0.0;
    let mut d_back: f64 = 0.0;
    for s in &brush.sides {
        let Some(w) = &s.winding else { continue };
        for p in &w.p {
            let d = p.dot(plane.normal) - plane.dist;
            if d > 0.0 && d > d_front {
                d_front = d;
            }
            if d < 0.0 && d < d_back {
                d_back = d;
            }
        }
    }
    if d_front < 0.1 {
        return (None, Some(brush.clone()));
    }
    if d_back > -0.1 {
        return (Some(brush.clone()), None);
    }

    let mut w = Some(Winding::base_for_plane(&plane));
    for s in &brush.sides {
        let Some(ww) = w else { break };
        let p2 = planes.get(s.planenum ^ 1);
        w = ww.chop(p2.normal, p2.dist, 0.0);
    }
    let mid = match w {
        Some(w) if !w.is_tiny() => w,
        _ => {
            let side = brush_mostly_on_side(planes, brush, planenum);
            return if side == PSIDE_FRONT {
                (Some(brush.clone()), None)
            } else {
                (None, Some(brush.clone()))
            };
        }
    };

    let mut b = [
        BspBrush {
            original: brush.original,
            contents: brush.contents,
            sides: Vec::new(),
            bounds: Aabb::EMPTY,
            side: 0,
            testside: 0,
        },
        BspBrush {
            original: brush.original,
            contents: brush.contents,
            sides: Vec::new(),
            bounds: Aabb::EMPTY,
            side: 0,
            testside: 0,
        },
    ];
    for s in &brush.sides {
        let Some(w) = &s.winding else { continue };
        let (f, bk) = w.clip_epsilon(plane.normal, plane.dist, 0.0);
        for (j, cw) in [f, bk].into_iter().enumerate() {
            if let Some(cw) = cw {
                let mut cs = s.clone();
                cs.winding = Some(cw);
                cs.tested = false;
                b[j].sides.push(cs);
            }
        }
    }
    let mut valid = [true, true];
    for i in 0..2 {
        b[i].bound();
        if b[i].sides.len() < 3 || b[i].bounds.is_empty() {
            valid[i] = false;
            continue;
        }
        for j in 0..3 {
            if b[i].bounds.mins[j] < -65536.0 || b[i].bounds.maxs[j] > 65536.0 {
                valid[i] = false;
            }
        }
    }
    if !(valid[0] && valid[1]) {
        return (
            if valid[0] { Some(brush.clone()) } else { None },
            if valid[1] { Some(brush.clone()) } else { None },
        );
    }
    for i in 0..2 {
        b[i].sides.push(BspSide {
            planenum: planenum ^ i ^ 1,
            winding: Some(if i == 0 { mid.clone() } else { mid.reverse() }),
            original: None,
            visible: false,
            tested: false,
            bevel: false,
            on_node: true,
            surf_flags: 0,
        });
    }
    for i in 0..2 {
        let last = b[i].sides.len() - 1;
        let pl = planes.get(b[i].sides[last].planenum);
        let w = b[i].sides[last].winding.as_ref().unwrap();
        if w.plane().normal.dot(pl.normal) < 0.0 {
            b[i].sides[last].winding = Some(w.reverse());
        }
    }
    let [b0, b1] = b;
    let v0 = b0.volume(planes);
    let v1 = b1.volume(planes);
    (
        if v0 < 1.0 { None } else { Some(b0) },
        if v1 < 1.0 { None } else { Some(b1) },
    )
}

#[derive(Clone, Debug, Default)]
pub struct Node {
    pub planenum: Option<usize>,
    pub children: [usize; 2],
    pub parent: Option<usize>,
    pub bounds: Aabb,
    pub volume: Option<BspBrush>,
    pub side: Option<(usize, usize)>,
    pub contents: i32,
    pub brushes: Vec<BspBrush>,
    pub portals: Vec<usize>,
    pub occupied: i32,
    pub occupant: Option<usize>,
    pub area: i32,
    pub cluster: i32,
    pub faces: Vec<usize>,
    pub leaf_faces: Vec<usize>,
    pub extra_brushes: Vec<usize>,
    pub output: i32,
    pub water_id: i32,
}

impl Node {
    pub fn is_leaf(&self) -> bool {
        self.planenum.is_none()
    }
}

pub struct Tree {
    pub nodes: Vec<Node>,
    pub head: usize,
    pub outside: usize,
    pub bounds: Aabb,
}

impl Tree {
    pub fn point_leaf(&self, planes: &PlaneSet, p: Vec3) -> usize {
        let mut n = self.head;
        while let Some(pn) = self.nodes[n].planenum {
            let pl = planes.get(pn);
            let d = p.dot(pl.normal) - pl.dist;
            n = if d >= 0.0 {
                self.nodes[n].children[0]
            } else {
                self.nodes[n].children[1]
            };
        }
        n
    }

    pub fn leafs(&self) -> Vec<usize> {
        let mut out = Vec::new();
        let mut stack = vec![self.head];
        while let Some(n) = stack.pop() {
            if self.nodes[n].is_leaf() {
                out.push(n);
            } else {
                stack.push(self.nodes[n].children[1]);
                stack.push(self.nodes[n].children[0]);
            }
        }
        out
    }
}

struct Builder<'a> {
    planes: &'a mut PlaneSet,
    nodes: Vec<Node>,
    use_blocks: bool,
    hint_priority: bool,
}

fn test_brush_to_planenum(
    planes: &PlaneSet,
    brush: &BspBrush,
    planenum: usize,
) -> (i32, i32, bool, i32) {
    for s in &brush.sides {
        if s.planenum == planenum {
            return (PSIDE_BACK | PSIDE_FACING, 0, false, 0);
        }
        if s.planenum == (planenum ^ 1) {
            return (PSIDE_FRONT | PSIDE_FACING, 0, false, 0);
        }
    }
    let plane = planes.get(planenum);
    let s = box_on_plane_side(
        brush.bounds.mins,
        brush.bounds.maxs,
        plane,
        planes.types[planenum],
    );
    if s != PSIDE_BOTH {
        return (s, 0, false, 0);
    }
    let mut numsplits = 0;
    let mut hintsplit = false;
    let mut d_front: f64 = 0.0;
    let mut d_back: f64 = 0.0;
    for side in &brush.sides {
        if side.on_node || !side.visible {
            continue;
        }
        let Some(w) = &side.winding else { continue };
        let mut front = false;
        let mut back = false;
        for p in &w.p {
            let d = p.dot(plane.normal) - plane.dist;
            d_front = d_front.max(d);
            d_back = d_back.min(d);
            if d > 0.1 {
                front = true;
            }
            if d < -0.1 {
                back = true;
            }
        }
        if front && back && side.surf_flags & SURF_SKIP == 0 {
            numsplits += 1;
            if side.surf_flags & SURF_HINT != 0 {
                hintsplit = true;
            }
        }
    }
    let eps = if (d_front > 0.0 && d_front < 1.0) || (d_back < 0.0 && d_back > -1.0) {
        1
    } else {
        0
    };
    (s, numsplits, hintsplit, eps)
}

impl<'a> Builder<'a> {
    fn check_plane_against_volume(&self, planenum: usize, node: usize) -> bool {
        let Some(vol) = &self.nodes[node].volume else {
            return false;
        };
        let (f, b) = split_brush(self.planes, vol, planenum);
        f.is_some() && b.is_some()
    }

    fn select_split_side(
        &mut self,
        brushes: &mut [BspBrush],
        node: usize,
    ) -> Option<(usize, Option<(usize, usize)>)> {
        let mut best: Option<(usize, Option<(usize, usize)>)> = None;
        let mut bestvalue = -99999999i64;
        for pass in 0..4 {
            for bi in 0..brushes.len() {
                let is_detail = brushes[bi].contents & CONTENTS_DETAIL != 0;
                if (pass & 1 != 0) != is_detail {
                    continue;
                }
                for si in 0..brushes[bi].sides.len() {
                    let side = &brushes[bi].sides[si];
                    if side.bevel || side.winding.is_none() || side.on_node || side.tested {
                        continue;
                    }
                    if side.surf_flags & SURF_SKIP != 0 {
                        continue;
                    }
                    if side.visible != (pass < 2) {
                        continue;
                    }
                    let is_hint = side.surf_flags & SURF_HINT != 0;
                    let orig = side.original;
                    let pnum = side.planenum & !1;
                    if !self.check_plane_against_volume(pnum, node) {
                        continue;
                    }
                    let mut front = 0i64;
                    let mut back = 0i64;
                    let mut facing = 0i64;
                    let mut splits = 0i64;
                    let mut epsilonbrush = 0i64;
                    let mut any_hint = false;
                    for ti in 0..brushes.len() {
                        let (s, bsplits, hs, eps) =
                            test_brush_to_planenum(self.planes, &brushes[ti], pnum);
                        splits += bsplits as i64;
                        any_hint |= hs;
                        epsilonbrush += eps as i64;
                        brushes[ti].testside = s;
                        if s & PSIDE_FACING != 0 {
                            facing += 1;
                            for s2 in brushes[ti].sides.iter_mut() {
                                if (s2.planenum & !1) == pnum {
                                    s2.tested = true;
                                }
                            }
                        }
                        if s & PSIDE_FRONT != 0 {
                            front += 1;
                        }
                        if s & PSIDE_BACK != 0 {
                            back += 1;
                        }
                    }
                    let mut value = 5 * facing - 5 * splits - (front - back).abs();
                    if self.planes.types[pnum] < 3 {
                        value += 5;
                    }
                    value -= epsilonbrush * 1000;
                    if any_hint && !is_hint {
                        value = -9999999;
                    }
                    if is_hint && self.hint_priority {
                        value += 1_000_000;
                    }
                    if value > bestvalue {
                        bestvalue = value;
                        best = Some((pnum, orig));
                        for t in brushes.iter_mut() {
                            t.side = t.testside;
                        }
                    }
                }
            }
            if best.is_some() {
                break;
            }
        }
        for b in brushes.iter_mut() {
            for s in b.sides.iter_mut() {
                s.tested = false;
            }
        }
        best
    }

    fn leaf_node(&mut self, node: usize, brushes: Vec<BspBrush>) {
        let mut contents = 0;
        for b in &brushes {
            if b.contents & CONTENTS_SOLID != 0 && b.sides.iter().all(|s| s.on_node) {
                contents = CONTENTS_SOLID;
                break;
            }
            contents |= b.contents;
        }
        if contents & CONTENTS_SOLID != 0 {
            contents = CONTENTS_SOLID | (contents & !(ALL_VISIBLE_CONTENTS));
            contents &= !CONTENTS_DETAIL;
        }
        let n = &mut self.nodes[node];
        n.planenum = None;
        n.contents = contents;
        n.brushes = brushes;
    }

    fn split_brush_list(
        &mut self,
        brushes: Vec<BspBrush>,
        planenum: usize,
    ) -> (Vec<BspBrush>, Vec<BspBrush>) {
        let mut front = Vec::new();
        let mut back = Vec::new();
        for mut b in brushes {
            let sides = b.side;
            if sides == PSIDE_BOTH {
                let (f, bk) = split_brush(self.planes, &b, planenum);
                if let Some(f) = f {
                    front.push(f);
                }
                if let Some(bk) = bk {
                    back.push(bk);
                }
                continue;
            }
            if sides & PSIDE_FACING != 0 {
                for s in b.sides.iter_mut() {
                    if (s.planenum & !1) == planenum {
                        s.on_node = true;
                    }
                }
            }
            if sides & PSIDE_FRONT != 0 {
                front.push(b);
            } else if sides & PSIDE_BACK != 0 {
                back.push(b);
            }
        }
        (front, back)
    }

    fn block_split(&mut self, node: usize) -> Option<usize> {
        let vol = self.nodes[node].volume.as_ref()?;
        let b = vol.bounds;
        let mut best: Option<(usize, f64, f64)> = None;
        for axis in 0..2 {
            let lo = (b.mins[axis] / BLOCK_SIZE).floor() as i64;
            let hi = ((b.maxs[axis]) / BLOCK_SIZE).ceil() as i64;
            let mut cands = Vec::new();
            for k in lo..=hi {
                let d = k as f64 * BLOCK_SIZE;
                if d > b.mins[axis] + 1.0 && d < b.maxs[axis] - 1.0 {
                    cands.push(d);
                }
            }
            if cands.is_empty() {
                continue;
            }
            let span = b.maxs[axis] - b.mins[axis];
            let mid = cands[cands.len() / 2];
            if best.map(|(_, _, s)| span > s).unwrap_or(true) {
                best = Some((axis, mid, span));
            }
        }
        let (axis, d, _) = best?;
        let mut n = Vec3::ZERO;
        n[axis] = 1.0;
        let pn = self.planes.find(n, d);
        Some(pn & !1)
    }

    fn build_r(&mut self, node: usize, mut brushes: Vec<BspBrush>) {
        let mut split: Option<(usize, Option<(usize, usize)>)> = None;
        if self.use_blocks
            && !brushes.is_empty()
            && let Some(pn) = self.block_split(node)
            && self.check_plane_against_volume(pn, node)
        {
            for b in brushes.iter_mut() {
                let (s, _, _, _) = test_brush_to_planenum(self.planes, b, pn);
                b.side = s;
            }
            split = Some((pn, None));
        }
        if split.is_none() {
            split = self.select_split_side(&mut brushes, node);
        }
        let Some((planenum, side)) = split else {
            self.leaf_node(node, brushes);
            return;
        };
        self.nodes[node].planenum = Some(planenum);
        self.nodes[node].side = side;
        let (front, back) = self.split_brush_list(brushes, planenum);
        let vol = self.nodes[node].volume.take().unwrap();
        let (fv, bv) = split_brush(self.planes, &vol, planenum);
        self.nodes[node].bounds = vol.bounds;
        let mut children = [0usize; 2];
        for (i, v) in [fv, bv].into_iter().enumerate() {
            let c = self.nodes.len();
            let mut n = Node {
                parent: Some(node),
                cluster: -1,
                output: -1,
                water_id: -1,
                ..Default::default()
            };
            if let Some(v) = &v {
                n.bounds = v.bounds;
            }
            n.volume = v;
            self.nodes.push(n);
            children[i] = c;
        }
        self.nodes[node].children = children;
        self.build_r(children[0], front);
        self.build_r(children[1], back);
    }
}

pub fn make_bsp_brush_list(map: &Map, brush_ids: &[usize], clip: &Aabb) -> Vec<BspBrush> {
    let mut out = Vec::new();
    for &bi in brush_ids {
        let mb = &map.brushes[bi];
        if mb.is_disp {
            continue;
        }
        if !mb.bounds.intersects(clip) {
            continue;
        }
        let mut b = BspBrush {
            original: bi,
            contents: mb.contents,
            sides: Vec::new(),
            bounds: mb.bounds,
            side: 0,
            testside: 0,
        };
        for (si, s) in mb.sides.iter().enumerate() {
            if s.bevel {
                continue;
            }
            b.sides.push(BspSide {
                planenum: s.planenum,
                winding: s.winding.clone(),
                original: Some((bi, si)),
                visible: s.visible,
                tested: false,
                bevel: false,
                on_node: false,
                surf_flags: s.surf_flags,
            });
        }
        out.push(b);
    }
    out
}

pub fn brush_bsp(
    planes: &mut PlaneSet,
    brushes: Vec<BspBrush>,
    bounds: Aabb,
    use_blocks: bool,
) -> Tree {
    let mins = bounds.mins - Vec3::splat(SIDESPACE);
    let maxs = bounds.maxs + Vec3::splat(SIDESPACE);
    let vol = BspBrush::from_bounds(planes, mins, maxs);
    let head = Node {
        bounds: Aabb::new(mins, maxs),
        volume: Some(vol),
        cluster: -1,
        output: -1,
        water_id: -1,
        ..Default::default()
    };
    let mut b = Builder {
        planes,
        nodes: vec![head],
        use_blocks,
        hint_priority: false,
    };
    b.build_r(0, brushes);
    let mut nodes = b.nodes;
    let outside = nodes.len();
    nodes.push(Node {
        planenum: None,
        cluster: -1,
        output: -1,
        water_id: -1,
        ..Default::default()
    });
    Tree {
        nodes,
        head: 0,
        outside,
        bounds: Aabb::new(mins, maxs),
    }
}

pub fn prune_nodes(tree: &mut Tree) -> usize {
    fn rec(tree: &mut Tree, n: usize, count: &mut usize) {
        if tree.nodes[n].is_leaf() {
            return;
        }
        let [c0, c1] = tree.nodes[n].children;
        rec(tree, c0, count);
        rec(tree, c1, count);
        let solid0 = tree.nodes[c0].is_leaf() && tree.nodes[c0].contents & CONTENTS_SOLID != 0;
        let solid1 = tree.nodes[c1].is_leaf() && tree.nodes[c1].contents & CONTENTS_SOLID != 0;
        if solid0 && solid1 && tree.nodes[n].faces.is_empty() {
            let mut brushes = std::mem::take(&mut tree.nodes[c0].brushes);
            brushes.append(&mut tree.nodes[c1].brushes);
            let mut extra = std::mem::take(&mut tree.nodes[c0].extra_brushes);
            for b in std::mem::take(&mut tree.nodes[c1].extra_brushes) {
                if !extra.contains(&b) {
                    extra.push(b);
                }
            }
            let merged = (tree.nodes[c0].contents | tree.nodes[c1].contents)
                & !ALL_VISIBLE_CONTENTS
                & !CONTENTS_DETAIL;
            let node = &mut tree.nodes[n];
            node.planenum = None;
            node.contents = CONTENTS_SOLID | merged;
            let node = &mut tree.nodes[n];
            node.brushes = brushes;
            node.extra_brushes = extra;
            node.portals.clear();
            *count += 1;
        }
    }
    let mut count = 0;
    let head = tree.head;
    rec(tree, head, &mut count);
    count
}

fn brushes_disjoint(a: &BspBrush, b: &BspBrush) -> bool {
    for i in 0..3 {
        if a.bounds.mins[i] >= b.bounds.maxs[i] || a.bounds.maxs[i] <= b.bounds.mins[i] {
            return true;
        }
    }
    a.sides
        .iter()
        .any(|sa| b.sides.iter().any(|sb| sa.planenum == (sb.planenum ^ 1)))
}

fn brush_ge(b1: &BspBrush, b2: &BspBrush) -> bool {
    if b1.contents & CONTENTS_DETAIL != 0 && b2.contents & CONTENTS_DETAIL == 0 {
        return false;
    }
    b1.contents & CONTENTS_SOLID != 0
}

enum Subtract {
    Unchanged,
    Swallowed,
    Parts(Vec<BspBrush>),
}

fn subtract_brush(planes: &PlaneSet, a: &BspBrush, b: &BspBrush) -> Subtract {
    let mut out = Vec::new();
    let mut inside = Some(a.clone());
    for s in &b.sides {
        if s.bevel {
            continue;
        }
        let Some(cur) = inside.take() else { break };
        let (f, bk) = split_brush(planes, &cur, s.planenum);
        if let Some(f) = f {
            out.push(f);
        }
        inside = bk;
    }
    match inside {
        None => Subtract::Unchanged,
        Some(_) if out.is_empty() => Subtract::Swallowed,
        Some(_) => Subtract::Parts(out),
    }
}

pub fn chop_brushes(planes: &PlaneSet, mut list: Vec<BspBrush>) -> Vec<BspBrush> {
    let mut guard = 0;
    'restart: loop {
        guard += 1;
        if guard > 100_000 {
            break;
        }
        let n = list.len();
        for i in 0..n {
            for j in i + 1..n {
                if brushes_disjoint(&list[i], &list[j]) {
                    continue;
                }
                let mut sub = None;
                let mut sub2 = None;
                let mut c1 = usize::MAX;
                let mut c2 = usize::MAX;
                if brush_ge(&list[j], &list[i]) {
                    match subtract_brush(planes, &list[i], &list[j]) {
                        Subtract::Unchanged => continue,
                        Subtract::Swallowed => {
                            list.remove(i);
                            continue 'restart;
                        }
                        Subtract::Parts(v) => {
                            c1 = v.len();
                            sub = Some(v);
                        }
                    }
                }
                if brush_ge(&list[i], &list[j]) {
                    match subtract_brush(planes, &list[j], &list[i]) {
                        Subtract::Unchanged => continue,
                        Subtract::Swallowed => {
                            list.remove(j);
                            continue 'restart;
                        }
                        Subtract::Parts(v) => {
                            c2 = v.len();
                            sub2 = Some(v);
                        }
                    }
                }
                if sub.is_none() && sub2.is_none() {
                    continue;
                }
                if c1 < c2 {
                    list.remove(i);
                    list.extend(sub.unwrap());
                } else {
                    list.remove(j);
                    list.extend(sub2.unwrap());
                }
                continue 'restart;
            }
        }
        break;
    }
    list
}
