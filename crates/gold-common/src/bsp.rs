use bytemuck::{Pod, Zeroable};
use std::io;
use std::path::Path;

pub const IDBSPHEADER: &[u8; 4] = b"VBSP";
pub const BSPVERSION: i32 = 20;
pub const HEADER_LUMPS: usize = 64;

pub const LUMP_ENTITIES: usize = 0;
pub const LUMP_PLANES: usize = 1;
pub const LUMP_TEXDATA: usize = 2;
pub const LUMP_VERTEXES: usize = 3;
pub const LUMP_VISIBILITY: usize = 4;
pub const LUMP_NODES: usize = 5;
pub const LUMP_TEXINFO: usize = 6;
pub const LUMP_FACES: usize = 7;
pub const LUMP_LIGHTING: usize = 8;
pub const LUMP_OCCLUSION: usize = 9;
pub const LUMP_LEAFS: usize = 10;
pub const LUMP_FACEIDS: usize = 11;
pub const LUMP_EDGES: usize = 12;
pub const LUMP_SURFEDGES: usize = 13;
pub const LUMP_MODELS: usize = 14;
pub const LUMP_WORLDLIGHTS: usize = 15;
pub const LUMP_LEAFFACES: usize = 16;
pub const LUMP_LEAFBRUSHES: usize = 17;
pub const LUMP_BRUSHES: usize = 18;
pub const LUMP_BRUSHSIDES: usize = 19;
pub const LUMP_AREAS: usize = 20;
pub const LUMP_AREAPORTALS: usize = 21;
pub const LUMP_DISPINFO: usize = 26;
pub const LUMP_ORIGINALFACES: usize = 27;
pub const LUMP_PHYSDISP: usize = 28;
pub const LUMP_PHYSCOLLIDE: usize = 29;
pub const LUMP_VERTNORMALS: usize = 30;
pub const LUMP_VERTNORMALINDICES: usize = 31;
pub const LUMP_DISP_LIGHTMAP_ALPHAS: usize = 32;
pub const LUMP_DISP_VERTS: usize = 33;
pub const LUMP_DISP_LIGHTMAP_SAMPLE_POSITIONS: usize = 34;
pub const LUMP_GAME_LUMP: usize = 35;
pub const LUMP_LEAFWATERDATA: usize = 36;
pub const LUMP_PRIMITIVES: usize = 37;
pub const LUMP_PRIMVERTS: usize = 38;
pub const LUMP_PRIMINDICES: usize = 39;
pub const LUMP_PAKFILE: usize = 40;
pub const LUMP_CLIPPORTALVERTS: usize = 41;
pub const LUMP_CUBEMAPS: usize = 42;
pub const LUMP_TEXDATA_STRING_DATA: usize = 43;
pub const LUMP_TEXDATA_STRING_TABLE: usize = 44;
pub const LUMP_OVERLAYS: usize = 45;
pub const LUMP_LEAFMINDISTTOWATER: usize = 46;
pub const LUMP_FACE_MACRO_TEXTURE_INFO: usize = 47;
pub const LUMP_DISP_TRIS: usize = 48;
pub const LUMP_PHYSCOLLIDESURFACE: usize = 49;
pub const LUMP_WATEROVERLAYS: usize = 50;
pub const LUMP_LEAF_AMBIENT_INDEX_HDR: usize = 51;
pub const LUMP_LEAF_AMBIENT_INDEX: usize = 52;
pub const LUMP_LIGHTING_HDR: usize = 53;
pub const LUMP_WORLDLIGHTS_HDR: usize = 54;
pub const LUMP_LEAF_AMBIENT_LIGHTING_HDR: usize = 55;
pub const LUMP_LEAF_AMBIENT_LIGHTING: usize = 56;
pub const LUMP_XZIPPAKFILE: usize = 57;
pub const LUMP_FACES_HDR: usize = 58;
pub const LUMP_MAP_FLAGS: usize = 59;
pub const LUMP_OVERLAY_FADES: usize = 60;

