//! Native Rust inference for zenz (GPT-2) kana-kanji conversion models,
//! used to rerank and refine the lattice converter's candidates.
//!
//! zenz-v3 prompt format:
//! `[\u{EE02}<left context>]\u{EE00}<reading in katakana>\u{EE01}<output></s>`

pub mod kernels;
pub mod model;
pub mod tokenizer;

use std::collections::HashSet;
use std::io;
use std::path::Path;
use std::sync::OnceLock;

use sweet_engine::dict::{TECH_TERMS, VRCHAT_TERMS};

pub use model::{KvCache, Model};
pub use tokenizer::Tokenizer;

const INPUT_TAG: char = '\u{EE00}';
const OUTPUT_TAG: char = '\u{EE01}';
const CONTEXT_TAG: char = '\u{EE02}';
/// Left context is limited to its last N characters.
pub const MAX_CONTEXT_CHARS: usize = 40;

pub struct Zenz {
    pub tokenizer: Tokenizer,
    pub model: Model,
    /// Prepend BOS to prompts.
    pub add_bos: bool,
}

fn normalize(s: &str) -> String {
    s.replace(' ', "\u{3000}").replace(['\n', '\r'], "")
}

impl Zenz {
    /// Loads either a packed `.swz` file or a Hugging Face model directory.
    pub fn load(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        if path.is_file() {
            let (model, vocab, merges) = Model::load_packed(path)?;
            return Ok(Self { tokenizer: Tokenizer::from_strings(&vocab, &merges)?, model, add_bos: false });
        }
        let dir = path;
        Ok(Self { tokenizer: Tokenizer::load(dir)?, model: Model::load(dir)?, add_bos: false })
    }

    /// Packs a Hugging Face model directory into a single `.swz` file.
    pub fn pack(hf_dir: &Path, out: &Path) -> io::Result<()> {
        let model = Model::load(hf_dir)?;
        let vocab = std::fs::read_to_string(hf_dir.join("vocab.json"))?;
        let merges = std::fs::read_to_string(hf_dir.join("merges.txt"))?;
        model.pack(&vocab, &merges, out)
    }

    /// Builds the v3 conversion prompt (up to and including the output tag).
    pub fn prompt(reading: &str, left_context: &str) -> String {
        let mut p = String::new();
        let ctx = normalize(left_context);
        let n = ctx.chars().count();
        let ctx: String = ctx.chars().skip(n.saturating_sub(MAX_CONTEXT_CHARS)).collect();
        if !ctx.is_empty() {
            p.push(CONTEXT_TAG);
            p.push_str(&ctx);
        }
        p.push(INPUT_TAG);
        p.push_str(&sweet_engine::romaji::to_katakana(&normalize(reading)));
        p.push(OUTPUT_TAG);
        p
    }

    pub fn encode_prompt(&self, prompt: &str) -> Vec<u32> {
        let mut ids = Vec::new();
        if self.add_bos {
            ids.push(self.tokenizer.bos);
        }
        ids.extend(self.tokenizer.encode(prompt));
        ids
    }

    /// Greedy decoding until EOS (for diagnostics and free generation).
    pub fn generate(&self, prompt: &str, max_tokens: usize) -> String {
        let ids = self.encode_prompt(prompt);
        let mut cache = self.model.new_cache();
        let mut logits = self.model.forward(&ids, &mut cache, false);
        let mut out = Vec::new();
        for _ in 0..max_tokens {
            let next = model::argmax(&logits) as u32;
            if next == self.tokenizer.eos {
                break;
            }
            out.push(next);
            logits = self.model.forward(&[next], &mut cache, false);
        }
        self.tokenizer.decode(&out)
    }

    /// Log-probability of each candidate (plus the end-of-sequence token)
    /// given the prompt. The candidates form a token trie that is evaluated
    /// in a single tree-attention pass, with the prompt as the trunk.
    pub fn score(&self, prompt: &str, candidates: &[String]) -> Vec<f32> {
        self.score_candidates(prompt, candidates, false)
    }

