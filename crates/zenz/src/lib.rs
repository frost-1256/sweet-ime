//! Native Rust inference for zenz (GPT-2) kana-kanji conversion models,
//! used to rerank and refine the lattice converter's candidates.
//!
//! zenz-v3 prompt format:
//! `[\u{EE02}<left context>]\u{EE00}<reading in katakana>\u{EE01}<output></s>`

pub mod kernels;
pub mod model;
pub mod tokenizer;

use std::io;
use std::path::Path;

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
        let ids = self.encode_prompt(prompt);
        let mut cache = self.model.new_cache();
        let toks: Vec<Vec<u32>> = candidates.iter().map(|c| self.tokenizer.encode(&normalize(c))).collect();
        // Trie: node -> (token, parent). The prompt is a chain at the front;
        // candidates hang off its last token.
        let p = ids.len();
        let mut nodes: Vec<u32> = ids.clone();
        let mut parents: Vec<Option<usize>> = (0..p).map(|i| i.checked_sub(1)).collect();
        let mut children: std::collections::HashMap<(Option<usize>, u32), usize> = Default::default();
        let mut ends: Vec<Option<usize>> = Vec::with_capacity(toks.len());
        for t in &toks {
            let mut cur: Option<usize> = p.checked_sub(1);
            for &tok in t {
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
        // Cumulative log-prob of the candidate part of the path to each node.
        let mut cum = vec![0f32; nodes.len()];
        for i in p..nodes.len() {
            let par = parents[i].expect("candidate nodes have a parent");
            cum[i] = cum[par] + model::log_prob(row(par), nodes[i] as usize);
        }
        ends.iter()
            .map(|&end| {
                let end = end.expect("non-empty prompt");
                cum[end] + model::log_prob(row(end), eos)
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
        Self { zenz, n: 5, mix: 0.3 }
    }
}

impl sweet_engine::Reranker for ZenzReranker {
    fn candidates(&self) -> usize {
        self.n
    }

    fn choose(&self, reading: &str, left_context: &str, cands: &[sweet_engine::Conversion]) -> Option<usize> {
        let prompt = Zenz::prompt(reading, left_context);
        let surfaces: Vec<String> = cands.iter().map(|c| c.surface()).collect();
        let scores = self.zenz.score(&prompt, &surfaces);
        (0..cands.len()).max_by(|&a, &b| {
            let s = |i: usize| scores[i] - self.mix * cands[i].cost as f32 / 500.0;
            s(a).total_cmp(&s(b))
        })
    }
}
