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
    /// Returns the index of the best candidate, or `None` to keep the
    /// lattice order.
    fn choose(&self, reading: &str, left_context: &str, candidates: &[Conversion])
    -> Option<usize>;
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

/// The reranker's candidates from the cost-ordered `pool`: the `n` best,
/// then up to [`RERANK_EXTRA`] cheapest alternatives that differ from the
/// best in a part of the reading none of those does. The plain N-best tends
/// to spend every slot on one spot (機会が故障/こしょう/胡椒...) and never
/// offer another word elsewhere (機械).
fn diverse(pool: Vec<Conversion>, n: usize) -> Vec<Conversion> {
    if pool.len() <= n {
        return pool;
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
    let mut regions: Vec<(usize, usize)> = pool[1..n]
        .iter()
        .filter(|candidate| keeps_personal_words(candidate, &personal))
        .filter_map(region)
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
        if !regions.iter().any(|&(a, b)| r.0 < b && a < r.1) {
            regions.push(r);
            picked.push(i);
        }
    }
    let mut pool: Vec<Option<Conversion>> = pool.into_iter().map(Some).collect();
    picked.into_iter().filter_map(|i| pool[i].take()).collect()
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
            .map_or(1, |r| r.candidates().max(1));
        let mut res = if n > 1 {
            diverse(self.convert(text, boundaries, n * RERANK_POOL), n)
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
