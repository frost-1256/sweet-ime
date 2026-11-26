//! Candidate selection and memoization for neural reranking.

use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Condvar, Mutex};

use super::{Conversion, Converter, Word};

/// Picks the best whole-sentence candidate (e.g. with a neural LM).
pub trait Reranker: Send + Sync {
    /// How many lattice candidates to offer.
    fn candidates(&self) -> usize {
        5
    }
    /// Candidate count for a reading with the given left context.
    fn candidates_for(&self, _reading: &str, _left_context: &str) -> usize {
        self.candidates()
    }
    /// Whether to offer additional combinations of repeated homophones.
    fn repeated_homophones(&self) -> bool {
        false
    }
    /// Returns the index of the best candidate, or `None` to keep the
    /// lattice order.
    fn choose(&self, reading: &str, left_context: &str, candidates: &[Conversion])
    -> Option<usize>;
    /// Optional neural-generated conversion when the lattice candidates look
    /// uncertain (e.g. proper nouns or rare compounds outside the N-best pool).
    fn generate(
        &self,
        _reading: &str,
        _left_context: &str,
        _best: &Conversion,
    ) -> Option<String> {
        None
    }
}

/// How many lattice candidates per reranker slot `convert_best` draws its
/// diverse candidates from.
const RERANK_POOL: usize = 6;

/// Words of `c` as (reading start, reading end, surface), offsets in bytes.
fn spans(c: &Conversion) -> impl Iterator<Item = (usize, usize, &Word)> {
    c.segments
        .iter()
        .flat_map(|s| &s.words)
        .scan(0, |at, word| {
            let start = *at;
            *at += word.reading.len();
            Some((start, *at, word))
        })
}

fn word_spans(c: &Conversion) -> Vec<(usize, usize, &str)> {
    spans(c)
        .map(|(start, end, word)| (start, end, word.surface.as_str()))
        .collect()
}

fn personal_spans(c: &Conversion) -> Vec<(usize, usize, &str)> {
    spans(c)
        .filter(|(_, _, word)| word.learned || word.user)
        .map(|(start, end, word)| (start, end, word.surface.as_str()))
        .collect()
}

fn keeps_personal_words(c: &Conversion, personal: &[(usize, usize, &str)]) -> bool {
    if personal.is_empty() {
        return true;
    }
    let words = word_spans(c);
    personal.iter().all(|span| words.contains(span))
}

/// Alternatives `convert_best` offers the reranker beyond its N-best.
const RERANK_EXTRA: usize = 2;

/// Maximum number of synthesized crossover candidates combining disjoint
/// single-region alternatives from `diverse`.
const RERANK_CROSSOVER: usize = 2;

/// Combines two candidates that each differ from `base` in disjoint byte
/// regions by taking `a`'s words in `ra`, `b`'s words in `rb`, and `base`'s
/// words everywhere else. Returns the combined surface if the spans align on
/// word boundaries.
fn crossover_surface(
    base: &Conversion,
    a: &Conversion,
    ra: (usize, usize),
    b: &Conversion,
    rb: (usize, usize),
) -> Option<String> {
    let (first_c, first_r, second_c, second_r) = if ra.1 <= rb.0 {
        (a, ra, b, rb)
    } else if rb.1 <= ra.0 {
        (b, rb, a, ra)
    } else {
        return None;
    };
    let slice_surface = |c: &Conversion, from: usize, to: usize| -> Option<String> {
        if from == to {
            return Some(String::new());
        }
        let mut out = String::new();
        let mut pos = from;
        for (start, end, word) in spans(c) {
            if end <= from {
                continue;
            }
            if start >= to {
                break;
            }
            if start != pos || end > to {
                return None;
            }
            out.push_str(&word.surface);
            pos = end;
        }
        (pos == to).then_some(out)
    };
    let total_len: usize = spans(base).last().map_or(0, |(_, end, _)| end);
    let mut s = slice_surface(base, 0, first_r.0)?;
    s.push_str(&slice_surface(first_c, first_r.0, first_r.1)?);
    s.push_str(&slice_surface(base, first_r.1, second_r.0)?);
    s.push_str(&slice_surface(second_c, second_r.0, second_r.1)?);
    s.push_str(&slice_surface(base, second_r.1, total_len)?);
    Some(s)
}

