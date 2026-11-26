//! Explicit conversion, including segment resizing and candidate selection.

use std::time::Duration;

use super::{
    Composing, KANA_TYPO_MARGIN, Key, MIN_KANA_TYPO_KEYS, Session, explicit_spelling_overrides,
    transliterate,
};
use crate::{Conversion, Converter, english, romaji, special, typo, width};

const TYPO_CONVERSION_BUDGET: Duration = Duration::from_millis(12);

pub(super) struct SegState {
    pub(super) reading: String,
    pub(super) cands: Vec<String>,
    /// Whether `cands` beyond the first entry have been expanded.
    pub(super) expanded: bool,
    pub(super) selected: usize,
    /// Candidates are predictions (not conversions of `reading`); don't learn.
    pub(super) predicted: bool,
    pub(super) cloud: std::ops::Range<usize>,
}

pub(super) struct Converting {
    pub(super) segs: Vec<SegState>,
    pub(super) focus: usize,
    pub(super) window: bool,
    pub(super) corrected: bool,
}

impl Session {
    /// The reading to send on a desktop Space key that starts conversion.
    pub fn simeji_conversion_reading(&self, key: Key) -> Option<String> {
        if key != Key::Space
            || self.is_converting()
            || self.comp.live_typo.is_some()
            || !self.options.simeji_cloud
            || self.private
        {
            return None;
        }
        self.simeji_reading()
    }

    pub fn request_simeji(
        &mut self,
        reading: String,
        ready: impl FnOnce(crate::simeji::Response) + Send + 'static,
    ) -> bool {
        if let Some(cv) = &self.conv
            && self.simeji_request.is_none()
            && cv.segs.len() == 1
            && cv.segs[0].reading == reading
            && self.options.simeji_cloud
            && !self.private
        {
            self.simeji_request = crate::simeji::submit(reading, ready);
            return self.simeji_request.is_some();
        }
        false
    }

    pub fn apply_simeji(&mut self, conv: &Converter, response: crate::simeji::Response) -> bool {
        if self.simeji_request != Some(response.id) {
            return false;
        }
        self.cancel_simeji();
        if !self.options.simeji_cloud
            || self.private
            || response.items.is_empty()
            || self.conv.is_none()
        {
            return false;
        }
        self.expand(conv);
        let cv = self.conv.as_mut().unwrap();
        let segment = &mut cv.segs[cv.focus];
        let start = segment.cands.len();
        for item in response.items {
            if !segment.cands.contains(&item) {
                segment.cands.push(item);
            }
        }
        segment.cloud = start..segment.cands.len();
        cv.window && !segment.cloud.is_empty()
    }

    /// Dates, emoji and kaomoji for a segment reading (see [`crate::special`]).
    fn special_candidates(&self, reading: &str) -> Vec<String> {
        let mut v = Vec::new();
        if self.options.dates {
            v.extend(special::date_candidates(reading, special::DateTime::now()));
        }
        if self.options.emoji {
            v.extend(special::emoji_candidates(reading));
        }
        v
    }

    pub(super) fn start_conversion(&mut self, conv: &Converter) {
        let kana = self.comp.kana();
        if self.options.calculator {
            let cands = special::calculator_candidates(&kana);
            if !cands.is_empty() {
                self.conv = Some(Converting {
                    segs: vec![SegState {
                        reading: kana,
                        cands,
                        expanded: true,
                        selected: 0,
                        predicted: true,
                        cloud: 0..0,
                    }],
                    focus: 0,
                    window: false,
                    corrected: false,
                });
                return;
            }
        }
        if let Some(candidates) = self.comp.bundled_candidates(&kana) {
            self.conv = Some(Converting {
                segs: vec![SegState {
                    reading: kana,
                    cands: candidates,
                    expanded: false,
                    selected: 0,
                    predicted: false,
                    cloud: 0..0,
                }],
                focus: 0,
                window: false,
                corrected: false,
            });
            return;
        }
        let listed = self.options.english_words && conv.english_words_enabled();
        let words = english::covers_letters(&kana, listed);
        let live_reading = self.comp.live_typo.as_ref().map(super::LiveTypo::kana);
        let unresolved = typo::has_unresolved_romaji(&kana);
        let correction = if live_reading.is_none() && !self.comp.live_off && !words {
            // Valid kana needs more context and a stronger score difference.
            // After Escape, let the user convert that reading as typed.
            self.comp
                .typo_input(&kana)
                .filter(|raw| unresolved || raw.len() >= MIN_KANA_TYPO_KEYS)
                .and_then(|raw| {
                    let margin = if unresolved { 0 } else { KANA_TYPO_MARGIN };
                    typo::correct(conv, raw, margin, TYPO_CONVERSION_BUDGET)
                })
        } else {
            None
        };
        let explicit_spelling = correction
            .as_ref()
            .is_some_and(|fix| explicit_spelling_overrides(conv, &kana, &fix.reading));
        let correction = correction.filter(|_| !explicit_spelling);
        let corrected = live_reading.is_some() || correction.is_some();
        if !explicit_spelling && unresolved {
            if let Some(word) = self.comp.english(&kana, corrected, listed) {
                // Keys that are not romaji: the word as typed, capitalized,
                // then the kana reading's conversion.
                let mut cands: Vec<String> = conv
                    .loanword_for(&word)
                    .into_iter()
                    .chain([word.clone()])
                    .collect();
                let mut chars = word.chars();
                let capitalized: String = chars
                    .next()
                    .map(|c| c.to_ascii_uppercase())
                    .into_iter()
                    .chain(chars)
                    .collect();
                let kana_text = conv
                    .convert_best(&kana, &[], &self.left_context)
                    .first()
                    .map(|r| r.surface());
                for c in std::iter::once(capitalized).chain(kana_text) {
                    if !cands.contains(&c) {
                        cands.push(c);
                    }
                }
                self.conv = Some(Converting {
                    segs: vec![SegState {
                        reading: kana,
                        cands,
                        expanded: false,
                        selected: 0,
                        predicted: false,
                        cloud: 0..0,
                    }],
                    focus: 0,
                    window: false,
                    corrected: false,
                });
                return;
            }
        }
        let reading = live_reading
            .as_deref()
            .or(correction.as_ref().map(|fix| fix.reading.as_str()))
            .unwrap_or(&kana);
        let result = conv.convert_best(reading, &[], &self.left_context);
        let mut segs = segs_from(result.first(), reading);
        // A special reading (えがお) stays one segment so its candidates show.
        if segs.len() > 1 && !self.special_candidates(reading).is_empty() {
            let first = conv
                .segment_candidates(reading, 1)
                .into_iter()
                .next()
                .unwrap_or_else(|| reading.to_string());
            segs = vec![SegState {
                reading: reading.to_string(),
                cands: vec![first],
                expanded: false,
                selected: 0,
                predicted: false,
                cloud: 0..0,
            }];
        }
        self.conv = Some(Converting {
            segs,
            focus: 0,
            window: false,
            corrected,
        });
    }

