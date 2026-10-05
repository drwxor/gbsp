fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, e) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB88320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            *e = c;
        }
        t
    });
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c = table[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

#[derive(Default, Clone)]
pub struct ZipArchive {
    pub files: Vec<(String, Vec<u8>)>,
}

impl ZipArchive {
    pub fn add(&mut self, name: &str, data: Vec<u8>) {
        let name = name.replace('\\', "/").to_ascii_lowercase();
        if let Some(f) = self.files.iter_mut().find(|(n, _)| *n == name) {
            f.1 = data;
        } else {
            self.files.push((name, data));
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, data) in &self.files {
            let ofs = out.len() as u32;
            let crc = crc32(data);
            let nb = name.as_bytes();
            out.extend_from_slice(&0x04034b50u32.to_le_bytes());
            out.extend_from_slice(&10u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(nb.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(nb);
            out.extend_from_slice(data);
            central.extend_from_slice(&0x02014b50u32.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&10u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&crc.to_le_bytes());
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(nb.len() as u16).to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u32.to_le_bytes());
            central.extend_from_slice(&ofs.to_le_bytes());
            central.extend_from_slice(nb);
        }
        let cd_ofs = out.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(&0x06054b50u32.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(self.files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(self.files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_ofs.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    pub fn parse(d: &[u8]) -> ZipArchive {
        let mut z = ZipArchive::default();
        let rd16 = |o: usize| u16::from_le_bytes([d[o], d[o + 1]]) as usize;
        let rd32 = |o: usize| u32::from_le_bytes(d[o..o + 4].try_into().unwrap()) as usize;
        if d.len() < 22 {
            return z;
        }
        let mut eocd = None;
        let mut i = d.len() - 22;
        loop {
            if rd32(i) == 0x06054b50 {
                eocd = Some(i);
                break;
            }
            if i == 0 {
                break;
            }
            i -= 1;
        }
        let Some(e) = eocd else { return z };
        let count = rd16(e + 10);
        let mut p = rd32(e + 16);
        for _ in 0..count {
            if p + 46 > d.len() || rd32(p) != 0x02014b50 {
                break;
            }
            let method = rd16(p + 10);
            let csize = rd32(p + 20);
            let nlen = rd16(p + 28);
            let elen = rd16(p + 30);
            let clen = rd16(p + 32);
            let lofs = rd32(p + 42);
            let name = String::from_utf8_lossy(&d[p + 46..p + 46 + nlen]).into_owned();
            if method == 0 && lofs + 30 <= d.len() {
                let ln = rd16(lofs + 26);
                let le = rd16(lofs + 28);
                let start = lofs + 30 + ln + le;
                if start + csize <= d.len() {
                    z.files.push((name, d[start..start + csize].to_vec()));
                }
            }
            p += 46 + nlen + elen + clen;
        }
        z
    }
}
