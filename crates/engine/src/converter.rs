//! Kana -> kanji conversion: lattice construction, Viterbi, N-best (A*),
//! and bunsetsu segmentation.

use crate::dict::{Dictionary, PosClass};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet};
use std::sync::Mutex;

use crate::learning::UserHistory;

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
    /// Came from the user's learning history.
    pub learned: bool,
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

    /// Number of words taken from the user's learning history.
    pub fn learned_words(&self) -> usize {
        self.segments.iter().flat_map(|s| &s.words).filter(|w| w.learned).count()
    }
}

/// A learned word placed into the lattice.
struct Extra {
    start: usize,
    end: usize,
    lid: u16,
    rid: u16,
    cost: i32,
    surface: String,
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
    learned: bool,
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

/// Picks the best whole-sentence candidate (e.g. with a neural LM).
pub trait Reranker: Send + Sync {
    /// How many lattice candidates to offer.
    fn candidates(&self) -> usize {
        5
    }
    /// Returns the index of the best candidate, or `None` to keep the
    /// lattice order.
    fn choose(&self, reading: &str, left_context: &str, candidates: &[Conversion]) -> Option<usize>;
}

pub struct Converter {
    dict: Dictionary,
    reranker: Option<Box<dyn Reranker>>,
    history: Mutex<UserHistory>,
    /// Runtime switches (see [`Converter::configure`]).
    rerank_on: std::sync::atomic::AtomicBool,
    learning_on: std::sync::atomic::AtomicBool,
    /// Recent reranker decisions: (reading, left context, candidates) -> index.
    rerank_cache: Mutex<Vec<(String, String, Vec<String>, Option<usize>)>>,
}

const RERANK_CACHE: usize = 16;

fn is_kana(c: char) -> bool {
    matches!(c, 'ぁ'..='ゖ' | 'ァ'..='ヺ' | 'ー' | 'ゝ' | 'ゞ')
}

impl Converter {
    pub fn new(dict: Dictionary) -> Self {
        Self { dict, reranker: None, history: Mutex::new(UserHistory::new()),
            rerank_on: std::sync::atomic::AtomicBool::new(true),
            learning_on: std::sync::atomic::AtomicBool::new(true),
            rerank_cache: Mutex::new(Vec::new()) }
    }

    pub fn with_history(self, h: UserHistory) -> Self {
        *self.history.lock().unwrap_or_else(|e| e.into_inner()) = h;
        self
    }

    fn history(&self) -> std::sync::MutexGuard<'_, UserHistory> {
        self.history.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Remembers that the user chose `surface` for `reading`.
    pub fn learn(&self, reading: &str, surface: &str) {
        if reading == surface || !self.learning_enabled() {
            return; // plain hiragana needs no learning
        }
        // Left/right POS ids: from a lattice path producing exactly this
        // surface, else a dictionary entry, else the unknown-word id.
        let (mut lid, mut rid) = (self.dict.unknown_id(), self.dict.unknown_id());
        let mut lat = self.build_lattice(reading, &[], &[]);
        self.viterbi(&mut lat);
        let mut found = false;
        for (_, path) in self.nbest_paths(&lat, 30) {
            let s: String = path.iter().map(|&i| lat.nodes[i as usize].surface).collect();
            if s == surface && !path.is_empty() {
                lid = lat.nodes[path[0] as usize].lid;
                rid = lat.nodes[*path.last().unwrap() as usize].rid;
                found = true;
                break;
            }
        }
        if !found {
            if let Some(e) = self.dict.lookup(reading).into_iter().find(|e| e.surface == surface) {
                lid = e.lid;
                rid = e.rid;
            }
        }
        self.history().learn(reading, surface, lid, rid);
    }

    pub fn forget(&self, reading: &str, surface: &str) {
        self.history().forget(reading, surface);
    }

    /// Learned words that occur in `text`, as lattice extras.
    fn learned_extras(&self, text: &str) -> Vec<Extra> {
        if !self.learning_enabled() {
            return Vec::new();
        }
        let mut h = self.history();
        h.reload_if_changed();
        self.learned_extras_from(text, &h)
    }