    pub(super) fn expand(&mut self, conv: &Converter) {
        let special = {
            let cv = self.conv.as_ref().unwrap();
            self.special_candidates(&cv.segs[cv.focus].reading)
        };
        let page_size = self.page_size.max(1);
        let cv = self.conv.as_mut().unwrap();
        let s = &mut cv.segs[cv.focus];
        if s.expanded {
            return;
        }
        s.expanded = true;
        let mut at = 1.min(s.cands.len());
        for c in special {
            if !s.cands.contains(&c) {
                s.cands.insert(at, c);
                at += 1;
            }
        }
        for c in conv.segment_candidates(&s.reading, 40) {
            if !s.cands.contains(&c) {
                s.cands.push(c);
            }
        }
        for c in [width::to_half_katakana(&romaji::to_katakana(&s.reading))] {
            if !s.cands.contains(&c) {
                s.cands.push(c);
            }
        }
        conv.append_exact_candidates(&s.reading, &mut s.cands);
        // Keep the plain reading reachable on page 1 even when many homophones
        // or special candidates precede it.
        if !s.reading.is_empty() {
            let target = (page_size - 1).min(6).min(s.cands.len());
            match s.cands.iter().position(|c| c == &s.reading) {
                Some(idx) if idx >= page_size => {
                    let plain = s.cands.remove(idx);
                    s.cands.insert(target, plain);
                }
                None => s.cands.insert(target, s.reading.clone()),
                _ => {}
            }
        }
    }

    pub(super) fn commit_conversion(&mut self, conv: &Converter) {
        if let Some(cv) = self.conv.take() {
            for s in &cv.segs {
                let surface = &s.cands[s.selected];
                // Learn explicit choices (anything but the first suggestion),
                // and English words not on the word list ("nginx", or a word
                // fixed with F10), so they convert inside sentences next time.
                let new_word = self.options.english_words
                    && english::typed_word(&s.reading, surface)
                    && !english::is_spelling(&s.reading, surface);
                if !s.cloud.contains(&s.selected) {
                    if s.predicted {
                        self.learn_selected_typo(conv, surface);
                    } else if (s.selected != 0 || new_word) && !self.private {
                        conv.learn(&s.reading, surface);
                    }
                }
                self.commit.push_str(surface);
            }
        }
        self.comp = Composing::default();
    }

    fn resize(&mut self, conv: &Converter, delta: isize) {
        let cv = self.conv.as_ref().unwrap();
        let focus = cv.focus;
        let kana: String = cv.segs.iter().map(|s| s.reading.as_str()).collect();
        let mut bounds = Vec::new();
        let mut pos = 0;
        for s in &cv.segs[..focus] {
            pos += s.reading.len();
            bounds.push(pos);
        }
        let start = pos;
        let cur = &cv.segs[focus].reading;
        let cur_chars = cur.chars().count() as isize;
        let total_after = kana[start..].chars().count() as isize;
        let new_len = (cur_chars + delta).clamp(1, total_after) as usize;
        if new_len as isize == cur_chars {
            return;
        }
        let end = start
            + kana[start..]
                .char_indices()
                .nth(new_len)
                .map(|(i, _)| i)
                .unwrap_or(kana.len() - start);
        if end < kana.len() {
            bounds.push(end);
        }
        let result = conv.convert(&kana, &bounds, 1);
        let mut segs = segs_from(result.first(), &kana);
        // Keep the user's choices for the untouched leading segments.
        let old = self.conv.take().unwrap();
        for (i, o) in old.segs.into_iter().enumerate().take(focus) {
            if i < segs.len() && segs[i].reading == o.reading {
                segs[i] = o;
            }
        }
        self.conv = Some(Converting {
            segs,
            focus,
            window: false,
            corrected: old.corrected,
        });
    }

