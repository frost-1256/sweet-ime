//! Kana-to-kanji conversion, predictions, and user vocabulary.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::Mutex;

use crate::dict::Dictionary;
use crate::learning::UserHistory;
use crate::user_dict::UserDictionary;

mod extras;
mod lattice;
mod rerank;

use rerank::RerankState;
pub use rerank::Reranker;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Word {
    pub reading: String,
    pub surface: String,
    pub lid: u16,
    pub rid: u16,
    pub cost: i32,
    /// Came from the user's learning history.
    pub learned: bool,
    /// Came from the user dictionary.
    pub user: bool,
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

    /// Number of words taken from the user dictionary.
    pub fn user_words(&self) -> usize {
        self.segments.iter().flat_map(|s| &s.words).filter(|w| w.user).count()
    }
}

pub struct Converter {
    dict: Dictionary,
    reranker: Option<Box<dyn Reranker>>,
    history: Mutex<UserHistory>,
    user_dict: Mutex<UserDictionary>,
    /// POS ids for numbers in kanji and in Arabic digits, when the
    /// dictionary records POS features.
    number_pos: Option<(u16, u16)>,
    /// Runtime switches (see [`Converter::configure`]).
    rerank_on: AtomicBool,
    learning_on: AtomicBool,
    english_on: AtomicBool,
    katakana_on: AtomicBool,
    /// Recent reranker decisions: (reading, left context, candidates) -> index.
    rerank: RerankState,
    /// Words typed by their spelling in the current compositions, as
    /// (reading, spelling), newest last (see [`Converter::note_typed_word`]).
    typed: Mutex<Vec<(String, String)>>,
}

/// How many typed words [`Converter::note_typed_word`] keeps.
const TYPED_WORDS: usize = 16;

impl Converter {
    pub fn new(dict: Dictionary) -> Self {
        let number_pos = dict
            .pos_id("名詞,数,*,*,*,*,*")
            .zip(dict.pos_id("名詞,数,アラビア数字,*,*,*,*"));
        Self {
            dict,
            reranker: None,
            history: Mutex::new(UserHistory::new()),
            user_dict: Mutex::new(UserDictionary::new()),
            number_pos,
            rerank_on: AtomicBool::new(true),
            learning_on: AtomicBool::new(true),
            english_on: AtomicBool::new(true),
            katakana_on: AtomicBool::new(true),
            rerank: RerankState::default(),
            typed: Mutex::new(Vec::new()),
        }
    }

    /// Remembers that `reading` was typed as the keys `word` ("systemctl"
    /// reads sysてmctl), so conversions place the word there. Only a few
    /// recent words are kept; committing one learns it for good.
    pub fn note_typed_word(&self, reading: &str, word: &str) {
        let mut typed = self.typed.lock().unwrap_or_else(|e| e.into_inner());
        typed.retain(|(r, _)| r != reading);
        typed.push((reading.to_string(), word.to_string()));
        if typed.len() > TYPED_WORDS {
            typed.remove(0);
        }
    }

    pub fn with_history(self, h: UserHistory) -> Self {
        *self.history.lock().unwrap_or_else(|e| e.into_inner()) = h;
        self
    }

    fn history(&self) -> std::sync::MutexGuard<'_, UserHistory> {
        self.history.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn with_user_dict(self, d: UserDictionary) -> Self {
        *self.user_dict.lock().unwrap_or_else(|e| e.into_inner()) = d;
        self
    }

    /// The user dictionary, re-read if its file changed.
    fn user_dict(&self) -> std::sync::MutexGuard<'_, UserDictionary> {
        let mut d = self.user_dict.lock().unwrap_or_else(|e| e.into_inner());
        d.reload_if_changed();
        d
    }

    pub fn set_english_words(&self, on: bool, katakana: bool) {
        self.english_on.store(on, Relaxed);
        self.katakana_on.store(katakana, Relaxed);
    }

    pub fn english_words_enabled(&self) -> bool {
        self.english_on.load(Relaxed)
    }

    pub fn forget(&self, reading: &str, surface: &str) {
        self.history().forget(reading, surface);
    }

    /// Turns neural reranking and user learning on or off.
    pub fn configure(&self, rerank: bool, learning: bool) {
        self.rerank_on.store(rerank, Relaxed);
        self.learning_on.store(learning, Relaxed);
    }

    /// Applies the conversion settings used by all input frontends.
    pub fn apply_options(&self, options: &crate::Options) {
        self.configure(options.neural, options.learning);
        self.set_english_words(options.english_words, options.english_katakana);
    }

    fn learning_enabled(&self) -> bool {
        self.learning_on.load(Relaxed)
    }

    pub fn dictionary(&self) -> &Dictionary {
        &self.dict
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
        // Then user dictionary words, including an exact reading (words
        // registered as suggestion-only appear nowhere else).
        if !prefix.is_empty() {
            for w in self.user_dict().predictive(prefix) {
                if out.len() >= n {
                    break;
                }
                if seen.insert(w.surface.clone()) {
                    out.push(w.surface.clone());
                }
            }
        }
        if out.len() >= n {
            return out;
        }
        // Full names (a family name joined to a given name) are seldom what
        // a prefix means and read like other words: ほしいな would suggest
        // 星井七瀬. Learned and user words above still predict them.
        let family = self.dict.pos_id("名詞,固有名詞,人名,姓,*,*,*");
        let given = self.dict.pos_id("名詞,固有名詞,人名,名,*,*,*");
        let mut scored: Vec<(i32, &str)> = Vec::new();
        self.dict.for_each_prediction(prefix, 3000, |reading, e| {
            let extra = reading[prefix.len()..].chars().count();
            let full_name = family == Some(e.lid) && given == Some(e.rid);
            if extra != 0 && !full_name {
                scored.push((e.cost as i32 + 700 * extra as i32, e.surface));
            }
        });
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
}
