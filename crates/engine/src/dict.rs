//! System dictionary: reading -> word entries, plus the connection cost matrix.
//!
//! File layout (all integers little-endian, sections 8-byte aligned):
//!
//! ```text
//! header (64 bytes)
//!   magic      [u8; 8]  = b"SWDICT01"
//!   dim        u32      number of POS ids (connection matrix is dim x dim)
//!   unk_id     u16      POS id used for unknown words
//!   _pad       u16
//!   fst_off    u64, fst_len u64
//!   ent_off    u64, ent_count u64
//!   str_off    u64, str_len u64
//!   conn_off   u64      connection matrix: dim*dim i16, then dim bytes of PosClass
//!   feat_off   u64      POS feature strings (0 in older files): dim times
//!                       u16 length + UTF-8 bytes
//! ```
//!
//! The FST maps each reading to `(first_entry << 24) | count`.
//! Each entry is 12 bytes: lid u16, rid u16, cost i16, surface_len u16, surface_off u32.

use fst::raw::Output;
use memmap2::Mmap;
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;
use std::sync::Arc;

const MAGIC: &[u8; 8] = b"SWDICT01";
const HEADER_LEN: usize = 80;
const ENTRY_LEN: usize = 12;

/// Bundled spellings shared by the dictionary compiler and neural reranker.
pub const VRCHAT_TERMS: &str = include_str!("vrchat.tsv");
/// Canonical technical spellings and their Japanese readings.
pub const TECH_TERMS: &str = include_str!("tech_terms.tsv");
/// Internet slang and meme phrases. Dictionary entries only: unlike the
/// spellings above, the reranker does not prefer them.
pub const NET_SLANG: &str = include_str!("net_slang.tsv");

/// Coarse part-of-speech class used for bunsetsu segmentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PosClass {
    /// Starts a new bunsetsu (nouns, verbs, adjectives, adverbs, ...).
    Content = 0,
    /// Attaches to the preceding bunsetsu (particles, auxiliaries, suffixes).
    Functional = 1,
    /// Prefix: starts a bunsetsu and the following content word joins it.
    Prefix = 2,
    /// BOS/EOS.
    Boundary = 3,
    /// Content noun: consecutive nouns form one compound bunsetsu.
    Noun = 4,
}

impl PosClass {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Functional,
            2 => Self::Prefix,
            3 => Self::Boundary,
            4 => Self::Noun,
            _ => Self::Content,
        }
    }

    /// Classify from an id.def feature string such as `名詞,接尾,一般,*,*,*,*`.
    pub fn classify(feature: &str) -> Self {
        let f: Vec<&str> = feature.split(',').collect();
        let p0 = f.first().copied().unwrap_or("");
        let p1 = f.get(1).copied().unwrap_or("");
        match p0 {
            "BOS/EOS" => Self::Boundary,
            "助詞" | "助動詞" => Self::Functional,
            "接頭詞" => Self::Prefix,
            "名詞" | "動詞" | "形容詞" if p1 == "接尾" => Self::Functional,
            "動詞" | "形容詞" if p1 == "非自立" => Self::Functional,
            "名詞" if p1 != "非自立" && p1 != "代名詞" => Self::Noun,
            _ => Self::Content,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Entry<'a> {
    pub lid: u16,
    pub rid: u16,
    pub cost: i16,
    pub surface: &'a str,
}

/// Source entry for building a dictionary.
#[derive(Clone, Debug)]
pub struct SourceEntry {
    pub reading: String,
    pub lid: u16,
    pub rid: u16,
    pub cost: i16,
    pub surface: String,
}

#[derive(Clone)]
struct Bytes(Arc<Mmap>, usize, usize);

impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {
        &self.0[self.1..self.1 + self.2]
    }
}

