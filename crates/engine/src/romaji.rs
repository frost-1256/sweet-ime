//! Incremental romaji -> hiragana composer.
//!
//! The table follows the common Japanese IME conventions (Hepburn + Kunrei
//! variants, `x`/`l` small kana, `nn`/`n'` for ん, doubled consonants for っ).

use std::collections::HashMap;
use std::sync::OnceLock;

/// (input, output, pending) — after `input` is matched, `output` is emitted
/// and `pending` is pushed back into the buffer.
const BASE: &[(&str, &str)] = &[
    ("a", "あ"), ("i", "い"), ("u", "う"), ("e", "え"), ("o", "お"),
    ("ka", "か"), ("ki", "き"), ("ku", "く"), ("ke", "け"), ("ko", "こ"),
    ("sa", "さ"), ("si", "し"), ("shi", "し"), ("su", "す"), ("se", "せ"), ("so", "そ"),
    ("ta", "た"), ("ti", "ち"), ("chi", "ち"), ("tu", "つ"), ("tsu", "つ"), ("te", "て"), ("to", "と"),
    ("na", "な"), ("ni", "に"), ("nu", "ぬ"), ("ne", "ね"), ("no", "の"),
    ("ha", "は"), ("hi", "ひ"), ("hu", "ふ"), ("fu", "ふ"), ("he", "へ"), ("ho", "ほ"),
    ("ma", "ま"), ("mi", "み"), ("mu", "む"), ("me", "め"), ("mo", "も"),
    ("ya", "や"), ("yu", "ゆ"), ("yo", "よ"), ("ye", "いぇ"), ("yi", "い"),
    ("ra", "ら"), ("ri", "り"), ("ru", "る"), ("re", "れ"), ("ro", "ろ"),
    ("wa", "わ"), ("wi", "うぃ"), ("wu", "う"), ("we", "うぇ"), ("wo", "を"),
    ("ga", "が"), ("gi", "ぎ"), ("gu", "ぐ"), ("ge", "げ"), ("go", "ご"),
    ("za", "ざ"), ("zi", "じ"), ("ji", "じ"), ("zu", "ず"), ("ze", "ぜ"), ("zo", "ぞ"),
    ("da", "だ"), ("di", "ぢ"), ("du", "づ"), ("de", "で"), ("do", "ど"),
    ("ba", "ば"), ("bi", "び"), ("bu", "ぶ"), ("be", "べ"), ("bo", "ぼ"),
    ("pa", "ぱ"), ("pi", "ぴ"), ("pu", "ぷ"), ("pe", "ぺ"), ("po", "ぽ"),
    ("va", "ゔぁ"), ("vi", "ゔぃ"), ("vu", "ゔ"), ("ve", "ゔぇ"), ("vo", "ゔぉ"),
    // Yoon
    ("kya", "きゃ"), ("kyi", "きぃ"), ("kyu", "きゅ"), ("kye", "きぇ"), ("kyo", "きょ"),
    ("gya", "ぎゃ"), ("gyi", "ぎぃ"), ("gyu", "ぎゅ"), ("gye", "ぎぇ"), ("gyo", "ぎょ"),
    ("sya", "しゃ"), ("syi", "しぃ"), ("syu", "しゅ"), ("sye", "しぇ"), ("syo", "しょ"),
    ("sha", "しゃ"), ("shu", "しゅ"), ("she", "しぇ"), ("sho", "しょ"),
    ("zya", "じゃ"), ("zyi", "じぃ"), ("zyu", "じゅ"), ("zye", "じぇ"), ("zyo", "じょ"),
    ("ja", "じゃ"), ("ju", "じゅ"), ("je", "じぇ"), ("jo", "じょ"),
    ("jya", "じゃ"), ("jyi", "じぃ"), ("jyu", "じゅ"), ("jye", "じぇ"), ("jyo", "じょ"),
    ("tya", "ちゃ"), ("tyi", "ちぃ"), ("tyu", "ちゅ"), ("tye", "ちぇ"), ("tyo", "ちょ"),
    ("cha", "ちゃ"), ("chu", "ちゅ"), ("che", "ちぇ"), ("cho", "ちょ"),
    ("cya", "ちゃ"), ("cyi", "ちぃ"), ("cyu", "ちゅ"), ("cye", "ちぇ"), ("cyo", "ちょ"),
    ("dya", "ぢゃ"), ("dyi", "ぢぃ"), ("dyu", "ぢゅ"), ("dye", "ぢぇ"), ("dyo", "ぢょ"),
    ("nya", "にゃ"), ("nyi", "にぃ"), ("nyu", "にゅ"), ("nye", "にぇ"), ("nyo", "にょ"),
    ("hya", "ひゃ"), ("hyi", "ひぃ"), ("hyu", "ひゅ"), ("hye", "ひぇ"), ("hyo", "ひょ"),
    ("bya", "びゃ"), ("byi", "びぃ"), ("byu", "びゅ"), ("bye", "びぇ"), ("byo", "びょ"),
    ("pya", "ぴゃ"), ("pyi", "ぴぃ"), ("pyu", "ぴゅ"), ("pye", "ぴぇ"), ("pyo", "ぴょ"),
    ("mya", "みゃ"), ("myi", "みぃ"), ("myu", "みゅ"), ("mye", "みぇ"), ("myo", "みょ"),
    ("rya", "りゃ"), ("ryi", "りぃ"), ("ryu", "りゅ"), ("rye", "りぇ"), ("ryo", "りょ"),
    ("fa", "ふぁ"), ("fi", "ふぃ"), ("fe", "ふぇ"), ("fo", "ふぉ"),
    ("fya", "ふゃ"), ("fyu", "ふゅ"), ("fyo", "ふょ"),
    ("tsa", "つぁ"), ("tsi", "つぃ"), ("tse", "つぇ"), ("tso", "つぉ"),
    ("thi", "てぃ"), ("thu", "てゅ"), ("dhi", "でぃ"), ("dhu", "でゅ"),
    ("twu", "とぅ"), ("dwu", "どぅ"), ("whi", "うぃ"), ("whe", "うぇ"), ("who", "うぉ"),
    ("qa", "くぁ"), ("qi", "くぃ"), ("qe", "くぇ"), ("qo", "くぉ"), ("kwa", "くぁ"),
    ("gwa", "ぐぁ"),
    // Small kana
    ("xa", "ぁ"), ("xi", "ぃ"), ("xu", "ぅ"), ("xe", "ぇ"), ("xo", "ぉ"),
    ("la", "ぁ"), ("li", "ぃ"), ("lu", "ぅ"), ("le", "ぇ"), ("lo", "ぉ"),
    ("xya", "ゃ"), ("xyu", "ゅ"), ("xyo", "ょ"), ("lya", "ゃ"), ("lyu", "ゅ"), ("lyo", "ょ"),
    ("xtu", "っ"), ("xtsu", "っ"), ("ltu", "っ"), ("ltsu", "っ"),
    ("xwa", "ゎ"), ("lwa", "ゎ"), ("xka", "ゕ"), ("xke", "ゖ"),
    // N
    ("nn", "ん"), ("n'", "ん"), ("xn", "ん"),
    // Symbols
    ("-", "ー"), (",", "、"), (".", "。"), ("[", "「"), ("]", "」"),
    ("~", "〜"), ("/", "・"), ("z/", "・"), ("z.", "…"), ("z,", "‥"),
    ("zh", "←"), ("zj", "↓"), ("zk", "↑"), ("zl", "→"), ("z-", "〜"), ("z[", "『"), ("z]", "』"),
    ("!", "！"), ("?", "？"),
];

