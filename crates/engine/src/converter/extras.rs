//! Additional lattice words from learning, the user dictionary, numbers, and English.

use super::{Converter, UserHistory};

/// Where an extra lattice word came from.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Origin {
    Learned,
    User,
    /// A number read out in kana (see [`crate::number`]).
    Number,
    /// An English word typed by its spelling (see [`crate::english`]).
    Spelling,
}

/// A word placed into the lattice in addition to the system dictionary.
pub(super) struct Extra {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) lid: u16,
    pub(super) rid: u16,
    pub(super) cost: i32,
    pub(super) surface: String,
    pub(super) origin: Origin,
}

/// Base cost of a user dictionary word whose spelling the system dictionary
/// does not have for that reading (a cheaper system cost is used instead).
const USER_WORD_COST: i32 = 3000;
/// How much cheaper a user word is than its base cost.
const USER_WORD_BONUS: i32 = 1000;
/// Cost of a number written in kanji (三千); the Arabic forms cost more.
const NUMBER_COST: i32 = 2000;
const NUMBER_ARABIC_EXTRA: i32 = 900;
/// Extra cost when the reading is also a dictionary word.
const NUMBER_WORD_PENALTY: i32 = 800;
/// Extra cost for a number ending in に, which is usually the particle.
const NUMBER_PARTICLE_PENALTY: i32 = 1500;
/// Cost of an English word typed by its spelling (ぎてゅb -> GitHub). The
/// letters around it would otherwise be unknown runs, which cost more.
const SPELLING_COST: i32 = 2600;
/// Cost of a word from the English word list typed by its spelling.
const WORD_LIST_COST: i32 = 3000;
/// How much more the English spelling costs than its loanword.
const LOANWORD_MARGIN: i32 = 400;
/// Dictionary cost above which a guessed loanword is not trusted.
const LOANWORD_MAX_COST: i32 = 5600;

impl Converter {
    /// User dictionary words that occur in `text`, as lattice extras.
    pub(super) fn user_extras(&self, text: &str, out: &mut Vec<Extra>) {
        let d = self.user_dict();
        if d.is_empty() {
            return;
        }
        for (pos, _) in text.char_indices() {
            d.common_prefix(&text[pos..], |l, w| {
                let Some(feature) = crate::user_dict::pos_feature(&w.pos) else {
                    return;
                };
                let id = self.dict.pos_id(feature).unwrap_or(self.dict.unknown_id());
                let base = self
                    .dict
                    .lookup(&text[pos..pos + l])
                    .into_iter()
                    .filter(|e| e.surface == w.surface)
                    .map(|e| e.cost as i32)
                    .min()
                    .unwrap_or(USER_WORD_COST)
                    .min(USER_WORD_COST);
                out.push(Extra {
                    start: pos,
                    end: pos + l,
                    lid: id,
                    rid: id,
                    cost: (base - USER_WORD_BONUS).max(0),
                    surface: w.surface.clone(),
                    origin: Origin::User,
                });
            });
        }
    }

    /// Numbers read out in kana in `text` (さんぜん), as lattice extras in
    /// kanji (三千) and Arabic digits (3000, and 10万 for large values).
    pub(super) fn number_extras(&self, text: &str, out: &mut Vec<Extra>) {
        let Some((kanji_id, arabic_id)) = self.number_pos else {
            return;
        };
        for (pos, _) in text.char_indices() {
            for n in crate::number::parse_prefixes(&text[pos..]) {
                let (start, end) = (pos, pos + n.len);
                // A reading that is also an ordinary word (さんじゅう 三重)
                // is more often that word.
                let word = self.dict.lookup(&text[start..end]).iter().any(|e| {
                    !e.surface.chars().all(|c| {
                        c.is_ascii_digit() || "〇一二三四五六七八九十百千万億兆".contains(c)
                    })
                });
                // A trailing に is more often the particle (三重に重なる).
                let particle = text[..end].ends_with("に") && end - start > "に".len();
                let cost = NUMBER_COST
                    + if word { NUMBER_WORD_PENALTY } else { 0 }
                    + if particle { NUMBER_PARTICLE_PENALTY } else { 0 };
                let mut push = |lid: u16, cost: i32, surface: String| {
                    out.push(Extra {
                        start,
                        end,
                        lid,
                        rid: lid,
                        cost,
                        surface,
                        origin: Origin::Number,
                    });
                };
                push(kanji_id, cost, crate::number::to_kanji(n.value));
                push(arabic_id, cost + NUMBER_ARABIC_EXTRA, n.value.to_string());
                if let Some(s) = crate::number::to_arabic_big(n.value) {
                    push(arabic_id, cost + NUMBER_ARABIC_EXTRA - 100, s);
                }
            }
        }
    }

