//! Readings of English words typed by their spelling. Shared with the build
//! script, which builds the word list index (see `english::word_index`).

use std::collections::HashSet;

use crate::romaji;

/// Bundled spellings with their optional kana candidate: technical and
/// VRChat terms first (in file order), then the English spellings file.
pub fn bundled_spellings<'a>(
    tech: &'a str,
    vrchat: &'a str,
    english: &'a str,
) -> impl Iterator<Item = (&'a str, Option<&'a str>)> {
    let terms = tech
        .lines()
        .chain(vrchat.lines())
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let mut fields = line.split('\t').skip(1);
            let spelling = fields.next()?;
            Some((spelling, fields.nth(1).filter(|s| !s.is_empty())))
        });
    let words = english
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .filter_map(|line| {
            let mut fields = line.split('\t');
            Some((fields.next()?, fields.next().filter(|s| !s.is_empty())))
        });
    terms.chain(words)
}

/// The readings typing `spelling` gives when its keys are not romaji: the
/// reading itself, and the one with a final consonant doubled by the next
/// word's first key ("discordde" reads ぢscおrっで).
pub fn readings_of(spelling: &str, min_len: usize) -> Vec<String> {
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
    let doubled = doubled_final_consonant(&reading)
        // Still not romaji without that letter (Apex: あぺっ is).
        .filter(|candidate| candidate.bytes().any(|b| b.is_ascii_alphabetic()));
    std::iter::once(reading).chain(doubled).collect()
}

pub fn doubled_final_consonant(reading: &str) -> Option<String> {
    reading
        .strip_suffix(|c: char| c.is_ascii_alphabetic() && c != 'n')
        .map(|stem| format!("{stem}っ"))
}

/// Readings of bundled spellings (3+ keys, or 2-key consonant abbreviations
/// like PC/PR/DB that leave both letters unresolved) that are not romaji.
pub fn bundled_readings<'a>(spellings: impl Iterator<Item = (&'a str, Option<&'a str>)>) -> Vec<(String, &'a str)> {
    let mut out: Vec<(String, &'a str)> = Vec::new();
    for (spelling, _) in spellings {
        for r in readings_of(spelling, 2) {
            if spelling.len() == 2 && r.bytes().filter(u8::is_ascii_alphabetic).count() < 2 {
                continue;
            }
            if !out.iter().any(|(x, _)| *x == r) {
                out.push((r, spelling));
            }
        }
    }
    out
}

/// One index entry: (reading offset, reading length) in the readings
/// string, (word offset, word length) in the word list.
pub type Entry = (u32, u16, u32, u16);

/// Bytes per entry in the serialized index.
pub const ENTRY_BYTES: usize = 12;

/// Builds the word list index: the readings of the words (4+ keys) whose
/// keys are not romaji, except bundled ones, sorted by reading. Of two words
/// with one reading, the first in the list wins.
#[allow(dead_code)] // build.rs and tests
pub fn build_index(word_list: &str, bundled: &HashSet<&str>) -> (String, Vec<Entry>) {
    let mut readings = String::new();
    let mut entries: Vec<Entry> = Vec::new();
    for word in word_list.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        let at = word.as_ptr() as usize - word_list.as_ptr() as usize;
        for r in readings_of(word, 4) {
            if bundled.contains(r.as_str()) || r.len() > u16::MAX as usize {
                continue;
            }
            entries.push((readings.len() as u32, r.len() as u16, at as u32, word.len() as u16));
            readings.push_str(&r);
        }
    }
    let reading = |e: &Entry| &readings[e.0 as usize..e.0 as usize + e.1 as usize];
    entries.sort_by(|a, b| reading(a).cmp(reading(b)));
    entries.dedup_by(|b, a| reading(a) == reading(b));
    (readings, entries)
}

/// The index entries as bytes (little endian), `ENTRY_BYTES` each.
#[allow(dead_code)] // build.rs and tests
pub fn encode(entries: &[Entry]) -> Vec<u8> {
    let mut out = Vec::with_capacity(entries.len() * ENTRY_BYTES);
    for &(r, rl, w, wl) in entries {
        out.extend_from_slice(&r.to_le_bytes());
        out.extend_from_slice(&rl.to_le_bytes());
        out.extend_from_slice(&w.to_le_bytes());
        out.extend_from_slice(&wl.to_le_bytes());
    }
    out
}

/// Entry `i` of `encode`'s output.
pub fn decode(bytes: &[u8], i: usize) -> Entry {
    let b = &bytes[i * ENTRY_BYTES..(i + 1) * ENTRY_BYTES];
    (
        u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        u16::from_le_bytes([b[4], b[5]]),
        u32::from_le_bytes([b[6], b[7], b[8], b[9]]),
        u16::from_le_bytes([b[10], b[11]]),
    )
}