struct Table {
    exact: HashMap<String, (String, String)>,
    /// All proper prefixes of every input key.
    prefixes: std::collections::HashSet<String>,
}

fn table() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut exact: HashMap<String, (String, String)> = HashMap::new();
        for (k, v) in BASE {
            exact.insert((*k).into(), ((*v).into(), String::new()));
        }
        // Doubled consonants -> っ + consonant.
        for c in "bcdfghjklmpqrstvwxyz".chars() {
            exact.insert(format!("{c}{c}"), ("っ".into(), c.to_string()));
        }
        // n + consonant -> ん + consonant.
        for c in "bcdfghjklmpqrstvwxz".chars() {
            exact.insert(format!("n{c}"), ("ん".into(), c.to_string()));
        }
        let mut prefixes = std::collections::HashSet::new();
        for k in exact.keys() {
            for (i, _) in k.char_indices().skip(1) {
                prefixes.insert(k[..i].to_string());
            }
        }
        Table { exact, prefixes }
    })
}

/// Incremental composer. Feed ASCII keystrokes, read the hiragana text plus
/// the still-unresolved romaji tail.
#[derive(Default, Clone, Debug)]
pub struct Composer {
    kana: String,
    pending: String,
}

impl Composer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, ch: char) {
        let ch = ch.to_ascii_lowercase();
        self.pending.push(ch);
        self.resolve(false);
        if is_digit(ch) {
            self.number_separator();
        }
    }

    /// Between digits, 。 and 、 are a decimal point and a thousands
    /// separator ("5.5", "1,000"); elsewhere they stay Japanese ("5。").
    fn number_separator(&mut self) {
        let mut tail = self.kana.chars().rev();
        let (Some(last), Some(sep), Some(before)) = (tail.next(), tail.next(), tail.next()) else { return };
        if !is_digit(last) || !is_digit(before) {
            return;
        }
        let ascii = match sep {
            '。' => '.',
            '、' => ',',
            _ => return,
        };
        let at = self.kana.len() - last.len_utf8() - sep.len_utf8();
        self.kana.replace_range(at..at + sep.len_utf8(), ascii.encode_utf8(&mut [0; 4]));
    }

    pub fn push_str(&mut self, s: &str) {
        for ch in s.chars() {
            self.push(ch);
        }
    }

    fn resolve(&mut self, flush: bool) {
        let t = table();
        loop {
            if self.pending.is_empty() {
                return;
            }
            let is_prefix = t.prefixes.contains(&self.pending);
            if !flush && is_prefix {
                return; // wait for more input
            }
            if let Some((out, rest)) = t.exact.get(&self.pending) {
                self.kana.push_str(out);
                self.pending = rest.clone();
                if self.pending.is_empty() {
                    return;
                }
                continue;
            }
            if flush && self.pending == "n" {
                self.kana.push('ん');
                self.pending.clear();
                return;
            }
            if flush && is_prefix {
                // e.g. "ky" at end: emit literally
                let p = std::mem::take(&mut self.pending);
                self.kana.push_str(&p);
                return;
            }
            // No entry can match: emit the first char literally and retry.
            let first = self.pending.remove(0);
            if first == 'n' {
                self.kana.push('ん');
            } else {
                self.kana.push(first);
            }
        }
    }

    /// Resolve trailing input (e.g. a lone `n` becomes `ん`).
    pub fn flush(&mut self) {
        self.resolve(true);
    }

    pub fn backspace(&mut self) -> bool {
        if self.pending.pop().is_some() {
            return true;
        }
        self.kana.pop().is_some()
    }

    pub fn kana(&self) -> &str {
        &self.kana
    }

    pub fn pending(&self) -> &str {
        &self.pending
    }

    /// Text to display in the preedit: resolved kana + raw pending romaji.
    pub fn preedit(&self) -> String {
        format!("{}{}", self.kana, self.pending)
    }

    pub fn is_empty(&self) -> bool {
        self.kana.is_empty() && self.pending.is_empty()
    }

    pub fn clear(&mut self) {
        self.kana.clear();
        self.pending.clear();
    }
}

