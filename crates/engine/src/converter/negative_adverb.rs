//! The adverb 大して ("not very", also written たいして) takes a negative
//! predicate. In affirmative sentences the reading たいして is 対して, but the
//! model often prefers the adverb after と because of phrases such as
//! と大して変わらない.

use crate::Conversion;

const READING: &str = "たいして";
/// Spellings of the adverb.
const SURFACES: &[&str] = &["大して", READING];

/// Negative endings, including colloquial and Kansai forms. Matching too
/// much only keeps the model's choice, as before.
const NEGATIVES: &[&str] = &[
    "ない",
    "なかっ",
    "なく",
    "なけれ",
    "ません",
    "ず",
    "ぬ",
    "ねえ",
    "ねぇ",
    "ねー",
    "へん",
    "らん",
    "せん",
];

/// Whether `c` uses the adverb for たいして with no negative ending after it
/// in `reading`.
pub(super) fn lacks_negation(reading: &str, c: &Conversion) -> bool {
    let mut at = 0;
    for word in c.segments.iter().flat_map(|s| &s.words) {
        let start = at;
        at += word.reading.len();
        if !SURFACES
            .iter()
            .any(|surface| word.surface.contains(surface))
        {
            continue;
        }
        // A generated conversion can be a single word for the whole reading.
        let Some(offset) = word.reading.find(READING) else {
            continue;
        };
        let rest = reading.get(start + offset + READING.len()..).unwrap_or("");
        if !NEGATIVES.iter().any(|negative| rest.contains(negative)) {
            return true;
        }
    }
    false
}
