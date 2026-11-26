//! Additional lattice words from learning, the user dictionary, numbers, and English.

use super::{Converter, UserHistory, release_phrase};

/// Where an extra lattice word came from.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Origin {
    Learned,
    User,
    /// A number read out in kana (see [`crate::number`]).
    Number,
    /// An English word typed by its spelling (see [`crate::english`]).
    Spelling,
    /// A colloquial long vowel kept in hiragana (ちょーだい).
    LongVowel,
    ReleasePhrase,
    Greeting,
    CasualPhrase,
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
pub(super) const NUMBER_ARABIC_EXTRA: i32 = 900;
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
/// How much cheaper a long vowel kept in hiragana (ちょーだい) is than the
/// katakana word for the same reading (チョーダイ).
const LONG_VOWEL_BONUS: i32 = 200;
/// How much more common the hiragana word (ちょうだい) must be than the
/// katakana one (チョーダイ). Loanwords are close to or below their hiragana
/// lookalikes (ソース 3911, そうす 3464).
const LONG_VOWEL_MARGIN: i32 = 1500;

/// The vowel a hiragana character ends in, for spelling out ー after it.
fn vowel(c: char) -> Option<char> {
    const ROWS: [(&str, char); 5] = [
        ("あかさたなはまやらわがざだばぱぁゃゎ", 'あ'),
        ("いきしちにひみりぎじぢびぴぃ", 'い'),
        ("うくすつぬふむゆるぐずづぶぷぅゅ", 'う'),
        ("えけせてねへめれげぜでべぺぇ", 'え'),
        ("おこそとのほもよろをごぞどぼぽぉょ", 'お'),
    ];
    ROWS.iter().find(|(row, _)| row.contains(c)).map(|&(_, v)| v)
}

/// The readings of `reading` with each ー spelled out the way it is in
/// ordinary words (ちょー -> ちょう, ちょお; ねー -> ねい, ねえ).
fn spelled_out(reading: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut prev = None;
    for c in reading.chars() {
        if c != 'ー' {
            out.iter_mut().for_each(|r| r.push(c));
            prev = Some(c);
            continue;
        }
        let Some(v) = prev.and_then(vowel) else {
            return Vec::new();
        };
        let spellings: &[char] = match v {
            'え' => &['い', 'え'],
            'お' => &['う', 'お'],
            _ => &[v],
        };
        out = out
            .iter()
            .flat_map(|r| spellings.iter().map(move |&s| format!("{r}{s}")))
            .collect();
        if out.len() > 8 {
            return Vec::new();
        }
    }
    out
}

