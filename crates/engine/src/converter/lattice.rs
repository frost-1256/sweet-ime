//! Lattice construction, Viterbi and N-best search, and bunsetsu segmentation.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet};

use crate::dict::PosClass;

use super::extras::{Extra, Origin};
use super::{Conversion, Converter, Segment, Word};

/// Cost of a single-character unknown word (hiragana passthrough).
const UNKNOWN_CHAR_COST: i32 = 12_000;
/// Cost of a run of non-kana characters (ASCII, digits, symbols).
const UNKNOWN_RUN_COST: i32 = 4_000;
const MAX_HOMOPHONE_CANDIDATES: usize = 240;

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
    origin: Option<Origin>,
}

struct Lattice<'a> {
    nodes: Vec<Node<'a>>,
    /// Node indices ending at each byte position.
    ends: Vec<Vec<u32>>,
    /// Node indices starting at each byte position.
    begins: Vec<Vec<u32>>,
}

impl Lattice<'_> {
    fn path_surface(&self, path: &[u32]) -> String {
        path.iter()
            .map(|&i| self.nodes[i as usize].surface)
            .collect()
    }
}

const BOS: u32 = 0;
const EOS: u32 = 1;

struct ViterbiScratch {
    incoming: Vec<(usize, i32)>,
    outgoing: Vec<(usize, usize)>,
    predecessors: Vec<(u16, i32)>,
    next_tag: usize,
}

impl ViterbiScratch {
    fn new(pos_count: usize) -> Self {
        Self {
            incoming: vec![(usize::MAX, i32::MAX); pos_count],
            outgoing: vec![(usize::MAX, 0); pos_count],
            predecessors: Vec::new(),
            next_tag: 0,
        }
    }
}

fn is_kana(c: char) -> bool {
    matches!(c, 'ぁ'..='ゖ' | 'ァ'..='ヺ' | 'ー' | 'ゝ' | 'ゞ' | 'ｦ'..='ﾟ')
}

fn standalone_penalty(input: &str, surface: &str) -> i32 {
    // A single typed し is usually a fragment. Learning should still be
    // able to prefer 死 in longer readings, where there is more context.
    if input == "し" && surface == "死" { 4_000 } else { 0 }
}

/// Prefer common homophones where the reading already supplies context.
fn context_bonus(before: &str, reading: &str, surface: &str) -> i32 {
    if surface == "円"
        && reading == "えん"
        && before.ends_with(|ch: char| ch.is_ascii_digit() || ('０'..='９').contains(&ch))
    {
        1200
    } else if surface.starts_with('降')
        && reading.starts_with('ふ')
        && (before.ends_with("あめが") || before.ends_with("ゆきが"))
    {
        200
    } else {
        0
    }
}

