//! Live conversion and suggestions while the reading is being composed.

use std::time::Duration;

use crate::romaji::Composer;
use crate::{Conversion, Converter, english, special, typo};

use super::{KANA_TYPO_MARGIN, MIN_KANA_TYPO_KEYS, Session, explicit_spelling_overrides};

const SUGGESTION_LIMIT: usize = 5;
const TYPO_SUGGESTION_BUDGET: Duration = Duration::from_millis(4);
const TYPO_REPLACEMENT_MARGIN: i32 = 1000;

#[derive(Clone)]
pub(super) struct LiveConversion {
    pub(super) reading: String,
    pub(super) surface: String,
}

/// A repair shown in live conversion. The original keys remain editable.
pub(super) struct LiveTypo {
    pub(super) prefix_len: usize,
    keys: String,
    pub(super) composer: Composer,
}

impl LiveTypo {
    fn rebuild(&mut self, raw: &str) {
        self.composer.clear();
        self.composer.push_str(&self.keys);
        self.composer.push_str(&raw[self.prefix_len..]);
    }

    pub(super) fn kana(&self) -> String {
        let mut composer = self.composer.clone();
        composer.flush();
        composer.kana().to_owned()
    }
}

impl Session {
    /// The reading and left context of the live conversion shown now, for
    /// refining it in the background (e.g. with a neural reranker).
    pub fn live_request(&self) -> Option<(String, String)> {
        let live = self
            .comp
            .live
            .as_ref()
            .filter(|_| !self.comp.live_english)?;
        Some((live.reading.clone(), self.left_context.clone()))
    }

    /// Reading to refine when confirming live conversion. A final pending
    /// `n` becomes ん on confirmation, so the preview may cover less text.
    pub fn live_commit_request(&self) -> Option<(String, String)> {
        let live = self.comp.live.as_ref()?;
        if self.conv.is_some() || self.comp.live_english || !self.options.live_conversion {
            return None;
        }
        let reading = match self.comp.live_composer().pending() {
            _ if self.comp.live_pending => live.reading.clone(),
            "" => live.reading.clone(),
            "n" => self.comp.live_kana(),
            _ => return None,
        };
        Some((reading, self.left_context.clone()))
    }

    /// Replaces the live conversion with `surface` if `reading` is still what
    /// is being typed. Returns whether the display changed.
    pub fn apply_live(&mut self, reading: &str, surface: &str) -> bool {
        let Some(live) = &mut self.comp.live else {
            return false;
        };
        if live.reading != reading || self.comp.live_english {
            return false;
        }
        self.comp.refined = Some(LiveConversion {
            reading: reading.to_string(),
            surface: surface.to_string(),
        });
        if live.surface == surface {
            return false;
        }
        live.surface = surface.to_string();
        true
    }