pub const LUMP_NAMES: [&str; HEADER_LUMPS] = [
    "ENTITIES",
    "PLANES",
    "TEXDATA",
    "VERTEXES",
    "VISIBILITY",
    "NODES",
    "TEXINFO",
    "FACES",
    "LIGHTING",
    "OCCLUSION",
    "LEAFS",
    "FACEIDS",
    "EDGES",
    "SURFEDGES",
    "MODELS",
    "WORLDLIGHTS",
    "LEAFFACES",
    "LEAFBRUSHES",
    "BRUSHES",
    "BRUSHSIDES",
    "AREAS",
    "AREAPORTALS",
    "UNUSED0",
    "UNUSED1",
    "UNUSED2",
    "UNUSED3",
    "DISPINFO",
    "ORIGINALFACES",
    "PHYSDISP",
    "PHYSCOLLIDE",
    "VERTNORMALS",
    "VERTNORMALINDICES",
    "DISP_LIGHTMAP_ALPHAS",
    "DISP_VERTS",
    "DISP_LIGHTMAP_SAMPLE_POSITIONS",
    "GAME_LUMP",
    "LEAFWATERDATA",
    "PRIMITIVES",
    "PRIMVERTS",
    "PRIMINDICES",
    "PAKFILE",
    "CLIPPORTALVERTS",
    "CUBEMAPS",
    "TEXDATA_STRING_DATA",
    "TEXDATA_STRING_TABLE",
    "OVERLAYS",
    "LEAFMINDISTTOWATER",
    "FACE_MACRO_TEXTURE_INFO",
    "DISP_TRIS",
    "PHYSCOLLIDESURFACE",
    "WATEROVERLAYS",
    "LEAF_AMBIENT_INDEX_HDR",
    "LEAF_AMBIENT_INDEX",
    "LIGHTING_HDR",
    "WORLDLIGHTS_HDR",
    "LEAF_AMBIENT_LIGHTING_HDR",
    "LEAF_AMBIENT_LIGHTING",
    "XZIPPAKFILE",
    "FACES_HDR",
    "MAP_FLAGS",
    "OVERLAY_FADES",
    "61",
    "62",
    "63",
];

pub const CONTENTS_EMPTY: i32 = 0;
pub const CONTENTS_SOLID: i32 = 0x1;
pub const CONTENTS_WINDOW: i32 = 0x2;
pub const CONTENTS_AUX: i32 = 0x4;
pub const CONTENTS_GRATE: i32 = 0x8;
pub const CONTENTS_SLIME: i32 = 0x10;
pub const CONTENTS_WATER: i32 = 0x20;
pub const CONTENTS_BLOCKLOS: i32 = 0x40;
pub const CONTENTS_OPAQUE: i32 = 0x80;
pub const CONTENTS_TESTFOGVOLUME: i32 = 0x100;
pub const CONTENTS_MOVEABLE: i32 = 0x4000;
pub const CONTENTS_AREAPORTAL: i32 = 0x8000;
pub const CONTENTS_PLAYERCLIP: i32 = 0x10000;
pub const CONTENTS_MONSTERCLIP: i32 = 0x20000;
pub const CONTENTS_ORIGIN: i32 = 0x1000000;
pub const CONTENTS_MONSTER: i32 = 0x2000000;
pub const CONTENTS_DEBRIS: i32 = 0x4000000;
pub const CONTENTS_DETAIL: i32 = 0x8000000;
pub const CONTENTS_TRANSLUCENT: i32 = 0x10000000;
pub const CONTENTS_LADDER: i32 = 0x20000000;
pub const CONTENTS_HITBOX: i32 = 0x40000000;
pub const LAST_VISIBLE_CONTENTS: i32 = CONTENTS_OPAQUE;
pub const ALL_VISIBLE_CONTENTS: i32 = LAST_VISIBLE_CONTENTS | (LAST_VISIBLE_CONTENTS - 1);

pub const SURF_LIGHT: i32 = 0x1;
pub const SURF_SKY2D: i32 = 0x2;
pub const SURF_SKY: i32 = 0x4;
pub const SURF_WARP: i32 = 0x8;
pub const SURF_TRANS: i32 = 0x10;
pub const SURF_NOPORTAL: i32 = 0x20;
pub const SURF_TRIGGER: i32 = 0x40;
pub const SURF_NODRAW: i32 = 0x80;
pub const SURF_HINT: i32 = 0x100;
pub const SURF_SKIP: i32 = 0x200;
pub const SURF_NOLIGHT: i32 = 0x400;
pub const SURF_BUMPLIGHT: i32 = 0x800;
pub const SURF_NOSHADOWS: i32 = 0x1000;
pub const SURF_NODECALS: i32 = 0x2000;
pub const SURF_NOCHOP: i32 = 0x4000;
pub const SURF_HITBOX: i32 = 0x8000;

