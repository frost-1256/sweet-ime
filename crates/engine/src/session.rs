//! Platform-independent input session: turns key events into preedit,
//! candidate list and commit text. Frontends (TSF, fcitx5) only translate
//! keys and render the resulting state.

use crate::converter::{Conversion, Converter};
use crate::romaji::{self, Composer};
use crate::width;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Space,
    Enter,
    Backspace,
    Delete,
    Escape,
    Tab,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    /// Shift+Left / Shift+Right: shrink / extend the focused segment.
    ShrinkSegment,
    ExtendSegment,
    /// F6..F10 style conversions.
    ToHiragana,
    ToKatakana,
    ToHalfKatakana,
    ToFullAscii,
    ToHalfAscii,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpanAttr {
    /// Unconverted input.
    Input,
    /// Converted, not focused.
    Converted,
    /// Converted and focused.
    Focused,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preedit {
    pub text: String,
    /// Caret position in bytes.
    pub caret: usize,
    /// (start, end, attr) in bytes.
    pub spans: Vec<(usize, usize, SpanAttr)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListKind {
    /// Conversion candidates for the focused segment.
    Conversion,
    /// Predictions shown while typing (Tab to pick).
    Suggestion,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CandidateList {
    pub items: Vec<String>,
    pub selected: usize,
    pub page_size: usize,
    pub kind: ListKind,
}

/// User-facing behaviour switches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// Show the converted text while typing; Enter commits it.
    pub live_conversion: bool,
    /// Show predictions while typing.
    pub suggestions: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { live_conversion: true, suggestions: true }
    }
}

impl Options {
    /// Parses `key = value` lines (`#` comments). Unknown keys are ignored.
    pub fn parse(text: &str) -> Self {
        let mut o = Self::default();
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            let Some((k, v)) = line.split_once('=') else { continue };
            let on = matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "on" | "yes");
            match k.trim() {
                "live_conversion" => o.live_conversion = on,
                "suggestions" => o.suggestions = on,
                _ => {}
            }
        }
        o
    }
}

impl CandidateList {
    pub fn page_start(&self) -> usize {
        self.selected / self.page_size * self.page_size
    }
}

struct SegState {
    reading: String,
    cands: Vec<String>,
    /// Whether `cands` beyond the first entry have been expanded.
    expanded: bool,
    selected: usize,
    /// Candidates are predictions (not conversions of `reading`); don't learn.
    predicted: bool,
}

struct Converting {
    segs: Vec<SegState>,
    focus: usize,
    window: bool,
}

#[derive(Default)]
struct Composing {
    before: String,
    composer: Composer,
    after: String,
    raw: String,
    /// Live conversion of the settled kana (`before` + composer kana).
    live: Option<String>,
    /// Live conversion turned off for this composition (Esc / caret moves).
    live_off: bool,
    suggestions: Vec<String>,
}

impl Composing {
    fn kana(&self) -> String {
        let mut c = self.composer.clone();
        c.flush();
        format!("{}{}{}", self.before, c.kana(), self.after)
    }
    fn settle(&mut self) {
        self.composer.flush();
        self.before.push_str(self.composer.kana());
        self.composer.clear();
    }
    fn is_empty(&self) -> bool {
        self.before.is_empty() && self.composer.is_empty() && self.after.is_empty()
    }
}

pub struct Session {
    comp: Composing,
    conv: Option<Converting>,
    commit: String,
    pub page_size: usize,
    pub options: Options,
    /// Text before the composition (for context-aware conversion).
    left_context: String,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    pub fn new() -> Self {
        Self { comp: Composing::default(), conv: None, commit: String::new(), page_size: 9, options: Options::default(), left_context: String::new() }
    }

    pub fn is_active(&self) -> bool {
        !self.comp.is_empty() || self.conv.is_some()
    }

    /// Sets the document text preceding the composition.
    pub fn set_left_context(&mut self, text: &str) {
        self.left_context.clear();
        self.left_context.push_str(text);
    }

