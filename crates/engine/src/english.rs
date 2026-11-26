//! English words typed by their spelling in romaji mode. Keys such as
//! "github" are not romaji and read as ぎてゅb; the spellings below give
//! those readings back their word, also inside a sentence ("githubni" ->
//! GitHubに).

use std::collections::HashMap;
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

/// Names written both in katakana and in English (アマゾン, Amazon): the
/// katakana and the spelling for a hiragana reading of the katakana. The
/// katakana comes first among the candidates, then the spelling.
pub fn katakana_name(reading: &str) -> Option<(&'static str, &'static str)> {
    static NAMES: OnceLock<HashMap<String, (&'static str, &'static str)>> = OnceLock::new();
    NAMES
        .get_or_init(|| {
            let mut names = HashMap::new();
            for line in ENGLISH_WORDS.lines().filter(|line| !line.starts_with('#')) {
                if let Some((spelling, kana)) = line.split_once('\t').filter(|(_, kana)| !kana.is_empty()) {
                    names.entry(to_hiragana(kana)).or_insert((kana, spelling));
                }
            }
            names
        })
        .get(reading)
        .copied()
}

fn to_hiragana(katakana: &str) -> String {
    katakana
        .chars()
        .map(|c| match c {
            'ァ'..='ヶ' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
            _ => c,
        })
        .collect()
}

/// Whether `word` is a bundled spelling or on the word list, in any case
/// (Google, github, Test).
pub fn is_known_word(word: &str) -> bool {
    canonical_spelling(word).is_some() || in_word_list(&word.to_ascii_lowercase())
}

pub(crate) fn canonical_spelling(raw: &str) -> Option<&'static str> {
    spellings().find_map(|(spelling, _)| spells(spelling, raw).then_some(spelling))
}

pub(crate) fn starts_spelling(raw: &str) -> bool {
    spellings().any(|(spelling, _)| matches_spelling(spelling, raw, true))
}

