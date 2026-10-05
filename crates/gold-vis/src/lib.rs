#![allow(clippy::needless_range_loop, clippy::too_many_arguments)]

use gold_common::bsp::*;
use gold_common::math::*;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

const ON_EPSILON: f64 = 0.1;

type Bits = Vec<u64>;

#[inline]
fn bit_get(b: &[u64], i: usize) -> bool {
    b[i >> 6] & (1u64 << (i & 63)) != 0
}
#[inline]
fn bit_set(b: &mut [u64], i: usize) {
    b[i >> 6] |= 1u64 << (i & 63);
}

#[derive(Clone, Debug)]
struct VPlane {
    normal: Vec3,
    dist: f64,
}

impl VPlane {
    fn flip(&self) -> VPlane {
        VPlane {
            normal: -self.normal,
            dist: -self.dist,
        }
    }
}

struct Portal {
    plane: VPlane,
    leaf: usize,
    winding: Vec<Vec3>,
    origin: Vec3,
    radius: f64,
    portalfront: Bits,
    portalflood: Bits,
    nummightsee: usize,
}

struct Leaf {
    portals: Vec<usize>,
}

pub struct VisData {
    portals: Vec<Portal>,
    leafs: Vec<Leaf>,
    num_clusters: usize,
    portal_words: usize,
    portalvis: Vec<OnceLock<Bits>>,
}

fn winding_plane(w: &[Vec3]) -> VPlane {
    let v1 = w[2] - w[1];
    let v2 = w[0] - w[1];
    let n = v2.cross(v1).normalize();
    let n = if n.length() < 0.5 {
        let mut nn = Vec3::ZERO;
        for i in 0..w.len() {
            let a = w[i];
            let b = w[(i + 1) % w.len()];
            nn.x += (a.y - b.y) * (a.z + b.z);
            nn.y += (a.z - b.z) * (a.x + b.x);
            nn.z += (a.x - b.x) * (a.y + b.y);
        }
        (-nn).normalize()
    } else {
        n
    };
    VPlane {
        dist: w[0].dot(n),
        normal: n,
    }
}

pub fn load_portals(path: &Path) -> Result<VisData, String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
    let mut toks = text
        .split(|c: char| c.is_whitespace() || c == '(' || c == ')')
        .filter(|t| !t.is_empty());
    let magic = toks.next().ok_or("empty portal file")?;
    if magic != "PRT1" {
        return Err(format!("bad portal file header {magic}"));
    }
    let mut next_num = |what: &str| -> Result<f64, String> {
        toks.next()
            .ok_or(format!("unexpected end of portal file reading {what}"))?
            .parse::<f64>()
            .map_err(|e| format!("bad number: {e}"))
    };
    let num_clusters = next_num("clusters")? as usize;
    let num_portals = next_num("portals")? as usize;
    let mut leafs: Vec<Leaf> = (0..num_clusters)
        .map(|_| Leaf {
            portals: Vec::new(),
        })
        .collect();
    let mut portals = Vec::with_capacity(num_portals * 2);
    for _ in 0..num_portals {
        let npts = next_num("points")? as usize;
        let l0 = next_num("leaf")? as usize;
        let l1 = next_num("leaf")? as usize;
        if l0 >= num_clusters || l1 >= num_clusters {
            return Err("portal file: leaf number out of range".into());
        }
        let mut w = Vec::with_capacity(npts);
        for _ in 0..npts {
            let x = next_num("x")?;
            let y = next_num("y")?;
            let z = next_num("z")?;
            w.push(v3(x, y, z));
        }
        let plane = winding_plane(&w);
        let fwd = portals.len();
        leafs[l0].portals.push(fwd);
        portals.push(make_portal(plane.flip(), l1, w.clone()));
        let back = portals.len();
        leafs[l1].portals.push(back);
        let mut rw = w.clone();
        rw.reverse();
        portals.push(make_portal(plane, l0, rw));
    }
    let portal_words = portals.len().div_ceil(64);
    let portalvis = (0..portals.len()).map(|_| OnceLock::new()).collect();
    Ok(VisData {
        portals,
        leafs,
        num_clusters,
        portal_words,
        portalvis,
    })
}

fn make_portal(plane: VPlane, leaf: usize, w: Vec<Vec3>) -> Portal {
    let mut origin = Vec3::ZERO;
    for p in &w {
        origin += *p;
    }
    origin = origin / w.len() as f64;
    let radius = w.iter().map(|p| p.dist(origin)).fold(0.0, f64::max);
    Portal {
        plane,
        leaf,
        winding: w,
        origin,
        radius,
        portalfront: Vec::new(),
        portalflood: Vec::new(),
        nummightsee: 0,
    }
}

