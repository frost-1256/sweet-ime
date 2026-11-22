//! English words typed by their spelling in romaji mode. Keys such as
//! "github" are not romaji and read as ぎてゅb; the spellings below give
//! those readings back their word, also inside a sentence ("githubni" ->
//! GitHubに).

use std::sync::OnceLock;

use crate::dict::{TECH_TERMS, VRCHAT_TERMS};
use crate::{romaji, word_keys};

/// English words matched by their typed spelling only (not dictionary
/// entries with a Japanese reading).
pub const ENGLISH_WORDS: &str = include_str!("english.tsv");

/// Common lowercase English words (SCOWL size 35), one per line. Only the
/// ones whose keys are not romaji are used ("test" reads てst; "sushi" stays
/// すし).
pub const WORD_LIST: &str = include_str!("english_words.txt");

/// Bundled spellings with their optional kana candidate: technical and
/// VRChat terms first (in file order), then the English word list.
pub fn spellings() -> impl Iterator<Item = (&'static str, Option<&'static str>)> {
    word_keys::bundled_spellings(TECH_TERMS, VRCHAT_TERMS, ENGLISH_WORDS)
}

/// Whether the keys `raw` type `spelling`: the same letters and digits in
/// any case, ignoring spaces, dots and hyphens ("vscode" types "VS Code").
pub fn spells(spelling: &str, raw: &str) -> bool {
    let mut s = spelling.chars().filter(|c| !matches!(c, ' ' | '.' | '-'));
    let mut r = raw.chars();
    loop {
        match (s.next(), r.next()) {
            (None, None) => return true,
            (Some(a), Some(b)) if a.is_ascii_alphanumeric() && a.eq_ignore_ascii_case(&b) => {}
            _ => return false,
        }
    }
}

