use crate::kv::{self, Kv};
use crate::math::*;

#[derive(Clone, Debug, Default)]
pub struct TexAxis {
    pub axis: Vec3,
    pub shift: f64,
    pub scale: f64,
}

impl TexAxis {
    fn parse(s: &str) -> TexAxis {
        let inner_start = s.find('[').map(|i| i + 1).unwrap_or(0);
        let inner_end = s.find(']').unwrap_or(s.len());
        let f: Vec<f64> = s[inner_start..inner_end]
            .split_whitespace()
            .filter_map(|t| t.parse().ok())
            .collect();
        let scale: f64 = s[(inner_end + 1).min(s.len())..]
            .trim()
            .parse()
            .unwrap_or(0.25);
        TexAxis {
            axis: v3(
                *f.first().unwrap_or(&0.0),
                *f.get(1).unwrap_or(&0.0),
                *f.get(2).unwrap_or(&0.0),
            ),
            shift: *f.get(3).unwrap_or(&0.0),
            scale: if scale == 0.0 { 0.25 } else { scale },
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct VmfDisp {
    pub power: i32,
    pub start_position: Vec3,
    pub elevation: f64,
    pub subdiv: bool,
    pub flags: i32,
    pub normals: Vec<Vec3>,
    pub distances: Vec<f64>,
    pub offsets: Vec<Vec3>,
    pub offset_normals: Vec<Vec3>,
    pub alphas: Vec<f64>,
    pub triangle_tags: Vec<i32>,
    pub allowed_verts: Vec<i32>,
}

impl VmfDisp {
    pub fn size(&self) -> usize {
        (1usize << self.power) + 1
    }
}

#[derive(Clone, Debug, Default)]
pub struct VmfSide {
    pub id: i32,
    pub points: [Vec3; 3],
    pub material: String,
    pub uaxis: TexAxis,
    pub vaxis: TexAxis,
    pub rotation: f64,
    pub lightmap_scale: f64,
    pub smoothing_groups: u32,
    pub disp: Option<VmfDisp>,
}

#[derive(Clone, Debug, Default)]
pub struct VmfSolid {
    pub id: i32,
    pub sides: Vec<VmfSide>,
}

#[derive(Clone, Debug, Default)]
pub struct VmfEntity {
    pub id: i32,
    pub pairs: Vec<(String, String)>,
    pub connections: Vec<(String, String)>,
    pub solids: Vec<VmfSolid>,
    pub overlay_transitions: Vec<Vec<(String, String)>>,
}

impl VmfEntity {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.pairs
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }
    pub fn classname(&self) -> &str {
        self.get("classname").unwrap_or("")
    }
}

#[derive(Clone, Debug, Default)]
pub struct Vmf {
    pub map_version: i32,
    pub entities: Vec<VmfEntity>,
}

fn parse_rows_vec(kv: Option<&Kv>, size: usize) -> Vec<Vec3> {
    let mut out = vec![Vec3::ZERO; size * size];
    if let Some(kv) = kv {
        for r in 0..size {
            if let Some(row) = kv.get(&format!("row{r}")) {
                let f: Vec<f64> = row
                    .split_whitespace()
                    .filter_map(|t| t.parse().ok())
                    .collect();
                for c in 0..size {
                    if f.len() >= c * 3 + 3 {
                        out[r * size + c] = v3(f[c * 3], f[c * 3 + 1], f[c * 3 + 2]);
                    }
                }
            }
        }
    }
    out
}

fn parse_rows_f(kv: Option<&Kv>, rows: usize, cols: usize) -> Vec<f64> {
    let mut out = vec![0.0; rows * cols];
    if let Some(kv) = kv {
        for r in 0..rows {
            if let Some(row) = kv.get(&format!("row{r}")) {
                for (c, t) in row.split_whitespace().take(cols).enumerate() {
                    out[r * cols + c] = t.parse().unwrap_or(0.0);
                }
            }
        }
    }
    out
}

fn parse_disp(kv: &Kv) -> VmfDisp {
    let power: i32 = kv
        .get("power")
        .and_then(|s| s.parse().ok())
        .unwrap_or(2)
        .clamp(2, 4);
    let size = (1usize << power) + 1;
    let quads = 1usize << power;
    let allowed_verts = kv
        .block("allowed_verts")
        .and_then(|b| b.get("10"))
        .map(|s| {
            s.split_whitespace()
                .filter_map(|t| t.parse().ok())
                .collect()
        })
        .unwrap_or_else(|| vec![-1; 10]);
    VmfDisp {
        power,
        start_position: kv
            .get("startposition")
            .and_then(parse_vec3)
            .unwrap_or_default(),
        elevation: kv
            .get("elevation")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0),
        subdiv: kv.get("subdiv").map(|s| s.trim() == "1").unwrap_or(false),
        flags: kv.get("flags").and_then(|s| s.parse().ok()).unwrap_or(0),
        normals: parse_rows_vec(kv.block("normals"), size),
        distances: parse_rows_f(kv.block("distances"), size, size),
        offsets: parse_rows_vec(kv.block("offsets"), size),
        offset_normals: parse_rows_vec(kv.block("offset_normals"), size),
        alphas: parse_rows_f(kv.block("alphas"), size, size),
        triangle_tags: parse_rows_f(kv.block("triangle_tags"), quads, quads * 2)
            .into_iter()
            .map(|f| f as i32)
            .collect(),
        allowed_verts,
    }
}

fn parse_side(kv: &Kv) -> Option<VmfSide> {
    let plane = kv.get("plane")?;
    let nums: Vec<f64> = plane
        .split(|c: char| c.is_whitespace() || c == '(' || c == ')')
        .filter(|t| !t.is_empty())
        .filter_map(|t| t.parse().ok())
        .collect();
    if nums.len() < 9 {
        return None;
    }
    Some(VmfSide {
        id: kv.get("id").and_then(|s| s.parse().ok()).unwrap_or(0),
        points: [
            v3(nums[0], nums[1], nums[2]),
            v3(nums[3], nums[4], nums[5]),
            v3(nums[6], nums[7], nums[8]),
        ],
        material: kv
            .get("material")
            .unwrap_or("TOOLS/TOOLSNODRAW")
            .to_string(),
        uaxis: TexAxis::parse(kv.get("uaxis").unwrap_or("[1 0 0 0] 0.25")),
        vaxis: TexAxis::parse(kv.get("vaxis").unwrap_or("[0 -1 0 0] 0.25")),
        rotation: kv
            .get("rotation")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0),
        lightmap_scale: kv
            .get("lightmapscale")
            .and_then(|s| s.parse().ok())
            .unwrap_or(16.0),
        smoothing_groups: kv
            .get("smoothing_groups")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
        disp: kv.block("dispinfo").map(parse_disp),
    })
}