fn base_portal_vis(vd: &VisData, pi: usize) -> (Bits, Bits) {
    let p = &vd.portals[pi];
    let mut front = vec![0u64; vd.portal_words];
    for (j, tp) in vd.portals.iter().enumerate() {
        if j == pi {
            continue;
        }
        let d = tp.origin.dot(p.plane.normal) - p.plane.dist;
        if d < -tp.radius {
            continue;
        }
        if !tp
            .winding
            .iter()
            .any(|pt| pt.dot(p.plane.normal) - p.plane.dist > ON_EPSILON)
        {
            continue;
        }
        if !p
            .winding
            .iter()
            .any(|pt| pt.dot(tp.plane.normal) - tp.plane.dist < -ON_EPSILON)
        {
            continue;
        }
        bit_set(&mut front, j);
    }
    let mut flood = vec![0u64; vd.portal_words];
    let mut stack = vec![p.leaf];
    while let Some(leaf) = stack.pop() {
        for &pn in &vd.leafs[leaf].portals {
            if !bit_get(&front, pn) || bit_get(&flood, pn) {
                continue;
            }
            bit_set(&mut flood, pn);
            stack.push(vd.portals[pn].leaf);
        }
    }
    (front, flood)
}

const MAXW: usize = 32;

#[derive(Clone, Copy)]
struct Wnd {
    n: usize,
    p: [Vec3; MAXW],
}

impl Wnd {
    fn from_slice(s: &[Vec3]) -> Wnd {
        let mut w = Wnd {
            n: s.len().min(MAXW),
            p: [Vec3::ZERO; MAXW],
        };
        w.p[..w.n].copy_from_slice(&s[..w.n]);
        w
    }
    #[inline]
    fn pts(&self) -> &[Vec3] {
        &self.p[..self.n]
    }
}

fn chop_inplace(w: &mut Wnd, plane: &VPlane) -> bool {
    let mut dists = [0.0f64; MAXW + 1];
    let mut sides = [0u8; MAXW + 1];
    let mut counts = [0usize; 3];
    for i in 0..w.n {
        let d = w.p[i].dot(plane.normal) - plane.dist;
        let s = if d > ON_EPSILON {
            0
        } else if d < -ON_EPSILON {
            1
        } else {
            2
        };
        counts[s as usize] += 1;
        dists[i] = d;
        sides[i] = s;
    }
    if counts[1] == 0 {
        return true;
    }
    if counts[0] == 0 {
        return false;
    }
    dists[w.n] = dists[0];
    sides[w.n] = sides[0];
    let mut out = [Vec3::ZERO; MAXW];
    let mut on = 0usize;
    let mut push = |v: Vec3| {
        if on < MAXW {
            out[on] = v;
            on += 1;
        }
    };
    for i in 0..w.n {
        let p1 = w.p[i];
        if sides[i] == 2 {
            push(p1);
            continue;
        }
        if sides[i] == 0 {
            push(p1);
        }
        if sides[i + 1] == 2 || sides[i + 1] == sides[i] {
            continue;
        }
        let p2 = w.p[(i + 1) % w.n];
        let dot = dists[i] / (dists[i] - dists[i + 1]);
        let mut mid = Vec3::ZERO;
        for j in 0..3 {
            mid[j] = if plane.normal[j] == 1.0 {
                plane.dist
            } else if plane.normal[j] == -1.0 {
                -plane.dist
            } else {
                p1[j] + dot * (p2[j] - p1[j])
            };
        }
        push(mid);
    }
    if on < 3 {
        return false;
    }
    w.p[..on].copy_from_slice(&out[..on]);
    w.n = on;
    true
}

fn clip_to_separators(source: &Wnd, pass: &Wnd, target: &mut Wnd, flipclip: bool) -> bool {
    let (src, pas) = (source.pts(), pass.pts());
    let ns = src.len();
    for i in 0..ns {
        let l = (i + 1) % ns;
        let v1 = src[l] - src[i];
        for j in 0..pas.len() {
            let v2 = pas[j] - src[i];
            let mut normal = v1.cross(v2);
            let len2 = normal.length_sq();
            if len2 < ON_EPSILON {
                continue;
            }
            normal = normal / len2.sqrt();
            let mut plane = VPlane {
                dist: pas[j].dot(normal),
                normal,
            };
            let mut fliptest = None;
            for (k, sp) in src.iter().enumerate() {
                if k == i || k == l {
                    continue;
                }
                let d = sp.dot(plane.normal) - plane.dist;
                if d < -ON_EPSILON {
                    fliptest = Some(false);
                    break;
                } else if d > ON_EPSILON {
                    fliptest = Some(true);
                    break;
                }
            }
            let Some(fliptest) = fliptest else { continue };
            if fliptest {
                plane = plane.flip();
            }
            let mut ok = true;
            let mut front = 0;
            for (k, pp) in pas.iter().enumerate() {
                if k == j {
                    continue;
                }
                let d = pp.dot(plane.normal) - plane.dist;
                if d < -ON_EPSILON {
                    ok = false;
                    break;
                } else if d > ON_EPSILON {
                    front += 1;
                }
            }
            if !ok || front == 0 {
                continue;
            }
            if flipclip {
                plane = plane.flip();
            }
            if !chop_inplace(target, &plane) {
                return false;
            }
        }
    }
    true
}