/// (reading, spelling) for each bundled spelling whose keys are not romaji:
/// the reading is what typing the spelling gives (ぎてゅb for GitHub), so a
/// lattice can place the word there. Spellings that are also romaji (IME,
/// いめ) are left out so they don't take over Japanese input.
pub fn romaji_readings() -> &'static [(String, &'static str)] {
    static READINGS: OnceLock<Vec<(String, &'static str)>> = OnceLock::new();
    READINGS.get_or_init(|| word_keys::bundled_readings(spellings()))
}

/// (reading, spelling) for each bundled spelling whose keys are all romaji
/// (ime reads いめ for IME). These only convert at the start of the text,
/// where Japanese rarely begins with such kana.
pub fn kana_spelled_readings() -> &'static [(String, &'static str)] {
    static READINGS: OnceLock<Vec<(String, &'static str)>> = OnceLock::new();
    READINGS.get_or_init(|| {
        let mut out: Vec<(String, &'static str)> = Vec::new();
        for (spelling, _) in spellings() {
            let keys: String = spelling.chars().filter(|c| !matches!(c, ' ' | '.' | '-')).collect();
            if keys.len() < 3 || !keys.bytes().all(|b| b.is_ascii_alphabetic()) {
                continue;
            }
            let reading = romaji::to_hiragana(&keys.to_ascii_lowercase());
            // A doubled first kana is usually Japanese (aao reads ああお,
            // as in ああおいしい).
            let mut kana = reading.chars();
            if kana.next() == kana.next() {
                continue;
            }
            if !reading.bytes().any(|b| b.is_ascii_alphabetic()) && !out.iter().any(|(r, _)| *r == reading) {
                out.push((reading, spelling));
            }
        }
        out
    })
}

/// Words in typed keys whose letters romaji splits around a little kana
/// ("kyouhasystemctl" reads きょうはsysてmctl): letter runs that are not
/// romaji, joined across at most two kana between them. Returns (reading,
/// keys) for each, when `raw` types exactly `reading`.
pub fn split_words(raw: &str, reading: &str) -> Vec<(String, String)> {
    let chars: Vec<(usize, char)> = reading.char_indices().collect();
    let letter = |c: char| c.is_ascii_alphabetic();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !letter(chars[i].1) {
            i += 1;
            continue;
        }
        // A letter run, then (≤2 kana, letter run) pairs.
        let start = i;
        let mut end = i;
        while end < chars.len() && letter(chars[end].1) {
            end += 1;
        }
        let mut joined = false;
        loop {
            let mut k = end;
            while k < chars.len() && !letter(chars[k].1) && k - end < 3 {
                k += 1;
            }
            let gap = k - end;
            if gap == 0 || gap > 2 || k >= chars.len() || !letter(chars[k].1) {
                break;
            }
            end = k;
            while end < chars.len() && letter(chars[end].1) {
                end += 1;
            }
            joined = true;
        }
        if joined {
            let (from, to) = (chars[start].0, chars.get(end).map_or(reading.len(), |c| c.0));
            let span = &reading[from..to];
            // Bundled and listed words already convert with their spelling.
            let known = romaji_readings().iter().any(|(r, _)| r == span) || word_index().get(span).is_some();
            if let Some(keys) = keys_for(raw, &reading[..from], &reading[..to]).filter(|_| !known) {
                out.push((span.to_string(), keys));
            }
        }
        i = end;
    }
    out
}

/// The keys of `raw` that type the part of the reading between the
/// prefixes `before` and `upto`.
fn keys_for(raw: &str, before: &str, upto: &str) -> Option<String> {
    let prefix = |text: &str| (0..=raw.len()).filter(|&i| raw.is_char_boundary(i)).find(|&i| romaji::to_hiragana(&raw[..i]) == text);
    let (a, b) = (prefix(before)?, prefix(upto)?);
    (a < b).then(|| raw[a..b].to_string())
}


/// Readings of the word list's words whose keys are not romaji, mapped to
/// the word (てst -> test). Bundled spellings are not included. The build
/// script builds it (see `word_keys::build_index`), so it is read-only data
/// shared by every process using the IME.
pub struct WordIndex {
    readings: &'static str,
    /// `word_keys::ENTRY_BYTES` per entry, sorted by reading.
    entries: &'static [u8],
    /// Longest reading, in bytes.
    pub max_len: usize,
}

static INDEX: WordIndex = WordIndex {
    readings: include_str!(concat!(env!("OUT_DIR"), "/word_readings.txt")),
    entries: include_bytes!(concat!(env!("OUT_DIR"), "/word_entries.bin")),
    max_len: include!(concat!(env!("OUT_DIR"), "/word_max_len.rs")),
};

impl WordIndex {
    fn reading(&self, i: usize) -> &'static str {
        let (start, len, ..) = word_keys::decode(self.entries, i);
        &self.readings[start as usize..start as usize + len as usize]
    }

    pub fn get(&self, reading: &str) -> Option<&'static str> {
        if reading.len() > self.max_len {
            return None;
        }
        let (mut lo, mut hi) = (0, self.len());
        while lo < hi {
            let mid = (lo + hi) / 2;
            match self.reading(mid).cmp(reading) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => {
                    let (.., start, len) = word_keys::decode(self.entries, mid);
                    return Some(&WORD_LIST[start as usize..start as usize + len as usize]);
                }
            }
        }
        None
    }

    pub fn len(&self) -> usize {
        self.entries.len() / word_keys::ENTRY_BYTES
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

pub fn word_index() -> &'static WordIndex {
    &INDEX
}

