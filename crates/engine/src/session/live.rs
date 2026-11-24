//! Live conversion and suggestions while the reading is being composed.

use std::time::Duration;

use crate::romaji::Composer;
use crate::{Conversion, Converter, english, special, typo};

use super::Session;

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
        let suggest =
            self.options.suggestions && c.after.is_empty() && settled.chars().count() >= 2;
        let unresolved = typo::has_unresolved_romaji(&settled);
        let mut correction = None;
        let listed = self.options.english_words && conv.english_words_enabled();
        let words = english::covers_letters(&settled, listed);
        if (suggest || (live && unresolved)) && c.composer.pending().is_empty() && !words {
            if let Some(raw) = c.typo_input(&settled) {
                correction = typo::correct(conv, raw, 1000, TYPO_SUGGESTION_BUDGET);
            }
        }
        // Keys that are not romaji ("github", "discord") show as typed. While
        // a typo fix is still possible (few stray letters, more keys to come)
        // keep the conversion, so a fix doesn't flash the keys first.
        // A bundled spelling also counts its pending keys ("github" settles
        // as ぎてゅ until the last "b" resolves).
        let kana = c.kana();
        if self.options.english_words && c.before.is_empty() && c.after.is_empty() {
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
            if bundled.is_none()
                && unresolved
                && !words
                && (previous_typo.is_some() || correction.is_some())
                && c.raw_spells(&kana)
            {
                let previous = previous_typo.filter(|fix| c.raw.len() >= fix.prefix_len);
                // A spelling the user explicitly selected must take priority
                // over an automatic repair of the same keys.
                let learned_spelling = conv.convert(&kana, &[], 1).first().is_some_and(|best| {
                    best.segments
                        .iter()
                        .flat_map(|segment| &segment.words)
                        .any(|word| {
                            (word.learned || word.user)
                                && english::typed_word(&word.reading, &word.surface)
                        })
                });
                if learned_spelling {
                    correction = None;
                } else {
                    c.live_typo = continue_typo(conv, &c.raw, previous, correction.take());
                }
            }
            let typo_possible =
                c.typo_input(&settled).is_some() && !c.composer.pending().is_empty();
            bundled.or_else(|| {
                if !unresolved_kana || !unresolved || typo_possible || c.live_typo.is_some() {
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
            if let Some(fix) = correction.filter(|fix| fix.surface != shown) {
                c.suggestions.push(fix.surface.clone());
                c.typo_suggestion = Some(fix);
            }
            let reading = c
                .live
                .as_ref()
                .map_or(settled.as_str(), |live| live.reading.as_str());
            for suggestion in
                conv.predict_with_conversion(reading, SUGGESTION_LIMIT, live_best.as_ref())
            {
                if suggestion != shown && !c.suggestions.contains(&suggestion) {
                    c.suggestions.push(suggestion);
                    if c.suggestions.len() == SUGGESTION_LIMIT {
                        break;
                    }
                }
            }
        }
    }
}

fn continue_typo(
    conv: &Converter,
    raw: &str,
    previous: Option<LiveTypo>,
    challenger: Option<typo::Correction>,
) -> Option<LiveTypo> {
    let previous = previous.map(|mut fix| {
        fix.rebuild(raw);
        fix
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
/// earlier part of the reading already settled. Then that text is kept,
/// followed by the lattice's conversion of the rest, if the lattice splits
/// the reading there. (A refined result that ended mid-word doesn't line up
/// with a split, and the lattice result is shown.)
fn stable_live(refined: Option<&LiveConversion>, reading: &str, lattice: &Conversion) -> String {
    let surface = lattice.surface();
    let Some(r) = refined else { return surface };
    if !reading.starts_with(r.reading.as_str()) || surface.starts_with(&r.surface) {
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
        // The lattice would flip it back: keep it, then the lattice's rest.
        let flip = conv(&[("きかい", "機会"), ("がこわれた", "が壊れた")]);
        assert_eq!(
            stable_live(Some(&r), "きかいがこわれた", &flip),
            "機械が壊れた"
        );
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