    /// Text committed since the last call.
    pub fn take_commit(&mut self) -> String {
        std::mem::take(&mut self.commit)
    }

    pub fn reset(&mut self) {
        self.comp = Composing::default();
        self.conv = None;
    }

    /// Commit whatever is shown (used when focus is lost).
    pub fn commit_all(&mut self) {
        let text = self.preedit().text;
        self.commit.push_str(&text);
        self.reset();
    }

    pub fn preedit(&self) -> Preedit {
        if let Some(cv) = &self.conv {
            let mut p = Preedit::default();
            for (i, s) in cv.segs.iter().enumerate() {
                let st = p.text.len();
                p.text.push_str(&s.cands[s.selected]);
                let attr = if i == cv.focus { SpanAttr::Focused } else { SpanAttr::Converted };
                p.spans.push((st, p.text.len(), attr));
            }
            p.caret = p.text.len();
            return p;
        }
        let c = &self.comp;
        if let Some(live) = &c.live {
            let text = format!("{live}{}", c.composer.pending());
            let spans = if text.is_empty() { vec![] } else { vec![(0, text.len(), SpanAttr::Input)] };
            return Preedit { caret: text.len(), text, spans };
        }
        let text = format!("{}{}{}", c.before, c.composer.preedit(), c.after);
        let caret = c.before.len() + c.composer.preedit().len();
        let spans = if text.is_empty() { vec![] } else { vec![(0, text.len(), SpanAttr::Input)] };
        Preedit { text, caret, spans }
    }

    /// Recomputes live conversion and suggestions after a composing edit.
    fn refresh(&mut self, conv: &Converter) {
        let c = &mut self.comp;
        c.live = None;
        c.suggestions.clear();
        if self.conv.is_some() || c.is_empty() {
            return;
        }
        let settled = format!("{}{}", c.before, c.composer.kana());
        if self.options.live_conversion && !c.live_off && c.after.is_empty() && !settled.is_empty() {
            // Lattice only: this runs on every keystroke.
            c.live = conv.convert(&settled, &[], 1).first().map(|r| r.surface());
        }
        if self.options.suggestions && c.after.is_empty() && settled.chars().count() >= 2 {
            let shown = c.live.clone().unwrap_or_else(|| settled.clone());
            c.suggestions = conv
                .predict_sentence(&settled, 5)
                .into_iter()
                .filter(|s| *s != shown)
                .collect();
        }
    }

    pub fn candidates(&self) -> Option<CandidateList> {
        let Some(cv) = self.conv.as_ref() else {
            if self.comp.suggestions.is_empty() {
                return None;
            }
            return Some(CandidateList {
                items: self.comp.suggestions.clone(),
                selected: 0,
                page_size: self.page_size,
                kind: ListKind::Suggestion,
            });
        };
        if !cv.window {
            return None;
        }
        let s = &cv.segs[cv.focus];
        Some(CandidateList {
            items: s.cands.clone(),
            selected: s.selected,
            page_size: self.page_size,
            kind: ListKind::Conversion,
        })
    }

    /// Picks entry `index` of the list returned by [`Session::candidates`]
    /// (e.g. a mouse click). Returns whether anything changed.
    pub fn select_candidate(&mut self, conv: &Converter, index: usize) -> bool {
        let changed = if self.conv.is_none() {
            // Suggestion list: commit the prediction directly.
            match self.comp.suggestions.get(index).cloned() {
                Some(text) => {
                    self.commit.push_str(&text);
                    self.reset();
                    true
                }
                None => false,
            }
        } else if self.conv.as_ref().is_some_and(|cv| cv.window) {
            self.choose(conv, index)
        } else {
            false
        };
        self.refresh(conv);
        changed
    }

