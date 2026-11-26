//! Keystroke typo correction.
//!
//! Variants of the typed romaji within one edit (a dropped, extra, swapped
//! or neighbouring key) are converted and compared with what was typed.
//! A variant wins when its conversion is much more plausible than the
//! original's, by more than a per-edit penalty.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::converter::Converter;
use crate::romaji::{self, Composer};

/// Lattice cost charged for one edit (see `Slip::penalty`).
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

/// Keys around those that are left as letters (the "y" of "kakuynin"): a
/// fix has to change one of them. Letters show once a later key rules out
/// every romaji spelling, at most a few keys after the slip.
fn near_stray_letters(b: &[char]) -> Vec<bool> {
    let mut at = vec![false; b.len() + 1];
    let mut composer = Composer::new();
    let letters = |c: &Composer| c.kana().bytes().filter(u8::is_ascii_alphabetic).count();
    let mark = |i: usize, at: &mut Vec<bool>| {
        for j in i.saturating_sub(4)..=(i + 1).min(b.len()) {
            at[j] = true;
        }
    };
    for (i, &ch) in b.iter().enumerate() {
        let before = letters(&composer);
        composer.push(ch);
        if letters(&composer) > before {
            mark(i, &mut at);
        }
    }
    let before = letters(&composer);
    composer.flush();
    if letters(&composer) > before {
        mark(b.len() - 1, &mut at);
        at[b.len()] = true;
    }
    at
}

/// Romaji strings one edit away from `raw`, with their hiragana. Only
/// variants that are fully kana (no leftover letters) are kept, one per
/// distinct reading. Edits are limited to the keys around letters left
/// over, if any, or else to the last `window` keys.
pub fn variants(raw: &str, window: usize) -> Vec<(String, String)> {
    edits(raw, window, false)
        .into_iter()
        .map(|(keys, kana, _)| (keys, kana))
        .collect()
}

/// The kind of slip a variant undoes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slip {
    /// A key was missed (the variant adds it).
    Missing,
    /// An omitted onset or doubled consonant. Many replacements are legal
    /// romaji, so these need stronger evidence than an omitted vowel.
    MissingConsonant,
    /// A key was pressed twice or brushed (the variant drops it).
    Extra,
    Repeated,
    Swapped,
    /// A key next to the right one was pressed.
    Neighbour,
}

impl Slip {
    /// Lattice cost charged for undoing the slip. Tuned with the
    /// `typo_eval` example: dropping a key is the riskiest fix (it often
    /// leaves another valid word), swapping two the safest.
    fn penalty(self) -> i32 {
        match self {
            Slip::Extra => EDIT_PENALTY + 1150,
            Slip::Repeated => EDIT_PENALTY,
            Slip::Swapped => EDIT_PENALTY - 500,
            Slip::Missing | Slip::Neighbour => EDIT_PENALTY,
            Slip::MissingConsonant => EDIT_PENALTY + 800,
        }
    }
}