pub struct Dictionary {
    map: Arc<Mmap>,
    fst: fst::Map<Bytes>,
    dim: usize,
    unk_id: u16,
    ent_off: usize,
    ent_count: usize,
    str_off: usize,
    conn_off: usize,
    class_off: usize,
    /// id.def feature strings, when the file has them.
    features: Vec<Box<str>>,
}

fn rd_u64(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}
fn rd_u32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn rd_u16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes(b[o..o + 2].try_into().unwrap())
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

impl Dictionary {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let file = File::open(path)?;
        // SAFETY: the dictionary file is treated as read-only and must not be
        // modified while mapped; all offsets are bounds-checked below.
        let map = Arc::new(unsafe { Mmap::map(&file)? });
        let b: &[u8] = &map;
        if b.len() < HEADER_LEN || &b[..8] != MAGIC {
            return Err(invalid("not a sweet-ime dictionary"));
        }
        let dim = rd_u32(b, 8) as usize;
        let unk_id = rd_u16(b, 12);
        let fst_off = rd_u64(b, 16) as usize;
        let fst_len = rd_u64(b, 24) as usize;
        let ent_off = rd_u64(b, 32) as usize;
        let ent_count = rd_u64(b, 40) as usize;
        let str_off = rd_u64(b, 48) as usize;
        let str_len = rd_u64(b, 56) as usize;
        let conn_off = rd_u64(b, 64) as usize;
        let class_off = conn_off + dim * dim * 2;
        let end = class_off + dim;
        if fst_off + fst_len > b.len()
            || ent_off + ent_count * ENTRY_LEN > b.len()
            || str_off + str_len > b.len()
            || end > b.len()
        {
            return Err(invalid("truncated dictionary"));
        }
        let fst = fst::Map::new(Bytes(map.clone(), fst_off, fst_len))
            .map_err(|e| invalid(&e.to_string()))?;
        let feat_off = rd_u64(b, 72) as usize;
        let mut features = Vec::new();
        if feat_off != 0 {
            let mut o = feat_off;
            for _ in 0..dim {
                if o + 2 > b.len() {
                    return Err(invalid("truncated POS features"));
                }
                let n = rd_u16(b, o) as usize;
                let s = b.get(o + 2..o + 2 + n).ok_or_else(|| invalid("truncated POS features"))?;
                features.push(std::str::from_utf8(s).map_err(|_| invalid("bad POS feature"))?.into());
                o += 2 + n;
            }
        }
        Ok(Self { map, fst, dim, unk_id, ent_off, ent_count, str_off, conn_off, class_off, features })
    }

    /// The POS id whose id.def feature is exactly `feature`, if the
    /// dictionary records features.
    pub fn pos_id(&self, feature: &str) -> Option<u16> {
        self.features.iter().position(|f| &**f == feature).map(|i| i as u16)
    }

    /// The id.def feature of a POS id, if the dictionary records features.
    pub fn feature(&self, id: u16) -> Option<&str> {
        self.features.get(usize::from(id)).map(|f| &**f)
    }

    pub fn pos_count(&self) -> usize {
        self.dim
    }

    pub fn unknown_id(&self) -> u16 {
        self.unk_id
    }

    pub fn entry_count(&self) -> usize {
        self.ent_count
    }

    /// Connection cost between the right id of the previous word and the
    /// left id of the next word.
    #[inline]
    pub fn connection(&self, rid: u16, lid: u16) -> i32 {
        let o = self.conn_off + (rid as usize * self.dim + lid as usize) * 2;
        i16::from_le_bytes([self.map[o], self.map[o + 1]]) as i32
    }

    #[inline]
    pub fn pos_class(&self, id: u16) -> PosClass {
        PosClass::from_u8(self.map[self.class_off + id as usize])
    }

    fn entries(&self, packed: u64) -> impl Iterator<Item = Entry<'_>> {
        let first = (packed >> 24) as usize;
        let count = (packed & 0xff_ffff) as usize;
        let b: &[u8] = &self.map;
        (first..first + count).map(move |i| {
            let o = self.ent_off + i * ENTRY_LEN;
            let slen = rd_u16(b, o + 6) as usize;
            let soff = self.str_off + rd_u32(b, o + 8) as usize;
            Entry {
                lid: rd_u16(b, o),
                rid: rd_u16(b, o + 2),
                cost: rd_u16(b, o + 4) as i16,
                // Surfaces are validated UTF-8 at build time.
                surface: std::str::from_utf8(&b[soff..soff + slen]).unwrap_or(""),
            }
        })
    }

    /// All words whose reading is exactly `reading`.
    pub fn lookup(&self, reading: &str) -> Vec<Entry<'_>> {
        match self.fst.get(reading.as_bytes()) {
            Some(v) => self.entries(v).collect(),
            None => Vec::new(),
        }
    }

    /// Calls `f(reading_len_bytes, lid, rid, cost)` for every word whose
    /// reading is a prefix of `text`, without decoding surface strings.
    pub(crate) fn common_prefix_costs(
        &self,
        text: &str,
        mut f: impl FnMut(usize, u16, u16, i16),
    ) {
        let fst = self.fst.as_fst();
        let mut node = fst.root();
        let mut out = Output::zero();
        let b: &[u8] = &self.map;
        for (i, &byte) in text.as_bytes().iter().enumerate() {
            let Some(ti) = node.find_input(byte) else { return };
            let t = node.transition(ti);
            out = out.cat(t.out);
            node = fst.node(t.addr);
            if node.is_final() {
                let packed = out.cat(node.final_output()).value();
                let first = (packed >> 24) as usize;
                let count = (packed & 0xff_ffff) as usize;
                let len = i + 1;
                for idx in first..first + count {
                    let o = self.ent_off + idx * ENTRY_LEN;
                    f(len, rd_u16(b, o), rd_u16(b, o + 2), rd_u16(b, o + 4) as i16);
                }
            }
        }
    }

    /// Calls `f(reading_len_bytes, entry)` for every word whose reading is a
    /// prefix of `text`.
    pub fn common_prefix<'s>(&'s self, text: &str, mut f: impl FnMut(usize, Entry<'s>)) {
        let fst = self.fst.as_fst();
        let mut node = fst.root();
        let mut out = Output::zero();
        for (i, &byte) in text.as_bytes().iter().enumerate() {
            let Some(ti) = node.find_input(byte) else { return };
            let t = node.transition(ti);
            out = out.cat(t.out);
            node = fst.node(t.addr);
            if node.is_final() {
                let packed = out.cat(node.final_output()).value();
                for e in self.entries(packed) {
                    f(i + 1, e);
                }
            }
        }
    }

    /// Words whose reading starts with `prefix` (for prediction), up to `limit`
    /// readings.
    pub fn predictive(&self, prefix: &str, limit: usize) -> Vec<(String, Entry<'_>)> {
        let mut entries = Vec::new();
        self.for_each_prediction(prefix, limit, |reading, entry| {
            entries.push((reading.to_owned(), entry));
        });
        entries
    }

    pub(crate) fn for_each_prediction<'s>(
        &'s self,
        prefix: &str,
        limit: usize,
        mut visit: impl FnMut(&str, Entry<'s>),
    ) {
        use fst::{IntoStreamer, Streamer};
        let mut upper = prefix.as_bytes().to_vec();
        upper.push(0xff);
        let mut stream = self.fst.range().ge(prefix).lt(upper).into_stream();
        let mut n = 0;
        while let Some((k, v)) = stream.next() {
            let reading = String::from_utf8_lossy(k);
            for e in self.entries(v) {
                visit(&reading, e);
            }
            n += 1;
            if n >= limit {
                break;
            }
        }
    }
}