fn parse_solid(kv: &Kv) -> VmfSolid {
    VmfSolid {
        id: kv.get("id").and_then(|s| s.parse().ok()).unwrap_or(0),
        sides: kv.blocks("side").filter_map(parse_side).collect(),
    }
}

fn parse_entity(kv: &Kv) -> VmfEntity {
    let mut e = VmfEntity::default();
    for c in kv.children() {
        match &c.value {
            kv::KvValue::Str(s) => {
                if c.key.eq_ignore_ascii_case("id") {
                    e.id = s.parse().unwrap_or(0);
                }
                e.pairs.push((c.key.clone(), s.clone()));
            }
            kv::KvValue::Block(_) => {
                if c.key.eq_ignore_ascii_case("solid") {
                    e.solids.push(parse_solid(c));
                } else if c.key.eq_ignore_ascii_case("overlaytransition") {
                    for d in c
                        .children()
                        .iter()
                        .filter(|d| d.key.eq_ignore_ascii_case("overlaydata"))
                    {
                        e.overlay_transitions.push(
                            d.children()
                                .iter()
                                .filter_map(|kv| kv.str().map(|v| (kv.key.clone(), v.to_string())))
                                .collect(),
                        );
                    }
                } else if c.key.eq_ignore_ascii_case("connections") {
                    for o in c.children() {
                        if let Some(v) = o.str() {
                            e.connections
                                .push((o.key.clone(), v.replace('\u{1b}', ",")));
                        }
                    }
                }
            }
        }
    }
    e
}

impl Vmf {
    pub fn parse(src: &str) -> Vmf {
        let top = kv::parse(src);
        let mut vmf = Vmf::default();
        let mut world = None;
        for kv in &top {
            match kv.key.to_ascii_lowercase().as_str() {
                "versioninfo" => {
                    vmf.map_version = kv
                        .get("mapversion")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                }
                "world" => world = Some(parse_entity(kv)),
                "entity" => vmf.entities.push(parse_entity(kv)),
                _ => {}
            }
        }
        let mut w = world.unwrap_or_default();
        if w.get("classname").is_none() {
            w.pairs.push(("classname".into(), "worldspawn".into()));
        }
        if vmf.map_version == 0 {
            vmf.map_version = w
                .get("mapversion")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
        }
        vmf.entities.insert(0, w);
        vmf
    }

    pub fn load(path: &std::path::Path) -> std::io::Result<Vmf> {
        let data = std::fs::read(path)?;
        Ok(Vmf::parse(&String::from_utf8_lossy(&data)))
    }
}
