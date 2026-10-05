use crate::disp::Disp;
use crate::faces::FaceSet;
use crate::map::Map;
use gold_common::bsp::*;
use gold_common::math::*;
use gold_common::zip::ZipArchive;
use std::collections::HashMap;

pub struct Cubemap {
    pub origin: [i32; 3],
    pub size: i32,
    pub sides: Vec<i32>,
}

impl Cubemap {
    pub fn sample(&self) -> DCubemapSample {
        DCubemapSample {
            origin: self.origin,
            size: self.size,
        }
    }
}

fn uses_env_cubemap(map: &Map, texinfo: i32) -> bool {
    if texinfo < 0 {
        return false;
    }
    let td = map.texinfos[texinfo as usize].texdata as usize;
    map.materials[td]
        .param("$envmap")
        .map(|v| v.trim().eq_ignore_ascii_case("env_cubemap"))
        .unwrap_or(false)
}

struct Patcher<'a> {
    map: &'a mut Map,
    pak: &'a mut ZipArchive,
    cubemaps: &'a [Cubemap],
    done: HashMap<(i32, usize), i32>,
}

impl<'a> Patcher<'a> {
    fn nearest(&self, side_id: i32, center: Vec3) -> usize {
        if let Some(i) = self
            .cubemaps
            .iter()
            .position(|c| c.sides.contains(&side_id))
        {
            return i;
        }
        let mut best = 0;
        let mut bd = f64::MAX;
        for (i, c) in self.cubemaps.iter().enumerate() {
            let o = v3(c.origin[0] as f64, c.origin[1] as f64, c.origin[2] as f64);
            let d = (o - center).length_sq();
            if d < bd {
                bd = d;
                best = i;
            }
        }
        best
    }

    fn patch(&mut self, texinfo: i32, cube: usize) -> i32 {
        if let Some(&t) = self.done.get(&(texinfo, cube)) {
            return t;
        }
        let map_name = self.map.map_name.to_ascii_lowercase();
        let td = self.map.texinfos[texinfo as usize].texdata as usize;
        let orig_name = self.map.texdata_names[td].clone();
        let o = self.cubemaps[cube].origin;
        let suffix = format!("{}_{}_{}", o[0], o[1], o[2]);
        let patch_name = format!(
            "maps/{}/{}_{}",
            map_name,
            orig_name.to_ascii_lowercase(),
            suffix
        );
        let envmap = format!("maps/{}/c{}", map_name, suffix);
        let vmt = format!(
            "\"patch\"\r\n{{\r\n\t\"include\"\t\t\"materials/{}.vmt\"\r\n\t\"insert\"\r\n\t{{\r\n\t\t\"$envmap\"\t\t\"{}\"\r\n\t}}\r\n}}\r\n",
            orig_name, envmap
        );
        self.pak
            .add(&format!("materials/{patch_name}.vmt"), vmt.into_bytes());
        let mut mat = self.map.materials[td].clone();
        if let Some(p) = mat.params.iter_mut().find(|(k, _)| k == "$envmap") {
            p.1 = envmap;
        }
        let ntd = self.map.texdata_for(&patch_name, &mat);
        let mut t = self.map.texinfos[texinfo as usize];
        t.texdata = ntd;
        let nt = self.map.add_texinfo(t);
        self.done.insert((texinfo, cube), nt);
        nt
    }
}

pub fn patch_cubemaps(
    map: &mut Map,
    fs: &mut FaceSet,
    face_list: &[usize],
    disps: &mut [Disp],
    cubemaps: &[Cubemap],
    pak: &mut ZipArchive,
) -> usize {
    if cubemaps.is_empty() {
        return 0;
    }
    let side_center = |map: &Map, (b, s): (usize, usize)| -> (i32, Vec3) {
        let side = &map.brushes[b].sides[s];
        let c = side
            .winding
            .as_ref()
            .map(|w| w.center())
            .unwrap_or(map.brushes[b].bounds.center());
        (side.side_id, c)
    };
    let mut p = Patcher {
        map,
        pak,
        cubemaps,
        done: HashMap::new(),
    };
    let mut count = 0;
    for &f in face_list {
        let face = &fs.faces[f];
        if !uses_env_cubemap(p.map, face.texinfo) {
            continue;
        }
        let (id, center) = match face.side {
            Some(s) => side_center(p.map, s),
            None => (0, face.winding.center()),
        };
        let cube = p.nearest(id, center);
        let nt = p.patch(face.texinfo, cube);
        fs.faces[f].texinfo = nt;
        count += 1;
    }
    for d in disps.iter_mut() {
        if !uses_env_cubemap(p.map, d.texinfo) {
            continue;
        }
        let (id, _) = side_center(p.map, (d.brush, d.side));
        let cube = p.nearest(id, d.bounds.center());
        d.texinfo = p.patch(d.texinfo, cube);
        count += 1;
    }
    count
}
