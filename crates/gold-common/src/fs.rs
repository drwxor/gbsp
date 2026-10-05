use crate::kv;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

struct VpkEntry {
    archive_index: u16,
    offset: u32,
    length: u32,
    preload: Vec<u8>,
}

pub struct Vpk {
    dir_path: PathBuf,
    base: String,
    data_offset: u64,
    entries: HashMap<String, VpkEntry>,
    handles: Mutex<HashMap<u16, File>>,
}

fn read_cstr(d: &[u8], pos: &mut usize) -> String {
    let start = *pos;
    while *pos < d.len() && d[*pos] != 0 {
        *pos += 1;
    }
    let s = String::from_utf8_lossy(&d[start..*pos]).into_owned();
    *pos += 1;
    s
}

impl Vpk {
    pub fn open(dir_path: &Path) -> Option<Vpk> {
        let d = std::fs::read(dir_path).ok()?;
        if d.len() < 12 || u32::from_le_bytes(d[0..4].try_into().ok()?) != 0x55aa1234 {
            return None;
        }
        let version = u32::from_le_bytes(d[4..8].try_into().ok()?);
        let tree_size = u32::from_le_bytes(d[8..12].try_into().ok()?) as usize;
        let header_size = match version {
            1 => 12,
            2 => 28,
            _ => return None,
        };
        let mut entries = HashMap::new();
        let mut pos = header_size;
        let end = header_size + tree_size;
        loop {
            let ext = read_cstr(&d, &mut pos);
            if ext.is_empty() || pos >= end {
                break;
            }
            loop {
                let path = read_cstr(&d, &mut pos);
                if path.is_empty() {
                    break;
                }
                loop {
                    let name = read_cstr(&d, &mut pos);
                    if name.is_empty() {
                        break;
                    }
                    let rd16 = |p: usize| u16::from_le_bytes([d[p], d[p + 1]]);
                    let rd32 = |p: usize| u32::from_le_bytes(d[p..p + 4].try_into().unwrap());
                    let preload_bytes = rd16(pos + 4) as usize;
                    let archive_index = rd16(pos + 6);
                    let offset = rd32(pos + 8);
                    let length = rd32(pos + 12);
                    pos += 18;
                    let preload = d[pos..pos + preload_bytes].to_vec();
                    pos += preload_bytes;
                    let mut full = String::new();
                    if path != " " {
                        full.push_str(&path);
                        full.push('/');
                    }
                    full.push_str(&name);
                    if ext != " " {
                        full.push('.');
                        full.push_str(&ext);
                    }
                    entries.insert(
                        full.to_ascii_lowercase(),
                        VpkEntry {
                            archive_index,
                            offset,
                            length,
                            preload,
                        },
                    );
                }
            }
        }
        let fname = dir_path.file_name()?.to_string_lossy().to_string();
        let base = fname.strip_suffix("_dir.vpk").unwrap_or(&fname).to_string();
        Some(Vpk {
            dir_path: dir_path.to_path_buf(),
            base,
            data_offset: end as u64,
            entries,
            handles: Mutex::new(HashMap::new()),
        })
    }

    pub fn contains(&self, path: &str) -> bool {
        self.entries.contains_key(path)
    }

    pub fn read(&self, path: &str) -> Option<Vec<u8>> {
        let e = self.entries.get(path)?;
        let mut out = e.preload.clone();
        if e.length > 0 {
            let mut handles = self.handles.lock().unwrap();
            let f = match handles.entry(e.archive_index) {
                std::collections::hash_map::Entry::Occupied(o) => o.into_mut(),
                std::collections::hash_map::Entry::Vacant(v) => {
                    let p = if e.archive_index == 0x7fff {
                        self.dir_path.clone()
                    } else {
                        self.dir_path
                            .with_file_name(format!("{}_{:03}.vpk", self.base, e.archive_index))
                    };
                    v.insert(File::open(p).ok()?)
                }
            };
            let ofs = if e.archive_index == 0x7fff {
                self.data_offset + e.offset as u64
            } else {
                e.offset as u64
            };
            f.seek(SeekFrom::Start(ofs)).ok()?;
            let mut buf = vec![0u8; e.length as usize];
            f.read_exact(&mut buf).ok()?;
            out.extend_from_slice(&buf);
        }
        Some(out)
    }
}

enum SearchPath {
    Dir(PathBuf),
    Vpk(Vpk),
}

pub struct GameFs {
    pub game_dir: PathBuf,
    paths: Vec<SearchPath>,
}

fn normalize(path: &str) -> String {
    let mut p = path.replace('\\', "/").to_ascii_lowercase();
    while p.contains("//") {
        p = p.replace("//", "/");
    }
    p.trim_start_matches('/').to_string()
}

impl GameFs {
    pub fn empty() -> GameFs {
        GameFs {
            game_dir: PathBuf::new(),
            paths: Vec::new(),
        }
    }