fn is_digit(c: char) -> bool {
    c.is_ascii_digit() || ('０'..='９').contains(&c)
}

/// Convert a whole romaji string to hiragana.
pub fn to_hiragana(romaji: &str) -> String {
    let mut c = Composer::new();
    c.push_str(romaji);
    c.flush();
    c.kana
}

/// Reconstructs a readable spelling when the original keystrokes are unavailable.
pub(crate) fn display_romaji(reading: &str) -> String {
    static REVERSE: OnceLock<HashMap<&str, &str>> = OnceLock::new();
    let table = REVERSE.get_or_init(|| {
        let mut table = HashMap::new();
        for &(keys, kana) in BASE {
            table.entry(kana).or_insert(keys);
        }
        table.extend([
            ("し", "shi"), ("ち", "chi"), ("つ", "tsu"), ("ふ", "fu"), ("じ", "ji"),
            ("しゃ", "sha"), ("しゅ", "shu"), ("しょ", "sho"), ("しぇ", "she"),
            ("ちゃ", "cha"), ("ちゅ", "chu"), ("ちょ", "cho"), ("ちぇ", "che"),
            ("じゃ", "ja"), ("じゅ", "ju"), ("じょ", "jo"), ("じぇ", "je"),
            ("ゐ", "wi"), ("ゑ", "we"),
        ]);
        table
    });
    let kana: String = reading
        .chars()
        .map(|ch| match ch {
            'ァ'..='ヶ' => char::from_u32(ch as u32 - 0x60).unwrap(),
            _ => ch,
        })
        .collect();
    let syllable = |text: &str| {
        let mut ends = text.char_indices().skip(1).map(|(i, _)| i).chain([text.len()]);
        let first = ends.next()?;
        ends.next()
            .and_then(|second| table.get(&text[..second]).map(|&keys| (keys, second)))
            .or_else(|| table.get(&text[..first]).map(|&keys| (keys, first)))
    };
    let mut rest = kana.as_str();
    let mut out = String::with_capacity(kana.len());
    while let Some(ch) = rest.chars().next() {
        let tail = &rest[ch.len_utf8()..];
        match ch {
            'ん' => {
                let next = syllable(tail).map_or(tail, |(keys, _)| keys);
                out.push_str(if next.starts_with(['a', 'i', 'u', 'e', 'o', 'n', 'y']) {
                    "n'"
                } else {
                    "n"
                });
                rest = tail;
            }
            'っ' => {
                let next = syllable(tail).and_then(|(keys, _)| keys.chars().next());
                if let Some(c) = next.filter(|&c| "bcdfghjklmpqrstvwxyz".contains(c)) {
                    out.push(c);
                } else {
                    out.push_str("xtsu");
                }
                rest = tail;
            }
            _ => {
                if let Some((keys, len)) = syllable(rest) {
                    out.push_str(keys);
                    rest = &rest[len..];
                } else {
                    out.push(ch);
                    rest = tail;
                }
            }
        }
    }
    out
}