fn edits(raw: &str, window: usize, earlier: bool) -> Vec<(String, String, Slip)> {
    let b: Vec<char> = raw.chars().collect();
    let mut at = near_stray_letters(&b);
    if !at.contains(&true) {
        let from = b.len().saturating_sub(window);
        if earlier {
            at[..from].fill(true);
        } else {
            at[from..].fill(true);
        }
    }
    let from = at.iter().position(|&x| x).unwrap_or(b.len());
    // Keep the pending romaji as well as the kana before each edit, so a
    // variant only reprocesses the part of the input that could change.
    let mut composer = Composer::new();
    for &ch in &b[..from] {
        composer.push(ch);
    }
    let mut prefixes = Vec::with_capacity(b.len() - from + 1);
    for &ch in &b[from..] {
        prefixes.push(composer.clone());
        composer.push(ch);
    }
    prefixes.push(composer.clone());
    composer.flush();
    let mut out: Vec<(String, String, Slip)> = Vec::new();
    let mut seen = HashMap::new();
    let mut add = |i: usize, edit: &[char], tail: &[char], slip: Slip| {
        if !at[i] {
            return;
        }
        let mut variant = prefixes[i - from].clone();
        for &ch in edit.iter().chain(tail) {
            variant.push(ch);
        }
        variant.flush();
        let kana = variant.kana();
        if kana.is_empty() || kana == composer.kana() || has_unresolved_romaji(kana) {
            return;
        }
        if let Some(&index) = seen.get(kana) {
            let (keys, _, previous): &mut (String, String, Slip) = &mut out[index];
            if slip.penalty() < previous.penalty() {
                *keys = b[..i].iter().chain(edit).chain(tail).collect();
                *previous = slip;
            }
        } else {
            seen.insert(kana.to_owned(), out.len());
            out.push((
                b[..i].iter().chain(edit).chain(tail).collect(),
                kana.to_owned(),
                slip,
            ));
        }
    };
    // Give a missing n (ん) a chance before the broader search.
    for i in from..=b.len() {
        add(i, &['n'], &b[i..], Slip::Missing);
    }
    for i in from..b.len() {
        if !b[i].is_ascii_alphabetic() {
            continue;
        }
        // Extra key.
        let repeated = b.get(i + 1) == Some(&b[i]);
        add(
            i,
            &[],
            &b[i + 1..],
            if repeated {
                Slip::Repeated
            } else {
                Slip::Extra
            },
        );
        // Swapped keys.
        if i + 1 < b.len() && b[i] != b[i + 1] {
            add(i, &[b[i + 1], b[i]], &b[i + 2..], Slip::Swapped);
        }
        // Neighbouring key.
        for n in neighbours(b[i]).chars() {
            add(i, &[n], &b[i + 1..], Slip::Neighbour);
        }
    }
    for i in from..b.len() {
        let ch = b[i];
        if "aiueoy".contains(ch) {
            for onset in "kstmgzdbprfwcjv".chars() {
                add(i, &[onset], &b[i..], Slip::MissingConsonant);
            }
        } else if ch == 'h' {
            for onset in ['s', 'c'] {
                add(i, &[onset], &b[i..], Slip::MissingConsonant);
            }
        } else if ch == 's' {
            add(i, &['t'], &b[i..], Slip::MissingConsonant);
            if ch != b.get(i.wrapping_sub(1)).copied().unwrap_or('\0') {
                add(i, &[ch], &b[i..], Slip::MissingConsonant);
            }
        } else if ch.is_ascii_lowercase()
            && ch != 'n'
            && ch != b.get(i.wrapping_sub(1)).copied().unwrap_or('\0')
        {
            add(i, &[ch], &b[i..], Slip::MissingConsonant);
        }
    }
    // Other missing keys: vowels, y (yoon) and h (sh/ch).
    for i in from..=b.len() {
        for k in "aiueoyh".chars() {
            add(i, &[k], &b[i..], Slip::Missing);
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
/// the edit's penalty (plus `margin`). Stops after `budget`.
pub fn correct(conv: &Converter, raw: &str, margin: i32, budget: Duration) -> Option<Correction> {
    correct_with_gain(conv, raw, margin, budget).map(|(fix, _)| fix)
}

pub(crate) fn correct_with_gain(
    conv: &Converter,
    raw: &str,
    margin: i32,
    budget: Duration,
) -> Option<(Correction, i64)> {
    let t = Instant::now();
    let typed = romaji::to_hiragana(raw);
    if typed.is_empty() {
        return None;
    }
    const RECENT_KEYS: usize = 16;
    let variants = edits(raw, RECENT_KEYS, false);
    let (raw, reading, gain) = conv.score_typos(|cost| {
        let base = cost(&typed);
        let mut pick = |variants: Vec<(String, String, Slip)>, minimum_gain| {
            let mut best = None;
            let mut best_gain = minimum_gain;
            for (r, kana, slip) in variants {
                if t.elapsed() > budget {
                    break;
                }
                // Whole costs: scaling them by length favours fixes that add kana.
                let gain = i64::from(base) - i64::from(cost(&kana)) - i64::from(slip.penalty());
                if gain > best_gain {
                    best_gain = gain;
                    best = Some((r, kana, gain));
                }
            }
            best
        };
        let recent = pick(variants, i64::from(margin));
        if !(RECENT_KEYS + 1..=64).contains(&raw.len())
            || has_unresolved_romaji(&typed)
            || t.elapsed() >= budget
        {
            return recent;
        }
        // Earlier changes need more evidence than edits near the caret.
        // An incidental late edit should not hide a substantially better
        // repair near the start.
        let earlier_margin = recent
            .as_ref()
            .map_or(i64::from(margin), |(_, _, gain)| *gain)
            + 1100;
        pick(edits(raw, RECENT_KEYS, true), earlier_margin).or(recent)
    })?;
    let surface = conv.convert(&reading, &[], 1).into_iter().next()?.surface();
    Some((
        Correction {
            raw,
            reading,
            surface,
        },
        gain,
    ))
}