    /// Recomputes live conversion and suggestions after a composing edit.
    pub(super) fn refresh(&mut self, conv: &Converter) {
        let c = &mut self.comp;
        let previous_typo = c.live_typo.take();
        c.live = None;
        c.live_english = false;
        c.live_pending = false;
        c.suggestions.clear();
        // Tab opens the current suggestions as a conversion list. Keep the
        // corrected reading until that list is confirmed or dismissed.
        if self.conv.is_none() {
            c.typo_suggestion = None;
        }
        if c.is_empty() {
            c.literal = false;
            return;
        }
        if c.literal || self.conv.is_some() {
            return;
        }
        let settled = format!("{}{}", c.before, c.composer.kana());
        let live = self.options.live_conversion
            && !c.live_off
            && c.after.is_empty()
            && !settled.is_empty()
            && !is_plain_interjection(&settled);
        let min_suggest_chars = if self.options.always_show_suggestions { 1 } else { 2 };
        let suggest = self.options.suggestions
            && c.after.is_empty()
            && settled.chars().count() >= min_suggest_chars;
        let unresolved = typo::has_unresolved_romaji(&settled);
        let mut correction = None;
        let mut confident = false;
        let listed = self.options.english_words && conv.english_words_enabled();
        let words = english::covers_letters(&settled, listed);
        if !c.live_off
            && (suggest || live)
            && c.composer.pending().is_empty()
            && !words
            && let Some(raw) = c.typo_input(&settled)
            && (suggest || unresolved || raw.len() >= MIN_KANA_TYPO_KEYS)
            && let Some((fix, gain)) =
                typo::correct_with_gain(conv, raw, 1000, TYPO_SUGGESTION_BUDGET)
        {
            confident = unresolved
                || (raw.len() >= MIN_KANA_TYPO_KEYS && gain > i64::from(KANA_TYPO_MARGIN));
            correction = Some(fix);
        }
        // Keys that are not romaji ("github", "discord") show as typed. While
        // a typo fix is still possible (few stray letters, more keys to come)
        // keep the conversion, so a fix doesn't flash the keys first.
        // A bundled spelling also counts its pending keys ("github" settles
        // as ぎてゅ until the last "b" resolves).
        let kana = c.kana();
        let all_letters_known = if kana == settled {
            words
        } else {
            english::covers_letters(&kana, listed)
        };
        if self.options.english_words
            && c.before.is_empty()
            && c.after.is_empty()
            && !all_letters_known
        {
            for (reading, word) in english::split_words(&c.raw, &kana) {
                conv.note_typed_word(&reading, &word);
            }
        }
        let unresolved_kana = typo::has_unresolved_romaji(&kana);
        let english = if live {
            let bundled = (unresolved_kana || c.raw.bytes().any(|b| !b.is_ascii_alphabetic()))
                .then(|| c.bundled_candidates(&kana))
                .flatten()
                .and_then(|candidates| candidates.into_iter().next());
            let mut explicit_spelling = false;
            if bundled.is_none()
                && !words
                && (previous_typo.is_some() || confident)
                && c.raw_spells(&kana)
            {
                let previous = previous_typo.filter(|fix| c.raw.len() >= fix.prefix_len);
                let recheck =
                    (!unresolved && c.composer.pending().is_empty()).then_some(settled.as_str());
                let challenger = if confident { correction.take() } else { None };
                let fix = continue_typo(conv, &c.raw, previous, challenger, recheck);
                explicit_spelling = fix
                    .as_ref()
                    .is_some_and(|fix| explicit_spelling_overrides(conv, &kana, &fix.kana()));
                c.live_typo = fix.filter(|_| !explicit_spelling);
            }
            let typo_possible =
                c.typo_input(&settled).is_some() && !c.composer.pending().is_empty();
            bundled.or_else(|| {
                if !unresolved_kana
                    || !unresolved
                    || typo_possible
                    || c.live_typo.is_some()
                    || explicit_spelling
                {
                    return None;
                }
                c.english(&kana, correction.is_some(), listed)
                    .map(|word| conv.loanword_for(&word).unwrap_or(word))
            })
        } else {
            None
        };
        let mut live_best = None;
        if let Some(text) = english {
            c.live = Some(LiveConversion {
                reading: settled.clone(),
                surface: text,
            });
            c.live_english = true;
            if let Some(fix) = correction.filter(|_| suggest) {
                c.suggestions.push(fix.surface.clone());
                c.typo_suggestion = Some(fix);
            }
            return;
        }
        if live {
            // Lattice only: this runs on every keystroke.
            let (live_settled, live_kana) = match &c.live_typo {
                Some(fix) => (fix.composer.kana(), fix.kana()),
                None => (settled.as_str(), kana),
            };
            // Pending keys that end an English word ("linux" keeps
            // its x pending) convert with it.
            let with_pending = (live_kana != live_settled)
                .then(|| conv.convert(&live_kana, &[], 1).into_iter().next())
                .flatten()
                .filter(|r| {
                    r.segments
                        .last()
                        .and_then(|s| s.words.last())
                        .is_some_and(|w| {
                            w.reading.bytes().any(|b| b.is_ascii_alphabetic())
                                && english::is_spelling(&w.reading, &w.surface)
                        })
                });
            c.live_pending = with_pending.is_some();
            live_best =
                with_pending.or_else(|| conv.convert(live_settled, &[], 1).into_iter().next());
            let reading = if c.live_pending {
                live_kana
            } else {
                live_settled.to_owned()
            };
            c.live = live_best.as_ref().map(|r| LiveConversion {
                surface: stable_live(c.refined.as_ref(), &reading, r),
                reading,
            });
        }
        if suggest && self.options.calculator {
            if let Some(value) = special::calculator_candidates(&settled).into_iter().next() {
                c.suggestions.push(value);
            }
        }
        if suggest {
            let shown = c
                .live
                .as_ref()
                .map_or(settled.as_str(), |live| live.surface.as_str());
            c.suggestions.extend(
                special::symbol_candidates(&settled)
                    .iter()
                    .copied()
                    .filter(|&symbol| symbol != shown)
                    .take(SUGGESTION_LIMIT - c.suggestions.len())
                    .map(str::to_owned),
            );
            if let Some(fix) = correction.filter(|fix| fix.surface != shown)
                && c.suggestions.len() < SUGGESTION_LIMIT
            {
                c.suggestions.push(fix.surface.clone());
                c.typo_suggestion = Some(fix);
            }
            let reading = c
                .live
                .as_ref()
                .map_or(settled.as_str(), |live| live.reading.as_str());
            for suggestion in conv.predict_with_conversion(
                reading,
                SUGGESTION_LIMIT,
                live_best.as_ref(),
                min_suggest_chars,
            ) {
                if c.suggestions.len() == SUGGESTION_LIMIT {
                    break;
                }
                if suggestion != shown && !c.suggestions.contains(&suggestion) {
                    c.suggestions.push(suggestion);
                }
            }
            // When always_show_suggestions is enabled, fall back to lattice
            // conversion alternatives if prefix predictions are sparse or empty,
            // so the suggestion list stays visible for any settled reading.
            if self.options.always_show_suggestions
                && !reading.is_empty()
                && c.suggestions.len() < SUGGESTION_LIMIT
            {
                for candidate in conv.convert(reading, &[], SUGGESTION_LIMIT) {
                    let surface = candidate.surface();
                    if surface != shown && !c.suggestions.contains(&surface) {
                        c.suggestions.push(surface);
                        if c.suggestions.len() == SUGGESTION_LIMIT {
                            break;
                        }
                    }
                }
                if c.suggestions.is_empty() {
                    c.suggestions.push(shown.to_string());
                }
            }
            // When live conversion turns hiragana into kanji, keep the plain
            // hiragana reading available in suggestions so users can pick it directly.
            if !reading.is_empty()
                && shown != reading
                && !c.suggestions.iter().any(|s| s == reading)
            {
                if c.suggestions.len() >= SUGGESTION_LIMIT {
                    c.suggestions.pop();
                }
                c.suggestions.push(reading.to_string());
            }
        }
    }
}