struct Level {
    source: Wnd,
    pass: Option<Wnd>,
    portalplane: VPlane,
}

struct FlowThread<'a> {
    vd: &'a VisData,
    base: usize,
    portalvis: Bits,
    might_pool: Vec<Bits>,
}

impl<'a> FlowThread<'a> {
    fn recursive_leaf_flow(&mut self, leafnum: usize, depth: usize, prev: &Level) {
        let vd = self.vd;
        let words = vd.portal_words;
        if self.might_pool.len() <= depth + 1 {
            self.might_pool.push(vec![0u64; words]);
        }
        let base = &vd.portals[self.base];
        for &pnum in &vd.leafs[leafnum].portals {
            if !bit_get(&self.might_pool[depth], pnum) {
                continue;
            }
            let p = &vd.portals[pnum];
            let test: &Bits = match vd.portalvis[pnum].get() {
                Some(v) => v,
                None => &p.portalflood,
            };
            let mut more = 0u64;
            {
                let prev_might = &self.might_pool[depth];
                for j in 0..words {
                    more |= prev_might[j] & test[j] & !self.portalvis[j];
                }
            }
            if more == 0 && bit_get(&self.portalvis, pnum) {
                continue;
            }
            let d = p.origin.dot(base.plane.normal) - base.plane.dist;
            if d < -p.radius {
                continue;
            }
            let mut pass = Wnd::from_slice(&p.winding);
            if d <= p.radius && !chop_inplace(&mut pass, &base.plane) {
                continue;
            }
            let d = base.origin.dot(p.plane.normal) - p.plane.dist;
            if d > base.radius {
                continue;
            }
            let mut source = prev.source;
            if d >= -base.radius && !chop_inplace(&mut source, &p.plane.flip()) {
                continue;
            }
            if let Some(prev_pass) = &prev.pass {
                if !chop_inplace(&mut pass, &prev.portalplane) {
                    continue;
                }
                if !clip_to_separators(&source, prev_pass, &mut pass, false) {
                    continue;
                }
                if !clip_to_separators(prev_pass, &source, &mut pass, true) {
                    continue;
                }
            }
            bit_set(&mut self.portalvis, pnum);
            {
                let (lo, hi) = self.might_pool.split_at_mut(depth + 1);
                let cur = &lo[depth];
                let next = &mut hi[0];
                for j in 0..words {
                    next[j] = cur[j] & test[j];
                }
            }
            let level = Level {
                source,
                pass: Some(pass),
                portalplane: p.plane.clone(),
            };
            self.recursive_leaf_flow(p.leaf, depth + 1, &level);
        }
    }
}

fn portal_flow(vd: &VisData, pi: usize) -> Bits {
    let p = &vd.portals[pi];
    let mut th = FlowThread {
        vd,
        base: pi,
        portalvis: vec![0u64; vd.portal_words],
        might_pool: vec![p.portalflood.clone()],
    };
    let head = Level {
        source: Wnd::from_slice(&p.winding),
        pass: None,
        portalplane: p.plane.clone(),
    };
    th.recursive_leaf_flow(p.leaf, 0, &head);
    th.portalvis
}

pub struct Options {
    pub bsp: PathBuf,
    pub prt: PathBuf,
    pub fast: bool,
}

fn usage() {
    eprintln!("usage: gvis [options] <mapfile>");
    eprintln!("  -fast            only do the quick base visibility pass");
}

pub fn main_with_args(args: Vec<String>) -> i32 {
    let mut input = None;
    let mut fast = false;
    let mut i = 1;
    while i < args.len() {
        let a = args[i].as_str();
        match a.to_ascii_lowercase().as_str() {
            "-fast" => fast = true,
            "-game" | "-vproject" | "-threads" => i += 1,
            _ if a.starts_with('-') => eprintln!("ignoring unknown option {a}"),
            _ => input = Some(PathBuf::from(a)),
        }
        i += 1;
    }
    let Some(input) = input else {
        usage();
        return 1;
    };
    let bsp = input.with_extension("bsp");
    let prt = input.with_extension("prt");
    match run(&Options { bsp, prt, fast }) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("Error: {e}");
            1
        }
    }
}

