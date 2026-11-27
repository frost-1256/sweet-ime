//! Lattice construction, Viterbi and N-best search, and bunsetsu segmentation.

use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet};

use crate::dict::PosClass;

use super::extras::{Extra, Origin};
use super::{Conversion, Converter, Segment, Word};

const NONE: u32 = u32::MAX;

thread_local! {
    static VITERBI_SCRATCH: RefCell<ViterbiScratch> = RefCell::new(ViterbiScratch::new(0));
}

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
    /// Intrusive singly-linked lists of node indices ending/starting at each byte position
    /// (stored in reverse insertion order).
    end_head: Vec<u32>,
    end_next: Vec<u32>,
    begin_head: Vec<u32>,
    begin_next: Vec<u32>,
}

impl Lattice<'_> {
    fn path_surface(&self, path: &[u32]) -> String {
        let cap: usize = path.iter().map(|&i| self.nodes[i as usize].surface.len()).sum();
        let mut out = String::with_capacity(cap);
        for &i in path {
            out.push_str(self.nodes[i as usize].surface);
        }
        out
    }

    #[inline]
    fn ends_at(&self, pos: usize) -> ListIter<'_> {
        ListIter {
            cur: self.end_head[pos],
            next: &self.end_next,
        }
    }
}

struct ListIter<'a> {
    cur: u32,
    next: &'a [u32],
}

impl Iterator for ListIter<'_> {
    type Item = u32;

    #[inline]
    fn next(&mut self) -> Option<u32> {
        if self.cur == NONE {
            return None;
        }
        let id = self.cur;
        self.cur = self.next[id as usize];
        Some(id)
    }
}

/// Compact, reusable lattice for cost-only Viterbi scoring (used by typo correction).
#[derive(Clone, Copy)]
struct CostNode {
    lid: u16,
    rid: u16,
    wcost: i32,
    best: i32,
}

#[derive(Default)]
struct CostLattice {
    nodes: Vec<CostNode>,
    end_head: Vec<u32>,
    end_next: Vec<u32>,
    begin_head: Vec<u32>,
    begin_next: Vec<u32>,
}

impl CostLattice {
    fn reset(&mut self, len: usize) {
        self.nodes.clear();
        self.end_next.clear();
        self.begin_next.clear();
        self.end_head.clear();
        self.end_head.resize(len + 1, NONE);
        self.begin_head.clear();
        self.begin_head.resize(len + 1, NONE);
    }

    #[inline]
    fn push(&mut self, start: usize, end: usize, lid: u16, rid: u16, wcost: i32) {
        let id = self.nodes.len() as u32;
        self.nodes.push(CostNode {
            lid,
            rid,
            wcost,
            best: i32::MAX,
        });
        self.end_next.push(self.end_head[end]);
        self.end_head[end] = id;
        self.begin_next.push(self.begin_head[start]);
        self.begin_head[start] = id;
    }
}

const BOS: u32 = 0;
const EOS: u32 = 1;

struct ViterbiScratch {
    incoming: Vec<(usize, i32)>,
    outgoing: Vec<(usize, usize)>,
    predecessors: Vec<(u16, i32)>,
    successors: Vec<u16>,
    next_tag: usize,
}

impl ViterbiScratch {
    fn new(pos_count: usize) -> Self {
        Self {
            incoming: vec![(usize::MAX, i32::MAX); pos_count],
            outgoing: vec![(usize::MAX, 0); pos_count],
            predecessors: Vec::new(),
            successors: Vec::new(),
            next_tag: 0,
        }
    }