    /// Selects candidate `idx` of the focused segment, closes the window and
    /// moves on (committing after the last segment).
    fn choose(&mut self, conv: &Converter, idx: usize) -> bool {
        let cv = self.conv.as_mut().unwrap();
        let s = &mut cv.segs[cv.focus];
        if idx >= s.cands.len() {
            return false;
        }
        s.selected = idx;
        cv.window = false;
        if cv.focus + 1 < cv.segs.len() {
            cv.focus += 1;
        } else {
            self.commit_conversion(conv);
        }
        true
    }

    /// Handles a key. Returns whether the key was consumed.
    pub fn handle(&mut self, conv: &Converter, key: Key) -> bool {
        let consumed = if self.conv.is_some() {
            self.handle_converting(conv, key)
        } else {
            self.handle_composing(conv, key)
        };
        self.refresh(conv);
        consumed
    }

    fn handle_composing(&mut self, conv: &Converter, key: Key) -> bool {
        let empty = self.comp.is_empty();
        match key {
            Key::Char(c) if !c.is_control() => {
                self.comp.composer.push(c);
                self.comp.raw.push(c);
                true
            }
            Key::Space if empty => {
                self.commit.push('\u{3000}');
                true
            }
            _ if empty => false,
            Key::Space | Key::Down => {
                self.start_conversion(conv, &[]);
                true
            }
            Key::Enter => {
                let kana = self.comp.kana();
                let text = if self.comp.live.is_some() {
                    // Commit what live conversion shows (pending romaji resolved).
                    conv.convert(&kana, &[], 1).first().map(|r| r.surface()).unwrap_or(kana)
                } else {
                    kana
                };
                self.commit.push_str(&text);
                self.reset();
                true
            }
            Key::Tab if !self.comp.suggestions.is_empty() => {
                let kana = self.comp.kana();
                let cands = std::mem::take(&mut self.comp.suggestions);
                self.conv = Some(Converting {
                    segs: vec![SegState { reading: kana, cands, expanded: true, selected: 0, predicted: true }],
                    focus: 0,
                    window: true,
                });
                true
            }
            Key::Backspace => {
                if !self.comp.composer.backspace() {
                    self.comp.before.pop();
                }
                self.comp.raw.pop();
                true
            }
            Key::Delete => {
                self.comp.settle();
                if !self.comp.after.is_empty() {
                    self.comp.after.remove(0);
                }
                true
            }
            Key::Escape => {
                if self.comp.live.is_some() {
                    // First Esc shows the reading; the second cancels.
                    self.comp.live_off = true;
                } else {
                    self.reset();
                }
                true
            }
            Key::Left => {
                self.comp.live_off = true;
                self.comp.settle();
                if let Some(ch) = self.comp.before.pop() {
                    self.comp.after.insert(0, ch);
                }
                true
            }
            Key::Right => {
                self.comp.settle();
                if !self.comp.after.is_empty() {
                    let ch = self.comp.after.remove(0);
                    self.comp.before.push(ch);
                }
                true
            }
            Key::Home => {
                self.comp.live_off = true;
                self.comp.settle();
                let b = std::mem::take(&mut self.comp.before);
                self.comp.after.insert_str(0, &b);
                true
            }
            Key::End => {
                self.comp.settle();
                let a = std::mem::take(&mut self.comp.after);
                self.comp.before.push_str(&a);
                true
            }
            Key::ToHiragana | Key::ToKatakana | Key::ToHalfKatakana | Key::ToFullAscii | Key::ToHalfAscii => {
                let kana = self.comp.kana();
                let raw = self.comp.raw.clone();
                self.start_conversion(conv, &[]);
                // Collapse to a single segment showing the transliteration.
                let text = transliterate(key, &kana, &raw);
                self.conv = Some(Converting {
                    segs: vec![SegState { reading: kana, cands: vec![text], expanded: false, selected: 0, predicted: false }],
                    focus: 0,
                    window: false,
                });
                true
            }
            _ => true,
        }
    }

    fn start_conversion(&mut self, conv: &Converter, boundaries: &[usize]) {
        let kana = self.comp.kana();
        let result = if boundaries.is_empty() {
            conv.convert_best(&kana, boundaries, &self.left_context)
        } else {
            conv.convert(&kana, boundaries, 1)
        };
        self.conv = Some(Converting { segs: segs_from(result.first(), &kana), focus: 0, window: false });
    }

