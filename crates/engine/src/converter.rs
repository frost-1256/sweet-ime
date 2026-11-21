//! Kana -> kanji conversion: lattice construction, Viterbi, N-best (A*),
//! and bunsetsu segmentation.

use crate::dict::{Dictionary, PosClass};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet};

/// Cost of a single-character unknown word (hiragana passthrough).
const UNKNOWN_CHAR_COST: i32 = 12_000;
/// Cost of a run of non-kana characters (ASCII, digits, symbols).
const UNKNOWN_RUN_COST: i32 = 4_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Word {
    pub reading: String,
    pub surface: String,
    pub lid: u16,
    pub rid: u16,
    pub cost: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    pub reading: String,
    pub surface: String,
    pub words: Vec<Word>,
}

#[derive(Clone, Debug)]
pub struct Conversion {
    pub segments: Vec<Segment>,
    pub cost: i32,
}

impl Conversion {
    pub fn surface(&self) -> String {
        self.segments.iter().map(|s| s.surface.as_str()).collect()
    }
}

#[derive(Clone, Copy)]
struct Node<'a> {
    start: usize,
    end: usize,
    lid: u16,
    rid: u16,
    wcost: i32,
    surface: &'a str,
    /// Best cost from BOS through this node (inclusive).
    best: i32,
}

struct Lattice<'a> {
    nodes: Vec<Node<'a>>,
    /// Node indices ending at each byte position.
    ends: Vec<Vec<u32>>,
    /// Node indices starting at each byte position.
    begins: Vec<Vec<u32>>,
}

const BOS: u32 = 0;
const EOS: u32 = 1;

pub struct Converter {
    dict: Dictionary,
}

fn is_kana(c: char) -> bool {
    matches!(c, 'ぁ'..='ゖ' | 'ァ'..='ヺ' | 'ー' | 'ゝ' | 'ゞ')
}

impl Converter {
    pub fn new(dict: Dictionary) -> Self {
        Self { dict }
    }

    pub fn dictionary(&self) -> &Dictionary {
        &self.dict
    }

    fn build_lattice<'a>(&'a self, text: &'a str, boundaries: &[usize]) -> Lattice<'a> {
        let len = text.len();
        let mut lat = Lattice {
            nodes: Vec::with_capacity(len * 8),
            ends: vec![Vec::new(); len + 1],
            begins: vec![Vec::new(); len + 1],
        };
        let push = |lat: &mut Lattice<'a>, n: Node<'a>| {
            let id = lat.nodes.len() as u32;
            lat.ends[n.end].push(id);
            lat.begins[n.start].push(id);
            lat.nodes.push(n);
        };
        // BOS / EOS
        lat.nodes.push(Node { start: 0, end: 0, lid: 0, rid: 0, wcost: 0, surface: "", best: 0 });
        lat.ends[0].push(BOS);
        lat.nodes.push(Node { start: len, end: len, lid: 0, rid: 0, wcost: 0, surface: "", best: i32::MAX });

        let crosses = |s: usize, e: usize| boundaries.iter().any(|&b| s < b && b < e);
        let unk = self.dict.unknown_id();

        let mut pos = 0;
        while pos < len {
            let rest = &text[pos..];
            let c = rest.chars().next().unwrap();
            if !is_kana(c) {
                // Group a run of non-kana characters into one node.
                let run: usize = rest
                    .char_indices()
                    .find(|&(_, ch)| is_kana(ch))
                    .map(|(i, _)| i)
                    .unwrap_or(rest.len());
                let mut e = pos + run;
                if let Some(&b) = boundaries.iter().find(|&&b| pos < b && b < e) {
                    e = b;
                }
                push(&mut lat, Node {
                    start: pos, end: e, lid: unk, rid: unk,
                    wcost: UNKNOWN_RUN_COST, surface: &text[pos..e], best: i32::MAX,
                });
                pos = e;
                continue;
            }
            let mut found = Vec::new();
            self.dict.common_prefix(rest, |l, e| found.push((l, e)));
            for (l, e) in found {
                if crosses(pos, pos + l) {
                    continue;
                }
                push(&mut lat, Node {
                    start: pos, end: pos + l, lid: e.lid, rid: e.rid,
                    wcost: e.cost as i32, surface: e.surface, best: i32::MAX,
                });
            }
            // Always allow a single-character fallback to keep the lattice connected.
            let cl = c.len_utf8();
            push(&mut lat, Node {
                start: pos, end: pos + cl, lid: unk, rid: unk,
                wcost: UNKNOWN_CHAR_COST, surface: &text[pos..pos + cl], best: i32::MAX,
            });
            pos += cl;
        }
        lat.begins[len].push(EOS);
        lat
    }

    fn viterbi(&self, lat: &mut Lattice<'_>) {
        let len = lat.ends.len() - 1;
        for pos in 0..=len {
            if pos > 0 && lat.ends[pos].is_empty() {
                continue;
            }
            for bi in 0..lat.begins[pos].len() {
                let id = lat.begins[pos][bi] as usize;
                let (lid, wcost) = (lat.nodes[id].lid, lat.nodes[id].wcost);
                let mut best = i32::MAX;
                for &p in &lat.ends[pos] {
                    let prev = &lat.nodes[p as usize];
                    if prev.best == i32::MAX {
                        continue;
                    }
                    let c = prev.best + self.dict.connection(prev.rid, lid);
                    if c < best {
                        best = c;
                    }
                }
                lat.nodes[id].best = if best == i32::MAX { i32::MAX } else { best + wcost };
            }
        }
    }