/// Guesses at the kana reading of an English word as a katakana loanword,
/// by filling in vowels the way loanwords do (best -> べすと, desk -> ですく,
/// check -> ちぇっく). Most guesses are wrong or not words; check them
/// against a dictionary.
pub fn loanword_readings(word: &str) -> Vec<String> {
    const MAX: usize = 16;
    let w: Vec<char> = word.to_ascii_lowercase().chars().collect();
    if w.iter().any(|c| !c.is_ascii_lowercase()) {
        return Vec::new();
    }
    let vowel = |c: char| "aeiou".contains(c);
    let vowel_at = |i: usize| w.get(i).is_some_and(|&c| vowel(c));
    // Only words read as spelled: vowel pairs (real, tail), th, w, ir/ur
    // (firm), a silent final e and plural -s (bucks) give wrong guesses
    // that are still words (レアル, タイル, フィルム, ブックス).
    let spelled = !w
        .windows(2)
        .any(|p| (vowel(p[0]) && vowel(p[1])) || matches!(p, ['t', 'h'] | ['i' | 'u', 'r']))
        && !w.contains(&'w')
        && !matches!(w.last(), Some('e' | 's'));
    if !spelled {
        return Vec::new();
    }
    let mut outs = vec![String::new()];
    let mut i = 0;
    while i < w.len() {
        let c = w[i];
        let last = i == w.len() - 1;
        let rest = &w[i..];
        let starts = |s: &str| rest.iter().copied().take(s.len()).eq(s.chars());
        let (opts, step): (Vec<String>, usize) =
            if rest.len() == 2 && matches!(rest, ['e' | 'o' | 'a', 'r']) {
                (vec!["a-".into()], 2)
            } else if starts("tion") {
                (vec!["shon".into()], 4)
            } else if starts("sh") || starts("ch") {
                let base: String = rest[..2].iter().collect();
                (
                    vec![if vowel_at(i + 2) {
                        base
                    } else {
                        format!("{base}i")
                    }],
                    2,
                )
            } else if starts("th") {
                (
                    vec![if vowel_at(i + 2) {
                        "s".into()
                    } else {
                        "su".into()
                    }],
                    2,
                )
            } else if starts("ck") {
                (vec!["kku".into()], 2)
            } else if c == 'y' && last {
                (vec!["i-".into(), "i".into()], 1)
            } else if vowel(c) {
                (vec![c.to_string()], 1)
            } else if c == 'x' {
                (vec!["kusu".into()], 1)
            } else {
                let next = w.get(i + 1).copied();
                let k = match c {
                    'l' => 'r',
                    'v' => 'b',
                    'q' => 'k',
                    'c' if next.is_some_and(|n| "eiy".contains(n)) => 's',
                    'c' => 'k',
                    x => x,
                };
                let opts = if vowel_at(i + 1) || (k == 'n' && !vowel_at(i + 1)) {
                    vec![k.to_string()]
                } else if next == Some(c) {
                    vec!["xtu".into()]
                } else if matches!(k, 't' | 'd') {
                    let mut v = vec![format!("{k}o")];
                    if last {
                        v.push(format!("xtu{k}o"));
                    }
                    v
                } else {
                    let mut v = vec![format!("{k}u")];
                    if last && matches!(k, 'k' | 'p' | 'g' | 'b') {
                        v.push(format!("xtu{k}u"));
                    }
                    v
                };
                (opts, 1)
            };
        let mut next_outs = Vec::with_capacity(MAX);
        'fill: for o in &outs {
            for p in &opts {
                if next_outs.len() == MAX {
                    break 'fill;
                }
                next_outs.push(format!("{o}{p}"));
            }
        }
        outs = next_outs;
        i += step;
    }
    outs.into_iter()
        .map(|r| romaji::to_hiragana(&r))
        .filter(|r| !r.bytes().any(|b| b.is_ascii_alphabetic()))
        .collect()
}

/// Whether every letter left in `reading` (keys that aren't romaji) is part
/// of an English word typed by its spelling: a bundled one, or with
/// `listed`, one from the word list. Such letters are a word, not a typo
/// ("busydesu" reads ぶsyです: busy, not 部署).
pub fn covers_letters(reading: &str, listed: bool) -> bool {
    if !reading.bytes().any(|b| b.is_ascii_alphabetic()) {
        return false;
    }
    let index = word_index();
    let mut covered = vec![false; reading.len()];
    let mut mark = |start: usize, len: usize| {
        covered[start..start + len]
            .iter_mut()
            .for_each(|c| *c = true)
    };
    for (r, _) in romaji_readings() {
        for (start, _) in reading.match_indices(r.as_str()) {
            mark(start, r.len());
        }
    }
    if listed {
        for (start, _) in reading.char_indices() {
            let rest = &reading[start..];
            for (i, c) in rest.char_indices() {
                let end = i + c.len_utf8();
                if end > index.max_len {
                    break;
                }
                if index.get(&rest[..end]).is_some() {
                    mark(start, end);
                }
            }
        }
    }
    reading
        .bytes()
        .zip(&covered)
        .all(|(b, c)| !b.is_ascii_alphabetic() || *c)
}