    /// Every extra lattice word for `text`: learned and user dictionary
    /// words, and numbers.
    pub(super) fn extras(&self, text: &str) -> Vec<Extra> {
        let mut out = self.learned_extras(text);
        self.user_extras(text, &mut out);
        self.number_extras(text, &mut out);
        self.spelling_extras(text, &mut out);
        out
    }

    /// English words typed by their spelling in `text` (ぎてゅb), as
    /// lattice extras with that spelling (GitHub).
    pub(super) fn spelling_extras(&self, text: &str, out: &mut Vec<Extra>) {
        let id = self
            .dict
            .pos_id("名詞,一般,*,*,*,*,*")
            .unwrap_or(self.dict.unknown_id());
        // Spellings typed as romaji (ime reads いめ), at the start only.
        for (reading, spelling) in crate::english::kana_spelled_readings() {
            if text.starts_with(reading.as_str()) {
                out.push(Extra {
                    start: 0,
                    end: reading.len(),
                    lid: id,
                    rid: id,
                    cost: SPELLING_COST,
                    surface: (*spelling).to_string(),
                    origin: Origin::Spelling,
                });
                self.n_row_after(
                    spelling,
                    reading,
                    &text[reading.len()..],
                    reading.len(),
                    out,
                );
            }
        }
        if !text.bytes().any(|b| b.is_ascii_alphabetic()) {
            return;
        }
        for (reading, spelling) in crate::english::romaji_readings() {
            for (start, _) in text.match_indices(reading.as_str()) {
                out.push(Extra {
                    start,
                    end: start + reading.len(),
                    lid: id,
                    rid: id,
                    cost: SPELLING_COST,
                    surface: (*spelling).to_string(),
                    origin: Origin::Spelling,
                });
                let end = start + reading.len();
                self.n_row_after(spelling, reading, &text[end..], end, out);
            }
        }
        if !self.english_words_enabled() {
            return;
        }
        for (reading, word) in self.typed.lock().unwrap_or_else(|e| e.into_inner()).iter() {
            for (start, _) in text.match_indices(reading.as_str()) {
                out.push(Extra {
                    start,
                    end: start + reading.len(),
                    lid: id,
                    rid: id,
                    cost: SPELLING_COST + 200,
                    surface: word.clone(),
                    origin: Origin::Spelling,
                });
            }
        }
        // Common English words ("kinoutestwo" reads きのうてstを).
        let index = crate::english::word_index();
        for (start, _) in text.char_indices() {
            let rest = &text[start..];
            for (i, c) in rest.char_indices() {
                let end = i + c.len_utf8();
                if end > index.max_len {
                    break;
                }
                if let Some(word) = index.get(&rest[..end]) {
                    // The loanword first when there is one (ベスト, then best).
                    let loanword = self.loanword(word);
                    let english_cost = WORD_LIST_COST
                        + if loanword.is_some() {
                            LOANWORD_MARGIN
                        } else {
                            0
                        };
                    if let Some((surface, lid, rid)) = loanword {
                        out.push(Extra {
                            start,
                            end: start + end,
                            lid,
                            rid,
                            cost: WORD_LIST_COST,
                            surface,
                            origin: Origin::Spelling,
                        });
                    }
                    out.push(Extra {
                        start,
                        end: start + end,
                        lid: id,
                        rid: id,
                        cost: english_cost,
                        surface: word.to_string(),
                        origin: Origin::Spelling,
                    });
                    self.n_row_after(word, &rest[..end], &rest[end..], start + end, out);
                }
            }
        }
    }

