//! Words from the SudachiDict lexicon sources (Apache-2.0), added to the Mozc
//! dictionary where it has no such word: newer words (リスキリング, 線状降水帯)
//! and names of works and products (呪術廻戦, ちいかわ).
//!
//! Sudachi's costs are for morphological analysis and do not follow how
//! often a word is used, so each added word gets a fixed cost by the lexicon
//! it comes from. Person and place names, spelling variants, lowercase
//! English words, other readings of words Mozc already has, and words in
//! older kanji spellings (昼御飯 for 昼ご飯, 彼方側 for あちら側) are left out.

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use sweet_engine::dict::SourceEntry;

/// The lexicons, from the most to the least common words, with the cost of
/// a word added from each.
const LEXICONS: [(&str, i16); 3] = [
    ("small_lex.csv", 7500),
    ("core_lex.csv", 7800),
    ("notcore_lex.csv", 8100),
];

/// Sudachi parts of speech (POS1, POS2, POS3) and the Mozc feature used for
/// them.
const PARTS: [(&str, &str); 8] = [
    ("名詞,普通名詞,一般", "名詞,一般,*,*,*,*,*"),
    ("名詞,普通名詞,サ変可能", "名詞,サ変接続,*,*,*,*,*"),
    ("名詞,普通名詞,サ変形状詞可能", "名詞,サ変接続,*,*,*,*,*"),
    ("名詞,普通名詞,形状詞可能", "名詞,形容動詞語幹,*,*,*,*,*"),
    ("名詞,普通名詞,副詞可能", "名詞,副詞可能,*,*,*,*,*"),
    ("名詞,普通名詞,助数詞可能", "名詞,一般,*,*,*,*,*"),
    ("名詞,固有名詞,一般", "名詞,固有名詞,一般,*,*,*,*"),
    ("感動詞,一般,*", "感動詞,*,*,*,*,*,*"),
];

/// Kanji spellings that are now written in kana (御飯 for ご飯, 此方 for
/// こちら); Mozc has the words in the usual spelling.
const OLD_SPELLINGS: [&str; 5] = ["御", "此", "其", "彼方", "何方"];

/// Readings shorter than this are left out: short readings already have many
/// words, and the ones Sudachi adds there are mostly rare.
const MIN_READING: usize = 3;

/// The words in `dir`'s lexicons that `entries` does not have.
pub fn words(dir: &Path, features: &[String], entries: &[SourceEntry]) -> Result<Vec<SourceEntry>> {
    let ids: Vec<(&str, u16)> = PARTS
        .iter()
        .map(|&(sudachi, mozc)| {
            let id = features
                .iter()
                .position(|f| f == mozc)
                .with_context(|| format!("no POS id for {mozc}"))?;
            Ok((sudachi, id as u16))
        })
        .collect::<Result<_>>()?;
    let mut seen: HashSet<(String, String)> = entries
        .iter()
        .map(|e| (e.reading.clone(), e.surface.clone()))
        .collect();
    let surfaces: HashSet<&str> = entries.iter().map(|e| e.surface.as_str()).collect();
    let mut out = Vec::new();
    for (file, cost) in LEXICONS {
        let path = dir.join(file);
        let reader = BufReader::new(File::open(&path).with_context(|| path.display().to_string())?);
        for line in reader.lines().skip(1) {
            let line = line?;
            let f = fields(&line, 13);
            if f.len() < 13 || f[1] == "-1" {
                continue;
            }
            let pos = format!("{},{},{}", f[5], f[6], f[7]);
            let Some(&(_, id)) = ids.iter().find(|(p, _)| *p == pos) else {
                continue;
            };
            let surface = unescape(if f[4].is_empty() { &f[0] } else { &f[4] });
            // A spelling variant of another word (100金 for 百均).
            let normal = f[12].split(',').next().unwrap_or("");
            if !normal.is_empty() && normal != surface {
                continue;
            }
            let Some(reading) = hiragana(&f[11]) else {
                continue;
            };
            if reading.chars().count() < MIN_READING
                || !usable(&surface)
                || OLD_SPELLINGS.iter().any(|old| surface.contains(old))
                || surfaces.contains(surface.as_str())
                || !seen.insert((reading.clone(), surface.clone()))
            {
                continue;
            }
            out.push(SourceEntry {
                reading,
                lid: id,
                rid: id,
                cost,
                surface,
            });
        }
    }
    Ok(out)
}

/// The first `n` comma-separated fields of a CSV line, with quotes removed.
fn fields(line: &str, n: usize) -> Vec<String> {
    let mut out = Vec::with_capacity(n);
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => {
                out.push(std::mem::take(&mut field));
                if out.len() == n {
                    return out;
                }
            }
            _ => field.push(c),
        }
    }
    out.push(field);
    out
}

/// Decodes Sudachi's `\uXXXX` escapes.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("\\u") {
        out.push_str(&rest[..i]);
        let code = rest
            .get(i + 2..i + 6)
            .and_then(|h| u32::from_str_radix(h, 16).ok())
            .and_then(char::from_u32);
        match code {
            Some(c) => {
                out.push(c);
                rest = &rest[i + 6..];
            }
            None => {
                out.push_str("\\u");
                rest = &rest[i + 2..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// A katakana reading in hiragana, if it is all kana.
fn hiragana(katakana: &str) -> Option<String> {
    katakana
        .chars()
        .map(|c| match c {
            'ァ'..='ヶ' => char::from_u32(c as u32 - 0x60),
            'ぁ'..='ゖ' | 'ー' => Some(c),
            _ => None,
        })
        .collect()
}

/// Whether a surface is written in Japanese (kanji or kana, with letters and
/// digits alongside: 生成AI) or is an abbreviation in capitals (SNS, SDGs).
/// Lowercase English words (boring) are typed by their spelling instead.
fn usable(surface: &str) -> bool {
    let japanese = |c: char| matches!(c, 'ぁ'..='ゖ' | 'ァ'..='ヺ' | 'ー' | '々' | '〆' | '・' | '\u{4e00}'..='\u{9fff}' | '\u{3400}'..='\u{4dbf}');
    let abbreviation = surface.bytes().filter(u8::is_ascii_uppercase).count() >= 2;
    (surface.chars().any(japanese) || abbreviation)
        && surface
            .chars()
            .all(|c| japanese(c) || c.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_quoted_fields() {
        let f = fields(
            r#"boring,5139,5139,5000,,名詞,普通名詞,一般,*,*,*,ボーリング,"ボーリング,名詞,普通名詞",,x"#,
            13,
        );
        assert_eq!(f.len(), 13);
        assert_eq!(f[11], "ボーリング");
        assert_eq!(f[12], "ボーリング,名詞,普通名詞");
    }

    #[test]
    fn keeps_japanese_surfaces() {
        assert_eq!(unescape(r"a,b"), "a,b");
        assert_eq!(hiragana("リスキリング").as_deref(), Some("りすきりんぐ"));
        assert!(hiragana("ABC").is_none());
        assert!(
            usable("生成AI")
                && usable("呪術廻戦")
                && usable("SDGs")
                && !usable("boring")
                && !usable("Tokyo")
                && !usable("a b")
        );
    }
}