    fn score_candidates(&self, prompt: &str, candidates: &[String], relative: bool) -> Vec<f32> {
        let mut ids = self.encode_prompt(prompt);
        let mut cache = self.model.new_cache();
        let toks: Vec<Vec<u32>> = candidates.iter().map(|c| self.tokenizer.encode(&normalize(c))).collect();
        // Shared prefix tokens contribute the same log-probability to every
        // candidate. Keep them in the prompt and skip their vocabulary projections.
        let mut shared = 0;
        if relative
            && !ids.is_empty()
            && let Some(first) = toks.first()
        {
            shared = first
                .iter()
                .enumerate()
                .take_while(|&(i, token)| {
                    toks[1..]
                        .iter()
                        .all(|candidate| candidate.get(i) == Some(token))
                })
                .count();
            ids.extend_from_slice(&first[..shared]);
        }
        // Trie: node -> (token, parent). The prompt is a chain at the front;
        // candidates hang off its last token.
        let p = ids.len();
        let mut nodes: Vec<u32> = ids.clone();
        let mut parents: Vec<Option<usize>> = (0..p).map(|i| i.checked_sub(1)).collect();
        let mut children: std::collections::HashMap<(Option<usize>, u32), usize> = Default::default();
        let mut ends: Vec<Option<usize>> = Vec::with_capacity(toks.len());
        for t in &toks {
            let mut cur: Option<usize> = p.checked_sub(1);
            for &tok in &t[shared..] {
                let next = *children.entry((cur, tok)).or_insert_with(|| {
                    nodes.push(tok);
                    parents.push(cur);
                    nodes.len() - 1
                });
                cur = Some(next);
            }
            ends.push(cur);
        }
        let v = self.model.cfg.vocab_size;
        let eos = self.tokenizer.eos as usize;
        if p == 0 {
            return vec![f32::NEG_INFINITY; toks.len()];
        }
        // Logits are only needed from the last prompt token on.
        let from = p - 1;
        let logits = self.model.forward_tree(&nodes, &parents, &mut cache, from);
        let row = |i: usize| -> &[f32] { &logits[(i - from) * v..(i - from + 1) * v] };
        // A branch can score several child tokens and EOS from the same logits.
        let normalizers: Vec<_> = if logits.len() >= 4 * v {
            use rayon::prelude::*;
            kernels::pool().install(|| {
                logits
                    .par_chunks_exact(v)
                    .map(model::log_normalizer)
                    .collect()
            })
        } else {
            logits.chunks_exact(v).map(model::log_normalizer).collect()
        };
        let log_prob = |i, token| {
            let (mx, log_sum) = normalizers[i - from];
            row(i)[token] - mx - log_sum
        };
        // Cumulative log-prob of the candidate part of the path to each node.
        let mut cum = vec![0f32; nodes.len()];
        for i in p..nodes.len() {
            let par = parents[i].expect("candidate nodes have a parent");
            cum[i] = cum[par] + log_prob(par, nodes[i] as usize);
        }
        ends.iter()
            .map(|&end| {
                let end = end.expect("non-empty prompt");
                cum[end] + log_prob(end, eos)
            })
            .collect()
    }
}

/// [`sweet_engine::Reranker`] backed by a zenz model: scores the lattice's
/// N-best sentences and mixes in the lattice cost.
pub struct ZenzReranker {
    pub zenz: Zenz,
    /// Number of lattice candidates to score.
    pub n: usize,
    /// Weight of the lattice cost (in nats, cost / 500) relative to the
    /// model log-probability.
    pub mix: f32,
}

impl ZenzReranker {
    pub fn new(zenz: Zenz) -> Self {
        Self { zenz, n: 5, mix: 0.12 }
    }
}

impl sweet_engine::Reranker for ZenzReranker {
    fn candidates(&self) -> usize {
        self.n
    }

    fn repeated_homophones(&self) -> bool {
        true
    }