/// The reranker's candidates from the cost-ordered `pool`: the `n` best,
/// then up to [`RERANK_EXTRA`] cheapest alternatives that differ from the
/// best in a part of the reading none of those does, plus up to
/// [`RERANK_CROSSOVER`] pool candidates that combine two disjoint single-region
/// alternatives (so multi-ambiguity sentences like 講座/口座 + 解説/開設 don't
/// drop the joint fix).
fn diverse(pool: &[Conversion], n: usize) -> Vec<Conversion> {
    if pool.len() <= n {
        return pool.to_vec();
    }
    let best = word_spans(&pool[0]);
    let personal = personal_spans(&pool[0]);
    let region = |c: &Conversion| {
        let diff: Vec<_> = word_spans(c)
            .into_iter()
            .filter(|w| !best.contains(w))
            .collect();
        diff.first().zip(diff.last()).map(|(a, b)| (a.0, b.1))
    };
    // Only alternatives the model can use should occupy a region.
    let mut region_sources: Vec<(usize, (usize, usize))> = pool[1..n]
        .iter()
        .enumerate()
        .filter(|(_, candidate)| keeps_personal_words(candidate, &personal))
        .filter_map(|(idx, candidate)| Some((idx + 1, region(candidate)?)))
        .collect();
    let mut picked: Vec<usize> = (0..n).collect();
    for (i, c) in pool.iter().enumerate().skip(n) {
        if picked.len() >= n + RERANK_EXTRA {
            break;
        }
        if !keeps_personal_words(c, &personal) {
            continue;
        }
        let Some(r) = region(c) else { continue };
        if !region_sources.iter().any(|&(_, (a, b))| r.0 < b && a < r.1) {
            region_sources.push((i, r));
            picked.push(i);
        }
    }
    // Look for pool candidates that combine two disjoint single-region changes.
    let mut added_crossovers = 0;
    'outer: for i in 0..region_sources.len() {
        for j in (i + 1)..region_sources.len() {
            if added_crossovers >= RERANK_CROSSOVER {
                break 'outer;
            }
            let (ia, ra) = region_sources[i];
            let (ib, rb) = region_sources[j];
            let Some(target) = crossover_surface(&pool[0], &pool[ia], ra, &pool[ib], rb) else {
                continue;
            };
            if picked.iter().any(|&idx| pool[idx].surface() == target) {
                continue;
            }
            if let Some(found) = pool
                .iter()
                .enumerate()
                .skip(n)
                .find(|(idx, c)| !picked.contains(idx) && keeps_personal_words(c, &personal) && c.surface() == target)
                .map(|(idx, _)| idx)
            {
                picked.push(found);
                added_crossovers += 1;
            }
        }
    }
    picked.into_iter().map(|i| pool[i].clone()).collect()
}

fn homophone_pool(
    pool: &[Conversion],
    ordinary: &mut Vec<Conversion>,
    reading: &str,
    count: usize,
) {
    let personal = personal_spans(&pool[0]);
    let repetitions = pool[0].homophone_repetitions(reading, count).unwrap_or(0);
    let original: Vec<_> = ordinary.iter().map(Conversion::surface).collect();
    let mut combinations: Vec<_> = pool
        .iter()
        .filter_map(|candidate| {
            let repeats = candidate.homophone_repetitions(reading, count)?;
            ((repeats < repetitions || (repeats == 0 && repetitions == 0))
                && keeps_personal_words(candidate, &personal)
                && !original.contains(&candidate.surface()))
                .then_some((repeats, candidate))
        })
        .collect();
    combinations.sort_by_key(|(repeats, candidate)| (*repeats, candidate.cost));
    let limit = if count >= 3 { 12 } else { 6 };
    ordinary.extend(
        combinations
            .into_iter()
            .take(limit)
            .map(|(_, candidate)| candidate.clone()),
    );
}