/// Whether the word list has `word` (lowercase). The list is sorted, so
/// this is a binary search over its lines.
fn in_word_list(word: &str) -> bool {
    let list = WORD_LIST.as_bytes();
    let mut body = 0;
    while list[body..].starts_with(b"#") {
        body = list[body..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(list.len(), |p| body + p + 1);
    }
    let text = &list[body..];
    let line_at = |i: usize| {
        let start = text[..i]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |p| p + 1);
        let end = text[start..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(text.len(), |p| start + p);
        (start, end)
    };
    let (mut lo, mut hi) = (0, text.len());
    while lo < hi {
        let (start, end) = line_at((lo + hi) / 2);
        let line = std::str::from_utf8(&text[start..end])
            .unwrap_or("")
            .trim_end_matches('\r');
        match line.cmp(word) {
            std::cmp::Ordering::Equal => return true,
            std::cmp::Ordering::Less => lo = (end + 1).min(text.len()),
            std::cmp::Ordering::Greater => hi = start,
        }
    }
    false
}

/// Detect a listed spelling at a kana boundary, including between two
/// other spans ("testnobest" reads てstのべst).
pub(crate) fn has_listed_part(raw: &str, reading: &str) -> bool {
    if in_word_list(&raw.to_ascii_lowercase()) {
        return false;
    }
    let index = word_index();
    let kana = |ch| matches!(ch, 'ぁ'..='ゖ' | 'ー');
    reading.char_indices().any(|(start, _)| {
        if start > 0 && !reading[..start].ends_with(kana) {
            return false;
        }
        let rest = &reading[start..];
        rest.char_indices()
            .map(|(i, ch)| i + ch.len_utf8())
            .take_while(|&end| end <= index.max_len)
            .any(|end| {
                let tail = &rest[end..];
                (start > 0 || !tail.is_empty())
                    && (tail.is_empty() || tail.starts_with(kana))
                    && index.get(&rest[..end]).is_some()
            })
    })
}

/// Whether `raw` types `spelling`, including its punctuation when present.
/// Spaces, dots, hyphens and underscores may be left out ("vscode" types
/// "VS Code"), while symbols like the # in C# identify the entry.
pub fn spells(spelling: &str, raw: &str) -> bool {
    matches_spelling(spelling, raw, false)
}

fn typed_spelling_char(ch: char) -> char {
    match crate::width::half_ascii_char(ch) {
        '。' | '｡' => '.',
        'ー' | '−' => '-',
        ch => ch,
    }
}

fn matches_spelling(spelling: &str, raw: &str, prefix: bool) -> bool {
    if !spelling.is_ascii() || !spelling.bytes().any(|b| b.is_ascii_alphabetic()) {
        return false;
    }
    let mut keys = raw.chars().map(typed_spelling_char).peekable();
    for ch in spelling.chars() {
        if prefix && keys.peek().is_none() {
            return true;
        }
        if keys.next_if(|key| key.eq_ignore_ascii_case(&ch)).is_none()
            && !matches!(ch, ' ' | '.' | '-' | '_')
        {
            return false;
        }
    }
    keys.next().is_none()
}

/// A dedicated symbol key can stay in the composition if it is at a
/// punctuation position in a bundled spelling, even before it is complete.
pub(crate) fn accepts_spelling_symbol(raw: &str, symbol: char) -> bool {
    if raw.is_empty() || !typed_spelling_char(symbol).is_ascii_punctuation() {
        return false;
    }
    let mut keys = raw.to_owned();
    keys.push(symbol);
    spellings().any(|(spelling, _)| matches_spelling(spelling, &keys, true))
}

/// (reading, spelling) for each bundled spelling whose keys are not romaji:
/// the reading is what typing the spelling gives (ぎてゅb for GitHub), so a
/// lattice can place the word there. Spellings that are also romaji (IME,
/// いめ) are left out so they don't take over Japanese input.
pub fn romaji_readings() -> &'static [(String, &'static str)] {
    static READINGS: OnceLock<Vec<(String, &'static str)>> = OnceLock::new();
    READINGS.get_or_init(|| word_keys::bundled_readings(spellings()))
}

/// A registered spelling adjacent to kana should be converted as part of
/// the sentence, even if the entire sequence could be an unlisted word.
pub(crate) fn has_bundled_part(text: &str) -> bool {
    let kana = |ch: char| ('ぁ'..='ゖ').contains(&ch);
    let adjoins_kana =
        |start: usize, end: usize| text[..start].ends_with(kana) || text[end..].starts_with(kana);
    romaji_readings().iter().any(|(reading, _)| {
        text.match_indices(reading.as_str())
            .any(|(start, found)| adjoins_kana(start, start + found.len()))
    }) || marked_spelling_matches(text).any(|(start, end, _)| adjoins_kana(start, end))
}

fn marked_spellings() -> &'static [(String, &'static str)] {
    static READINGS: OnceLock<Vec<(String, &'static str)>> = OnceLock::new();
    READINGS.get_or_init(|| {
        let mut readings = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (spelling, _) in spellings().filter(|(s, _)| {
            s.is_ascii() && s.bytes().any(|b| b.is_ascii_digit() || b.is_ascii_punctuation())
        }) {
            let mut inputs = vec![String::new()];
            for key in spelling.bytes().filter(|&key| key != b' ') {
                if matches!(key, b'.' | b'-' | b'_') {
                    let omitted = inputs.clone();
                    for input in &mut inputs {
                        input.push(char::from(key));
                    }
                    inputs.extend(omitted);
                } else {
                    for input in &mut inputs {
                        input.push(char::from(key.to_ascii_lowercase()));
                    }
                }
            }
            for input in inputs {
                // Compact spellings without any markers already use the word index.
                if !input.bytes().any(|b| b.is_ascii_digit() || b.is_ascii_punctuation()) {
                    continue;
                }
                let reading = romaji::to_hiragana(&input);
                let doubled = word_keys::doubled_final_consonant(&reading);
                for reading in std::iter::once(reading).chain(doubled) {
                    if seen.insert(reading.clone()) {
                        readings.push((reading, spelling));
                    }
                }
            }
        }
        readings
    })
}

pub(crate) fn marked_spelling_matches(
    text: &str,
) -> impl Iterator<Item = (usize, usize, &'static str)> {
    let marked = text
        .bytes()
        .any(|b| b.is_ascii_digit() || b.is_ascii_punctuation())
        || text.contains(['。', 'ー', '・']);
    let spellings = if marked { marked_spellings() } else { &[] };
    spellings.iter().flat_map(move |(reading, spelling)| {
        text.match_indices(reading.as_str())
            .filter_map(move |(start, _)| {
                let end = start + reading.len();
                let in_identifier = |ch: char| ch.is_ascii_alphanumeric() || ch == '_';
                (!text[..start].ends_with(in_identifier) && !text[end..].starts_with(in_identifier))
                    .then_some((start, end, *spelling))
            })
    })
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
            // Typing "amazon" is typing the reading of アマゾン: the katakana
            // comes first there (katakana_name).
            if katakana_name(&reading).is_some() {
                continue;
            }
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
    for (start, end, _) in marked_spelling_matches(reading) {
        mark(start, end - start);
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
    for (merged, suffix_ch) in [("しゃ", 's'), ("ちゃ", 'c')] {
        if !reading.contains(merged) {
            continue;
        }
        let max_stem = index.max_len.max(32);
        let mut probe = String::new();
        for (pos, _) in reading.match_indices(merged) {
            let prefix = &reading[..pos];
            for (start, _) in prefix.char_indices().rev() {
                if pos - start > max_stem {
                    break;
                }
                probe.clear();
                probe.push_str(&prefix[start..]);
                probe.push(suffix_ch);
                let matched = romaji_readings().iter().any(|(r, _)| r == &probe)
                    || (listed && index.get(&probe).is_some());
                if matched {
                    mark(start, pos - start);
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
        || marked_spellings().iter().any(|(r, s)| r == reading && *s == surface)
        || kana_spelled_readings().iter().any(|(r, s)| r == reading && *s == surface)
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
    fn spellings_accept_optional_separators_and_normalize_width() {
        assert!(spells("GitHub", "github"));
        assert!(spells("VS Code", "vscode"));
        assert!(spells("Node.js", "nodejs"));
        assert!(spells("M.2", "ｍ。２"));
        assert!(spells("GLM-5.3", "glm5。3"));
        assert!(spells("Q4_K_M", "q4km"));
        assert!(spells("C#", "c＃"));
        assert!(!spells("M.2", "m-2"));
        assert!(!spells("GGUF", "gg.uf"));
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
    fn word_list_lookup_finds_every_word() {
        let words: Vec<&str> = WORD_LIST
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
            .collect();
        for w in &words {
            assert!(in_word_list(w), "{w}");
        }
        for w in ["", "a", "zzzzzz", "helloo", "aaaaa"] {
            assert!(!in_word_list(w), "{w}");
        }
        assert!(is_known_word("Hello") && is_known_word("GitHub") && !is_known_word("Hellowo"));
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
