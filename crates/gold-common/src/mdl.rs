use crate::fs::GameFs;
use crate::math::*;
use crate::phys;

#[derive(Clone, Debug)]
pub struct ModelInfo {
    pub name: String,
    pub hull_min: Vec3,
    pub hull_max: Vec3,
    pub view_min: Vec3,
    pub view_max: Vec3,
    pub collision: Vec<Vec<[Vec3; 3]>>,
    pub num_skins: i32,
    pub flags: i32,
}

impl ModelInfo {
    pub fn bounds(&self) -> Aabb {
        let mut b = Aabb::EMPTY;
        for h in &self.collision {
            for t in h {
                for p in t {
                    b.add_point(*p);
                }
            }
        }
        if b.is_empty() {
            b = Aabb::new(self.hull_min, self.hull_max);
            if b.is_empty() || b.size().length() < 1e-3 {
                b = Aabb::new(self.view_min, self.view_max);
            }
        }
        b
    }
}

pub fn load_model(fs: &GameFs, name: &str) -> Option<ModelInfo> {
    let path = name.replace('\\', "/");
    let d = fs.read(&path)?;
    if d.len() < 160 || &d[0..4] != b"IDST" {
        return None;
    }
    let rv = |o: usize| {
        let f =
            |k: usize| f32::from_le_bytes(d[o + k * 4..o + k * 4 + 4].try_into().unwrap()) as f64;
        v3(f(0), f(1), f(2))
    };
    let phy_path = if path.to_ascii_lowercase().ends_with(".mdl") {
        format!("{}.phy", &path[..path.len() - 4])
    } else {
        format!("{path}.phy")
    };
    let collision = fs
        .read(&phy_path)
        .map(|p| phys::read_phy_file(&p))
        .unwrap_or_default();
    let num_skins = if d.len() > 228 {
        i32::from_le_bytes(d[224..228].try_into().unwrap())
    } else {
        1
    };
    Some(ModelInfo {
        name: name.to_string(),
        hull_min: rv(104),
        hull_max: rv(116),
        view_min: rv(128),
        view_max: rv(140),
        collision,
        num_skins,
        flags: i32::from_le_bytes(d[152..156].try_into().unwrap()),
    })
}

#[derive(Clone, Debug)]
pub struct HwMesh {
    pub lod: u32,
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
}

pub struct ModelMeshes {
    pub checksum: i32,
    pub meshes: Vec<HwMesh>,
}

fn rd_i32(d: &[u8], o: usize) -> Option<i32> {
    d.get(o..o + 4)
        .map(|b| i32::from_le_bytes(b.try_into().unwrap()))
}
fn rd_u16(d: &[u8], o: usize) -> Option<u16> {
    d.get(o..o + 2)
        .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
}
fn rd_vec(d: &[u8], o: usize) -> Option<Vec3> {
    let f = |k: usize| {
        d.get(o + k * 4..o + k * 4 + 4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()) as f64)
    };
    Some(v3(f(0)?, f(1)?, f(2)?))
}

fn load_vvd(d: &[u8]) -> Option<(Vec<Vec3>, Vec<Vec3>)> {
    if d.get(0..4)? != b"IDSV" {
        return None;
    }
    let num_lod0 = rd_i32(d, 16)? as usize;
    let num_fixups = rd_i32(d, 48)? as usize;
    let fixup_start = rd_i32(d, 52)? as usize;
    let vertex_start = rd_i32(d, 56)? as usize;
    let vert = |i: usize| -> Option<(Vec3, Vec3)> {
        let o = vertex_start + i * 48;
        Some((rd_vec(d, o + 16)?, rd_vec(d, o + 28)?))
    };
    let mut pos = Vec::with_capacity(num_lod0);
    let mut nrm = Vec::with_capacity(num_lod0);
    if num_fixups == 0 {
        for i in 0..num_lod0 {
            let (p, n) = vert(i)?;
            pos.push(p);
            nrm.push(n);
        }
    } else {
        for f in 0..num_fixups {
            let o = fixup_start + f * 12;
            let src = rd_i32(d, o + 4)? as usize;
            let n = rd_i32(d, o + 8)? as usize;
            for i in src..src + n {
                let (p, nn) = vert(i)?;
                pos.push(p);
                nrm.push(nn);
            }
        }
    }
    Some((pos, nrm))
}