    fn prepare(&mut self, pos_count: usize, span: usize) -> usize {
        if self.incoming.len() < pos_count {
            self.incoming.resize(pos_count, (usize::MAX, i32::MAX));
            self.outgoing.resize(pos_count, (usize::MAX, 0));
        }
        if self.next_tag.checked_add(span).is_none_or(|end| end == usize::MAX) {
            self.incoming.fill((usize::MAX, i32::MAX));
            self.outgoing.fill((usize::MAX, 0));
            self.next_tag = 0;
        }
        let first_tag = self.next_tag;
        self.next_tag += span;
        first_tag
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

/// Discourages splitting a word at BOS into a bare 1-char particle plus the
/// rest (e.g. はいか -> は|以下 instead of 配下). When the user actually meant
/// a leading particle after committed text, it remains in the N-best pool for
/// the neural reranker to pick using left context.
/// Auxiliary verbs and adjectives (the くる of 持ってくる) follow another word,
/// but Mozc connects them to the sentence start more cheaply than the main
/// verb, so くるじゃん stayed in kana instead of 来るじゃん.
fn bos_auxiliary_penalty(feature: Option<&str>) -> i32 {
    if feature.is_some_and(|f| f.starts_with("動詞,非自立,") || f.starts_with("形容詞,非自立,"))
    {
        2_000
    } else {
        0
    }
}

fn bos_particle_penalty(
    pos: usize,
    reading: &str,
    text: &str,
    surface: &str,
    class: PosClass,
) -> i32 {
    if pos == 0
        && reading.len() < text.len()
        && class == PosClass::Functional
        && surface == reading
        && matches!(surface, "は" | "が" | "を" | "に" | "の" | "へ" | "で" | "も")
    {
        2_000
    } else {
        0
    }
}

/// Prefer common homophones where the surrounding reading already supplies
/// unambiguous collocation context.
fn context_bonus(before: &str, reading: &str, after: &str, surface: &str) -> i32 {
    if surface == "円"
        && reading == "えん"
        && before.ends_with(|ch: char| ch.is_ascii_digit() || ('０'..='９').contains(&ch))
    {
        1200
    } else if surface.starts_with('降')
        && reading.starts_with('ふ')
        && (before.ends_with("あめが") || before.ends_with("ゆきが"))
    {
        1200
    } else if (surface.starts_with('止') || surface.starts_with("やん") || surface.starts_with("やむ"))
        && reading.starts_with('や')
        && (before.ends_with("あめが") || before.ends_with("ゆきが") || before.ends_with("かぜが"))
    {
        1200
    } else if surface.starts_with('撮')
        && reading.starts_with('と')
        && (before.ends_with("しゃしんを")
            || before.ends_with("どうがを")
            || before.ends_with("えいがを"))
    {
        1200
    } else if surface == "髪"
        && reading == "かみ"
        && (after.starts_with("をき")
            || after.starts_with("がのび")
            || after.starts_with("をあら")
            || after.starts_with("をそめ")
            || after.starts_with("をかわか"))
    {
        1200
    } else if surface == "風邪"
        && reading == "かぜ"
        && (after.starts_with("をひ")
            || after.starts_with("がなお")
            || after.starts_with("ぐすり"))
    {
        1200
    } else if surface == "箸"
        && reading == "はし"
        && (after.starts_with("でたべ")
            || after.starts_with("をつか")
            || after.starts_with("をもつ")
            || after.starts_with("をおく"))
    {
        1200
    } else if surface.starts_with('渡')
        && reading.starts_with("わた")
        && before.ends_with("はしを")
    {
        1200
    } else if surface.starts_with('着')
        && (reading.starts_with("つく") || reading.starts_with("つい"))
        && (before.ends_with("えきに")
            || before.ends_with("なんじに")
            || before.ends_with("くうこうに")
            || before.ends_with("いえに")
            || before.ends_with("ほてるに"))
    {
        1200
    } else if (surface.starts_with('入') || surface.starts_with('淹'))
        && reading.starts_with("いれ")
        && (before.ends_with("こーひーを")
            || before.ends_with("おちゃを")
            || before.ends_with("こうちゃを"))
    {
        1200
    } else if surface == "機械"
        && reading == "きかい"
        && (after.starts_with("がこしょう")
            || after.starts_with("をしゅうり")
            || after.starts_with("をそうさ"))
    {
        1200
    } else if surface == "満車"
        && reading == "まんしゃ"
        && before.ends_with("ちゅうしゃじょうが")
    {
        1500
    } else if surface == "口座"
        && reading == "こうざ"
        && (after.starts_with("をかいせつ")
            || after.starts_with("にふりこ")
            || after.starts_with("からひきお"))
    {
        1200
    } else if surface == "開設"
        && reading == "かいせつ"
        && before.ends_with("こうざを")
    {
        1200
    } else if surface == "漢字"
        && reading == "かんじ"
        && (after.starts_with("のよみ")
            || after.starts_with("をけんさく")
            || after.starts_with("をかく")
            || after.starts_with("をおぼ")
            || before.contains("ひらがな")
            || before.contains("かたかな")
            || before.contains("ろーまじ")
            || before.contains("かなに")
            || before.contains("かなを")
            || before.contains("かなと"))
    {
        1500
    } else if surface == "感じ"
        && reading == "かんじ"
        && (before.ends_with("いい")
            || before.ends_with("よい")
            || before.ends_with("こんな")
            || before.ends_with("そんな")
            || before.ends_with("あんな")
            || before.ends_with("どんな")
            || before.ends_with("おなじ")
            || before.ends_with("へんな")
            || before.ends_with("いやな")
            || before.ends_with("すてきな"))
    {
        1200
    } else if surface == "ジム"
        && reading == "じむ"
        && (after.starts_with("にかよ") || after.starts_with("にいく"))
    {
        1200
    } else if surface == "仕様"
        && reading == "しよう"
        && (after.starts_with("をきめ")
            || after.starts_with("をかえ")
            || after.starts_with("をへんこう")
            || after.starts_with("しょ"))
    {
        1200
    } else if surface == "地震"
        && reading == "じしん"
        && (after.starts_with("があっ")
            || after.starts_with("がおき")
            || after.starts_with("がくる")
            || after.starts_with("そくほう"))
    {
        1200
    } else if surface == "異常"
        && reading == "いじょう"
        && (after.starts_with("がみつ")
            || after.starts_with("がはっせい")
            || after.starts_with("がない")
            || after.starts_with("がある"))
    {
        1500
    } else {
        0
    }
}

impl Converter {
    /// Use punctuation's dictionary connections while preserving the typed glyph.
    fn punctuation_cost(&self, literal: &str) -> Option<(u16, u16, i32)> {
        let key = match literal {
            "！" => "!",
            "？" => "?",
            "、" | "。" | "「" | "」" | "!" | "?" => literal,
            _ => return None,
        };
        self.dict
            .lookup(key)
            .into_iter()
            .filter(|entry| entry.surface == key)
            .min_by_key(|entry| entry.cost)
            .map(|entry| (entry.lid, entry.rid, i32::from(entry.cost)))
    }

    fn build_lattice<'a>(
        &'a self,
        text: &'a str,
        boundaries: &[usize],
        extras: &'a [Extra],
    ) -> Lattice<'a> {
        let len = text.len();
        let cap = len * 8;
        let mut lat = Lattice {
            nodes: Vec::with_capacity(cap),
            end_head: vec![NONE; len + 1],
            end_next: Vec::with_capacity(cap),
            begin_head: vec![NONE; len + 1],
            begin_next: Vec::with_capacity(cap),
        };
        let push = |lat: &mut Lattice<'a>, n: Node<'a>| {
            let id = lat.nodes.len() as u32;
            lat.end_next.push(lat.end_head[n.end]);
            lat.end_head[n.end] = id;
            lat.begin_next.push(lat.begin_head[n.start]);
            lat.begin_head[n.start] = id;
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
        lat.end_next.push(NONE);
        lat.end_head[0] = BOS;
        lat.begin_next.push(NONE);
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
        lat.end_next.push(NONE);
        lat.begin_next.push(NONE);

        let crosses = |s: usize, e: usize| boundaries.iter().any(|&b| s < b && b < e);
        let unk = self.dict.unknown_id();

        let mut seen_counters: Vec<(usize, &'static str)> = Vec::new();
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
                let num_id = self
                    .number_pos
                    .filter(|_| {
                        text[pos..e]
                            .chars()
                            .all(|ch| ch.is_ascii_digit() || ('０'..='９').contains(&ch))
                    })
                    .map_or(unk, |(_, arabic_id)| arabic_id);
                let (lid, rid, wcost) = self.punctuation_cost(&text[pos..e]).unwrap_or((
                    num_id,
                    num_id,
                    UNKNOWN_RUN_COST,
                ));
                push(
                    &mut lat,
                    Node {
                        start: pos,
                        end: e,
                        lid,
                        rid,
                        wcost,
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
            seen_counters.clear();
            let emit_arabic_counters = self.number_pos.is_some();
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
                            - context_bonus(&text[..pos], &rest[..l], &rest[l..], e.surface)
                            + standalone_penalty(text, e.surface)
                            + bos_particle_penalty(
                                pos,
                                &rest[..l],
                                text,
                                e.surface,
                                self.dict.pos_class(e.lid),
                            )
                            + if pos == 0 {
                                bos_auxiliary_penalty(self.dict.feature(e.lid))
                            } else {
                                0
                            },
                        surface: e.surface,
                        best: i32::MAX,
                        origin: None,
                    },
                );
                if emit_arabic_counters
                    && let Some(arabic) = Self::arabic_counter_variant(e.surface)
                {
                    let end = pos + l;
                    if !seen_counters.contains(&(end, arabic)) {
                        seen_counters.push((end, arabic));
                        push(
                            &mut lat,
                            Node {
                                start: pos,
                                end,
                                lid: e.lid,
                                rid: e.rid,
                                wcost: i32::from(e.cost) + super::extras::NUMBER_ARABIC_EXTRA,
                                surface: arabic,
                                best: i32::MAX,
                                origin: Some(Origin::Number),
                            },
                        );
                    }
                }
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
        lat.begin_next[EOS as usize] = lat.begin_head[len];
        lat.begin_head[len] = EOS;
        lat
    }

    fn viterbi(&self, lat: &mut Lattice<'_>) {
        VITERBI_SCRATCH.with(|cell| {
            let mut scratch = cell.borrow_mut();
            self.viterbi_with_scratch(lat, &mut scratch);
        });
    }

    fn viterbi_with_scratch(&self, lat: &mut Lattice<'_>, scratch: &mut ViterbiScratch) {
        let len = lat.end_head.len() - 1;
        // Alternative spellings with the same left POS share their incoming
        // transition cost at this position. Tags span lattices so typo scoring
        // and repeated calls reuse the POS tables without clearing them.
        let first_tag = scratch.prepare(self.dict.pos_count(), lat.end_head.len());
        let ViterbiScratch {
            incoming,
            outgoing,
            predecessors,
            successors,
            ..
        } = scratch;
        for pos in 0..=len {
            if lat.begin_head[pos] == NONE || lat.end_head[pos] == NONE {
                continue;
            }
            let tag = first_tag + pos;
            predecessors.clear();
            let mut id = lat.end_head[pos];
            while id != NONE {
                let prev = &lat.nodes[id as usize];
                if prev.best != i32::MAX {
                    let entry = &mut outgoing[prev.rid as usize];
                    if entry.0 == tag {
                        let best = &mut predecessors[entry.1].1;
                        *best = (*best).min(prev.best);
                    } else {
                        *entry = (tag, predecessors.len());
                        predecessors.push((prev.rid, prev.best));
                    }
                }
                id = lat.end_next[id as usize];
            }
            if predecessors.is_empty() {
                continue;
            }
            // Collect unique successor left-POS ids so we can stream each
            // predecessor's contiguous connection-matrix row once.
            successors.clear();
            let mut id = lat.begin_head[pos];
            while id != NONE {
                let lid = lat.nodes[id as usize].lid;
                let entry = &mut incoming[lid as usize];
                if entry.0 != tag {
                    *entry = (tag, i32::MAX);
                    successors.push(lid);
                }
                id = lat.begin_next[id as usize];
            }
            for &(rid, cost) in predecessors.iter() {
                if let Some(row) = self.dict.connection_row_le(rid) {
                    for &lid in successors.iter() {
                        let c = cost + i32::from(row[lid as usize]);
                        let best = &mut incoming[lid as usize].1;
                        if c < *best {
                            *best = c;
                        }
                    }
                } else {
                    for &lid in successors.iter() {
                        let c = cost + self.dict.connection(rid, lid);
                        let best = &mut incoming[lid as usize].1;
                        if c < *best {
                            *best = c;
                        }
                    }
                }
            }
            let mut id = lat.begin_head[pos];
            while id != NONE {
                let node = &mut lat.nodes[id as usize];
                let best = incoming[node.lid as usize].1;
                node.best = if best == i32::MAX {
                    i32::MAX
                } else {
                    best + node.wcost
                };
                id = lat.begin_next[id as usize];
            }
        }
    }

    fn build_and_score_cost_lattice(
        &self,
        text: &str,
        extras: &[Extra],
        lat: &mut CostLattice,
        scratch: &mut ViterbiScratch,
    ) -> i32 {
        let len = text.len();
        lat.reset(len);
        // BOS (0) and EOS (1)
        lat.nodes.push(CostNode {
            lid: 0,
            rid: 0,
            wcost: 0,
            best: 0,
        });
        lat.end_next.push(NONE);
        lat.end_head[0] = BOS;
        lat.begin_next.push(NONE);
        lat.nodes.push(CostNode {
            lid: 0,
            rid: 0,
            wcost: 0,
            best: i32::MAX,
        });
        lat.end_next.push(NONE);
        lat.begin_next.push(NONE);

        let unk = self.dict.unknown_id();
        let mut pos = 0;
        while pos < len {
            let rest = &text[pos..];
            let c = rest.chars().next().unwrap();
            if !is_kana(c) {
                let run: usize = rest
                    .char_indices()
                    .find(|&(_, ch)| is_kana(ch))
                    .map(|(i, _)| i)
                    .unwrap_or(rest.len());
                let e = pos + run;
                let (lid, rid, cost) =
                    self.punctuation_cost(&text[pos..e])
                        .unwrap_or((unk, unk, UNKNOWN_RUN_COST));
                lat.push(pos, e, lid, rid, cost);
                pos = e;
                continue;
            }
            self.dict.common_prefix_costs(rest, |l, lid, rid, cost| {
                lat.push(pos, pos + l, lid, rid, i32::from(cost));
            });
            let cl = c.len_utf8();
            lat.push(pos, pos + cl, unk, unk, UNKNOWN_CHAR_COST);
            pos += cl;
        }
        for x in extras {
            lat.push(
                x.start,
                x.end,
                x.lid,
                x.rid,
                x.cost
                    + if x.origin == Origin::Learned {
                        standalone_penalty(text, &x.surface)
                    } else {
                        0
                    },
            );
        }
        lat.begin_next[EOS as usize] = lat.begin_head[len];
        lat.begin_head[len] = EOS;

        let first_tag = scratch.prepare(self.dict.pos_count(), len + 1);
        let ViterbiScratch {
            incoming,
            outgoing,
            predecessors,
            successors,
            ..
        } = scratch;
        for pos in 0..=len {
            if lat.begin_head[pos] == NONE || lat.end_head[pos] == NONE {
                continue;
            }
            let tag = first_tag + pos;
            predecessors.clear();
            let mut id = lat.end_head[pos];
            while id != NONE {
                let prev = &lat.nodes[id as usize];
                if prev.best != i32::MAX {
                    let entry = &mut outgoing[prev.rid as usize];
                    if entry.0 == tag {
                        let best = &mut predecessors[entry.1].1;
                        *best = (*best).min(prev.best);
                    } else {
                        *entry = (tag, predecessors.len());
                        predecessors.push((prev.rid, prev.best));
                    }
                }
                id = lat.end_next[id as usize];
            }
            if predecessors.is_empty() {
                continue;
            }
            successors.clear();
            let mut id = lat.begin_head[pos];
            while id != NONE {
                let lid = lat.nodes[id as usize].lid;
                let entry = &mut incoming[lid as usize];
                if entry.0 != tag {
                    *entry = (tag, i32::MAX);
                    successors.push(lid);
                }
                id = lat.begin_next[id as usize];
            }
            for &(rid, cost) in predecessors.iter() {
                if let Some(row) = self.dict.connection_row_le(rid) {
                    for &lid in successors.iter() {
                        let c = cost + i32::from(row[lid as usize]);
                        let best = &mut incoming[lid as usize].1;
                        if c < *best {
                            *best = c;
                        }
                    }
                } else {
                    for &lid in successors.iter() {
                        let c = cost + self.dict.connection(rid, lid);
                        let best = &mut incoming[lid as usize].1;
                        if c < *best {
                            *best = c;
                        }
                    }
                }
            }
            let mut id = lat.begin_head[pos];
            while id != NONE {
                let node = &mut lat.nodes[id as usize];
                let best = incoming[node.lid as usize].1;
                node.best = if best == i32::MAX {
                    i32::MAX
                } else {
                    best + node.wcost
                };
                id = lat.begin_next[id as usize];
            }
        }
        lat.nodes[EOS as usize].best
    }

    /// The best path can be traced from the saved Viterbi costs. `ends_at`
    /// already yields predecessors in reverse insertion order to match A* on ties.
    fn best_path(&self, lat: &Lattice<'_>) -> Option<Vec<u32>> {
        let mut path = Vec::new();
        let mut current = EOS;
        while current != BOS {
            let node = &lat.nodes[current as usize];
            let next = lat.ends_at(node.start).find(|&id| {
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
        let cap = n.saturating_mul(4).clamp(64, 4096);
        let mut arena: Vec<(u32, u32, i32)> = Vec::with_capacity(cap);
        arena.push((EOS, u32::MAX, 0));
        let mut heap = BinaryHeap::with_capacity(cap);
        heap.push((Reverse(lat.nodes[EOS as usize].best), Reverse(0u32)));
        let mut pops = 0;
        std::iter::from_fn(move || {
            while pops < 20_000 {
                let (Reverse(f), Reverse(ai)) = heap.pop()?;
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
                for p in lat.ends_at(nd.start) {
                    let prev = &lat.nodes[p as usize];
                    if prev.best == i32::MAX {
                        continue;
                    }
                    let g3 = g2 + self.dict.connection(prev.rid, nd.lid);
                    arena.push((p, ai, g3));
                    heap.push((Reverse(prev.best + g3), Reverse((arena.len() - 1) as u32)));
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
                    PosClass::Content => {
                        let is_suru = matches!(
                            w.surface.as_str(),
                            "し" | "す" | "する" | "すれ" | "しろ" | "せよ" | "さ" | "せ"
                        );
                        prev_class != PosClass::Prefix && !(is_suru && prev_class == PosClass::Noun)
                    }
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

    /// Returns kana byte ranges `(start, end)` on the Viterbi best path where
    /// `connection(prev, node) + node.wcost >= min_step_cost`, indicating a
    /// likely typo-damaged region.
    pub(crate) fn suspicious_kana_spans(&self, text: &str, min_step_cost: i32) -> Vec<(usize, usize)> {
        if text.is_empty() {
            return Vec::new();
        }
        let extras = self.extras(text);
        let mut lat = self.build_lattice(text, &[], &extras);
        self.viterbi(&mut lat);
        let Some(path) = self.best_path(&lat) else {
            return Vec::new();
        };
        let mut spans = Vec::new();
        let mut prev_rid = lat.nodes[BOS as usize].rid;
        let mut prev_start = 0usize;
        for &id in &path {
            let node = &lat.nodes[id as usize];
            let step = self.dict.connection(prev_rid, node.lid) + node.wcost;
            if step >= min_step_cost {
                spans.push((prev_start, node.end));
            }
            prev_rid = node.rid;
            prev_start = node.start;
        }
        let eos_step = self.dict.connection(prev_rid, lat.nodes[EOS as usize].lid);
        if eos_step >= min_step_cost {
            spans.push((prev_start, text.len()));
        }
        spans
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
        let mut lat = CostLattice::default();
        VITERBI_SCRATCH.with(|cell| {
            let mut scratch = cell.borrow_mut();
            evaluate(&mut |text| {
                let extras = history
                    .as_ref()
                    .map_or_else(Vec::new, |h| self.learned_extras_from(text, h));
                self.build_and_score_cost_lattice(text, &extras, &mut lat, &mut scratch)
            })
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
        let initial_cap = n.min(MAX_HOMOPHONE_CANDIDATES);
        let mut seen = HashSet::with_capacity(initial_cap);
        let mut res = Vec::with_capacity(initial_cap);
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
        if reading == surface
            || !self.learning_enabled()
            || crate::special::is_ephemeral_candidate(reading, surface)
        {
            return; // plain hiragana or ephemeral date/time/calc needs no learning
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
        add(crate::romaji::to_katakana(reading), &mut out);
        if !reading.is_empty() {
            let target = 6.min(out.len());
            match out.iter().position(|s| s == reading) {
                Some(idx) if idx >= 9 => {
                    let plain = out.remove(idx);
                    out.insert(target, plain);
                }
                None => out.insert(target, reading.to_string()),
                _ => {}
            }
        }
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
