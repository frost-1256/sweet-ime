//! Kana-to-kanji conversion, predictions, and user vocabulary.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::Mutex;

use crate::dict::{Dictionary, PosClass};
use crate::learning::UserHistory;
use crate::user_dict::UserDictionary;

mod extras;
mod lattice;
mod release_phrase;
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

    /// A reading used by more than one kanji word in this conversion.
    pub fn repeated_kanji_reading(&self) -> Option<(&str, usize)> {
        let words = self.segments.iter().flat_map(|segment| &segment.words);
        let mut readings = Vec::new();
        for word in words.filter(|word| kanji_homophone(word)) {
            if let Some((_, count)) = readings
                .iter_mut()
                .find(|(r, _)| *r == word.reading.as_str())
            {
                *count += 1;
            } else {
                readings.push((word.reading.as_str(), 1));
            }
        }
        readings
            .into_iter()
            .filter(|(_, count)| *count >= 2)
            .max_by_key(|(_, count)| *count)
    }

    /// Number of pairs that use the same spelling for a repeated reading.
    /// A candidate must keep the number of separately converted occurrences.
    pub fn homophone_repetitions(&self, reading: &str, count: usize) -> Option<usize> {
        let mut spellings = Vec::new();
        let mut pairs = 0;
        for word in self.segments.iter().flat_map(|segment| &segment.words) {
            if word.reading == reading && kanji_homophone(word) {
                pairs += spellings
                    .iter()
                    .filter(|&&surface| surface == word.surface)
                    .count();
                spellings.push(word.surface.as_str());
            }
        }
        (spellings.len() == count).then_some(pairs)
    }
}

fn kanji_homophone(word: &Word) -> bool {
    word.reading.len() >= 6
        && word
            .reading
            .chars()
            .all(|c| matches!(c, 'ぁ'..='ゖ' | 'ー'))
        && !word.surface.is_empty()
        && word.surface.chars().all(|c| matches!(c, '一'..='鿿'))
        && !word
            .surface
            .chars()
            .all(|c| "〇一二三四五六七八九十百千万億兆".contains(c))
}

pub struct Converter {
    dict: Dictionary,
    reranker: Option<Box<dyn Reranker>>,
    history: Mutex<UserHistory>,
    user_dict: Mutex<UserDictionary>,
    /// POS ids for numbers in kanji and in Arabic digits, when the
    /// dictionary records POS features.
    number_pos: Option<(u16, u16)>,
    /// Cached POS id for `名詞,一般,*,*,*,*,*` (falls back to `unknown_id`).
    noun_pos: u16,
    /// Cached POS ids for the release-announcement phrase (`でたね`).
    release_phrase_pos: Option<(u16, u16)>,
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
        let noun_pos = dict
            .pos_id("名詞,一般,*,*,*,*,*")
            .unwrap_or_else(|| dict.unknown_id());
        let release_phrase_pos = dict
            .pos_id("動詞,自立,*,*,一段,連用形,出る")
            .zip(dict.pos_id("助詞,終助詞,*,*,*,*,ね"));
        Self {
            dict,
            reranker: None,
            history: Mutex::new(UserHistory::new()),
            user_dict: Mutex::new(UserDictionary::new()),
            number_pos,
            noun_pos,
            release_phrase_pos,
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
        self.predict_after(prefix, n, None)
    }

    fn predict_after(&self, prefix: &str, n: usize, previous: Option<u16>) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        if self.learning_enabled() {
            let h = self.history();
            for (_, e) in h.predictive(prefix) {
                if out.len() >= n {
                    break;
                }
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
                let connection = previous.map_or(0, |rid| self.dict.connection(rid, e.lid));
                scored.push((e.cost as i32 + 700 * extra as i32 + connection, e.surface));
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
    /// then completions at the end of the best conversion. The unfinished
    /// word can span a boundary in the provisional conversion.
    pub fn predict_sentence(&self, text: &str, n: usize) -> Vec<String> {
        self.predict_with_conversion(text, n, None)
    }

    pub(crate) fn predict_with_conversion(
        &self,
        text: &str,
        n: usize,
        best: Option<&Conversion>,
    ) -> Vec<String> {
        let mut out = self.predict(text, n);
        if out.len() >= n {
            return out;
        }
        let computed;
        let best = if best.is_some() {
            best
        } else {
            computed = self.convert(text, &[], 1);
            computed.first()
        };
        let Some((last, head)) = best.and_then(|conv| conv.segments.split_last()) else {
            return out;
        };
        let mut surface: String = head.iter().map(|s| s.surface.as_str()).collect();
        let previous = head
            .last()
            .and_then(|segment| segment.words.last())
            .map(|word| word.rid);
        // Try a longer prefix first. For example the provisional
        // ライブラリー | おブル can complete as LibraryOfRuina.
        if let Some((penultimate, earlier)) =
            head.split_last().filter(|(_, earlier)| !earlier.is_empty())
        {
            let prefix = format!("{}{}", penultimate.reading, last.reading);
            if prefix.chars().count() >= 4 {
                Self::append_predictions(
                    &mut out,
                    self.predict_after(
                        &prefix,
                        n,
                        earlier.last().and_then(|s| s.words.last()).map(|w| w.rid),
                    ),
                    &surface[..surface.len() - penultimate.surface.len()],
                    n,
                );
            }
        }
        if !head.is_empty() && out.len() < n && last.reading.chars().count() >= 2 {
            Self::append_predictions(
                &mut out,
                self.predict_after(&last.reading, n, previous),
                &surface,
                n,
            );
        }
        // If no completion fits the bunsetsu, a noun in it may already
        // be complete (お返事 | あり). Leave verb inflections intact.
        let mut tail = last.reading.as_str();
        for word in &last.words {
            if !out.is_empty() {
                break;
            }
            tail = &tail[word.reading.len()..];
            surface.push_str(&word.surface);
            if tail.chars().count() < 2 {
                break;
            }
            if self.dict.pos_class(word.rid) == PosClass::Noun {
                Self::append_predictions(
                    &mut out,
                    self.predict_after(tail, n, Some(word.rid)),
                    &surface,
                    n,
                );
            }
        }
        out
    }

    fn append_predictions(out: &mut Vec<String>, predictions: Vec<String>, head: &str, n: usize) {
        for prediction in predictions {
            if out.len() >= n {
                break;
            }
            let full = format!("{head}{prediction}");
            if !out.contains(&full) {
                out.push(full);
            }
        }
    }
}