fn continue_typo(
    conv: &Converter,
    raw: &str,
    previous: Option<LiveTypo>,
    challenger: Option<typo::Correction>,
    original: Option<&str>,
) -> Option<LiveTypo> {
    let previous = previous.and_then(|mut fix| {
        fix.rebuild(raw);
        // A new suffix can make the original kana reading plausible again.
        original
            .is_none_or(|reading| conv.score_typos(|cost| cost(&fix.kana()) < cost(reading)))
            .then_some(fix)
    });
    let challenger = challenger.filter(|next| {
        previous.as_ref().is_none_or(|fix| {
            let reading = fix.kana();
            // More text can clarify an ambiguous repair. Keep the displayed
            // repair unless the new reading is materially better.
            next.reading != reading
                && conv.score_typos(|cost| {
                    cost(&next.reading) + TYPO_REPLACEMENT_MARGIN < cost(&reading)
                })
        })
    });
    match challenger {
        Some(next) => {
            let mut fix = LiveTypo {
                prefix_len: raw.len(),
                keys: next.raw,
                composer: Composer::new(),
            };
            fix.rebuild(raw);
            Some(fix)
        }
        None => previous,
    }
}

fn is_plain_interjection(reading: &str) -> bool {
    // Keep standalone reactions intact in live conversion. Space still
    // offers kanji for the same reading.
    let reading = reading.trim_end_matches(['、', '。', '！', '？', '…', 'ー', '～']);
    matches!(
        reading,
        "おお" | "おおっ" | "ああ" | "あっ" | "おっ" | "へえ" | "へぇ" | "うわ" | "うわっ" | "わあ"
    )
}