/// Whether `surface` is a bundled spelling or listed word typed as
/// `reading` (for keeping such words when reranking).
pub fn is_spelling(reading: &str, surface: &str) -> bool {
    word_index().get(reading) == Some(surface)
        || romaji_readings()
            .iter()
            .any(|(r, s)| r == reading && *s == surface)
        // A listed word's loanword (てst -> テスト): only these put katakana
        // over letters.
        || (reading.bytes().any(|b| b.is_ascii_alphabetic())
            && !surface.is_empty()
            && surface.chars().all(|c| matches!(c, 'ァ'..='ヺ' | 'ー')))
}

/// Whether `surface` is the English word typed as `reading`: ASCII
/// letters whose keys are not romaji ("test" for てst; not "kore" for これ).
pub fn typed_word(reading: &str, surface: &str) -> bool {
    surface.len() >= 3
        && surface.bytes().all(|b| b.is_ascii_alphabetic())
        && reading.bytes().any(|b| b.is_ascii_alphabetic())
        && romaji::to_hiragana(&surface.to_ascii_lowercase()) == reading
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spelling_ignores_case_and_separators() {
        assert!(spells("GitHub", "github"));
        assert!(spells("VS Code", "vscode"));
        assert!(spells("Node.js", "nodejs"));
        assert!(!spells("GitHub", "githu"));
        assert!(!spells("C#", "c"));
    }

    #[test]
    fn readings_are_only_for_keys_that_are_not_romaji() {
        let r = romaji_readings();
        assert!(
            r.iter()
                .any(|(reading, s)| reading == "ぎてゅb" && *s == "GitHub")
        );
        assert!(r.iter().any(|(_, s)| *s == "Discord"));
        // "ime" is also いめ; it stays Japanese.
        assert!(!r.iter().any(|(_, s)| *s == "IME"));
    }

    #[test]
    fn word_list_keeps_only_words_that_are_not_romaji() {
        let w = word_index();
        assert!(w.len() > 10_000, "{}", w.len());
        assert_eq!(w.get("てst"), Some("test"));
        assert_eq!(w.get("てsっ"), Some("test"));
        assert!(w.get("すし").is_none());
        assert!(w.get("からおけ").is_none());
        // Bundled spellings keep their own case.
        assert!(w.get("ぎてゅb").is_none());
        assert!(is_spelling("ぎてゅb", "GitHub"));
        assert!(is_spelling("てst", "test"));
        assert!(typed_word("てst", "test"));
        assert!(!typed_word("これ", "kore"));
        assert!(loanword_readings("best").contains(&"べすと".to_string()));
        assert!(loanword_readings("desktop").contains(&"ですくとっぷ".to_string()));
        assert!(loanword_readings("check").contains(&"ちぇっく".to_string()));
        assert!(covers_letters("こんしゅうはぶsyです", true));
        assert!(!covers_letters("こんしゅうはぶsyです", false));
        assert!(covers_letters("んぎんxをつかう", false));
        assert!(!covers_letters("しgとお", true));
        assert!(!covers_letters("これ", true));
    }
}

#[cfg(test)]
mod split_tests {
    use super::*;

    #[test]
    fn prebuilt_index_matches_the_word_list() {
        let bundled: std::collections::HashSet<&str> = romaji_readings().iter().map(|(r, _)| r.as_str()).collect();
        let (readings, entries) = word_keys::build_index(WORD_LIST, &bundled);
        assert_eq!(readings, INDEX.readings);
        assert_eq!(word_keys::encode(&entries), INDEX.entries);
        assert!(INDEX.len() > 50_000, "{}", INDEX.len());
        assert_eq!(INDEX.get("てst"), Some("test"));
        assert_eq!(INDEX.get("ぶsy"), Some("busy"));
        assert_eq!(INDEX.get("これ"), None);
    }

    #[test]
    fn letter_runs_join_across_little_kana() {
        let raw = "kyouhasystemctl";
        let reading = romaji::to_hiragana(raw);
        assert_eq!(split_words(raw, &reading), vec![("sysてmctl".to_string(), "systemctl".to_string())]);
        // One run alone, or runs far apart, are not joined.
        assert!(split_words("kinoutest", &romaji::to_hiragana("kinoutest")).is_empty());
        let raw = "bxkaisyanox";
        assert!(split_words(raw, &romaji::to_hiragana(raw)).is_empty());
    }
}
