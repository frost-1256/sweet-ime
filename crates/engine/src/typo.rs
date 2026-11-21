//! Keystroke typo correction.
//!
//! Variants of the typed romaji within one edit (a dropped, extra, swapped
//! or neighbouring key) are converted and compared with what was typed.
//! A variant wins when its conversion is much more plausible than the
//! original's, by more than a per-edit penalty.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use crate::converter::Converter;
use crate::romaji;

/// Lattice cost charged for one edit.
pub const EDIT_PENALTY: i32 = 2500;

/// QWERTY neighbours (same row and the rows above / below).
fn neighbours(c: char) -> &'static str {
    match c {
        'q' => "wa",
        'w' => "qeas",
        'e' => "wrsd",
        'r' => "etdf",
        't' => "ryfg",
        'y' => "tugh",
        'u' => "yihj",
        'i' => "uojk",
        'o' => "ipkl",
        'p' => "ol",
        'a' => "qwsz",
        's' => "awedxz",
        'd' => "serfcx",
        'f' => "drtgvc",
        'g' => "ftyhbv",
        'h' => "gyujnb",
        'j' => "huikmn",
        'k' => "jiolm",
        'l' => "kop",
        'z' => "asx",
        'x' => "zsdc",
        'c' => "xdfv",
        'v' => "cfgb",
        'b' => "vghn",
        'n' => "bhjm",
        'm' => "njk",
        _ => "",
    }
}

/// Romaji strings one edit away from `raw`, with their hiragana. Only
/// variants that are fully kana (no leftover letters) are kept, one per
/// distinct reading. Edits are limited to the last `window` keys.
pub fn variants(raw: &str, window: usize) -> Vec<(String, String)> {
    let b: Vec<char> = raw.chars().collect();
    let from = b.len().saturating_sub(window);
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    seen.insert(romaji::to_hiragana(raw));
    let mut add = |v: Vec<char>| {
        let s: String = v.into_iter().collect();
        let kana = romaji::to_hiragana(&s);
        if !kana.is_empty() && !has_unresolved_romaji(&kana) && seen.insert(kana.clone()) {
            out.push((s, kana));
        }
    };
    // Give a missing n (ん) a chance before the broader search.
    for i in from..=b.len() {
        let mut v = b.clone();
        v.insert(i, 'n');
        add(v);
    }
    for i in from..b.len() {
        if !b[i].is_ascii_alphabetic() {
            continue;
        }
        // Extra key.
        let mut v = b.clone();
        v.remove(i);
        add(v);
        // Swapped keys.
        if i + 1 < b.len() && b[i] != b[i + 1] {
            let mut v = b.clone();
            v.swap(i, i + 1);
            add(v);
        }
        // Neighbouring key.
        for n in neighbours(b[i]).chars() {
            let mut v = b.clone();
            v[i] = n;
            add(v);
        }
    }
    // Other missing keys: vowels, y (yoon) and h (sh/ch).
    for i in from..=b.len() {
        for k in "aiueoyh".chars() {
            let mut v = b.clone();
            v.insert(i, k);
            add(v);
        }
    }
    out
}

/// A suggested correction of the typed romaji.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Correction {
    pub raw: String,
    pub reading: String,
    /// Best conversion of `reading`.
    pub surface: String,
}

pub(crate) fn has_unresolved_romaji(reading: &str) -> bool {
    reading.chars().any(|c| c.is_ascii_alphabetic())
}

/// The best correction of `raw` if it beats the typed text by more than
/// `EDIT_PENALTY` (plus `margin`). Stops after `budget`.
pub fn correct(conv: &Converter, raw: &str, margin: i32, budget: Duration) -> Option<Correction> {
    let t = Instant::now();
    let typed = romaji::to_hiragana(raw);
    if typed.is_empty() {
        return None;
    }
    let variants = variants(raw, 12);
    let typed_len = typed.chars().count() as i64;
    let (raw, reading) = conv.score_typos(|cost| {
        let base = cost(&typed);
        let mut best = None;
        let mut best_gain = i64::from(margin);
        for (r, kana) in variants {
            if t.elapsed() > budget {
                break;
            }
            // Compare per-character cost so dropping a kana isn't rewarded
            // just because it makes the reading shorter.
            let scaled = i64::from(cost(&kana)) * typed_len / kana.chars().count() as i64;
            let gain = i64::from(base) - scaled - i64::from(EDIT_PENALTY);
            if gain > best_gain {
                best_gain = gain;
                best = Some((r, kana));
            }
        }
        best
    })?;
    let surface = conv.convert(&reading, &[], 1).into_iter().next()?.surface();
    Some(Correction { raw, reading, surface })
}