    fn learned_extras_from(&self, text: &str, h: &UserHistory) -> Vec<Extra> {
        if h.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (pos, _) in text.char_indices() {
            h.common_prefix(&text[pos..], |l, e| {
                let base = self
                    .dict
                    .lookup(&text[pos..pos + l])
                    .into_iter()
                    .filter(|d| d.surface == e.surface)
                    .map(|d| d.cost as i32)
                    .min()
                    .unwrap_or(4000)
                    .min(4000);
                let bonus = 1500.0 + 300.0 * (1.0 + e.count as f32).ln();
                out.push(Extra {
                    start: pos,
                    end: pos + l,
                    lid: e.lid,
                    rid: e.rid,
                    cost: (base as f32 - bonus).max(0.0) as i32,
                    surface: e.surface.clone(),
                });
            });
        }
        out
    }

    pub fn with_reranker(mut self, r: Box<dyn Reranker>) -> Self {
        self.reranker = Some(r);
        self
    }

    /// Whether neural reranking is available and enabled.
    pub fn has_reranker(&self) -> bool {
        self.reranker.is_some() && self.rerank_on.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Whether a reranker is loaded at all (regardless of the switch).
    pub fn reranker_loaded(&self) -> bool {
        self.reranker.is_some()
    }

    /// Turns neural reranking and user learning on or off.
    pub fn configure(&self, rerank: bool, learning: bool) {
        use std::sync::atomic::Ordering::Relaxed;
        self.rerank_on.store(rerank, Relaxed);
        self.learning_on.store(learning, Relaxed);
    }

    fn learning_enabled(&self) -> bool {
        self.learning_on.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Best conversion: lattice N-best refined by the reranker if present.
    /// The chosen candidate is first; the rest follow in lattice order.
    pub fn convert_best(&self, text: &str, boundaries: &[usize], left_context: &str) -> Vec<Conversion> {
        let n = self.reranker.as_ref().filter(|_| self.has_reranker()).map_or(1, |r| r.candidates().max(1));
        let mut res = self.convert(text, boundaries, n);
        if let Some(r) = self.reranker.as_ref().filter(|_| self.has_reranker()) {
            // Don't let the model override what the user taught us.
            if res.first().is_some_and(|c| c.learned_words() > 0) {
                return res;
            }
            if res.len() > 1 {
                let surfaces: Vec<String> = res.iter().map(|c| c.surface()).collect();
                if let Some(i) = self.cached_choice(r.as_ref(), text, left_context, &res, surfaces) {
                    if i < res.len() {
                        let c = res.remove(i);
                        res.insert(0, c);
                    }
                }
            }
        }
        res
    }

    /// `Reranker::choose`, memoized so that a result computed in the
    /// background (live conversion) makes the following Space instant.
    fn cached_choice(
        &self,
        r: &dyn Reranker,
        text: &str,
        left_context: &str,
        res: &[Conversion],
        surfaces: Vec<String>,
    ) -> Option<usize> {
        let lock = || self.rerank_cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(hit) = lock()
            .iter()
            .find(|(t, l, s, _)| t == text && l == left_context && *s == surfaces)
            .map(|e| e.3)
        {
            return hit;
        }
        // Not holding the lock while the model runs.
        let choice = r.choose(text, left_context, res);
        let mut cache = lock();
        cache.retain(|(t, l, _, _)| !(t == text && l == left_context));
        cache.push((text.to_string(), left_context.to_string(), surfaces, choice));
        if cache.len() > RERANK_CACHE {
            cache.remove(0);
        }
        choice
    }

    pub fn dictionary(&self) -> &Dictionary {
        &self.dict
    }

    fn build_lattice<'a>(&'a self, text: &'a str, boundaries: &[usize], extras: &'a [Extra]) -> Lattice<'a> {
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
        lat.nodes.push(Node { start: 0, end: 0, lid: 0, rid: 0, wcost: 0, surface: "", best: 0, learned: false });
        lat.ends[0].push(BOS);
        lat.nodes.push(Node { start: len, end: len, lid: 0, rid: 0, wcost: 0, surface: "", best: i32::MAX, learned: false });

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
                    wcost: UNKNOWN_RUN_COST, surface: &text[pos..e], best: i32::MAX, learned: false,
                });
                pos = e;
                continue;
            }
            self.dict.common_prefix(rest, |l, e| {
                if crosses(pos, pos + l) {
                    return;
                }
                push(&mut lat, Node {
                    start: pos, end: pos + l, lid: e.lid, rid: e.rid,
                    wcost: e.cost as i32, surface: e.surface, best: i32::MAX, learned: false,
                });
            });
            // Always allow a single-character fallback to keep the lattice connected.
            let cl = c.len_utf8();
            push(&mut lat, Node {
                start: pos, end: pos + cl, lid: unk, rid: unk,
                wcost: UNKNOWN_CHAR_COST, surface: &text[pos..pos + cl], best: i32::MAX, learned: false,
            });
            pos += cl;
        }
        for x in extras {
            if !crosses(x.start, x.end) {
                push(&mut lat, Node {
                    start: x.start, end: x.end, lid: x.lid, rid: x.rid,
                    wcost: x.cost, surface: &x.surface, best: i32::MAX, learned: true,
                });
            }
        }
        lat.begins[len].push(EOS);
        lat
    }

    fn viterbi(&self, lat: &mut Lattice<'_>) {
        let len = lat.ends.len() - 1;
        // Alternative spellings with the same left POS share their incoming
        // transition cost at this position.
        let mut incoming = vec![(usize::MAX, i32::MAX); self.dict.pos_count()];
        let mut outgoing = vec![(usize::MAX, 0); self.dict.pos_count()];
        let mut predecessors: Vec<(u16, i32)> = Vec::new();
        for pos in 0..=len {
            if lat.begins[pos].is_empty() || lat.ends[pos].is_empty() {
                continue;
            }
            predecessors.clear();
            for &id in &lat.ends[pos] {
                let prev = &lat.nodes[id as usize];
                if prev.best == i32::MAX {
                    continue;
                }
                let entry = &mut outgoing[prev.rid as usize];
                if entry.0 == pos {
                    let best = &mut predecessors[entry.1].1;
                    *best = (*best).min(prev.best);
                } else {
                    *entry = (pos, predecessors.len());
                    predecessors.push((prev.rid, prev.best));
                }
            }
            for bi in 0..lat.begins[pos].len() {
                let id = lat.begins[pos][bi] as usize;
                let (lid, wcost) = (lat.nodes[id].lid, lat.nodes[id].wcost);
                let entry = &mut incoming[lid as usize];
                if entry.0 != pos {
                    let mut best = i32::MAX;
                    for &(rid, cost) in &predecessors {
                        best = best.min(cost + self.dict.connection(rid, lid));
                    }
                    *entry = (pos, best);
                }
                let best = entry.1;
                lat.nodes[id].best = if best == i32::MAX { i32::MAX } else { best + wcost };
            }
        }
    }

    /// The best path can be traced from the saved Viterbi costs. Walk
    /// predecessors in reverse insertion order to match A* on ties.
    fn best_path(&self, lat: &Lattice<'_>) -> Option<Vec<u32>> {
        let mut path = Vec::new();
        let mut current = EOS;
        while current != BOS {
            let node = &lat.nodes[current as usize];
            let next = lat.ends[node.start].iter().rev().copied().find(|&id| {
                let prev = &lat.nodes[id as usize];
                prev.best != i32::MAX
                    && prev.best + self.dict.connection(prev.rid, node.lid) == node.best - node.wcost
            })?;
            if next != BOS {
                path.push(next);
            }
            current = next;
        }
        path.reverse();
        Some(path)
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
                    learned: n.learned,
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
        // Spans up to the last boundary are fixed by the user; after it the
        // automatic rules apply.
        let fixed_until = boundaries.iter().copied().max().unwrap_or(0);
        for w in words {
            let class = self.dict.pos_class(w.lid);
            let start_new = if boundaries.contains(&pos) {
                true
            } else if pos < fixed_until {
                false
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

    /// Scores typo variants against the same history, checking its file once.
    pub(crate) fn score_typos<T>(&self, evaluate: impl FnOnce(&dyn Fn(&str) -> i32) -> T) -> T {
        let mut history = self.learning_enabled().then(|| self.history());
        if let Some(h) = history.as_mut() {
            h.reload_if_changed();
        }
        evaluate(&|text| {
            let extras = history.as_ref().map_or_else(Vec::new, |h| self.learned_extras_from(text, h));
            let mut lat = self.build_lattice(text, &[], &extras);
            self.viterbi(&mut lat);
            lat.nodes[EOS as usize].best
        })
    }

    /// Convert `text` (hiragana) into up to `n` whole-sentence candidates.
    /// `boundaries` are byte offsets that segments must break at (user-resized
    /// bunsetsu); pass `&[]` for automatic segmentation.
    pub fn convert(&self, text: &str, boundaries: &[usize], n: usize) -> Vec<Conversion> {
        if text.is_empty() {
            return Vec::new();
        }
        let extras = self.learned_extras(text);
        let mut lat = self.build_lattice(text, boundaries, &extras);
        self.viterbi(&mut lat);
        if n == 1 {
            let Some(path) = self.best_path(&lat) else { return Vec::new() };
            let segments = self.segment(self.to_words(&lat, text, &path), boundaries);
            return vec![Conversion { segments, cost: lat.nodes[EOS as usize].best }];
        }
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

    /// Completions for a reading prefix: learned words first, then
    /// dictionary words with a longer reading, ranked by cost with a penalty
    /// for each extra character.
    pub fn predict(&self, prefix: &str, n: usize) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        if self.learning_enabled() {
            let h = self.history();
            for (_, e) in h.predictive(prefix).into_iter().take(n) {
                if seen.insert(e.surface.clone()) {
                    out.push(e.surface.clone());
                }
            }
        }
        let plen = prefix.chars().count();
        let mut scored: Vec<(i32, &str)> = Vec::new();
        for (reading, e) in self.dict.predictive(prefix, 3000) {
            let extra = reading.chars().count().saturating_sub(plen);
            if extra == 0 {
                continue;
            }
            scored.push((e.cost as i32 + 700 * extra as i32, e.surface));
        }
        scored.sort_by_key(|x| x.0);
        for (_, surface) in scored {
            if out.len() >= n {
                break;
            }
            if seen.insert(surface.to_string()) {
                out.push(surface.to_string());
            }
        }
        out
    }

    /// Suggestions for text being typed: completions of the whole reading,
    /// then the best conversion of all but the last bunsetsu followed by
    /// completions of the last one (so predictions keep working mid-sentence).
    pub fn predict_sentence(&self, text: &str, n: usize) -> Vec<String> {
        self.predict_with_conversion(text, n, None)
    }

    pub(crate) fn predict_with_conversion(&self, text: &str, n: usize, best: Option<&Conversion>) -> Vec<String> {
        let mut out = self.predict(text, n);
        if out.len() >= n {
            return out;
        }
        let computed;
        let best = if best.is_some() { best } else {
            computed = self.convert(text, &[], 1);
            computed.first()
        };
        if let Some(conv) = best {
            if conv.segments.len() >= 2 {
                let (last, head) = conv.segments.split_last().unwrap();
                let head: String = head.iter().map(|s| s.surface.as_str()).collect();
                if last.reading.chars().count() >= 2 {
                    for p in self.predict(&last.reading, n) {
                        let full = format!("{head}{p}");
                        if !out.contains(&full) {
                            out.push(full);
                        }
                    }
                }
            }
        }
        out.truncate(n);
        out
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
        // Learned surfaces for this exact reading come first (most recent first).
        if self.learning_enabled() {
            for e in self.history().lookup(reading) {
                add(e.surface.clone(), &mut out);
            }
        }
        let mut lat = self.build_lattice(reading, &[], &[]);
        self.viterbi(&mut lat);
        for (_, path) in self.nbest_paths(&lat, n * 2) {
            // Drop paths that fall back to single unknown characters
            // (e.g. "き者"); the plain hiragana candidate covers them.
            if path.iter().any(|&i| lat.nodes[i as usize].wcost == UNKNOWN_CHAR_COST) {
                continue;
            }
            let s: String = path.iter().map(|&i| lat.nodes[i as usize].surface).collect();
            add(s, &mut out);
            if out.len() >= n {
                break;
            }
        }
        add(reading.to_string(), &mut out);
        add(crate::romaji::to_katakana(reading), &mut out);
        out
    }
}