pub fn run(opts: &Options) -> Result<(), String> {
    let start = Instant::now();
    let _ = rayon::ThreadPoolBuilder::new()
        .stack_size(256 << 20)
        .build_global();
    println!("gvis: {}", opts.bsp.display());
    let mut bsp =
        BspFile::read(&opts.bsp).map_err(|e| format!("can't read {}: {e}", opts.bsp.display()))?;
    let mut vd = load_portals(&opts.prt)?;
    println!("{:5} portalclusters", vd.num_clusters);
    println!("{:5} numportals", vd.portals.len() / 2);

    let t = Instant::now();
    let base: Vec<(Bits, Bits)> = (0..vd.portals.len())
        .into_par_iter()
        .map(|i| base_portal_vis(&vd, i))
        .collect();
    let mut total_might = 0usize;
    for (i, (front, flood)) in base.into_iter().enumerate() {
        let n = flood.iter().map(|w| w.count_ones() as usize).sum();
        total_might += n;
        vd.portals[i].portalfront = front;
        vd.portals[i].portalflood = flood;
        vd.portals[i].nummightsee = n;
    }
    println!(
        "BasePortalVis: {:.2}s, {} mightsee",
        t.elapsed().as_secs_f64(),
        total_might
    );

    let t = Instant::now();
    if opts.fast {
        for i in 0..vd.portals.len() {
            let _ = vd.portalvis[i].set(vd.portals[i].portalflood.clone());
        }
    } else {
        let mut order: Vec<usize> = (0..vd.portals.len()).collect();
        order.sort_by_key(|&i| vd.portals[i].nummightsee);
        let next = AtomicUsize::new(0);
        let done = AtomicUsize::new(0);
        let n = order.len();
        let vdr = &vd;
        let threads = rayon::current_num_threads().max(1);
        std::thread::scope(|s| {
            for _ in 0..threads {
                let worker = || loop {
                    let k = next.fetch_add(1, Ordering::Relaxed);
                    if k >= n {
                        break;
                    }
                    let pi = order[k];
                    let v = portal_flow(vdr, pi);
                    let _ = vdr.portalvis[pi].set(v);
                    let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                    if n >= 10 && d.is_multiple_of((n / 10).max(1)) {
                        print!("{}...", d * 10 / n);
                        use std::io::Write;
                        let _ = std::io::stdout().flush();
                    }
                };
                std::thread::Builder::new()
                    .stack_size(256 << 20)
                    .spawn_scoped(s, worker)
                    .expect("spawn vis thread");
            }
        });
        println!();
    }
    println!("PortalFlow: {:.2}s", t.elapsed().as_secs_f64());

    let nc = vd.num_clusters;
    let row_bytes = nc.div_ceil(8);
    let mut pvs = vec![vec![0u8; row_bytes]; nc];
    let mut total_visible = 0usize;
    for (c, row) in pvs.iter_mut().enumerate() {
        let mut portalvector = vec![0u64; vd.portal_words];
        for &p in &vd.leafs[c].portals {
            let pv = vd.portalvis[p].get().unwrap();
            for j in 0..vd.portal_words {
                portalvector[j] |= pv[j];
            }
            bit_set(&mut portalvector, p);
        }
        for j in 0..vd.portals.len() {
            if bit_get(&portalvector, j) {
                let l = vd.portals[j].leaf;
                row[l >> 3] |= 1 << (l & 7);
            }
        }
        row[c >> 3] |= 1 << (c & 7);
        total_visible += row.iter().map(|b| b.count_ones() as usize).sum::<usize>();
    }
    println!("Total clusters visible: {}", total_visible);
    println!("Average clusters visible: {}", total_visible / nc.max(1));

    let mut pas = vec![vec![0u8; row_bytes]; nc];
    let mut total_audible = 0usize;
    for c in 0..nc {
        let mut row = pvs[c].clone();
        for o in 0..nc {
            if pvs[c][o >> 3] & (1 << (o & 7)) != 0 {
                for k in 0..row_bytes {
                    row[k] |= pvs[o][k];
                }
            }
        }
        total_audible += row.iter().map(|b| b.count_ones() as usize).sum::<usize>();
        pas[c] = row;
    }
    println!("Average clusters audible: {}", total_audible / nc.max(1));

    let header = 4 + nc * 8;
    let mut data = Vec::new();
    let mut ofs = vec![[0i32; 2]; nc];
    for c in 0..nc {
        ofs[c][0] = (header + data.len()) as i32;
        data.extend(compress_vis(&pvs[c]));
    }
    for c in 0..nc {
        ofs[c][1] = (header + data.len()) as i32;
        data.extend(compress_vis(&pas[c]));
    }
    let mut lump = Vec::with_capacity(header + data.len());
    lump.extend_from_slice(&(nc as i32).to_le_bytes());
    for o in &ofs {
        lump.extend_from_slice(&o[0].to_le_bytes());
        lump.extend_from_slice(&o[1].to_le_bytes());
    }
    lump.extend(data);
    println!("visdatasize: {}", lump.len());
    bsp.lumps[LUMP_VISIBILITY] = lump;

    update_sky_flags(&mut bsp, &pvs, row_bytes);
    compute_water_distances(&mut bsp, &pvs);

    bsp.write(&opts.bsp)
        .map_err(|e| format!("can't write {}: {e}", opts.bsp.display()))?;
    println!(
        "wrote {} ({:.2}s)",
        opts.bsp.display(),
        start.elapsed().as_secs_f64()
    );
    Ok(())
}