/// What to show for `reading` while its refined conversion is pending:
/// the lattice result, unless it rewrites text the refined result for an
/// earlier part while a short continuation is still being entered. As
/// more context appears, allow the lattice to revise ambiguous homophones.
/// A complete sentence can keep its refinement across the next sentence.
fn stable_live(refined: Option<&LiveConversion>, reading: &str, lattice: &Conversion) -> String {
    let surface = lattice.surface();
    let Some(r) = refined else { return surface };
    let Some(tail) = reading.strip_prefix(r.reading.as_str()) else { return surface };
    if surface.starts_with(&r.surface) ||
        (!r.reading.ends_with(['。', '！', '？']) && tail.chars().nth(2).is_some())
    {
        return surface;
    }
    let mut consumed = 0;
    for (i, seg) in lattice.segments.iter().enumerate() {
        if consumed == r.reading.len() {
            let rest: String = lattice.segments[i..]
                .iter()
                .map(|s| s.surface.as_str())
                .collect();
            return format!("{}{rest}", r.surface);
        }
        consumed += seg.reading.len();
    }
    if consumed == r.reading.len() {
        return r.surface.clone();
    }
    surface
}

#[cfg(test)]
mod tests {
    use super::{Conversion, LiveConversion, stable_live};
    use crate::Segment;

    fn conv(segs: &[(&str, &str)]) -> Conversion {
        Conversion {
            segments: segs
                .iter()
                .map(|(r, s)| Segment {
                    reading: r.to_string(),
                    surface: s.to_string(),
                    words: Vec::new(),
                })
                .collect(),
            cost: 0,
        }
    }

    #[test]
    fn keeps_refined_text_while_typing() {
        let r = LiveConversion {
            reading: "きかい".into(),
            surface: "機械".into(),
        };
        // The lattice agrees with the refined text: use it.
        assert_eq!(
            stable_live(
                Some(&r),
                "きかいが",
                &conv(&[("きかい", "機械"), ("が", "が")])
            ),
            "機械が"
        );
        // A newly typed particle need not flip the displayed homophone.
        let flip = conv(&[("きかい", "機会"), ("が", "が")]);
        assert_eq!(
            stable_live(Some(&r), "きかいが", &flip),
            "機械が"
        );
        // Once the continuation disambiguates the reading, prefer the full lattice.
        let clarified = conv(&[("きかい", "機会"), ("をいかす", "を生かす")]);
        assert_eq!(stable_live(Some(&r), "きかいをいかす", &clarified), "機会を生かす");
        assert_eq!(
            stable_live(Some(&r), "きかい", &conv(&[("きかい", "機会")])),
            "機械"
        );
        // No segment boundary there: the lattice result.
        let joined = conv(&[("きかいが", "機会が")]);
        assert_eq!(stable_live(Some(&r), "きかいが", &joined), "機会が");
        // A different reading (e.g. after Backspace): the lattice result.
        assert_eq!(
            stable_live(Some(&r), "きか", &conv(&[("きか", "時価")])),
            "時価"
        );
        assert_eq!(
            stable_live(None, "きかい", &conv(&[("きかい", "機会")])),
            "機会"
        );
    }
}