/// Casual readings that are usually written as typed on their own.
pub(super) fn casual_phrase(text: &str) -> Option<(&'static str, &'static str)> {
    [
        ("もうそろ", "そろそろ"),
        ("あーね", "ああ"),
        ("りょ", "ああ"),
    ]
    .into_iter()
    .find(|(reading, _)| {
        text.strip_prefix(reading).is_some_and(|tail| {
            tail.is_empty()
                || tail.starts_with(['、', '。', '！', '？', '…', '!', '?'])
                || (*reading == "もうそろ"
                    && ["いこう", "いく", "かえ", "ねる", "でかけ", "はじめ", "おわ"]
                        .iter()
                        .any(|head| tail.starts_with(head)))
        })
    })
}

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
                } else if (200..=900).contains(&n.value) && n.value % 100 == 0 {
                    push(
                        arabic_id,
                        cost + NUMBER_ARABIC_EXTRA + 200,
                        format!("{}百", n.value / 100),
                    );
                } else if (2000..=9000).contains(&n.value) && n.value % 1000 == 0 {
                    push(
                        arabic_id,
                        cost + NUMBER_ARABIC_EXTRA + 200,
                        format!("{}千", n.value / 1000),
                    );
                }
            }
        }
    }

    /// Returns an Arabic-digit variant (`"1人"`, `"2個"`, ...) when `surface`
    /// consists of a single kanji digit (`一`..`九`) followed by a common counter.
    /// Backed by a static table so lattice construction needs neither a second
    /// FST walk nor per-node string allocations.
    #[inline]
    pub(super) fn arabic_counter_variant(surface: &str) -> Option<&'static str> {
        // Every kanji digit is 3 UTF-8 bytes and every counter below is 3..=6 bytes.
        let len = surface.len();
        if !(6..=9).contains(&len) {
            return None;
        }
        let mut chars = surface.chars();
        let digit_idx = match chars.next()? {
            '一' => 0,
            '二' => 1,
            '三' => 2,
            '四' => 3,
            '五' => 4,
            '六' => 5,
            '七' => 6,
            '八' => 7,
            '九' => 8,
            _ => return None,
        };
        let rest = chars.as_str();
        const COUNTERS: [&str; 59] = [
            "人", "個", "本", "枚", "冊", "台", "回", "階", "年", "月", "日", "時", "分", "秒",
            "円", "歳", "才", "票", "位", "番", "件", "名", "つ", "度", "軒", "匹", "頭", "羽",
            "杯", "脚", "組", "点", "社", "校", "駅", "曲", "話", "章", "行", "列", "面", "勝",
            "敗", "倍", "割", "歩", "泊", "巻", "着", "足", "丁", "粒", "袋", "箱", "缶", "瓶",
            "皿", "玉", "つ目",
        ];
        let counter_idx = match rest {
            "人" => 0, "個" => 1, "本" => 2, "枚" => 3, "冊" => 4, "台" => 5, "回" => 6,
            "階" => 7, "年" => 8, "月" => 9, "日" => 10, "時" => 11, "分" => 12, "秒" => 13,
            "円" => 14, "歳" => 15, "才" => 16, "票" => 17, "位" => 18, "番" => 19, "件" => 20,
            "名" => 21, "つ" => 22, "度" => 23, "軒" => 24, "匹" => 25, "頭" => 26, "羽" => 27,
            "杯" => 28, "脚" => 29, "組" => 30, "点" => 31, "社" => 32, "校" => 33, "駅" => 34,
            "曲" => 35, "話" => 36, "章" => 37, "行" => 38, "列" => 39, "面" => 40, "勝" => 41,
            "敗" => 42, "倍" => 43, "割" => 44, "歩" => 45, "泊" => 46, "巻" => 47, "着" => 48,
            "足" => 49, "丁" => 50, "粒" => 51, "袋" => 52, "箱" => 53, "缶" => 54, "瓶" => 55,
            "皿" => 56, "玉" => 57, "つ目" => 58,
            _ => return None,
        };
        static TABLE: std::sync::OnceLock<Vec<Box<str>>> = std::sync::OnceLock::new();
        let table = TABLE.get_or_init(|| {
            let mut v = Vec::with_capacity(9 * COUNTERS.len());
            for d in '1'..='9' {
                for c in COUNTERS {
                    v.push(format!("{d}{c}").into_boxed_str());
                }
            }
            v
        });
        Some(&table[digit_idx * COUNTERS.len() + counter_idx])
    }

    /// Every extra lattice word for `text`: learned and user dictionary
    /// words, and numbers.
    pub(super) fn extras(&self, text: &str) -> Vec<Extra> {
        let mut out = self.learned_extras(text);
        self.user_extras(text, &mut out);
        self.number_extras(text, &mut out);
        self.spelling_extras(text, &mut out);
        self.long_vowel_extras(text, &mut out);
        self.release_phrase_extra(text, &mut out);
        self.greeting_extra(text, &mut out);
        self.casual_phrase_extra(text, &mut out);
        out
    }

    fn casual_phrase_extra(&self, text: &str, out: &mut Vec<Extra>) {
        let Some((reading, reference)) = casual_phrase(text) else {
            return;
        };
        // A registered or learned spelling can override the default.
        if out.iter().any(|entry| {
            entry.start == 0
                && entry.end == reading.len()
                && matches!(entry.origin, Origin::Learned | Origin::User)
        }) {
            return;
        }
        let Some(word) = self
            .dict
            .lookup(reference)
            .into_iter()
            .filter(|entry| entry.surface == reference)
            .min_by_key(|entry| entry.cost)
        else {
            return;
        };
        out.push(Extra {
            start: 0,
            end: reading.len(),
            lid: word.lid,
            rid: word.rid,
            cost: i32::from(word.cost).min(1500),
            surface: reading.into(),
            origin: Origin::CasualPhrase,
        });
    }

    fn greeting_extra(&self, text: &str, out: &mut Vec<Extra>) {
        let Some((reading, surface)) = [("こんにちわ", "こんにちは"), ("こんばんわ", "こんばんは")]
            .into_iter()
            .find(|(reading, _)| {
                text.strip_prefix(reading).is_some_and(|rest| {
                    rest.is_empty() || rest.starts_with(['、', '。', '！', '？', '…'])
                })
            })
        else {
            return;
        };
        // Borrow the dictionary's POS and prefer the standard spelling over
        // the phonetic one. The original reading remains a candidate.
        let standard = self
            .dict
            .lookup(surface)
            .into_iter()
            .filter(|entry| entry.surface == surface)
            .min_by_key(|entry| entry.cost);
        if let Some(entry) = standard {
            out.push(Extra {
                start: 0,
                end: reading.len(),
                lid: entry.lid,
                rid: entry.rid,
                cost: i32::from(entry.cost) - 700,
                surface: surface.to_owned(),
                origin: Origin::Greeting,
            });
        }
    }

    fn release_phrase_extra(&self, text: &str, out: &mut Vec<Extra>) {
        let Some((start, end)) = release_phrase::span(text) else {
            return;
        };
        if let Some((lid, rid)) = self.release_phrase_pos {
            out.push(Extra {
                start,
                end,
                lid,
                rid,
                // Lower than the uncontracted path through 出 + た + ね.
                cost: 1800,
                surface: release_phrase::SURFACE.into(),
                origin: Origin::ReleasePhrase,
            });
        }
    }

    /// Colloquial long vowels kept in hiragana. A katakana dictionary word
    /// with ー (チョーダイ) whose reading, with ー spelled out, is a much
    /// more common hiragana word (ちょうだい) is offered in hiragana too
    /// (ちょーだい), a little ahead of the katakana. Loanwords keep their
    /// katakana: ホール is more common than ほうる.
    pub(super) fn long_vowel_extras(&self, text: &str, out: &mut Vec<Extra>) {
        if !text.contains('ー') {
            return;
        }
        for (pos, _) in text.char_indices() {
            // Katakana words with ー starting here, by length.
            let mut katakana: Vec<(usize, u16, u16, i32)> = Vec::new();
            self.dict.common_prefix(&text[pos..], |l, e| {
                let reading = &text[pos..pos + l];
                if reading.contains('ー') && e.surface == crate::romaji::to_katakana(reading) {
                    katakana.push((l, e.lid, e.rid, i32::from(e.cost)));
                }
            });
            let mut lengths: Vec<usize> = katakana.iter().map(|k| k.0).collect();
            lengths.dedup();
            for l in lengths {
                let reading = &text[pos..pos + l];
                let words = katakana.iter().filter(|k| k.0 == l);
                // Against the most common katakana word (シーン, not a rare
                // シーン with another part of speech).
                let Some(least) = words.clone().map(|k| k.3).min() else { continue };
                let hiragana = spelled_out(reading).iter().any(|r| {
                    self.dict.lookup(r).iter().any(|h| h.surface == r && i32::from(h.cost) + LONG_VOWEL_MARGIN < least)
                });
                if !hiragana {
                    continue;
                }
                for &(_, lid, rid, cost) in words {
                    out.push(Extra {
                        start: pos,
                        end: pos + l,
                        lid,
                        rid,
                        cost: cost - LONG_VOWEL_BONUS,
                        surface: reading.to_string(),
                        origin: Origin::LongVowel,
                    });
                }
            }
        }
    }

    /// English words typed by their spelling in `text` (ぎてゅb), as
    /// lattice extras with that spelling (GitHub).
    pub(super) fn spelling_extras(&self, text: &str, out: &mut Vec<Extra>) {
        let id = self.noun_pos;
        for (start, end, spelling) in crate::english::marked_spelling_matches(text) {
            out.push(Extra {
                start,
                end,
                lid: id,
                rid: id,
                cost: SPELLING_COST,
                surface: spelling.to_owned(),
                origin: Origin::Spelling,
            });
        }
        // Spellings typed as romaji (ime reads いめ), at the start only, or
        // anywhere when no Japanese word starts like them (nvidia reads
        // んゔぃぢあ).
        let mut push_spelling = |start: usize, reading: &str, spelling: &str| {
            let end = start + reading.len();
            out.push(Extra {
                start,
                end,
                lid: id,
                rid: id,
                cost: SPELLING_COST,
                surface: spelling.to_string(),
                origin: Origin::Spelling,
            });
            self.n_row_after(spelling, reading, &text[end..], end, out);
        };
        // Pre-partition kana-spelled readings once so each conversion only checks
        // `anywhere` entries and `start_only` entries matching `text`'s first char.
        struct KanaSpelledIndex {
            anywhere: Vec<(&'static str, &'static str)>,
            by_first_char: std::collections::HashMap<char, Vec<(&'static str, &'static str)>>,
        }
        static KANA_SPELLED: std::sync::OnceLock<KanaSpelledIndex> = std::sync::OnceLock::new();
        let kana_index = KANA_SPELLED.get_or_init(|| {
            let mut anywhere = Vec::new();
            let mut by_first_char: std::collections::HashMap<char, Vec<(&'static str, &'static str)>> =
                std::collections::HashMap::new();
            for (reading, spelling) in crate::english::kana_spelled_readings() {
                let is_anywhere = *spelling == "issue"
                    || reading
                        .starts_with(['ん', 'ぁ', 'ぃ', 'ぅ', 'ぇ', 'ぉ', 'っ', 'ゃ', 'ゅ', 'ょ']);
                if is_anywhere {
                    anywhere.push((reading.as_str(), *spelling));
                } else if let Some(first) = reading.chars().next() {
                    by_first_char
                        .entry(first)
                        .or_default()
                        .push((reading.as_str(), *spelling));
                }
            }
            KanaSpelledIndex {
                anywhere,
                by_first_char,
            }
        });
        for &(reading, spelling) in &kana_index.anywhere {
            for (start, _) in text.match_indices(reading) {
                push_spelling(start, reading, spelling);
            }
        }
        if let Some(first) = text.chars().next()
            && let Some(entries) = kana_index.by_first_char.get(&first)
        {
            for &(reading, spelling) in entries {
                if text.starts_with(reading) {
                    push_spelling(0, reading, spelling);
                }
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