/// Builds a dictionary file.
///
/// `pos_features[id]` is the id.def feature string; `matrix` is row-major
/// `[rid * dim + lid]`.
pub fn build(
    out: &mut impl Write,
    mut entries: Vec<SourceEntry>,
    pos_features: &[String],
    matrix: &[i16],
) -> io::Result<()> {
    let dim = pos_features.len();
    if matrix.len() != dim * dim {
        return Err(invalid("matrix size does not match POS count"));
    }
    let unk_id = pos_features
        .iter()
        .position(|f| f.starts_with("名詞,一般,"))
        .unwrap_or(0) as u16;

    entries.sort_by(|a, b| {
        a.reading.as_bytes().cmp(b.reading.as_bytes()).then(a.cost.cmp(&b.cost))
    });
    entries.dedup_by(|a, b| a.reading == b.reading && a.surface == b.surface && a.lid == b.lid && a.rid == b.rid);

    let mut strings: Vec<u8> = Vec::new();
    let mut string_ids: std::collections::HashMap<String, u32> = Default::default();
    let mut ent_bytes: Vec<u8> = Vec::with_capacity(entries.len() * ENTRY_LEN);
    let mut builder = fst::MapBuilder::memory();

    let mut i = 0;
    while i < entries.len() {
        let mut j = i;
        while j < entries.len() && entries[j].reading == entries[i].reading {
            j += 1;
        }
        let count = j - i;
        if count > 0xff_ffff {
            return Err(invalid("too many homophones"));
        }
        builder
            .insert(entries[i].reading.as_bytes(), ((i as u64) << 24) | count as u64)
            .map_err(|e| invalid(&e.to_string()))?;
        for e in &entries[i..j] {
            let off = *string_ids.entry(e.surface.clone()).or_insert_with(|| {
                let o = strings.len() as u32;
                strings.extend_from_slice(e.surface.as_bytes());
                o
            });
            ent_bytes.extend_from_slice(&e.lid.to_le_bytes());
            ent_bytes.extend_from_slice(&e.rid.to_le_bytes());
            ent_bytes.extend_from_slice(&e.cost.to_le_bytes());
            ent_bytes.extend_from_slice(&(e.surface.len() as u16).to_le_bytes());
            ent_bytes.extend_from_slice(&off.to_le_bytes());
        }
        i = j;
    }
    let fst_bytes = builder.into_inner().map_err(|e| invalid(&e.to_string()))?;

    let align = |x: usize| (x + 7) & !7;
    let fst_off = HEADER_LEN;
    let ent_off = align(fst_off + fst_bytes.len());
    let str_off = align(ent_off + ent_bytes.len());
    let conn_off = align(str_off + strings.len());

    let mut buf: Vec<u8> = Vec::with_capacity(conn_off + matrix.len() * 2 + dim);
    buf.extend_from_slice(MAGIC);
    buf.extend_from_slice(&(dim as u32).to_le_bytes());
    buf.extend_from_slice(&unk_id.to_le_bytes());
    buf.extend_from_slice(&0u16.to_le_bytes());
    for v in [
        fst_off,
        fst_bytes.len(),
        ent_off,
        entries.len(),
        str_off,
        strings.len(),
        conn_off,
    ] {
        buf.extend_from_slice(&(v as u64).to_le_bytes());
    }
    buf.resize(HEADER_LEN, 0);
    buf.extend_from_slice(&fst_bytes);
    buf.resize(ent_off, 0);
    buf.extend_from_slice(&ent_bytes);
    buf.resize(str_off, 0);
    buf.extend_from_slice(&strings);
    buf.resize(conn_off, 0);
    for c in matrix {
        buf.extend_from_slice(&c.to_le_bytes());
    }
    for f in pos_features {
        buf.push(PosClass::classify(f) as u8);
    }
    let feat_off = buf.len();
    for f in pos_features {
        buf.extend_from_slice(&(f.len() as u16).to_le_bytes());
        buf.extend_from_slice(f.as_bytes());
    }
    buf[72..80].copy_from_slice(&(feat_off as u64).to_le_bytes());
    out.write_all(&buf)
}