fn update_sky_flags(bsp: &mut BspFile, pvs: &[Vec<u8>], row_bytes: usize) {
    let texinfo: Vec<TexInfo> = bsp.get(LUMP_TEXINFO);
    let faces: Vec<DFace> = bsp.get(LUMP_FACES);
    let leaffaces: Vec<u16> = bsp.get(LUMP_LEAFFACES);
    let mut leafs: Vec<DLeaf> = bsp.get(LUMP_LEAFS);
    let nc = pvs.len();
    let mut cluster_sky = vec![0i32; nc];
    for l in &leafs {
        if l.cluster < 0 || l.cluster as usize >= nc {
            continue;
        }
        for k in 0..l.numleaffaces {
            let f = &faces[leaffaces[(l.firstleafface + k) as usize] as usize];
            let fl = texinfo[f.texinfo as usize].flags;
            if fl & SURF_SKY2D != 0 {
                cluster_sky[l.cluster as usize] |= LEAF_FLAGS_SKY2D;
            } else if fl & SURF_SKY != 0 {
                cluster_sky[l.cluster as usize] |= LEAF_FLAGS_SKY;
            }
        }
    }
    for l in leafs.iter_mut() {
        let area = l.area();
        let other = l.flags() & !(LEAF_FLAGS_SKY | LEAF_FLAGS_SKY2D);
        let mut flags = other;
        if l.cluster >= 0 && (l.cluster as usize) < nc {
            let row = &pvs[l.cluster as usize];
            for c in 0..nc {
                if c >> 3 < row_bytes && row[c >> 3] & (1 << (c & 7)) != 0 {
                    flags |= cluster_sky[c];
                }
            }
        } else {
            flags |= l.flags() & (LEAF_FLAGS_SKY | LEAF_FLAGS_SKY2D);
        }
        l.set_area_flags(area, flags);
    }
    bsp.set(LUMP_LEAFS, &leafs);
}

fn compute_water_distances(bsp: &mut BspFile, pvs: &[Vec<u8>]) {
    let leafs: Vec<DLeaf> = bsp.get(LUMP_LEAFS);
    let nc = pvs.len();
    let water: Vec<&DLeaf> = leafs
        .iter()
        .filter(|l| l.leaf_water_data_id >= 0 && l.cluster >= 0)
        .collect();
    let dist_box = |a: &DLeaf, b: &DLeaf| -> f64 {
        let mut d2 = 0.0;
        for k in 0..3 {
            let gap = (b.mins[k] as f64 - a.maxs[k] as f64)
                .max(a.mins[k] as f64 - b.maxs[k] as f64)
                .max(0.0);
            d2 += gap * gap;
        }
        d2.sqrt()
    };
    let out: Vec<u16> = leafs
        .iter()
        .map(|l| {
            if l.leaf_water_data_id >= 0 {
                return 0;
            }
            if l.cluster < 0 || l.cluster as usize >= nc {
                return 65535;
            }
            let row = &pvs[l.cluster as usize];
            let mut best = f64::MAX;
            for w in &water {
                let c = w.cluster as usize;
                if c < nc && row[c >> 3] & (1 << (c & 7)) != 0 {
                    best = best.min(dist_box(l, w));
                }
            }
            if best == f64::MAX {
                65535
            } else {
                best.min(65534.0) as u16
            }
        })
        .collect();
    bsp.set(LUMP_LEAFMINDISTTOWATER, &out);
}
