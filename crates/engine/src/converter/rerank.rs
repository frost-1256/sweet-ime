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
    fn generate(&self, _reading: &str, _left_context: &str, _best: &Conversion) -> Option<String> {
        None
    }
    /// Like [`Reranker::generate`], with `spelled_cost` giving the lattice
    /// cost of a surface that dictionary words can spell for this reading
    /// (`None` when they cannot), so the generated text can be weighed like
    /// a lattice candidate.
    fn generate_spelled(
        &self,
        reading: &str,
        left_context: &str,
        best: &Conversion,
        _spelled_cost: &dyn Fn(&str) -> Option<i32>,
    ) -> Option<String> {
        self.generate(reading, left_context, best)
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
                .find(|(idx, c)| {
                    !picked.contains(idx)
                        && keeps_personal_words(c, &personal)
                        && c.surface() == target
                })
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
type RerankKey = (String, String, Vec<Conversion>);

/// Generation cache key: (reading, left context, best total cost, best words'
/// `(reading, surface, cost, learned, user)`).
type GenerateKey = (String, String, i32, Vec<(String, String, i32, bool, bool)>);

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
    /// Recent generation outcomes (including `None`), oldest first.
    gen_cache: Vec<(GenerateKey, Option<String>)>,
    /// Generation runs in progress on some thread.
    gen_running: Vec<GenerateKey>,
}

/// How long a conversion waits for another thread's identical reranker run
/// before running it itself.
const RERANK_WAIT: std::time::Duration = std::time::Duration::from_millis(1500);

const RERANK_CACHE: usize = 16;

