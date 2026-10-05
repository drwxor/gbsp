#[derive(Clone, Debug)]
pub enum KvValue {
    Str(String),
    Block(Vec<Kv>),
}

#[derive(Clone, Debug)]
pub struct Kv {
    pub key: String,
    pub value: KvValue,
}

impl Kv {
    pub fn children(&self) -> &[Kv] {
        match &self.value {
            KvValue::Block(c) => c,
            _ => &[],
        }
    }
    pub fn children_mut(&mut self) -> Option<&mut Vec<Kv>> {
        match &mut self.value {
            KvValue::Block(c) => Some(c),
            _ => None,
        }
    }
    pub fn str(&self) -> Option<&str> {
        match &self.value {
            KvValue::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn is_block(&self) -> bool {
        matches!(self.value, KvValue::Block(_))
    }
    pub fn get(&self, key: &str) -> Option<&str> {
        self.children()
            .iter()
            .find(|c| c.key.eq_ignore_ascii_case(key))
            .and_then(|c| c.str())
    }
    pub fn block(&self, key: &str) -> Option<&Kv> {
        self.children()
            .iter()
            .find(|c| c.key.eq_ignore_ascii_case(key) && c.is_block())
    }
    pub fn blocks<'a>(&'a self, key: &'a str) -> impl Iterator<Item = &'a Kv> + 'a {
        self.children()
            .iter()
            .filter(move |c| c.key.eq_ignore_ascii_case(key) && c.is_block())
    }
}

#[derive(Debug, PartialEq)]
enum Tok {
    Str(String, bool),
    Open,
    Close,
}

fn tokenize(src: &str) -> Vec<Tok> {
    let b = src.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
        } else if c == b'/' && i + 1 < b.len() && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if c == b'{' {
            out.push(Tok::Open);
            i += 1;
        } else if c == b'}' {
            out.push(Tok::Close);
            i += 1;
        } else if c == b'"' {
            let start = i + 1;
            let mut j = start;
            while j < b.len() && b[j] != b'"' {
                j += 1;
            }
            out.push(Tok::Str(
                String::from_utf8_lossy(&b[start..j.min(b.len())]).into_owned(),
                true,
            ));
            i = j + 1;
        } else {
            let start = i;
            while i < b.len()
                && !b[i].is_ascii_whitespace()
                && b[i] != b'{'
                && b[i] != b'}'
                && b[i] != b'"'
            {
                i += 1;
            }
            out.push(Tok::Str(
                String::from_utf8_lossy(&b[start..i]).into_owned(),
                false,
            ));
        }
    }
    out
}

fn is_conditional(t: &Tok) -> bool {
    matches!(t, Tok::Str(s, false) if s.starts_with('[') && s.ends_with(']'))
}

fn parse_block(toks: &[Tok], pos: &mut usize) -> Vec<Kv> {
    let mut out = Vec::new();
    while *pos < toks.len() {
        match &toks[*pos] {
            Tok::Close => {
                *pos += 1;
                return out;
            }
            Tok::Open => {
                *pos += 1;
                let c = parse_block(toks, pos);
                out.push(Kv {
                    key: String::new(),
                    value: KvValue::Block(c),
                });
            }
            Tok::Str(k, _) => {
                let key = k.clone();
                *pos += 1;
                if *pos < toks.len() && is_conditional(&toks[*pos]) {
                    *pos += 1;
                }
                match toks.get(*pos) {
                    Some(Tok::Open) => {
                        *pos += 1;
                        let c = parse_block(toks, pos);
                        out.push(Kv {
                            key,
                            value: KvValue::Block(c),
                        });
                    }
                    Some(Tok::Str(v, _)) => {
                        let v = v.clone();
                        *pos += 1;
                        if *pos < toks.len() && is_conditional(&toks[*pos]) {
                            let cond = if let Tok::Str(s, _) = &toks[*pos] {
                                s.to_ascii_uppercase()
                            } else {
                                String::new()
                            };
                            *pos += 1;
                            if cond.contains("X360") && !cond.contains('!') {
                                continue;
                            }
                        }
                        out.push(Kv {
                            key,
                            value: KvValue::Str(v),
                        });
                    }
                    Some(Tok::Close) | None => {
                        out.push(Kv {
                            key,
                            value: KvValue::Str(String::new()),
                        });
                    }
                }
            }
        }
    }
    out
}

pub fn parse(src: &str) -> Vec<Kv> {
    let toks = tokenize(src);
    let mut pos = 0;
    let mut out = Vec::new();
    while pos < toks.len() {
        let mut blk = parse_block(&toks, &mut pos);
        out.append(&mut blk);
    }
    out
}

pub fn to_string(kvs: &[Kv]) -> String {
    fn rec(out: &mut String, kvs: &[Kv], depth: usize) {
        for kv in kvs {
            let ind = "\t".repeat(depth);
            match &kv.value {
                KvValue::Str(s) => out.push_str(&format!("{ind}\"{}\" \"{}\"\n", kv.key, s)),
                KvValue::Block(c) => {
                    out.push_str(&format!("{ind}\"{}\"\n{ind}{{\n", kv.key));
                    rec(out, c, depth + 1);
                    out.push_str(&format!("{ind}}}\n"));
                }
            }
        }
    }
    let mut s = String::new();
    rec(&mut s, kvs, 0);
    s
}