pub fn load_model_meshes(fs: &GameFs, name: &str) -> Option<ModelMeshes> {
    let path = name.replace('\\', "/");
    let base = path.strip_suffix(".mdl").unwrap_or(&path).to_string();
    let mdl = fs.read(&path)?;
    let vvd = fs.read(&format!("{base}.vvd"))?;
    let vtx = fs
        .read(&format!("{base}.dx90.vtx"))
        .or_else(|| fs.read(&format!("{base}.vtx")))?;
    let (vpos, vnrm) = load_vvd(&vvd)?;
    let checksum = rd_i32(&mdl, 8)?;
    let num_bodyparts = rd_i32(&mdl, 232)? as usize;
    let bodypart_index = rd_i32(&mdl, 236)? as usize;
    let vtx_bodyparts = rd_i32(&vtx, 28)? as usize;
    let vtx_bodypart_ofs = rd_i32(&vtx, 32)? as usize;
    if vtx_bodyparts != num_bodyparts {
        return None;
    }
    let mut meshes = Vec::new();
    for bp in 0..num_bodyparts {
        let mbp = bodypart_index + bp * 16;
        let nummodels = rd_i32(&mdl, mbp + 4)? as usize;
        let modelindex = rd_i32(&mdl, mbp + 12)? as usize;
        let vbp = vtx_bodypart_ofs + bp * 8;
        let vtx_models = rd_i32(&vtx, vbp)? as usize;
        let vtx_model_ofs = vbp + rd_i32(&vtx, vbp + 4)? as usize;
        for m in 0..nummodels.min(vtx_models) {
            let mm = mbp + modelindex + m * 148;
            let nummeshes = rd_i32(&mdl, mm + 72)? as usize;
            let meshindex = rd_i32(&mdl, mm + 76)? as usize;
            let model_vert_start = rd_i32(&mdl, mm + 84)? as usize / 48;
            let vm = vtx_model_ofs + m * 8;
            let num_lods = rd_i32(&vtx, vm)? as usize;
            let lod_ofs = vm + rd_i32(&vtx, vm + 4)? as usize;
            for lod in 0..num_lods {
                let vl = lod_ofs + lod * 12;
                let vtx_meshes = rd_i32(&vtx, vl)? as usize;
                let mesh_ofs = vl + rd_i32(&vtx, vl + 4)? as usize;
                for me in 0..nummeshes.min(vtx_meshes) {
                    let mmesh = mm + meshindex + me * 116;
                    let mesh_vert_start = rd_i32(&mdl, mmesh + 12)? as usize;
                    let vme = mesh_ofs + me * 9;
                    let num_sg = rd_i32(&vtx, vme)? as usize;
                    let sg_ofs = vme + rd_i32(&vtx, vme + 4)? as usize;
                    let mut hm = HwMesh {
                        lod: lod as u32,
                        positions: Vec::new(),
                        normals: Vec::new(),
                    };
                    for sg in 0..num_sg {
                        let vs = sg_ofs + sg * 25;
                        let nverts = rd_i32(&vtx, vs)? as usize;
                        let vert_ofs = vs + rd_i32(&vtx, vs + 4)? as usize;
                        for v in 0..nverts {
                            let orig = rd_u16(&vtx, vert_ofs + v * 9 + 4)? as usize;
                            let idx = model_vert_start + mesh_vert_start + orig;
                            hm.positions.push(*vpos.get(idx)?);
                            hm.normals.push(*vnrm.get(idx)?);
                        }
                    }
                    meshes.push(hm);
                }
            }
        }
    }
    Some(ModelMeshes { checksum, meshes })
}