    /// N-best paths (BOS..EOS exclusive) via backward A* over the Viterbi
    /// forward costs, which are an exact heuristic.
    fn nbest_paths(&self, lat: &Lattice<'_>, n: usize) -> Vec<(i32, Vec<u32>)> {
        // arena entries: (node, parent arena index, g = cost after node)
        let mut arena: Vec<(u32, u32, i32)> = vec![(EOS, u32::MAX, 0)];
        let mut heap = BinaryHeap::new();
        heap.push((Reverse(lat.nodes[EOS as usize].best), 0u32));
        let mut out = Vec::new();
        let mut pops = 0;
        while let Some((Reverse(f), ai)) = heap.pop() {
            pops += 1;
            if pops > 20_000 {
                break;
            }
            let (node, _, g) = arena[ai as usize];
            if node == BOS {
                let mut path = Vec::new();
                let mut cur = arena[ai as usize].1;
                while cur != u32::MAX {
                    let (nd, parent, _) = arena[cur as usize];
                    if nd != EOS {
                        path.push(nd);
                    }
                    cur = parent;
                }
                out.push((f, path));
                if out.len() >= n {
                    break;
                }
                continue;
            }
            let nd = &lat.nodes[node as usize];
            let g2 = g + nd.wcost;
            for &p in &lat.ends[nd.start] {
                let prev = &lat.nodes[p as usize];
                if prev.best == i32::MAX {
                    continue;
                }
                let g3 = g2 + self.dict.connection(prev.rid, nd.lid);
                arena.push((p, ai, g3));
                heap.push((Reverse(prev.best + g3), (arena.len() - 1) as u32));
            }
        }
        out
    }

    fn to_words(&self, lat: &Lattice<'_>, text: &str, path: &[u32]) -> Vec<Word> {
        path.iter()
            .map(|&i| {
                let n = &lat.nodes[i as usize];
                Word {
                    reading: text[n.start..n.end].to_string(),
                    surface: n.surface.to_string(),
                    lid: n.lid,
                    rid: n.rid,
                    cost: n.wcost,
                }
            })
            .collect()
    }

    /// Group words into bunsetsu. If `boundaries` is non-empty the segments
    /// are exactly the given spans.
    fn segment(&self, words: Vec<Word>, boundaries: &[usize]) -> Vec<Segment> {
        let mut segs: Vec<Segment> = Vec::new();
        let mut prev_class = PosClass::Boundary;
        let mut pos = 0usize;
        for w in words {
            let class = self.dict.pos_class(w.lid);
            let start_new = if !boundaries.is_empty() {
                pos == 0 || boundaries.contains(&pos)
            } else {
                match class {
                    PosClass::Functional => segs.is_empty(),
                    PosClass::Noun => !matches!(prev_class, PosClass::Noun | PosClass::Prefix),
                    PosClass::Content => prev_class != PosClass::Prefix,
                    PosClass::Prefix | PosClass::Boundary => true,
                }
            };
            pos += w.reading.len();
            prev_class = class;
            if start_new || segs.is_empty() {
                segs.push(Segment { reading: String::new(), surface: String::new(), words: Vec::new() });
            }
            let s = segs.last_mut().unwrap();
            s.reading.push_str(&w.reading);
            s.surface.push_str(&w.surface);
            s.words.push(w);
        }
        segs
    }

    /// Convert `text` (hiragana) into up to `n` whole-sentence candidates.
    /// `boundaries` are byte offsets that segments must break at (user-resized
    /// bunsetsu); pass `&[]` for automatic segmentation.
    pub fn convert(&self, text: &str, boundaries: &[usize], n: usize) -> Vec<Conversion> {
        if text.is_empty() {
            return Vec::new();
        }
        let mut lat = self.build_lattice(text, boundaries);
        self.viterbi(&mut lat);
        let mut seen = HashSet::new();
        let mut res = Vec::new();
        for (cost, path) in self.nbest_paths(&lat, n * 3) {
            let words = self.to_words(&lat, text, &path);
            let segments = self.segment(words, boundaries);
            let conv = Conversion { segments, cost };
            if seen.insert(conv.surface()) {
                res.push(conv);
                if res.len() >= n {
                    break;
                }
            }
        }
        res
    }

    /// Candidate surfaces for a single bunsetsu reading, best first.
    pub fn segment_candidates(&self, reading: &str, n: usize) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        let mut add = |s: String, out: &mut Vec<String>| {
            if !s.is_empty() && seen.insert(s.clone()) {
                out.push(s);
            }
        };
        let mut lat = self.build_lattice(reading, &[]);
        self.viterbi(&mut lat);
        for (_, path) in self.nbest_paths(&lat, n) {
            let s: String = path.iter().map(|&i| lat.nodes[i as usize].surface).collect();
            add(s, &mut out);
        }
        add(reading.to_string(), &mut out);
        add(crate::romaji::to_katakana(reading), &mut out);
        out
    }
}