/// Reranker key: (reading, left context, candidate surfaces).
type RerankKey = (String, String, Vec<String>);

#[derive(Default)]
pub(super) struct RerankState {
    decisions: Mutex<Decisions>,
    done: Condvar,
}

#[derive(Default)]
struct Decisions {
    /// Recent decisions, oldest first.
    cache: Vec<(RerankKey, Option<usize>)>,
    /// Runs in progress on some thread.
    running: Vec<RerankKey>,
}

/// How long a conversion waits for another thread's identical reranker run
/// before running it itself.
const RERANK_WAIT: std::time::Duration = std::time::Duration::from_millis(1500);

const RERANK_CACHE: usize = 16;

impl Converter {
    pub fn with_reranker(mut self, r: Box<dyn Reranker>) -> Self {
        self.reranker = Some(r);
        self
    }

    /// Whether neural reranking is available and enabled.
    pub fn has_reranker(&self) -> bool {
        self.reranker.is_some() && self.rerank_on.load(Relaxed)
    }

    /// Whether a reranker is loaded at all (regardless of the switch).
    pub fn reranker_loaded(&self) -> bool {
        self.reranker.is_some()
    }

    /// Best conversion: lattice N-best refined by the reranker if present.
    /// The chosen candidate is first; the rest follow in lattice order.
    pub fn convert_best(
        &self,
        text: &str,
        boundaries: &[usize],
        left_context: &str,
    ) -> Vec<Conversion> {
        let n = self
            .reranker
            .as_ref()
            .filter(|_| self.has_reranker())
            .map_or(1, |r| r.candidates_for(text, left_context).max(1));
        let mut res = if n > 1 {
            let normal = n * RERANK_POOL;
            let expand = self
                .reranker
                .as_ref()
                .is_some_and(|r| r.repeated_homophones());
            let pool = self.convert_pool(text, boundaries, normal, expand);
            let mut candidates = diverse(&pool[..normal.min(pool.len())], n);
            if expand
                && let Some((reading, count)) =
                    pool.first().and_then(Conversion::repeated_kanji_reading)
            {
                homophone_pool(&pool, &mut candidates, reading, count);
            }
            candidates
        } else {
            self.convert(text, boundaries, 1)
        };
        if let Some(r) = self.reranker.as_ref().filter(|_| self.has_reranker()) {
            // After a version number, でたね is usually a release
            // announcement. Keep the lattice's release phrase when present.
            if super::release_phrase::follows_version(text, left_context)
                && res.first().is_some_and(|c| {
                    c.segments
                        .iter()
                        .flat_map(|segment| &segment.words)
                        .any(|word| {
                            word.reading == super::release_phrase::READING
                                && word.surface == super::release_phrase::SURFACE
                        })
                })
            {
                return res;
            }
            if let Some(i) = self.rerank_index(r.as_ref(), text, left_context, &res) {
                let c = res.remove(i);
                res.insert(0, c);
            }
            if let Some(best) = res.first()
                && personal_spans(best).is_empty()
                && super::extras::casual_phrase(text).is_none()
                && let Some(surface) = r.generate(text, left_context, best)
                && !res.iter().any(|c| c.surface() == surface)
            {
                let generated = Conversion {
                    segments: vec![super::Segment {
                        reading: text.to_string(),
                        surface: surface.clone(),
                        words: vec![Word {
                            reading: text.to_string(),
                            surface,
                            lid: self.noun_pos,
                            rid: self.noun_pos,
                            cost: best.cost,
                            learned: false,
                            user: false,
                        }],
                    }],
                    cost: best.cost,
                };
                res.insert(0, generated);
            }
        }
        res
    }