    pub(super) fn handle_converting(&mut self, conv: &Converter, key: Key) -> bool {
        let window = self.conv.as_ref().unwrap().window;
        match key {
            Key::KanaVariant => false,
            Key::Char(c) if window && ('1'..='9').contains(&c) => {
                let page = self.candidates().unwrap().page_start();
                self.choose(conv, page + (c as usize - '1' as usize));
                true
            }
            Key::Char(c) if !c.is_control() => {
                self.commit_conversion(conv);
                self.handle_composing(conv, key)
            }
            Key::Space | Key::Down | Key::Tab => {
                self.expand(conv);
                let cv = self.conv.as_mut().unwrap();
                let s = &mut cv.segs[cv.focus];
                if cv.window {
                    s.selected = (s.selected + 1) % s.cands.len();
                } else {
                    cv.window = true;
                    if s.cands.len() > 1 {
                        s.selected = (s.selected + 1) % s.cands.len();
                    }
                }
                true
            }
            Key::Up => {
                self.expand(conv);
                let cv = self.conv.as_mut().unwrap();
                let s = &mut cv.segs[cv.focus];
                cv.window = true;
                s.selected = (s.selected + s.cands.len() - 1) % s.cands.len();
                true
            }
            Key::PageDown | Key::PageUp if window => {
                let ps = self.page_size;
                let cv = self.conv.as_mut().unwrap();
                let s = &mut cv.segs[cv.focus];
                let n = s.cands.len();
                s.selected = if key == Key::PageDown {
                    (s.selected / ps * ps + ps).min(n - 1)
                } else {
                    (s.selected / ps * ps).saturating_sub(ps)
                };
                true
            }
            Key::Enter => {
                self.commit_conversion(conv);
                true
            }
            Key::Escape if window => {
                self.conv.as_mut().unwrap().window = false;
                true
            }
            Key::Escape | Key::Backspace => {
                let cv = self.conv.take().unwrap();
                if cv.corrected {
                    // The composition still has the original keystrokes.
                    self.comp.live_off = true;
                    return true;
                }
                let kana: String = cv.segs.iter().map(|s| s.reading.as_str()).collect();
                self.comp.before = kana;
                self.comp.after.clear();
                self.comp.composer.clear();
                true
            }
            Key::Left | Key::Right => {
                let cv = self.conv.as_mut().unwrap();
                cv.window = false;
                if key == Key::Left {
                    cv.focus = cv.focus.checked_sub(1).unwrap_or(cv.segs.len() - 1);
                } else {
                    cv.focus = (cv.focus + 1) % cv.segs.len();
                }
                true
            }
            Key::Home => {
                let cv = self.conv.as_mut().unwrap();
                cv.focus = 0;
                cv.window = false;
                true
            }
            Key::End => {
                let cv = self.conv.as_mut().unwrap();
                cv.focus = cv.segs.len() - 1;
                cv.window = false;
                true
            }
            Key::ShrinkSegment => {
                self.resize(conv, -1);
                true
            }
            Key::ExtendSegment => {
                self.resize(conv, 1);
                true
            }
            Key::ToHiragana
            | Key::ToKatakana
            | Key::ToHalfKatakana
            | Key::ToFullAscii
            | Key::ToHalfAscii => {
                let raw = self.comp.raw.clone();
                let cv = self.conv.as_mut().unwrap();
                let single = cv.segs.len() == 1;
                let s = &mut cv.segs[cv.focus];
                let text =
                    if single && cv.corrected && matches!(key, Key::ToFullAscii | Key::ToHalfAscii)
                    {
                        transliterate(key, &self.comp.kana(), &raw)
                    } else {
                        transliterate(key, &s.reading, if single { &raw } else { "" })
                    };
                if let Some(i) = s.cands.iter().position(|c| *c == text) {
                    s.selected = i;
                } else {
                    s.cands.push(text);
                    s.selected = s.cands.len() - 1;
                }
                cv.window = false;
                true
            }
            _ => true,
        }
    }
}

fn segs_from(conv: Option<&Conversion>, kana: &str) -> Vec<SegState> {
    match conv {
        Some(c) if !c.segments.is_empty() => c
            .segments
            .iter()
            .map(|s| SegState {
                reading: s.reading.clone(),
                cands: vec![s.surface.clone()],
                expanded: false,
                selected: 0,
                predicted: false,
                cloud: 0..0,
            })
            .collect(),
        _ => vec![SegState {
            reading: kana.into(),
            cands: vec![kana.into()],
            expanded: false,
            selected: 0,
            predicted: false,
            cloud: 0..0,
        }],
    }
}
