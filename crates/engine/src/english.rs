//! English words typed by their spelling in romaji mode. Keys such as
//! "github" are not romaji and read as ぎてゅb; the spellings below give
//! those readings back their word, also inside a sentence ("githubni" ->
//! GitHubに).

use std::sync::OnceLock;

use crate::dict::{TECH_TERMS, VRCHAT_TERMS};
use crate::romaji;

/// English words matched by their typed spelling only (not dictionary
/// entries with a Japanese reading).
pub const ENGLISH_WORDS: &str = include_str!("english.tsv");

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
            let keys: String = spelling
                .chars()
                .filter(|c| !matches!(c, ' ' | '.' | '-'))
                .collect();
            if keys.len() < 3 || !keys.bytes().all(|b| b.is_ascii_alphabetic()) {
                continue;
            }
            let reading = romaji::to_hiragana(&keys.to_ascii_lowercase());
            if !reading.bytes().any(|b| b.is_ascii_alphabetic()) {
                continue;
            }
            // A final consonant doubled by the next word's first key turns
            // into っ ("discordde" reads ぢscおrっで).
            let doubled = reading
                .strip_suffix(|c: char| c.is_ascii_alphabetic() && c != 'n')
                // Still not romaji without that letter (Apex: あぺっ is).
                .filter(|stem| stem.bytes().any(|b| b.is_ascii_alphabetic()))
                .map(|stem| format!("{stem}っ"));
            for r in std::iter::once(reading).chain(doubled) {
                if !out.iter().any(|(x, _)| *x == r) {
                    out.push((r, spelling));
                }
            }
        }
        out
    })
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
}