    fn expand(&mut self, conv: &Converter) {
        let cv = self.conv.as_mut().unwrap();
        let s = &mut cv.segs[cv.focus];
        if s.expanded {
            return;
        }
        s.expanded = true;
        for c in conv.segment_candidates(&s.reading, 40) {
            if !s.cands.contains(&c) {
                s.cands.push(c);
            }
        }
        for c in [
            width::to_half_katakana(&romaji::to_katakana(&s.reading)),
        ] {
            if !s.cands.contains(&c) {
                s.cands.push(c);
            }
        }
    }

    fn commit_conversion(&mut self, conv: &Converter) {
        if let Some(cv) = self.conv.take() {
            for s in &cv.segs {
                let surface = &s.cands[s.selected];
                // Learn explicit choices (anything but the first suggestion).
                if s.selected != 0 && !s.predicted {
                    conv.learn(&s.reading, surface);
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
        let end = start + kana[start..].char_indices().nth(new_len).map(|(i, _)| i).unwrap_or(kana.len() - start);
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
        self.conv = Some(Converting { segs, focus, window: false });
    }

    fn handle_converting(&mut self, conv: &Converter, key: Key) -> bool {
        let window = self.conv.as_ref().unwrap().window;
        match key {
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
            Key::Escape => {
                let cv = self.conv.as_mut().unwrap();
                if cv.window {
                    cv.window = false;
                } else {
                    // Back to composing with the full reading.
                    let kana: String = cv.segs.iter().map(|s| s.reading.as_str()).collect();
                    self.conv = None;
                    self.comp.before = kana;
                    self.comp.after.clear();
                    self.comp.composer.clear();
                }
                true
            }
            Key::Backspace => {
                let cv = self.conv.take().unwrap();
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
            Key::ToHiragana | Key::ToKatakana | Key::ToHalfKatakana | Key::ToFullAscii | Key::ToHalfAscii => {
                let raw = self.comp.raw.clone();
                let cv = self.conv.as_mut().unwrap();
                let single = cv.segs.len() == 1;
                let s = &mut cv.segs[cv.focus];
                let text = transliterate(key, &s.reading, if single { &raw } else { "" });
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
            })
            .collect(),
        _ => vec![SegState { reading: kana.into(), cands: vec![kana.into()], expanded: false, selected: 0, predicted: false }],
    }
}

fn transliterate(key: Key, kana: &str, raw: &str) -> String {
    let ascii = if !raw.is_empty() && romaji::to_hiragana(raw) == kana { raw.to_string() } else { kana.to_string() };
    match key {
        Key::ToHiragana => kana.to_string(),
        Key::ToKatakana => romaji::to_katakana(kana),
        Key::ToHalfKatakana => width::to_half_katakana(&romaji::to_katakana(kana)),
        Key::ToFullAscii => width::to_full_ascii(&ascii),
        Key::ToHalfAscii => ascii,
        _ => kana.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transliteration() {
        assert_eq!(transliterate(Key::ToKatakana, "こーひー", "ko-hi-"), "コーヒー");
        assert_eq!(transliterate(Key::ToHalfAscii, "こーひー", "ko-hi-"), "ko-hi-");
        assert_eq!(transliterate(Key::ToFullAscii, "こーひー", "ko-hi-"), "ｋｏ－ｈｉ－");
        assert_eq!(transliterate(Key::ToHalfKatakana, "がっこう", ""), "ｶﾞｯｺｳ");
    }
}

#[cfg(test)]
mod option_tests {
    use super::Options;

    #[test]
    fn parse_options() {
        let o = Options::parse("# comment\nlive_conversion = off\n suggestions=1 # trailing\nunknown = 3\n");
        assert_eq!(o, Options { live_conversion: false, suggestions: true });
        assert_eq!(Options::parse(""), Options::default());
    }
}
