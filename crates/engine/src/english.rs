//! English words typed by their spelling in romaji mode. Keys such as
//! "github" are not romaji and read as ぎてゅb; the spellings below give
//! those readings back their word, also inside a sentence ("githubni" ->
//! GitHubに).

use std::collections::HashSet;
use std::sync::OnceLock;

use crate::dict::{TECH_TERMS, VRCHAT_TERMS};
use crate::romaji;

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
    let terms = TECH_TERMS
        .lines()
        .chain(VRCHAT_TERMS.lines())
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let mut fields = line.split('\t').skip(1);
            let spelling = fields.next()?;
            Some((spelling, fields.nth(1).filter(|s| !s.is_empty())))
        });
    let words = ENGLISH_WORDS
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .filter_map(|line| {
            let mut fields = line.split('\t');
            Some((fields.next()?, fields.next().filter(|s| !s.is_empty())))
        });
    terms.chain(words)
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
    READINGS.get_or_init(|| {
        let mut out: Vec<(String, &'static str)> = Vec::new();
        for (spelling, _) in spellings() {
            for r in readings_of(spelling, 3) {
                if !out.iter().any(|(x, _)| *x == r) {
                    out.push((r, spelling));
                }
            }
        }
        out
    })
}

/// The readings typing `spelling` gives when its keys are not romaji: the
/// reading itself, and the one with a final consonant doubled by the next
/// word's first key ("discordde" reads ぢscおrっで).
fn readings_of(spelling: &str, min_len: usize) -> Vec<String> {
    let keys: String = spelling
        .chars()
        .filter(|c| !matches!(c, ' ' | '.' | '-'))
        .collect();
    if keys.len() < min_len || !keys.bytes().all(|b| b.is_ascii_alphabetic()) {
        return Vec::new();
    }
    let reading = romaji::to_hiragana(&keys.to_ascii_lowercase());
    if !reading.bytes().any(|b| b.is_ascii_alphabetic()) {
        return Vec::new();
    }
    let doubled = reading
        .strip_suffix(|c: char| c.is_ascii_alphabetic() && c != 'n')
        // Still not romaji without that letter (Apex: あぺっ is).
        .filter(|stem| stem.bytes().any(|b| b.is_ascii_alphabetic()))
        .map(|stem| format!("{stem}っ"));
    std::iter::once(reading).chain(doubled).collect()
}

/// Readings of the word list's words whose keys are not romaji, mapped to
/// the word (てst -> test). Bundled spellings are not included. Every
/// process using the IME holds one, so the readings share one string and
/// entries are sorted for binary search (about 2 MB in all).
pub struct WordIndex {
    readings: String,
    /// Sorted by reading.
    entries: Vec<WordEntry>,
    /// Longest reading, in bytes.
    pub max_len: usize,
}

#[derive(Clone, Copy)]
struct WordEntry {
    /// Byte range in `WordIndex::readings`.
    reading: (u32, u16),
    /// Byte range in `WORD_LIST`.
    word: (u32, u16),
}

impl WordIndex {
    fn reading(&self, e: &WordEntry) -> &str {
        &self.readings[e.reading.0 as usize..e.reading.0 as usize + e.reading.1 as usize]
    }

    pub fn get(&self, reading: &str) -> Option<&'static str> {
        if reading.len() > self.max_len {
            return None;
        }
        let i = self
            .entries
            .binary_search_by(|e| self.reading(e).cmp(reading))
            .ok()?;
        let (start, len) = self.entries[i].word;
        Some(&WORD_LIST[start as usize..start as usize + len as usize])
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

pub fn word_index() -> &'static WordIndex {
    static INDEX: OnceLock<WordIndex> = OnceLock::new();
    INDEX.get_or_init(|| {
        let bundled: HashSet<&str> = romaji_readings().iter().map(|(r, _)| r.as_str()).collect();
        let mut readings = String::new();
        let mut entries = Vec::new();
        for word in WORD_LIST
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
        {
            let at = word.as_ptr() as usize - WORD_LIST.as_ptr() as usize;
            for r in readings_of(word, 4) {
                if bundled.contains(r.as_str()) || r.len() > u16::MAX as usize {
                    continue;
                }
                entries.push(WordEntry {
                    reading: (readings.len() as u32, r.len() as u16),
                    word: (at as u32, word.len() as u16),
                });
                readings.push_str(&r);
            }
        }
        let mut index = WordIndex {
            readings,
            entries: Vec::new(),
            max_len: 0,
        };
        // Stable: of two words with one reading, the first in the file wins.
        entries.sort_by(|a, b| index.reading(a).cmp(index.reading(b)));
        entries.dedup_by(|b, a| index.reading(a) == index.reading(b));
        index.max_len = entries
            .iter()
            .map(|e| e.reading.1 as usize)
            .max()
            .unwrap_or(0);
        index.entries = entries;
        index.entries.shrink_to_fit();
        index.readings.shrink_to_fit();
        index
    })
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
        assert!(covers_letters("こんしゅうはぶsyです", true));
        assert!(!covers_letters("こんしゅうはぶsyです", false));
        assert!(covers_letters("んぎんxをつかう", false));
        assert!(!covers_letters("しgとお", true));
        assert!(!covers_letters("これ", true));
    }
}