    /// After a word ending in n, "nn" reads ん: "pythonno" is ぴyてょんお,
    /// not Python + の. Lets the vowel after such a word read as the n-row
    /// kana (お -> の) when it is a dictionary word spelled in kana.
    fn n_row_after(&self, word: &str, reading: &str, next: &str, at: usize, out: &mut Vec<Extra>) {
        if !word.ends_with(['n', 'N']) || !reading.ends_with('ん') {
            return;
        }
        let Some(vowel) = next.chars().next() else {
            return;
        };
        let Some(i) = "あいうえお".chars().position(|v| v == vowel) else {
            return;
        };
        let kana = "なにぬねの".chars().nth(i).unwrap_or('の').to_string();
        for e in self
            .dict
            .lookup(&kana)
            .into_iter()
            .filter(|e| e.surface == kana)
        {
            out.push(Extra {
                start: at,
                end: at + vowel.len_utf8(),
                lid: e.lid,
                rid: e.rid,
                cost: i32::from(e.cost),
                surface: kana.clone(),
                origin: Origin::Spelling,
            });
        }
    }

    /// The common katakana loanword for an English word (best -> ベスト),
    /// with its POS ids, when katakana is preferred: a guessed reading whose
    /// katakana is a dictionary word. Rare dictionary words are left out;
    /// most of them are wrong guesses (game -> ガメ).
    fn loanword(&self, word: &str) -> Option<(String, u16, u16)> {
        if !self.katakana_on.load(std::sync::atomic::Ordering::Relaxed) {
            return None;
        }
        for reading in crate::english::loanword_readings(word) {
            if reading.chars().count() < 3 {
                continue;
            }
            let kata = crate::romaji::to_katakana(&reading);
            let best = self
                .dict
                .lookup(&reading)
                .into_iter()
                .filter(|e| e.surface == kata && i32::from(e.cost) <= LOANWORD_MAX_COST)
                .min_by_key(|e| e.cost);
            if let Some(e) = best {
                return Some((kata, e.lid, e.rid));
            }
        }
        None
    }

    /// The loanword for an English word typed whole, if it has one.
    pub fn loanword_for(&self, word: &str) -> Option<String> {
        if !self.english_words_enabled() {
            return None;
        }
        let reading = crate::romaji::to_hiragana(&word.to_ascii_lowercase());
        crate::english::word_index()
            .get(&reading)
            .filter(|w| w.eq_ignore_ascii_case(word))?;
        self.loanword(word).map(|(k, ..)| k)
    }

    /// Turns the English word list on or off (bundled spellings stay), and
    /// whether a listed word shows as its katakana loanword first.
    /// Learned words that occur in the reading, as lattice extras.
    fn learned_extras(&self, text: &str) -> Vec<Extra> {
        if !self.learning_enabled() {
            return Vec::new();
        }
        let mut h = self.history();
        h.reload_if_changed();
        self.learned_extras_from(text, &h)
    }

    pub(super) fn learned_extras_from(&self, text: &str, h: &UserHistory) -> Vec<Extra> {
        if h.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (pos, _) in text.char_indices() {
            h.common_prefix(&text[pos..], |l, e| {
                let base = self
                    .dict
                    .lookup(&text[pos..pos + l])
                    .into_iter()
                    .filter(|d| d.surface == e.surface)
                    .map(|d| d.cost as i32)
                    .min()
                    .unwrap_or(4000)
                    .min(4000);
                let bonus = 1500.0 + 300.0 * (1.0 + e.count as f32).ln();
                out.push(Extra {
                    start: pos,
                    end: pos + l,
                    lid: e.lid,
                    rid: e.rid,
                    cost: (base as f32 - bonus).max(0.0) as i32,
                    surface: e.surface.clone(),
                    origin: Origin::Learned,
                });
            });
        }
        out
    }
}