/// Hiragana -> katakana (other characters pass through).
pub fn to_katakana(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'ぁ'..='ゖ' => char::from_u32(c as u32 + 0x60).unwrap(),
            _ => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic() {
        assert_eq!(to_hiragana("konnnichiha"), "こんにちは");
        assert_eq!(to_hiragana("kannji"), "かんじ");
        assert_eq!(to_hiragana("kanji"), "かんじ");
        assert_eq!(to_hiragana("gakkou"), "がっこう");
        assert_eq!(to_hiragana("shinbun"), "しんぶん");
        assert_eq!(to_hiragana("hon"), "ほん");
        assert_eq!(to_hiragana("kon'ya"), "こんや");
        assert_eq!(to_hiragana("tyotto"), "ちょっと");
        assert_eq!(to_hiragana("ra-menn"), "らーめん");
        assert_eq!(to_hiragana("watashiha,"), "わたしは、");
    }

    #[test]
    fn separators_between_digits() {
        assert_eq!(to_hiragana("5.5"), "5.5");
        assert_eq!(to_hiragana("1,000en"), "1,000えん");
        assert_eq!(to_hiragana("3.14"), "3.14");
        // Not between digits: Japanese punctuation.
        assert_eq!(to_hiragana("5."), "5。");
        assert_eq!(to_hiragana("5.desu"), "5。です");
        assert_eq!(to_hiragana("a.5"), "あ。5");
        // Typed one key at a time, the point turns once the digit comes.
        let mut c = Composer::new();
        c.push_str("5.");
        assert_eq!(c.preedit(), "5。");
        c.push('5');
        assert_eq!(c.preedit(), "5.5");
    }

    #[test]
    fn incremental() {
        let mut c = Composer::new();
        c.push_str("ky");
        assert_eq!(c.preedit(), "ky");
        c.push('o');
        assert_eq!(c.preedit(), "きょ");
        c.push('n');
        assert_eq!(c.preedit(), "きょn");
        c.flush();
        assert_eq!(c.kana(), "きょん");
    }

    #[test]
    fn katakana() {
        assert_eq!(to_katakana("こーひー"), "コーヒー");
    }

    #[test]
    fn display_romaji_handles_combined_kana_and_preserves_literal_input() {
        assert_eq!(
            display_romaji("こんにちは、がっこう！コーヒー Qwen3.6"),
            "kon'nichiha,gakkou!ko-hi- Qwen3.6"
        );
        assert_eq!(display_romaji("しゃちょっ"), "shachoxtsu");
    }
}