impl Converter {
    fn build_lattice<'a>(
        &'a self,
        text: &'a str,
        boundaries: &[usize],
        extras: &'a [Extra],
    ) -> Lattice<'a> {
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
        lat.nodes.push(Node {
            start: 0,
            end: 0,
            lid: 0,
            rid: 0,
            wcost: 0,
            surface: "",
            best: 0,
            origin: None,
        });
        lat.ends[0].push(BOS);
        lat.nodes.push(Node {
            start: len,
            end: len,
            lid: 0,
            rid: 0,
            wcost: 0,
            surface: "",
            best: i32::MAX,
            origin: None,
        });

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
                push(
                    &mut lat,
                    Node {
                        start: pos,
                        end: e,
                        lid: unk,
                        rid: unk,
                        wcost: UNKNOWN_RUN_COST,
                        surface: &text[pos..e],
                        best: i32::MAX,
                        origin: None,
                    },
                );
                // Keep the literal run as a fallback, but also allow a
                // recognized spelling to connect to punctuation inside it
                // (the c and 、 of "anthropic," share a non-kana run).
                pos = extras
                    .iter()
                    .filter(|x| {
                        x.origin == Origin::Spelling
                            && pos < x.end
                            && x.end < e
                            && !crosses(x.start, x.end)
                            && text[x.end..].starts_with([
                                '、', '。', '！', '？', '「', '」', '・', '…', '～', ',', '.', '!',
                                '?', ':', ';', '(', ')', '（', '）',
                            ])
                    })
                    .map(|x| x.end)
                    .min()
                    .unwrap_or(e);
                continue;
            }
            self.dict.common_prefix(rest, |l, e| {
                if crosses(pos, pos + l) {
                    return;
                }
                push(
                    &mut lat,
                    Node {
                        start: pos,
                        end: pos + l,
                        lid: e.lid,
                        rid: e.rid,
                        wcost: i32::from(e.cost)
                            - context_bonus(&text[..pos], &rest[..l], e.surface)
                            + standalone_penalty(text, e.surface),
                        surface: e.surface,
                        best: i32::MAX,
                        origin: None,
                    },
                );
            });
            // Always allow a single-character fallback to keep the lattice connected.
            let cl = c.len_utf8();
            push(
                &mut lat,
                Node {
                    start: pos,
                    end: pos + cl,
                    lid: unk,
                    rid: unk,
                    wcost: UNKNOWN_CHAR_COST,
                    surface: &text[pos..pos + cl],
                    best: i32::MAX,
                    origin: None,
                },
            );
            pos += cl;
        }
        for x in extras {
            if !crosses(x.start, x.end) {
                push(
                    &mut lat,
                    Node {
                        start: x.start,
                        end: x.end,
                        lid: x.lid,
                        rid: x.rid,
                        wcost: x.cost + if x.origin == Origin::Learned {
                            standalone_penalty(text, &x.surface)
                        } else {
                            0
                        },
                        surface: &x.surface,
                        best: i32::MAX,
                        origin: Some(x.origin),
                    },
                );
            }
        }
        lat.begins[len].push(EOS);
        lat
    }

    fn viterbi(&self, lat: &mut Lattice<'_>) {
        let mut scratch = ViterbiScratch::new(self.dict.pos_count());
        self.viterbi_with_scratch(lat, &mut scratch);
    }

    fn viterbi_with_scratch(&self, lat: &mut Lattice<'_>, scratch: &mut ViterbiScratch) {
        let len = lat.ends.len() - 1;
        // Alternative spellings with the same left POS share their incoming
        // transition cost at this position. Tags span lattices so typo scoring
        // can reuse the POS tables without clearing them for each variant.
        let first_tag = scratch.next_tag;
        scratch.next_tag += lat.ends.len();
        let ViterbiScratch {
            incoming,
            outgoing,
            predecessors,
            ..
        } = scratch;
        for pos in 0..=len {
            if lat.begins[pos].is_empty() || lat.ends[pos].is_empty() {
                continue;
            }
            let tag = first_tag + pos;
            predecessors.clear();
            for &id in &lat.ends[pos] {
                let prev = &lat.nodes[id as usize];
                if prev.best == i32::MAX {
                    continue;
                }
                let entry = &mut outgoing[prev.rid as usize];
                if entry.0 == tag {
                    let best = &mut predecessors[entry.1].1;
                    *best = (*best).min(prev.best);
                } else {
                    *entry = (tag, predecessors.len());
                    predecessors.push((prev.rid, prev.best));
                }
            }
            for bi in 0..lat.begins[pos].len() {
                let id = lat.begins[pos][bi] as usize;
                let (lid, wcost) = (lat.nodes[id].lid, lat.nodes[id].wcost);
                let entry = &mut incoming[lid as usize];
                if entry.0 != tag {
                    let mut best = i32::MAX;
                    for &(rid, cost) in predecessors.iter() {
                        best = best.min(cost + self.dict.connection(rid, lid));
                    }
                    *entry = (tag, best);
                }
                let best = entry.1;
                lat.nodes[id].best = if best == i32::MAX {
                    i32::MAX
                } else {
                    best + wcost
                };
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
                    && prev.best + self.dict.connection(prev.rid, node.lid)
                        == node.best - node.wcost
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
    fn nbest_paths<'a>(
        &'a self,
        lat: &'a Lattice<'_>,
        n: usize,
    ) -> impl Iterator<Item = (i32, Vec<u32>)> + 'a {
        // arena entries: (node, parent arena index, g = cost after node)
        let mut arena: Vec<(u32, u32, i32)> = vec![(EOS, u32::MAX, 0)];
        let mut heap = BinaryHeap::new();
        heap.push((Reverse(lat.nodes[EOS as usize].best), 0u32));
        let mut pops = 0;
        std::iter::from_fn(move || {
            while pops < 20_000 {
                let (Reverse(f), ai) = heap.pop()?;
                pops += 1;
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
                    return Some((f, path));
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
            None
        })
        .take(n)
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
                    learned: n.origin == Some(Origin::Learned),
                    user: n.origin == Some(Origin::User),
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
                segs.push(Segment {
                    reading: String::new(),
                    surface: String::new(),
                    words: Vec::new(),
                });
            }
            let s = segs.last_mut().unwrap();
            s.reading.push_str(&w.reading);
            s.surface.push_str(&w.surface);
            s.words.push(w);
        }
        segs
    }

    /// Scores typo variants against the same history, checking its file once.
    pub(crate) fn score_typos<T>(
        &self,
        evaluate: impl FnOnce(&mut dyn FnMut(&str) -> i32) -> T,
    ) -> T {
        let mut history = self.learning_enabled().then(|| self.history());
        if let Some(h) = history.as_mut() {
            h.reload_if_changed();
        }
        let mut scratch = ViterbiScratch::new(self.dict.pos_count());
        evaluate(&mut |text| {
            let extras = history
                .as_ref()
                .map_or_else(Vec::new, |h| self.learned_extras_from(text, h));
            let mut lat = self.build_lattice(text, &[], &extras);
            self.viterbi_with_scratch(&mut lat, &mut scratch);
            lat.nodes[EOS as usize].best
        })
    }

    /// Convert `text` (hiragana) into up to `n` whole-sentence candidates.
    /// `boundaries` are byte offsets that segments must break at (user-resized
    /// bunsetsu); pass `&[]` for automatic segmentation.
    pub fn convert(&self, text: &str, boundaries: &[usize], n: usize) -> Vec<Conversion> {
        self.convert_pool(text, boundaries, n, false)
    }

    pub(super) fn convert_pool(
        &self,
        text: &str,
        boundaries: &[usize],
        n: usize,
        expand_homophones: bool,
    ) -> Vec<Conversion> {
        if text.is_empty() {
            return Vec::new();
        }
        let extras = self.extras(text);
        let mut lat = self.build_lattice(text, boundaries, &extras);
        self.viterbi(&mut lat);
        if n == 1 {
            let Some(path) = self.best_path(&lat) else {
                return Vec::new();
            };
            let segments = self.segment(self.to_words(&lat, text, &path), boundaries);
            return vec![Conversion {
                segments,
                cost: lat.nodes[EOS as usize].best,
            }];
        }
        let mut seen = HashSet::new();
        let mut res = Vec::new();
        let mut limit = n;
        let maximum = if expand_homophones {
            n.max(MAX_HOMOPHONE_CANDIDATES)
        } else {
            n
        };
        for (index, (cost, path)) in self.nbest_paths(&lat, maximum * 3).enumerate() {
            if index >= limit * 3 {
                break;
            }
            let surface: String = lat.path_surface(&path);
            if seen.insert(surface) {
                let segments = self.segment(self.to_words(&lat, text, &path), boundaries);
                let candidate = Conversion { segments, cost };
                if expand_homophones
                    && res.is_empty()
                    && let Some((_, occurrences)) = candidate.repeated_kanji_reading()
                {
                    // Several occurrences can require several simultaneous replacements.
                    limit = limit.max(if occurrences >= 4 {
                        MAX_HOMOPHONE_CANDIDATES
                    } else {
                        64
                    });
                }
                res.push(candidate);
                if res.len() >= limit {
                    break;
                }
            }
        }
        res
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
            let s: String = lat.path_surface(&path);
            if s == surface && !path.is_empty() {
                lid = lat.nodes[path[0] as usize].lid;
                rid = lat.nodes[*path.last().unwrap() as usize].rid;
                found = true;
                break;
            }
        }
        if !found {
            if let Some(e) = self
                .dict
                .lookup(reading)
                .into_iter()
                .find(|e| e.surface == surface)
            {
                lid = e.lid;
                rid = e.rid;
            }
        }
        self.history().learn(reading, surface, lid, rid);
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
        // Then user dictionary words for exactly this reading.
        let mut extras = Vec::new();
        self.user_extras(reading, &mut extras);
        for x in extras
            .iter()
            .filter(|x| x.start == 0 && x.end == reading.len())
        {
            add(x.surface.clone(), &mut out);
        }
        // A name written both ways: katakana, then English (エヌビディア, NVIDIA).
        if let Some((katakana, spelling)) = crate::english::katakana_name(reading) {
            add(katakana.to_string(), &mut out);
            add(spelling.to_string(), &mut out);
        }
        self.number_extras(reading, &mut extras);
        self.spelling_extras(reading, &mut extras);
        self.long_vowel_extras(reading, &mut extras);
        let mut lat = self.build_lattice(reading, &[], &extras);
        self.viterbi(&mut lat);
        for (_, path) in self.nbest_paths(&lat, n * 2) {
            // Drop paths that fall back to single unknown characters
            // (e.g. "き者"); the plain hiragana candidate covers them.
            if path
                .iter()
                .any(|&i| lat.nodes[i as usize].wcost == UNKNOWN_CHAR_COST)
            {
                continue;
            }
            let s: String = lat.path_surface(&path);
            add(s, &mut out);
            if out.len() >= n {
                break;
            }
        }
        add(reading.to_string(), &mut out);
        add(crate::romaji::to_katakana(reading), &mut out);
        out
    }

    /// Adds exact dictionary matches that did not make the ranked lattice list.
    pub(crate) fn append_exact_candidates(&self, reading: &str, candidates: &mut Vec<String>) {
        let mut seen: HashSet<_> = candidates.iter().map(String::as_str).collect();
        let missing: Vec<_> = self
            .dict
            .lookup(reading)
            .into_iter()
            .filter(|entry| !entry.surface.is_empty() && seen.insert(entry.surface))
            .map(|entry| entry.surface.to_owned())
            .collect();
        candidates.extend(missing);
    }
}