pub const PLANE_X: i32 = 0;
pub const PLANE_Y: i32 = 1;
pub const PLANE_Z: i32 = 2;
pub const PLANE_ANYX: i32 = 3;
pub const PLANE_ANYY: i32 = 4;
pub const PLANE_ANYZ: i32 = 5;

pub const MAXLIGHTMAPS: usize = 4;
pub const NUM_BUMP_VECTS: usize = 3;
pub const MAX_BRUSH_LIGHTMAP_DIM_WITHOUT_BORDER: i32 = 32;
pub const MAX_DISP_LIGHTMAP_DIM_WITHOUT_BORDER: i32 = 125;

pub const DISPTRI_TAG_SURFACE: u16 = 0x1;
pub const DISPTRI_TAG_WALKABLE: u16 = 0x2;
pub const DISPTRI_TAG_BUILDABLE: u16 = 0x4;
pub const DISPTRI_FLAG_SURFPROP1: u16 = 0x8;
pub const DISPTRI_FLAG_SURFPROP2: u16 = 0x10;

pub const LVLFLAGS_BAKED_STATIC_PROP_LIGHTING_NONHDR: u32 = 0x1;
pub const LVLFLAGS_BAKED_STATIC_PROP_LIGHTING_HDR: u32 = 0x2;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct DPlane {
    pub normal: [f32; 3],
    pub dist: f32,
    pub type_: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct DTexData {
    pub reflectivity: [f32; 3],
    pub name_string_table_id: i32,
    pub width: i32,
    pub height: i32,
    pub view_width: i32,
    pub view_height: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct DNode {
    pub planenum: i32,
    pub children: [i32; 2],
    pub mins: [i16; 3],
    pub maxs: [i16; 3],
    pub firstface: u16,
    pub numfaces: u16,
    pub area: i16,
    pub padding: i16,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable, PartialEq)]
pub struct TexInfo {
    pub texture_vecs: [[f32; 4]; 2],
    pub lightmap_vecs: [[f32; 4]; 2],
    pub flags: i32,
    pub texdata: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct DFace {
    pub planenum: u16,
    pub side: u8,
    pub on_node: u8,
    pub firstedge: i32,
    pub numedges: i16,
    pub texinfo: i16,
    pub dispinfo: i16,
    pub surface_fog_volume_id: i16,
    pub styles: [u8; MAXLIGHTMAPS],
    pub lightofs: i32,
    pub area: f32,
    pub lightmap_texture_mins_in_luxels: [i32; 2],
    pub lightmap_texture_size_in_luxels: [i32; 2],
    pub orig_face: i32,
    pub num_prims: u16,
    pub first_prim_id: u16,
    pub smoothing_groups: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct DLeaf {
    pub contents: i32,
    pub cluster: i16,
    pub area_flags: i16,
    pub mins: [i16; 3],
    pub maxs: [i16; 3],
    pub firstleafface: u16,
    pub numleaffaces: u16,
    pub firstleafbrush: u16,
    pub numleafbrushes: u16,
    pub leaf_water_data_id: i16,
    pub padding: i16,
}

impl DLeaf {
    pub fn area(&self) -> i32 {
        (self.area_flags as u16 & 0x1ff) as i32
    }
    pub fn flags(&self) -> i32 {
        ((self.area_flags as u16) >> 9) as i32
    }
    pub fn set_area_flags(&mut self, area: i32, flags: i32) {
        self.area_flags = ((area as u16 & 0x1ff) | ((flags as u16 & 0x7f) << 9)) as i16;
    }
}

pub const LEAF_FLAGS_SKY: i32 = 0x01;
pub const LEAF_FLAGS_RADIAL: i32 = 0x02;
pub const LEAF_FLAGS_SKY2D: i32 = 0x04;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct DEdge {
    pub v: [u16; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct DModel {
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
    pub origin: [f32; 3],
    pub headnode: i32,
    pub firstface: i32,
    pub numfaces: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct DBrush {
    pub firstside: i32,
    pub numsides: i32,
    pub contents: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct DBrushSide {
    pub planenum: u16,
    pub texinfo: i16,
    pub dispinfo: i16,
    pub bevel: i16,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct DArea {
    pub numareaportals: i32,
    pub firstareaportal: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct DAreaPortal {
    pub portal_key: u16,
    pub otherarea: u16,
    pub first_clip_portal_vert: u16,
    pub num_clip_portal_verts: u16,
    pub planenum: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct DispSubNeighbor {
    pub neighbor: u16,
    pub neighbor_orientation: u8,
    pub span: u8,
    pub neighbor_span: u8,
    pub pad: u8,
}

impl DispSubNeighbor {
    pub const NONE: DispSubNeighbor = DispSubNeighbor {
        neighbor: 0xffff,
        neighbor_orientation: 0,
        span: 0,
        neighbor_span: 0,
        pad: 0,
    };
    pub fn is_valid(&self) -> bool {
        self.neighbor != 0xffff
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct DispNeighbor {
    pub sub: [DispSubNeighbor; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct DispCornerNeighbors {
    pub neighbors: [u16; 4],
    pub num_neighbors: u8,
    pub pad: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct DDispInfo {
    pub start_position: [f32; 3],
    pub disp_vert_start: i32,
    pub disp_tri_start: i32,
    pub power: i32,
    pub min_tess: i32,
    pub smoothing_angle: f32,
    pub contents: i32,
    pub map_face: u16,
    pub pad0: u16,
    pub lightmap_alpha_start: i32,
    pub lightmap_sample_position_start: i32,
    pub edge_neighbors: [DispNeighbor; 4],
    pub corner_neighbors: [DispCornerNeighbors; 4],
    pub allowed_verts: [u32; 10],
}

impl DDispInfo {
    pub fn num_verts(&self) -> usize {
        let s = (1usize << self.power) + 1;
        s * s
    }
    pub fn num_tris(&self) -> usize {
        let s = 1usize << self.power;
        2 * s * s
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct DDispVert {
    pub vector: [f32; 3],
    pub dist: f32,
    pub alpha: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct DWorldLight {
    pub origin: [f32; 3],
    pub intensity: [f32; 3],
    pub normal: [f32; 3],
    pub cluster: i32,
    pub type_: i32,
    pub style: i32,
    pub stopdot: f32,
    pub stopdot2: f32,
    pub exponent: f32,
    pub radius: f32,
    pub constant_attn: f32,
    pub linear_attn: f32,
    pub quadratic_attn: f32,
    pub flags: i32,
    pub texinfo: i32,
    pub owner: i32,
}

pub const EMIT_SURFACE: i32 = 0;
pub const EMIT_POINT: i32 = 1;
pub const EMIT_SPOTLIGHT: i32 = 2;
pub const EMIT_SKYLIGHT: i32 = 3;
pub const EMIT_QUAKELIGHT: i32 = 4;
pub const EMIT_SKYAMBIENT: i32 = 5;
pub const DWL_FLAGS_INAMBIENTCUBE: i32 = 0x1;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable, PartialEq, Eq)]
pub struct ColorRGBExp32 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub exponent: i8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct DLeafAmbientIndex {
    pub ambient_sample_count: u16,
    pub first_ambient_sample: u16,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct DLeafAmbientLighting {
    pub cube: [ColorRGBExp32; 6],
    pub x: u8,
    pub y: u8,
    pub z: u8,
    pub pad: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct DCubemapSample {
    pub origin: [i32; 3],
    pub size: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct DLeafWaterData {
    pub surface_z: f32,
    pub min_z: f32,
    pub surface_texinfo_id: i16,
    pub pad: i16,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct DPrimitive {
    pub type_: u8,
    pub pad: u8,
    pub first_index: u16,
    pub index_count: u16,
    pub first_vert: u16,
    pub vert_count: u16,
}

pub fn vector_to_color_rgbexp32(v: [f32; 3]) -> ColorRGBExp32 {
    let max = v[0].max(v[1]).max(v[2]);
    if max <= 0.0 {
        return ColorRGBExp32 {
            r: 0,
            g: 0,
            b: 0,
            exponent: 0,
        };
    }
    let mut exp: i32 = 0;
    let mut m = max;
    while m > 255.0 {
        m *= 0.5;
        exp += 1;
    }
    while m < 128.0 && exp > -128 {
        m *= 2.0;
        exp -= 1;
    }
    exp = exp.clamp(-128, 127);
    let scale = 2f32.powi(-exp);
    let q = |c: f32| -> u8 { (c * scale + 0.5).clamp(0.0, 255.0) as u8 };
    ColorRGBExp32 {
        r: q(v[0]),
        g: q(v[1]),
        b: q(v[2]),
        exponent: exp as i8,
    }
}

pub fn color_rgbexp32_to_vector(c: ColorRGBExp32) -> [f32; 3] {
    let s = 2f32.powi(c.exponent as i32);
    [c.r as f32 * s, c.g as f32 * s, c.b as f32 * s]
}

#[derive(Clone, Debug)]
pub struct GameLump {
    pub id: [u8; 4],
    pub flags: u16,
    pub version: u16,
    pub data: Vec<u8>,
}

pub const GAMELUMP_STATIC_PROPS: [u8; 4] = *b"prps";
pub const GAMELUMP_DETAIL_PROPS: [u8; 4] = *b"prpd";
pub const GAMELUMP_DETAIL_PROP_LIGHTING: [u8; 4] = *b"tlpd";
pub const GAMELUMP_DETAIL_PROP_LIGHTING_HDR: [u8; 4] = *b"hlpd";

#[derive(Clone)]
pub struct BspFile {
    pub lumps: Vec<Vec<u8>>,
    pub lump_versions: [i32; HEADER_LUMPS],
    pub map_revision: i32,
    pub game_lumps: Vec<GameLump>,
}

impl Default for BspFile {
    fn default() -> Self {
        Self::new()
    }
}

fn default_lump_version(idx: usize) -> i32 {
    match idx {
        LUMP_LEAFS
        | LUMP_FACES
        | LUMP_FACES_HDR
        | LUMP_LIGHTING
        | LUMP_LIGHTING_HDR
        | LUMP_LEAF_AMBIENT_LIGHTING
        | LUMP_LEAF_AMBIENT_LIGHTING_HDR => 1,
        LUMP_OCCLUSION => 2,
        _ => 0,
    }
}

impl BspFile {
    pub fn new() -> Self {
        let mut lump_versions = [0; HEADER_LUMPS];
        for (i, v) in lump_versions.iter_mut().enumerate() {
            *v = default_lump_version(i);
        }
        BspFile {
            lumps: vec![Vec::new(); HEADER_LUMPS],
            lump_versions,
            map_revision: 0,
            game_lumps: Vec::new(),
        }
    }

    pub fn read(path: &Path) -> io::Result<Self> {
        let data = std::fs::read(path)?;
        Self::parse(&data)
    }

    pub fn parse(data: &[u8]) -> io::Result<Self> {
        let err = |m: &str| io::Error::new(io::ErrorKind::InvalidData, m.to_string());
        if data.len() < 8 + HEADER_LUMPS * 16 + 4 || &data[0..4] != IDBSPHEADER {
            return Err(err("not a VBSP file"));
        }
        let rd_i32 = |o: usize| i32::from_le_bytes(data[o..o + 4].try_into().unwrap());
        let version = rd_i32(4);
        if version != BSPVERSION && version != 19 && version != 21 {
            return Err(err(&format!("unsupported BSP version {version}")));
        }
        let mut bsp = BspFile::new();
        for i in 0..HEADER_LUMPS {
            let base = 8 + i * 16;
            let ofs = rd_i32(base) as usize;
            let len = rd_i32(base + 4) as usize;
            bsp.lump_versions[i] = rd_i32(base + 8);
            let fourcc = rd_i32(base + 12);
            if fourcc != 0 {
                return Err(err("compressed lumps are not supported"));
            }
            if len > 0 {
                if ofs + len > data.len() {
                    return Err(err(&format!("lump {i} out of range")));
                }
                bsp.lumps[i] = data[ofs..ofs + len].to_vec();
            }
        }
        bsp.map_revision = rd_i32(8 + HEADER_LUMPS * 16);

        let gl = &bsp.lumps[LUMP_GAME_LUMP];
        if gl.len() >= 4 {
            let count = i32::from_le_bytes(gl[0..4].try_into().unwrap()) as usize;
            let mut games = Vec::new();
            for i in 0..count {
                let b = 4 + i * 16;
                if b + 16 > gl.len() {
                    break;
                }
                let mut id = [0u8; 4];
                id.copy_from_slice(&gl[b..b + 4]);
                let flags = u16::from_le_bytes([gl[b + 4], gl[b + 5]]);
                let version = u16::from_le_bytes([gl[b + 6], gl[b + 7]]);
                let ofs = i32::from_le_bytes(gl[b + 8..b + 12].try_into().unwrap()) as usize;
                let len = i32::from_le_bytes(gl[b + 12..b + 16].try_into().unwrap()) as usize;
                let d = if len > 0 && ofs + len <= data.len() {
                    data[ofs..ofs + len].to_vec()
                } else {
                    Vec::new()
                };
                games.push(GameLump {
                    id,
                    flags,
                    version,
                    data: d,
                });
            }
            bsp.game_lumps = games;
        }
        Ok(bsp)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let header_size = 8 + HEADER_LUMPS * 16 + 4;
        let mut out = vec![0u8; header_size];
        out[0..4].copy_from_slice(IDBSPHEADER);
        out[4..8].copy_from_slice(&BSPVERSION.to_le_bytes());
        let mut dir = vec![(0i32, 0i32); HEADER_LUMPS];
        for i in 0..HEADER_LUMPS {
            while !out.len().is_multiple_of(4) {
                out.push(0);
            }
            let ofs = out.len();
            if i == LUMP_GAME_LUMP {
                let n = self.game_lumps.len();
                let dir_size = 4 + n * 16;
                let mut data_ofs = ofs + dir_size;
                let mut hdr = Vec::with_capacity(dir_size);
                hdr.extend_from_slice(&(n as i32).to_le_bytes());
                for g in &self.game_lumps {
                    hdr.extend_from_slice(&g.id);
                    hdr.extend_from_slice(&g.flags.to_le_bytes());
                    hdr.extend_from_slice(&g.version.to_le_bytes());
                    hdr.extend_from_slice(&(data_ofs as i32).to_le_bytes());
                    hdr.extend_from_slice(&(g.data.len() as i32).to_le_bytes());
                    data_ofs += g.data.len();
                }
                out.extend_from_slice(&hdr);
                for g in &self.game_lumps {
                    out.extend_from_slice(&g.data);
                }
            } else {
                out.extend_from_slice(&self.lumps[i]);
            }
            let len = out.len() - ofs;
            dir[i] = (if len > 0 { ofs as i32 } else { 0 }, len as i32);
        }
        for i in 0..HEADER_LUMPS {
            let base = 8 + i * 16;
            out[base..base + 4].copy_from_slice(&dir[i].0.to_le_bytes());
            out[base + 4..base + 8].copy_from_slice(&dir[i].1.to_le_bytes());
            out[base + 8..base + 12].copy_from_slice(&self.lump_versions[i].to_le_bytes());
        }
        out[8 + HEADER_LUMPS * 16..header_size].copy_from_slice(&self.map_revision.to_le_bytes());
        out
    }

    pub fn write(&self, path: &Path) -> io::Result<()> {
        std::fs::write(path, self.to_bytes())
    }

    pub fn get<T: Pod>(&self, idx: usize) -> Vec<T> {
        let raw = &self.lumps[idx];
        let n = raw.len() / std::mem::size_of::<T>();
        bytemuck::pod_collect_to_vec(&raw[..n * std::mem::size_of::<T>()])
    }

    pub fn set<T: Pod>(&mut self, idx: usize, v: &[T]) {
        self.lumps[idx] = bytemuck::cast_slice(v).to_vec();
    }

    pub fn game_lump(&self, id: [u8; 4]) -> Option<&GameLump> {
        self.game_lumps.iter().find(|g| g.id == id)
    }

    pub fn set_game_lump(&mut self, id: [u8; 4], version: u16, data: Vec<u8>) {
        if let Some(g) = self.game_lumps.iter_mut().find(|g| g.id == id) {
            g.version = version;
            g.data = data;
        } else {
            self.game_lumps.push(GameLump {
                id,
                flags: 0,
                version,
                data,
            });
        }
    }

    pub fn entity_string(&self) -> String {
        let raw = &self.lumps[LUMP_ENTITIES];
        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        String::from_utf8_lossy(&raw[..end]).into_owned()
    }

    pub fn set_entity_string(&mut self, s: &str) {
        let mut v = s.as_bytes().to_vec();
        v.push(0);
        self.lumps[LUMP_ENTITIES] = v;
    }

    pub fn texdata_names(&self) -> Vec<String> {
        let table: Vec<i32> = self.get(LUMP_TEXDATA_STRING_TABLE);
        let data = &self.lumps[LUMP_TEXDATA_STRING_DATA];
        table
            .iter()
            .map(|&o| {
                let o = o as usize;
                let end = data[o..]
                    .iter()
                    .position(|&b| b == 0)
                    .map(|p| o + p)
                    .unwrap_or(data.len());
                String::from_utf8_lossy(&data[o..end]).into_owned()
            })
            .collect()
    }

    pub fn set_texdata_names(&mut self, names: &[String]) {
        let mut data = Vec::new();
        let mut table = Vec::new();
        for n in names {
            table.push(data.len() as i32);
            data.extend_from_slice(n.as_bytes());
            data.push(0);
        }
        self.lumps[LUMP_TEXDATA_STRING_DATA] = data;
        self.set(LUMP_TEXDATA_STRING_TABLE, &table);
    }
}

#[derive(Clone, Debug, Default)]
pub struct Entity {
    pub pairs: Vec<(String, String)>,
}

impl Entity {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.pairs
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }
    pub fn get_or<'a>(&'a self, key: &str, def: &'a str) -> &'a str {
        self.get(key).unwrap_or(def)
    }
    pub fn classname(&self) -> &str {
        self.get("classname").unwrap_or("")
    }
    pub fn float(&self, key: &str) -> f32 {
        self.get(key)
            .and_then(|v| v.trim().parse::<f32>().ok())
            .unwrap_or(0.0)
    }
    pub fn int(&self, key: &str) -> i32 {
        self.get(key)
            .map(|v| {
                let v = v.trim();
                v.parse::<i32>()
                    .ok()
                    .or_else(|| v.parse::<f32>().ok().map(|f| f as i32))
                    .unwrap_or(0)
            })
            .unwrap_or(0)
    }
    pub fn vector(&self, key: &str) -> Option<[f32; 3]> {
        let v = self.get(key)?;
        let f: Vec<f32> = v
            .split_whitespace()
            .filter_map(|t| t.parse().ok())
            .collect();
        if f.len() >= 3 {
            Some([f[0], f[1], f[2]])
        } else {
            None
        }
    }
    pub fn set(&mut self, key: &str, value: &str) {
        if let Some(p) = self
            .pairs
            .iter_mut()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
        {
            p.1 = value.to_string();
        } else {
            self.pairs.push((key.to_string(), value.to_string()));
        }
    }
    pub fn remove(&mut self, key: &str) {
        self.pairs.retain(|(k, _)| !k.eq_ignore_ascii_case(key));
    }
}

pub fn parse_entities(s: &str) -> Vec<Entity> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    let mut cur: Option<Entity> = None;
    let mut pending_key: Option<String> = None;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
        } else if c == b'{' {
            cur = Some(Entity::default());
            pending_key = None;
            i += 1;
        } else if c == b'}' {
            if let Some(e) = cur.take() {
                out.push(e);
            }
            i += 1;
        } else if c == b'"' {
            let start = i + 1;
            let mut j = start;
            while j < b.len() && b[j] != b'"' {
                j += 1;
            }
            let tok = String::from_utf8_lossy(&b[start..j.min(b.len())]).into_owned();
            i = j + 1;
            if let Some(e) = cur.as_mut() {
                if let Some(k) = pending_key.take() {
                    e.pairs.push((k, tok));
                } else {
                    pending_key = Some(tok);
                }
            }
        } else {
            i += 1;
        }
    }
    out
}

pub fn write_entities(ents: &[Entity]) -> String {
    let mut s = String::new();
    for e in ents {
        s.push_str("{\n");
        for (k, v) in &e.pairs {
            s.push_str(&format!("\"{k}\" \"{v}\"\n"));
        }
        s.push_str("}\n");
    }
    s
}

pub fn compress_vis(vis: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(vis.len());
    let mut j = 0;
    while j < vis.len() {
        out.push(vis[j]);
        if vis[j] != 0 {
            j += 1;
            continue;
        }
        let mut rep = 1u8;
        j += 1;
        while j < vis.len() && vis[j] == 0 && rep < 255 {
            rep += 1;
            j += 1;
        }
        out.push(rep);
    }
    out
}

pub fn decompress_vis(data: &[u8], ofs: usize, row_bytes: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(row_bytes);
    let mut i = ofs;
    while out.len() < row_bytes && i < data.len() {
        if data[i] != 0 {
            out.push(data[i]);
            i += 1;
        } else {
            let rep = *data.get(i + 1).unwrap_or(&1) as usize;
            out.extend(std::iter::repeat_n(0u8, rep));
            i += 2;
        }
    }
    out.resize(row_bytes, 0);
    out
}