    fn choose(&self, reading: &str, left_context: &str, cands: &[sweet_engine::Conversion]) -> Option<usize> {
        static SPELLINGS: OnceLock<HashSet<(&str, &str)>> = OnceLock::new();
        let spellings = SPELLINGS.get_or_init(|| {
            VRCHAT_TERMS.lines().chain(TECH_TERMS.lines()).filter(|line| !line.starts_with('#')).filter_map(|line| {
                let mut fields = line.split('\t');
                Some((fields.next()?, fields.next()?))
            })
            .collect()
        });
        // Preserve dictionary spellings such as IME and UdonSharp when the model
        // prefers spelling them phonetically. Still rerank the surrounding text.
        let coverage: Vec<usize> = cands.iter().map(|c| {
            c.segments.iter().flat_map(|s| &s.words)
                .filter(|w| {
                    spellings.contains(&(w.reading.as_str(), w.surface.as_str()))
                        // English words typed by their spelling (ぎてゅb -> GitHub, てst -> test).
                        || sweet_engine::english::is_spelling(&w.reading, &w.surface)
                })
                .map(|w| w.reading.len())
                .sum()
        }).collect();
        let best_coverage = *coverage.iter().max()?;
        let mut eligible: Vec<usize> = (0..cands.len()).filter(|&i| coverage[i] == best_coverage).collect();
        // Distant lattice paths are usually poor homophones. Left context
        // gives the model more evidence, so keep a wider window in that case.
        let best_cost = eligible.iter().map(|&i| cands[i].cost).min()?;
        let first = cands.first()?;
        let repeated = first.repeated_kanji_reading().map(|(reading, count)| {
            (
                reading,
                count,
                first.homophone_repetitions(reading, count).unwrap_or(0),
            )
        });
        let repetitions = |candidate: &sweet_engine::Conversion| {
            repeated.map_or(0, |(reading, count, original)| {
                candidate
                    .homophone_repetitions(reading, count)
                    .unwrap_or(original)
            })
        };
        let cost_window = if left_context.trim().is_empty() {
            2000
        } else {
            3200
        };
        eligible.retain(|&i| {
            let candidate = &cands[i];
            let window = if repeated
                .is_some_and(|(_, count, original)| count >= 3 && repetitions(candidate) < original)
            {
                4500
            } else {
                cost_window
            };
            candidate.cost <= best_cost.saturating_add(window)
        });
        if eligible.len() == 1 {
            return eligible.first().copied();
        }
        let prompt = Zenz::prompt(reading, left_context);
        let surfaces: Vec<String> = eligible.iter().map(|&i| cands[i].surface()).collect();
        let scores = self.zenz.score_candidates(&prompt, &surfaces, true);
        // Repeated readings can make the model copy the first spelling even
        // where another word is plausible. Keep the original as an option.
        let repetition_weight = if repeated.is_some_and(|(_, count, _)| count >= 3) {
            3.2
        } else {
            2.7
        };
        (0..eligible.len())
            .max_by(|&a, &b| {
                let s = |i: usize| {
                    let candidate = &cands[eligible[i]];
                    scores[i]
                        - self.mix * candidate.cost as f32 / 500.0
                        - repetition_weight * repetitions(candidate) as f32
                };
                s(a).total_cmp(&s(b))
            })
            .map(|i| eligible[i])
    }
}

#[cfg(test)]
mod tests {
    use super::{Zenz, ZenzReranker};
    use sweet_engine::{Converter, Dictionary};

    #[test]
    fn relative_scores_drop_only_the_shared_prefix_probability() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/zenz-v3.1-xsmall.swz"
        );
        let Ok(zenz) = Zenz::load(path) else { return };
        let prompt = Zenz::prompt("きょうははしをわたる", "");
        let candidates = ["今日は橋を渡る", "今日は箸を渡る", "今日は"].map(str::to_owned);
        let absolute = zenz.score(&prompt, &candidates);
        let relative = zenz.score_candidates(&prompt, &candidates, true);
        assert!(relative[0] > absolute[0]);
        for i in 1..candidates.len() {
            let difference = (absolute[i] - absolute[0]) - (relative[i] - relative[0]);
            assert!(difference.abs() < 1e-3, "{}: {difference}", candidates[i]);
        }
    }

    #[test]
    fn reranks_different_homophones_and_keeps_intentional_repetition() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/");
        let Ok(model) = Zenz::load(format!("{root}zenz-v3.1-xsmall.swz")) else {
            return;
        };
        let Ok(dict) = Dictionary::open(format!("{root}system.dict")) else {
            return;
        };
        let conv = Converter::new(dict).with_reranker(Box::new(ZenzReranker::new(model)));
        for (reading, expected) in [
            (
                "きしゃのきしゃがきしゃできしゃした",
                "貴社の記者が汽車で帰社した",
            ),
            (
                "きかいがあればきかいをしゅうりする",
                "機会があれば機械を修理する",
            ),
            ("きしゃがきしゃにしゅざいした", "記者が記者に取材した"),
            ("こうえんとこうえんをつなぐ", "公園と公園をつなぐ"),
        ] {
            assert_eq!(
                conv.convert_best(reading, &[], "")[0].surface(),
                expected,
                "{reading}"
            );
        }
    }
}