impl Converter {
    pub fn with_reranker(mut self, r: Box<dyn Reranker>) -> Self {
        self.rerank = RerankState::default();
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
            if boundaries.is_empty()
                && let Some(best) = res.first()
                && personal_spans(best).is_empty()
                && super::extras::casual_phrases(text).next().is_none()
                && let Some(surface) =
                    self.rerank
                        .cached_generate(r.as_ref(), text, left_context, best, &|surface| {
                            self.spell(text, surface).map(|c| c.cost)
                        })
                && !res.iter().any(|c| c.surface() == surface)
            {
                // Dictionary words keep their segments for later editing.
                // It keeps the best candidate's cost, as before.
                let segments = self.spell(text, &surface).map_or_else(
                    || {
                        vec![super::Segment {
                            reading: text.to_string(),
                            surface: surface.clone(),
                            words: vec![Word {
                                reading: text.to_string(),
                                surface: surface.clone(),
                                lid: self.noun_pos,
                                rid: self.noun_pos,
                                cost: best.cost,
                                learned: false,
                                user: false,
                            }],
                        }]
                    },
                    |c| c.segments,
                );
                let generated = Conversion {
                    segments,
                    cost: best.cost,
                };
                if !super::negative_adverb::lacks_negation(text, &generated) {
                    res.insert(0, generated);
                }
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
        for (start, phrase, _) in super::extras::casual_phrases(reading) {
            let end = start + phrase.len();
            if spans(&candidates[0])
                .any(|(a, b, word)| a == start && b == end && word.surface == phrase)
            {
                protected.push((start, end, phrase));
            }
        }
        let misused: Vec<_> = candidates
            .iter()
            .map(|candidate| super::negative_adverb::lacks_negation(reading, candidate))
            .collect();
        if protected.is_empty() && !misused[1..].contains(&true) {
            return self.choose_ranked(reranker, reading, context, candidates);
        }
        // The model can improve the rest of the sentence while the words
        // learned or registered by the user and casual phrases stay at their original offsets.
        // It may not switch to the adverb 大して without a negative ending.
        let (indices, eligible): (Vec<_>, Vec<_>) = candidates
            .iter()
            .enumerate()
            .filter(|&(index, candidate)| {
                keeps_personal_words(candidate, &protected) && (index == 0 || !misused[index])
            })
            .map(|(index, candidate)| (index, candidate.clone()))
            .unzip();
        if eligible.len() < 2 {
            return None;
        }
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
        let index = self
            .rerank
            .cached_choice(reranker, reading, context, candidates)?;
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
    ) -> Option<usize> {
        // Learning can change costs and word origins without changing the surfaces.
        let key: RerankKey = (text.to_string(), left_context.to_string(), res.to_vec());
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
                    // Other snapshots with the same text and context stay cached.
                    st.cache.retain(|(k, _)| k != self.1);
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

    /// `Reranker::generate`, memoized (including `None`) and coalesced across
    /// concurrent identical requests so background live conversion + Space
    /// does not run neural generation twice.
    fn cached_generate(
        &self,
        r: &dyn Reranker,
        text: &str,
        left_context: &str,
        best: &Conversion,
        spelled_cost: &dyn Fn(&str) -> Option<i32>,
    ) -> Option<String> {
        let words = best
            .segments
            .iter()
            .flat_map(|s| &s.words)
            .map(|w| {
                (
                    w.reading.clone(),
                    w.surface.clone(),
                    w.cost,
                    w.learned,
                    w.user,
                )
            })
            .collect();
        let key: GenerateKey = (text.to_string(), left_context.to_string(), best.cost, words);
        let mut st = self.decisions.lock().unwrap_or_else(|e| e.into_inner());
        let deadline = std::time::Instant::now() + RERANK_WAIT;
        loop {
            if let Some(hit) = st
                .gen_cache
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.clone())
            {
                return hit;
            }
            let now = std::time::Instant::now();
            if !st.gen_running.contains(&key) || now >= deadline {
                break;
            }
            st = self
                .done
                .wait_timeout(st, deadline - now)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        st.gen_running.push(key.clone());
        drop(st);
        struct Done<'a>(&'a RerankState, &'a GenerateKey, Option<Option<String>>);
        impl Drop for Done<'_> {
            fn drop(&mut self) {
                let mut st = self.0.decisions.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(pos) = st.gen_running.iter().position(|k| k == self.1) {
                    st.gen_running.swap_remove(pos);
                }
                if let Some(outcome) = self.2.take() {
                    st.gen_cache.retain(|(k, _)| k != self.1);
                    st.gen_cache.push((self.1.clone(), outcome));
                    if st.gen_cache.len() > RERANK_CACHE {
                        st.gen_cache.remove(0);
                    }
                }
                drop(st);
                self.0.done.notify_all();
            }
        }
        let mut done = Done(self, &key, None);
        let generated = r.generate_spelled(text, left_context, best, spelled_cost);
        done.2 = Some(generated.clone());
        generated
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};

    use super::*;
    use crate::converter::Segment;

    fn sample_conversion(surface: &str, cost: i32, word_cost: i32) -> Conversion {
        Conversion {
            segments: vec![Segment {
                reading: "あずみの".into(),
                surface: surface.into(),
                words: vec![Word {
                    reading: "あずみの".into(),
                    surface: surface.into(),
                    lid: 0,
                    rid: 0,
                    cost: word_cost,
                    learned: false,
                    user: false,
                }],
            }],
            cost,
        }
    }

    struct CountingReranker {
        calls: Arc<AtomicUsize>,
        result: Option<String>,
        barrier: Option<Arc<Barrier>>,
    }

    impl Reranker for CountingReranker {
        fn choose(&self, _: &str, _: &str, candidates: &[Conversion]) -> Option<usize> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            candidates
                .iter()
                .enumerate()
                .min_by_key(|(_, candidate)| candidate.cost)
                .map(|(index, _)| index)
        }

        fn generate(&self, _: &str, _: &str, _: &Conversion) -> Option<String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if let Some(b) = &self.barrier {
                b.wait();
            }
            self.result.clone()
        }
    }

    #[test]
    fn cached_choice_recomputes_when_the_same_surfaces_change_cost() {
        let state = RerankState::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let reranker = CountingReranker {
            calls: calls.clone(),
            result: None,
            barrier: None,
        };
        let mut candidates = [
            sample_conversion("阿曇野", 12000, 9000),
            sample_conversion("安曇野", 13000, 9500),
        ];
        for _ in 0..2 {
            assert_eq!(
                state.cached_choice(&reranker, "あずみの", "", &candidates),
                Some(0)
            );
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        candidates[1].cost = 11000;
        assert_eq!(
            state.cached_choice(&reranker, "あずみの", "", &candidates),
            Some(1)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn cached_generate_memoizes_some_and_none_and_invalidates_on_cost_change() {
        let state = RerankState::default();
        let best = sample_conversion("阿曇野", 12000, 9000);

        let calls_some = Arc::new(AtomicUsize::new(0));
        let r_some = CountingReranker {
            calls: calls_some.clone(),
            result: Some("安曇野".into()),
            barrier: None,
        };
        assert_eq!(
            state
                .cached_generate(&r_some, "あずみの", "", &best, &|_| None)
                .as_deref(),
            Some("安曇野")
        );
        assert_eq!(
            state
                .cached_generate(&r_some, "あずみの", "", &best, &|_| None)
                .as_deref(),
            Some("安曇野")
        );
        assert_eq!(calls_some.load(Ordering::SeqCst), 1);

        // Changing word cost invalidates the cached outcome.
        let changed = sample_conversion("阿曇野", 12000, 9500);
        assert_eq!(
            state
                .cached_generate(&r_some, "あずみの", "", &changed, &|_| None)
                .as_deref(),
            Some("安曇野")
        );
        assert_eq!(calls_some.load(Ordering::SeqCst), 2);

        // `None` results are cached as well.
        let calls_none = Arc::new(AtomicUsize::new(0));
        let r_none = CountingReranker {
            calls: calls_none.clone(),
            result: None,
            barrier: None,
        };
        assert_eq!(
            state.cached_generate(&r_none, "ほげ", "", &best, &|_| None),
            None
        );
        assert_eq!(
            state.cached_generate(&r_none, "ほげ", "", &best, &|_| None),
            None
        );
        assert_eq!(calls_none.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn cached_generate_coalesces_concurrent_requests() {
        let state = Arc::new(RerankState::default());
        let calls = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(Barrier::new(2));

        struct SlowReranker {
            calls: Arc<AtomicUsize>,
            started: Arc<Barrier>,
        }
        impl Reranker for SlowReranker {
            fn choose(&self, _: &str, _: &str, _: &[Conversion]) -> Option<usize> {
                None
            }
            fn generate(&self, _: &str, _: &str, _: &Conversion) -> Option<String> {
                if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    self.started.wait();
                }
                std::thread::sleep(std::time::Duration::from_millis(40));
                Some("安曇野".into())
            }
        }

        let reranker = Arc::new(SlowReranker {
            calls: calls.clone(),
            started: started.clone(),
        });
        let best = sample_conversion("阿曇野", 12000, 9000);

        let s1 = state.clone();
        let r1 = reranker.clone();
        let b1 = best.clone();
        let t1 = std::thread::spawn(move || {
            s1.cached_generate(r1.as_ref(), "あずみの", "", &b1, &|_| None)
        });

        // Wait until t1 has entered `generate` (so key is in `gen_running`), then invoke t2.
        started.wait();
        let res2 = state.cached_generate(reranker.as_ref(), "あずみの", "", &best, &|_| None);
        let res1 = t1.join().unwrap();

        assert_eq!(res1.as_deref(), Some("安曇野"));
        assert_eq!(res2.as_deref(), Some("安曇野"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
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