    pub fn default_game_dir() -> Option<PathBuf> {
        if let Ok(v) = std::env::var("VPROJECT") {
            let p = PathBuf::from(v);
            if p.join("gameinfo.txt").exists() {
                return Some(p);
            }
        }
        let roots = [
            "C:/Program Files (x86)/Steam/steamapps/common",
            "C:/Program Files/Steam/steamapps/common",
            "D:/SteamLibrary/steamapps/common",
        ];
        let games = ["Source SDK Base 2013 Singleplayer/hl2", "Half-Life 2/hl2"];
        for r in roots {
            for g in games {
                let p = Path::new(r).join(g);
                if p.join("gameinfo.txt").exists() {
                    return Some(p);
                }
            }
        }
        None
    }

    pub fn open(game_dir: &Path) -> GameFs {
        let game_dir = game_dir
            .canonicalize()
            .unwrap_or_else(|_| game_dir.to_path_buf());
        let base_dir = game_dir
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| game_dir.clone());
        let mut fs = GameFs {
            game_dir: game_dir.clone(),
            paths: Vec::new(),
        };
        let mut seen: Vec<PathBuf> = Vec::new();
        let gi = std::fs::read(game_dir.join("gameinfo.txt"))
            .map(|d| String::from_utf8_lossy(&d).into_owned());
        let mut specs: Vec<String> = Vec::new();
        if let Ok(gi) = gi {
            let kvs = kv::parse(&gi);
            if let Some(sp) = kvs
                .iter()
                .find(|k| k.key.eq_ignore_ascii_case("GameInfo"))
                .and_then(|g| g.block("FileSystem"))
                .and_then(|f| f.block("SearchPaths"))
            {
                for c in sp.children() {
                    let keys: Vec<String> = c
                        .key
                        .to_ascii_lowercase()
                        .split('+')
                        .map(|s| s.to_string())
                        .collect();
                    if keys.iter().any(|k| k == "game")
                        && let Some(v) = c.str()
                    {
                        specs.push(v.to_string());
                    }
                }
            }
        }
        if specs.is_empty() {
            specs.push("|gameinfo_path|.".into());
        }
        for spec in specs {
            let s = spec.replace('\\', "/");
            let resolved = if let Some(rest) = s.strip_prefix("|gameinfo_path|") {
                game_dir.join(rest)
            } else if let Some(rest) = s.strip_prefix("|all_source_engine_paths|") {
                base_dir.join(rest)
            } else if Path::new(&s).is_absolute() {
                PathBuf::from(&s)
            } else {
                base_dir.join(&s)
            };
            let rs = resolved.to_string_lossy().to_string();
            let mut candidates = Vec::new();
            if rs.ends_with("/*") || rs.ends_with("\\*") {
                let dir = PathBuf::from(&rs[..rs.len() - 2]);
                if let Ok(rd) = std::fs::read_dir(&dir) {
                    let mut v: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
                    v.sort();
                    candidates.extend(v);
                }
            } else {
                candidates.push(resolved);
            }
            for c in candidates {
                let cs = c.to_string_lossy().to_ascii_lowercase();
                if cs.ends_with(".vpk") {
                    let dirp = if cs.ends_with("_dir.vpk") {
                        c.clone()
                    } else {
                        PathBuf::from(format!("{}_dir.vpk", &c.to_string_lossy()[..cs.len() - 4]))
                    };
                    let key = dirp.canonicalize().unwrap_or(dirp.clone());
                    if seen.contains(&key) {
                        continue;
                    }
                    if let Some(v) = Vpk::open(&dirp) {
                        seen.push(key);
                        fs.paths.push(SearchPath::Vpk(v));
                    }
                } else if c.is_dir() {
                    let key = c.canonicalize().unwrap_or(c.clone());
                    if seen.contains(&key) {
                        continue;
                    }
                    seen.push(key);
                    let pak = c.join("pak01_dir.vpk");
                    if pak.exists()
                        && let Some(v) = Vpk::open(&pak)
                    {
                        fs.paths.push(SearchPath::Vpk(v));
                    }
                    fs.paths.push(SearchPath::Dir(c));
                }
            }
        }
        fs
    }

    pub fn read(&self, path: &str) -> Option<Vec<u8>> {
        let n = normalize(path);
        for sp in &self.paths {
            match sp {
                SearchPath::Dir(d) => {
                    let p = d.join(&n);
                    if let Ok(data) = std::fs::read(&p) {
                        return Some(data);
                    }
                }
                SearchPath::Vpk(v) => {
                    if v.contains(&n)
                        && let Some(d) = v.read(&n)
                    {
                        return Some(d);
                    }
                }
            }
        }
        None
    }

    pub fn exists(&self, path: &str) -> bool {
        let n = normalize(path);
        self.paths.iter().any(|sp| match sp {
            SearchPath::Dir(d) => d.join(&n).exists(),
            SearchPath::Vpk(v) => v.contains(&n),
        })
    }

    pub fn num_search_paths(&self) -> usize {
        self.paths.len()
    }
}
