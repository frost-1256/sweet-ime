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
use std::sync::{Mutex, OnceLock};

use sweet_engine::dict::{TECH_TERMS, VRCHAT_TERMS};

pub use model::{KvCache, Model};
pub use tokenizer::Tokenizer;

const INPUT_TAG: char = '\u{EE00}';
const OUTPUT_TAG: char = '\u{EE01}';
const CONTEXT_TAG: char = '\u{EE02}';
/// Left context is limited to its last N characters.
pub const MAX_CONTEXT_CHARS: usize = 40;

struct PrefixCache {
    tokens: Vec<u32>,
    kv: KvCache,
}

pub struct Zenz {
    pub tokenizer: Tokenizer,
    pub model: Model,
    /// Prepend BOS to prompts.
    pub add_bos: bool,
    /// Single-slot KV cache for the prompt prefix so incremental live
    /// conversions reuse K/V states for the unchanged left context & reading.
    prefix_cache: Mutex<Option<PrefixCache>>,
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
            return Ok(Self {
                tokenizer: Tokenizer::from_strings(&vocab, &merges)?,
                model,
                add_bos: false,
                prefix_cache: Mutex::new(None),
            });
        }
        let dir = path;
        Ok(Self {
            tokenizer: Tokenizer::load(dir)?,
            model: Model::load(dir)?,
            add_bos: false,
            prefix_cache: Mutex::new(None),
        })
    }

    /// Packs either a Hugging Face model directory or a GPT-2 `.gguf` file
    /// into a single `.swz` file.
    pub fn pack(src: &Path, out: &Path) -> io::Result<()> {
        if src.is_file() {
            let (model, vocab, merges) = Model::load_gguf(src)?;
            return model.pack(&vocab, &merges, out);
        }
        let model = Model::load(src)?;
        let vocab = std::fs::read_to_string(src.join("vocab.json"))?;
        let merges = std::fs::read_to_string(src.join("merges.txt"))?;
        model.pack(&vocab, &merges, out)
    }

    /// Builds the v3 conversion prompt (up to and including the output tag).
    pub fn prompt(reading: &str, left_context: &str) -> String {
        let mut p = String::with_capacity(reading.len() + left_context.len().min(MAX_CONTEXT_CHARS * 3) + 12);
        if !left_context.is_empty() {
            let ctx = normalize(left_context);
            let n = ctx.chars().count();
            let start = ctx
                .char_indices()
                .nth(n.saturating_sub(MAX_CONTEXT_CHARS))
                .map_or(ctx.len(), |(i, _)| i);
            let tail = &ctx[start..];
            if !tail.is_empty() {
                p.push(CONTEXT_TAG);
                p.push_str(tail);
            }
        }
        p.push(INPUT_TAG);
        for c in reading.chars() {
            match c {
                ' ' => p.push('\u{3000}'),
                '\n' | '\r' => {}
                'ぁ'..='ゖ' => p.push(char::from_u32(c as u32 + 0x60).unwrap_or(c)),
                _ => p.push(c),
            }
        }
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

    /// Greedy decoding until EOS, reusing the prompt prefix KV cache when
    /// available so uncertain sentences can generate a fallback candidate fast.
    pub fn generate(&self, prompt: &str, max_tokens: usize) -> String {
        self.generate_with_draft(prompt, max_tokens, "")
    }

    /// Greedy decoding that checks `draft` (usually the best lattice
    /// candidate) in batches. Whenever the next greedy token starts the rest
    /// of the draft, one pass scores the following draft tokens and keeps
    /// those the model would have picked anyway, so the output stays the
    /// greedy output while the tokens shared with the draft cost one pass.
    pub fn generate_with_draft(&self, prompt: &str, max_tokens: usize, draft: &str) -> String {
        let ids = self.encode_prompt(prompt);
        if ids.is_empty() {
            return String::new();
        }
        let eos = self.tokenizer.eos;
        let mut draft_ids = Vec::new();
        if !draft.is_empty() {
            draft_ids = self.tokenizer.encode(&normalize(draft));
            draft_ids.push(eos);
        }
        let cached = self
            .prefix_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        let (mut cache, reuse) = match cached {
            Some(mut entry) => {
                let common = entry
                    .tokens
                    .iter()
                    .zip(&ids[..ids.len().saturating_sub(1)])
                    .take_while(|(a, b)| a == b)
                    .count();
                if common > 0 {
                    entry.kv.truncate(common);
                    (entry.kv, common)
                } else {
                    (self.model.new_cache(), 0)
                }
            }
            None => (self.model.new_cache(), 0),
        };
        let v = self.model.cfg.vocab_size;
        let mut logits = self.model.forward(&ids[reuse..], &mut cache, false);
        let prompt_cache_len = ids.len().saturating_sub(1);
        let mut out = Vec::new();
        // Position in `draft_ids` that the output is expected to continue with.
        let mut at = 0;
        while out.len() < max_tokens {
            let next = model::argmax(&logits) as u32;
            if next == eos {
                break;
            }
            if draft_ids.get(at) != Some(&next) {
                out.push(next);
                // Resynchronize after a replaced word: the draft usually
                // continues after the next occurrence of this token.
                if let Some(i) = draft_ids[at.min(draft_ids.len())..]
                    .iter()
                    .position(|&t| t == next)
                {
                    at += i + 1;
                }
                logits = self.model.forward(&[next], &mut cache, false);
                continue;
            }
            let spec = &draft_ids[at..draft_ids.len().min(at + max_tokens - out.len())];
            let base = cache.len;
            let rows = self.model.forward(spec, &mut cache, true);
            // `next` is the greedy token, so the first draft token is kept.
            let mut kept = 1;
            while kept < spec.len()
                && model::argmax(&rows[(kept - 1) * v..kept * v]) as u32 == spec[kept]
            {
                kept += 1;
            }
            let done = spec[..kept].contains(&eos);
            out.extend(spec[..kept].iter().copied().filter(|&t| t != eos));
            cache.truncate(base + kept);
            logits = rows[(kept - 1) * v..kept * v].to_vec();
            at += kept;
            if done {
                break;
            }
        }
        if prompt_cache_len > 0 && cache.len >= prompt_cache_len {
            cache.truncate(prompt_cache_len);
            *self.prefix_cache.lock().unwrap_or_else(|e| e.into_inner()) = Some(PrefixCache {
                tokens: ids[..prompt_cache_len].to_vec(),
                kv: cache,
            });
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
        let prompt_len = ids.len();
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
        let p = ids.len();
        if p == 0 {
            return vec![f32::NEG_INFINITY; toks.len()];
        }
        // Cache up to `prompt_len - 1` (everything before the output tag, which
        // stays identical as the user types additional characters or re-scores).
        let cacheable_len = prompt_len.saturating_sub(1);
        let cached = self
            .prefix_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        let (mut cache, reuse) = match cached {
            Some(mut entry) => {
                let common = entry
                    .tokens
                    .iter()
                    .zip(&ids[..p - 1])
                    .take_while(|(a, b)| a == b)
                    .count();
                if common > 0 {
                    entry.kv.truncate(common);
                    (entry.kv, common)
                } else {
                    (self.model.new_cache(), 0)
                }
            }
            None => (self.model.new_cache(), 0),
        };
        // Suffix of the prompt+shared chain that is not in `cache` yet.
        let tail_p = p - reuse;
        let mut nodes: Vec<u32> = ids[reuse..].to_vec();
        let mut parents: Vec<Option<usize>> = (0..tail_p).map(|i| i.checked_sub(1)).collect();
        let mut children: std::collections::HashMap<(Option<usize>, u32), usize> = Default::default();
        let mut ends: Vec<Option<usize>> = Vec::with_capacity(toks.len());
        for t in &toks {
            let mut cur: Option<usize> = tail_p.checked_sub(1);
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
        let save_kv_prefix = cacheable_len.saturating_sub(reuse);
        let v = self.model.cfg.vocab_size;
        let eos = self.tokenizer.eos as usize;
        // Logits are only needed from the last prompt token on.
        let from = tail_p - 1;
        let scores = self.model.forward_tree_caching_with(
            &nodes,
            &parents,
            &mut cache,
            save_kv_prefix,
            from,
            |logits| {
                let row = |i: usize| -> &[f32] { &logits[(i - from) * v..(i - from + 1) * v] };
                // A branch can score several child tokens and EOS from the same logits.
                let normalizers: Vec<_> = if logits.len() >= 4 * v {
                    use rayon::prelude::*;
                    logits
                        .par_chunks_exact(v)
                        .map(model::log_normalizer)
                        .collect()
                } else {
                    logits.chunks_exact(v).map(model::log_normalizer).collect()
                };
                let log_prob = |i, token| {
                    let (mx, log_sum) = normalizers[i - from];
                    row(i)[token] - mx - log_sum
                };
                // Cumulative log-prob of the candidate part of the path to each node.
                let mut cum = vec![0f32; nodes.len()];
                for i in tail_p..nodes.len() {
                    let par = parents[i].expect("candidate nodes have a parent");
                    cum[i] = cum[par] + log_prob(par, nodes[i] as usize);
                }
                ends.iter()
                    .map(|&end| {
                        let end = end.expect("non-empty prompt");
                        cum[end] + log_prob(end, eos)
                    })
                    .collect()
            },
        );
        if cache.len > 0 {
            let cached_len = cache.len.min(ids.len());
            *self.prefix_cache.lock().unwrap_or_else(|e| e.into_inner()) = Some(PrefixCache {
                tokens: ids[..cached_len].to_vec(),
                kv: cache,
            });
        }
        scores
    }
}

/// [`sweet_engine::Reranker`] backed by a zenz model: scores the lattice's
/// N-best sentences and mixes in the lattice cost.
pub struct ZenzReranker {
    pub zenz: Zenz,
    /// Number of lattice candidates to offer in ordinary conversions.
    pub n: usize,
    /// Number of lattice candidates to offer for a long reading without context.
    pub n_without_context: usize,
    /// Weight of the lattice cost (in nats, cost / 500) relative to the
    /// model log-probability.
    pub mix: f32,
    /// Shortest reading (in characters) for which the model generates its
    /// own conversion.
    pub generate_min_chars: usize,
    /// How much higher (in nats) the model must score its own conversion
    /// than the best lattice candidate to use it.
    pub generate_margin: f32,
    /// The least the model must prefer its own conversion when dictionary
    /// words spell it, on top of the lattice cost difference.
    pub generate_spelled_margin: f32,
    /// Longest reading (in characters) for which the model generates.
    pub generate_max_chars: usize,
}

impl ZenzReranker {
    pub fn new(zenz: Zenz) -> Self {
        Self {
            zenz,
            n: 8,
            n_without_context: 10,
            mix: 0.40,
            generate_min_chars: 10,
            generate_margin: 2.2,
            generate_spelled_margin: 1.5,
            generate_max_chars: 120,
        }
    }
}

fn protected_spellings() -> &'static HashSet<(&'static str, &'static str)> {
    static SPELLINGS: OnceLock<HashSet<(&str, &str)>> = OnceLock::new();
    SPELLINGS.get_or_init(|| {
        VRCHAT_TERMS
            .lines()
            .chain(TECH_TERMS.lines())
            .filter(|line| !line.starts_with('#'))
            .filter_map(|line| {
                let mut fields = line.split('\t');
                Some((fields.next()?, fields.next()?))
            })
            .collect()
    })
}

fn is_protected_word(w: &sweet_engine::Word) -> bool {
    protected_spellings().contains(&(w.reading.as_str(), w.surface.as_str()))
        || sweet_engine::english::is_spelling(&w.reading, &w.surface)
}

impl sweet_engine::Reranker for ZenzReranker {
    fn candidates(&self) -> usize {
        self.n
    }

    fn candidates_for(&self, reading: &str, left_context: &str) -> usize {
        // Longer readings can have several independent ambiguous spans.
        const LONG_READING_CHARS: usize = 16;
        if left_context.trim().is_empty() && reading.chars().nth(LONG_READING_CHARS - 1).is_some() {
            self.n_without_context
        } else {
            self.n
        }
    }

    fn repeated_homophones(&self) -> bool {
        true
    }

    fn choose(&self, reading: &str, left_context: &str, cands: &[sweet_engine::Conversion]) -> Option<usize> {
        // Preserve dictionary spellings such as IME and UdonSharp when the model
        // prefers spelling them phonetically. Still rerank the surrounding text.
        let coverage: Vec<usize> = cands.iter().map(|c| {
            c.segments.iter().flat_map(|s| &s.words)
                .filter(|w| is_protected_word(w))
                .map(|w| w.reading.len())
                .sum()
        }).collect();
        // Keep the lattice best's coverage without forcing spellings from other
        // candidates (しょっぴんぐ -> しょっPNG).
        let lattice_coverage = *coverage.first()?;
        let mut eligible: Vec<usize> = (0..cands.len()).filter(|&i| coverage[i] >= lattice_coverage).collect();
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
        // where another word is plausible. Keep the original as an option, and
        // reduce lattice cost weight so the lattice's repetition bias does not
        // override distinct homophones.
        let (repetition_weight, effective_mix) = match repeated {
            Some((_, count, _)) if count >= 3 => (3.2, self.mix.min(0.12)),
            Some(_) => (2.7, self.mix.min(0.12)),
            // When preceding left context is available, the lattice is blind to
            // it while zenz sees it directly, so let zenz carry more weight.
            None if !left_context.is_empty() => (2.7, self.mix.min(0.28)),
            None => (2.7, self.mix),
        };
        (0..eligible.len())
            .max_by(|&a, &b| {
                let s = |i: usize| {
                    let candidate = &cands[eligible[i]];
                    scores[i]
                        - effective_mix * candidate.cost as f32 / 500.0
                        - repetition_weight * repetitions(candidate) as f32
                };
                s(a).total_cmp(&s(b))
            })
            .map(|i| eligible[i])
    }

    fn generate(
        &self,
        reading: &str,
        left_context: &str,
        best: &sweet_engine::Conversion,
    ) -> Option<String> {
        self.generate_spelled(reading, left_context, best, &|_| None)
    }

    fn generate_spelled(
        &self,
        reading: &str,
        left_context: &str,
        best: &sweet_engine::Conversion,
        spelled_cost: &dyn Fn(&str) -> Option<i32>,
    ) -> Option<String> {
        let reading_chars = reading.chars().count();
        // Long readings combine several ambiguous spans, so the model's own
        // conversion often beats every lattice candidate. On short readings
        // it mostly replaces correct dictionary words.
        if !(self.generate_min_chars..=self.generate_max_chars).contains(&reading_chars)
            || reading.bytes().any(|b| b.is_ascii_alphabetic() || b.is_ascii_digit())
            || best.repeated_kanji_reading().is_some()
            || best
                .segments
                .iter()
                .flat_map(|s| &s.words)
                .any(is_protected_word)
        {
            return None;
        }
        let best_surface = best.surface();
        let prompt = Zenz::prompt(reading, left_context);
        let max_tokens = (reading_chars * 2).clamp(16, 80.max(reading_chars + 16));
        let generated = self
            .zenz
            .generate_with_draft(&prompt, max_tokens, &best_surface)
            .trim()
            .to_string();
        if generated.is_empty()
            || generated == best_surface
            || generated.chars().any(|c| c.is_control() || matches!(c, '\u{EE00}'..='\u{EE02}'))
        {
            return None;
        }
        let gen_chars = generated.chars().count();
        if gen_chars < (reading_chars / 3).max(2) || gen_chars > reading_chars + 2 {
            return None;
        }
        // Guard against hallucination: every hiragana character in the
        // generated text must appear in `reading` in order.
        let mut r_iter = reading.chars();
        for ch in generated.chars() {
            if ('ぁ'..='ゖ').contains(&ch) && !r_iter.any(|rc| rc == ch) {
                return None;
            }
        }
        // When dictionary words spell the generated text, weigh it like a
        // lattice candidate against the best one. Otherwise accept it only
        // when the model strongly prefers it.
        let margin = match spelled_cost(&generated) {
            Some(cost) => {
                let mix = if left_context.is_empty() {
                    self.mix
                } else {
                    self.mix.min(0.28)
                };
                (mix * (cost - best.cost) as f32 / 500.0).max(self.generate_spelled_margin)
            }
            None => self.generate_margin,
        };
        let pair = [best_surface, generated.clone()];
        let scores = self.zenz.score_candidates(&prompt, &pair, true);
        if scores.len() == 2 && scores[1] > scores[0] + margin {
            Some(generated)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Zenz, ZenzReranker};
    use sweet_engine::{Converter, Dictionary, Reranker};

    #[test]
    fn prefix_kv_cache_matches_uncached_scores() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/zenz-v3.1-xsmall.swz"
        );
        let Ok(cached) = Zenz::load(path) else { return };
        let Ok(fresh) = Zenz::load(path) else { return };
        // Warm cache with a shorter prefix (simulating incremental live typing).
        let p1 = Zenz::prompt("きょうははしを", "先ほどから");
        let c1 = ["今日は橋を", "今日は箸を"].map(str::to_owned);
        let _ = cached.score_candidates(&p1, &c1, true);

        // Extend the reading with the same left context — must reuse prefix KV.
        let p2 = Zenz::prompt("きょうははしをわたる", "先ほどから");
        let c2 = ["今日は橋を渡る", "今日は箸を渡る"].map(str::to_owned);
        let got = cached.score_candidates(&p2, &c2, true);
        let want = fresh.score_candidates(&p2, &c2, true);
        for (i, (g, w)) in got.iter().zip(&want).enumerate() {
            assert!((g - w).abs() < 1e-3, "cand {i}: cached={g} vs fresh={w}");
        }
    }

    #[test]
    fn drafts_do_not_change_the_greedy_output() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/zenz-v3.1-xsmall.swz"
        );
        let Ok(zenz) = Zenz::load(path) else { return };
        let prompt = Zenz::prompt("きょうはいいてんきなのでさんぽにいきます", "");
        let greedy = zenz.generate(&prompt, 40);
        assert!(!greedy.is_empty());
        let replaced = greedy.replacen("今日", "京", 1);
        for draft in [
            greedy.as_str(),
            "今日葉良い天気なので散歩に行きます",
            &replaced,
            "全然違う文章",
        ] {
            assert_eq!(
                zenz.generate_with_draft(&prompt, 40, draft),
                greedy,
                "draft {draft}"
            );
        }
        // The token limit still applies when the draft is accepted.
        assert_eq!(
            zenz.generate_with_draft(&prompt, 3, &greedy),
            zenz.generate(&prompt, 3)
        );
    }

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
        let reranker = ZenzReranker::new(model);
        let long = "きしゃのきしゃがきしゃできしゃした";
        assert_eq!(reranker.candidates_for(long, "　"), 10);
        assert_eq!(reranker.candidates_for(long, "先ほど、"), 8);
        assert_eq!(reranker.candidates_for("へんしんまち", ""), 8);
        let conv = Converter::new(dict).with_reranker(Box::new(reranker));
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