    fn rerank_index(
        &self,
        reranker: &dyn Reranker,
        reading: &str,
        context: &str,
        candidates: &[Conversion],
    ) -> Option<usize> {
        if candidates.len() < 2 {
            return None;
        }
        let mut protected = personal_spans(&candidates[0]);
        if let Some((phrase, _)) = super::extras::casual_phrase(reading)
            && spans(&candidates[0]).next().is_some_and(|(start, end, word)| {
                start == 0 && end == phrase.len() && word.surface == phrase
            })
        {
            protected.push((0, phrase.len(), phrase));
        }
        if protected.is_empty() {
            return self.choose_ranked(reranker, reading, context, candidates);
        }
        // The model can improve the rest of the sentence while the words
        // learned or registered by the user and casual phrases stay at their original offsets.
        let (indices, eligible): (Vec<_>, Vec<_>) = candidates
            .iter()
            .enumerate()
            .filter(|(_, candidate)| keeps_personal_words(candidate, &protected))
            .map(|(index, candidate)| (index, candidate.clone()))
            .unzip();
        self.choose_ranked(reranker, reading, context, &eligible)
            .map(|index| indices[index])
    }

    fn choose_ranked(
        &self,
        reranker: &dyn Reranker,
        reading: &str,
        context: &str,
        candidates: &[Conversion],
    ) -> Option<usize> {
        if candidates.len() < 2 {
            return None;
        }
        let surfaces = candidates.iter().map(Conversion::surface).collect();
        let index = self
            .rerank
            .cached_choice(reranker, reading, context, candidates, surfaces)?;
        let picked = candidates.get(index)?;
        if self.katakana_on.load(Relaxed) {
            // Preserve the lattice choice between spellings like best and ベスト.
            let key = english_key(picked);
            return candidates.iter().position(|c| english_key(c) == key);
        }
        Some(index)
    }
}

impl RerankState {
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
        let key: RerankKey = (text.to_string(), left_context.to_string(), surfaces);
        let mut st = self.decisions.lock().unwrap_or_else(|e| e.into_inner());
        // The same run already in progress (the live conversion worker, when
        // Space follows the last key): wait for it rather than run the model
        // twice at once, which would slow both down.
        let deadline = std::time::Instant::now() + RERANK_WAIT;
        loop {
            if let Some(hit) = st.cache.iter().find(|(k, _)| *k == key).map(|e| e.1) {
                return hit;
            }
            let now = std::time::Instant::now();
            if !st.running.contains(&key) || now >= deadline {
                break;
            }
            st = self
                .done
                .wait_timeout(st, deadline - now)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        st.running.push(key.clone());
        drop(st);
        // Not holding the lock while the model runs. A panic still clears the
        // running entry, so waiters don't stall.
        struct Done<'a>(&'a RerankState, &'a RerankKey, Option<Option<usize>>);
        impl Drop for Done<'_> {
            fn drop(&mut self) {
                let mut st = self.0.decisions.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(pos) = st.running.iter().position(|k| k == self.1) {
                    st.running.swap_remove(pos);
                }
                if let Some(choice) = self.2 {
                    let (t, l, _) = self.1;
                    st.cache.retain(|((ct, cl, _), _)| !(ct == t && cl == l));
                    st.cache.push((self.1.clone(), choice));
                    if st.cache.len() > RERANK_CACHE {
                        st.cache.remove(0);
                    }
                }
                drop(st);
                self.0.done.notify_all();
            }
        }
        let mut done = Done(self, &key, None);
        let choice = r.choose(text, left_context, res);
        done.2 = Some(choice);
        choice
    }
}

/// The surface with each listed English word spelled out, so that a word
/// and its katakana loanword compare equal.
fn english_key(c: &Conversion) -> String {
    let index = crate::english::word_index();
    let mut key = String::new();
    for w in c.segments.iter().flat_map(|s| &s.words) {
        match index.get(&w.reading) {
            Some(word)
                if w.surface.eq_ignore_ascii_case(word)
                    || w.surface
                        .chars()
                        .all(|ch| ('\u{30A1}'..='\u{30FC}').contains(&ch)) =>
            {
                key.push_str(word)
            }
            _ => key.push_str(&w.surface),
        }
    }
    key
}
