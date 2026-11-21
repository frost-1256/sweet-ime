//! Platform-independent input session: turns key events into preedit,
//! candidate list and commit text. Frontends (TSF, fcitx5) only translate
//! keys and render the resulting state.

use std::time::Duration;

use crate::converter::{Conversion, Converter};
use crate::{english, special};
use crate::romaji::{self, Composer};
use crate::{typo, width};

const SUGGESTION_LIMIT: usize = 5;
const TYPO_SUGGESTION_BUDGET: Duration = Duration::from_millis(4);
const TYPO_CONVERSION_BUDGET: Duration = Duration::from_millis(12);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    /// A letter typed with Shift held, which starts a temporary English word.
    ShiftedLetter(char),
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
    /// Right after a commit: remove the committed text and edit its
    /// reading again (see [`Session::take_undo`]).
    UndoCommit,
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
    /// Rerank conversions with the neural model when it is installed.
    pub neural: bool,
    /// Learn from chosen candidates and prefer them later.
    pub learning: bool,
    /// Show "あ" / "A" near the caret when the input mode changes.
    pub mode_indicator: bool,
    /// Colors of the candidate window and the mode badge.
    pub theme: crate::Theme,
    /// Look for a newer release on GitHub (settings app and sign-in).
    pub update_check: bool,
    /// Offer dates and times for readings such as きょう and いま.
    pub dates: bool,
    /// Offer emoji and kaomoji for readings such as えがお and かお.
    pub emoji: bool,
    /// Offer the value of arithmetic typed with a trailing = (1+2=).
    pub calculator: bool,
    /// Ctrl+Backspace right after committing goes back to the reading.
    pub undo_commit: bool,
    /// Learn nothing in password fields and private browser windows.
    pub private_fields: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            live_conversion: true,
            suggestions: true,
            neural: true,
            learning: true,
            mode_indicator: true,
            theme: crate::Theme::Default,
            update_check: true,
            dates: true,
            emoji: true,
            calculator: true,
            undo_commit: true,
            private_fields: true,
        }
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
                "neural" => o.neural = on,
                "learning" => o.learning = on,
                "mode_indicator" => o.mode_indicator = on,
                "theme" => o.theme = crate::Theme::from_id(v).unwrap_or_default(),
                "update_check" => o.update_check = on,
                "dates" => o.dates = on,
                "emoji" => o.emoji = on,
                "calculator" => o.calculator = on,
                "undo_commit" => o.undo_commit = on,
                "private_fields" => o.private_fields = on,
                _ => {}
            }
        }
        o
    }

    /// Serializes in the format [`Options::parse`] reads.
    pub fn to_text(&self) -> String {
        let b = |v: bool| if v { "on" } else { "off" };
        format!(
            "# Sweet IME settings (applied when the IME is next activated)
             live_conversion = {}
             suggestions = {}
             neural = {}
             learning = {}
             mode_indicator = {}
             theme = {}
             update_check = {}
             dates = {}
             emoji = {}
             calculator = {}
             undo_commit = {}
             private_fields = {}
",
            b(self.live_conversion),
            b(self.suggestions),
            b(self.neural),
            b(self.learning),
            b(self.mode_indicator),
            self.theme.id(),
            b(self.update_check),
            b(self.dates),
            b(self.emoji),
            b(self.calculator),
            b(self.undo_commit),
            b(self.private_fields),
        )
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
    corrected: bool,
}

#[derive(Clone)]
struct LiveConversion {
    reading: String,
    surface: String,
}

#[derive(Default)]
struct Composing {
    before: String,
    composer: Composer,
    after: String,
    raw: String,
    /// Store a temporary English word verbatim in before/after.
    literal: bool,
    live: Option<LiveConversion>,
    /// `live` shows the typed keys as an English word (see `Composing::english`).
    live_english: bool,
    /// Live conversion turned off for this composition (Esc / caret moves).
    live_off: bool,
    /// The last background-refined live conversion. While the reading only
    /// grows, its text is kept so the display doesn't flip between the
    /// lattice and the refined result on every keystroke.
    refined: Option<LiveConversion>,
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

    /// Whether `raw` holds the keys of the whole composition, in order.
    fn raw_spells(&self, reading: &str) -> bool {
        self.before.is_empty() && self.after.is_empty() && romaji::to_hiragana(&self.raw) == reading
    }

    fn bundled_candidates(&self, reading: &str) -> Option<Vec<String>> {
        if !self.raw_spells(reading) {
            return None;
        }
        let mut candidates = Vec::new();
        for (spelling, kana) in english::spellings().filter(|(s, _)| english::spells(s, &self.raw)) {
            for surface in std::iter::once(spelling).chain(kana) {
                if !candidates.iter().any(|c| c == surface) {
                    candidates.push(surface.to_owned());
                }
            }
        }
        (!candidates.is_empty()).then_some(candidates)
    }

    /// The typed keys as an English word, for keys that are not romaji
    /// ("github" would otherwise read ぎtふb): a bundled spelling, or the
    /// keys themselves unless `corrected` (a typo fix reads as Japanese).
    fn english(&self, reading: &str, corrected: bool) -> Option<String> {
        if !self.raw.bytes().all(|b| b.is_ascii_alphabetic()) {
            return None;
        }
        if let Some(c) = self.bundled_candidates(reading) {
            return c.into_iter().next();
        }
        // A word, not Japanese after a stray letter ("vrchatdeasobu" reads
        // vrちゃtであそぶ): little kana follows the last letter that isn't romaji.
        // Likewise little kana comes before the first one: "kyouhadisc"
        // is Japanese followed by a word being typed, not one word.
        let tail = reading.chars().rev().take_while(|c| !c.is_ascii_alphabetic()).count();
        let lead = reading.chars().take_while(|c| !c.is_ascii_alphabetic()).count();
        (!corrected && tail <= 2 && lead <= 2 && self.raw_spells(reading)).then(|| self.raw.clone())
    }

    fn typo_input(&self, reading: &str) -> Option<&str> {
        // Once the caret splits the composition, the raw keys may be out of order.
        if !self.before.is_empty()
            || !self.after.is_empty()
            || !(5..=64).contains(&self.raw.len())
            || !self.raw.bytes().all(|b| b.is_ascii_lowercase() || matches!(b, b'-' | b'\''))
            || romaji::to_hiragana(&self.raw) != reading
        {
            return None;
        }
        let kana = reading.chars().filter(|c| matches!(c, 'ぁ'..='ゖ')).count();
        let letters = reading.bytes().filter(u8::is_ascii_alphabetic).count();
        (kana >= 2 && letters <= 3).then_some(&self.raw)
    }
}

/// What to show for `reading` while its refined conversion is pending:
/// the lattice result, unless it rewrites text the refined result for an
/// earlier part of the reading already settled. Then that text is kept,
/// followed by the lattice's conversion of the rest, if the lattice splits
/// the reading there. (A refined result that ended mid-word doesn't line up
/// with a split, and the lattice result is shown.)
fn stable_live(refined: Option<&LiveConversion>, reading: &str, lattice: &crate::Conversion) -> String {
    let surface = lattice.surface();
    let Some(r) = refined else { return surface };
    if !reading.starts_with(r.reading.as_str()) || surface.starts_with(&r.surface) {
        return surface;
    }
    let mut consumed = 0;
    for (i, seg) in lattice.segments.iter().enumerate() {
        if consumed == r.reading.len() {
            let rest: String = lattice.segments[i..].iter().map(|s| s.surface.as_str()).collect();
            return format!("{}{rest}", r.surface);
        }
        consumed += seg.reading.len();
    }
    if consumed == r.reading.len() {
        return r.surface.clone();
    }
    surface
}

pub struct Session {
    comp: Composing,
    conv: Option<Converting>,
    commit: String,
    pub page_size: usize,
    pub options: Options,
    /// The text field is private (a password, a private browser window):
    /// nothing typed there is learned. Set by the frontend.
    pub private: bool,
    /// Text before the composition (for context-aware conversion).
    left_context: String,
    /// The last commit, while the next key may still take it back.
    last_commit: Option<LastCommit>,
    /// A commit just taken back (see [`Session::take_undo`]).
    undo: Option<String>,
}

/// What a commit replaced, to restore it.
struct LastCommit {
    text: String,
    kana: String,
    raw: String,
    literal: bool,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    pub fn new() -> Self {
        Self {
            comp: Composing::default(),
            conv: None,
            commit: String::new(),
            page_size: 9,
            options: Options::default(),
            private: false,
            left_context: String::new(),
            last_commit: None,
            undo: None,
        }
    }

    pub fn is_active(&self) -> bool {
        !self.comp.is_empty() || self.conv.is_some()
    }

    /// Whether a conversion (Space) is in progress, as opposed to typing.
    pub fn is_converting(&self) -> bool {
        self.conv.is_some()
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
            // An English word already includes the pending keys.
            let pending = if c.live_english { "" } else { c.composer.pending() };
            let text = format!("{}{}", live.surface, pending);
            let spans = if text.is_empty() { vec![] } else { vec![(0, text.len(), SpanAttr::Input)] };
            return Preedit { caret: text.len(), text, spans };
        }
        let text = format!("{}{}{}", c.before, c.composer.preedit(), c.after);
        let caret = c.before.len() + c.composer.preedit().len();
        let spans = if text.is_empty() { vec![] } else { vec![(0, text.len(), SpanAttr::Input)] };
        Preedit { text, caret, spans }
    }

    /// The reading and left context of the live conversion shown now, for
    /// refining it in the background (e.g. with a neural reranker).
    pub fn live_request(&self) -> Option<(String, String)> {
        let live = self.comp.live.as_ref().filter(|_| !self.comp.live_english)?;
        Some((live.reading.clone(), self.left_context.clone()))
    }

    /// Replaces the live conversion with `surface` if `reading` is still what
    /// is being typed. Returns whether the display changed.
    pub fn apply_live(&mut self, reading: &str, surface: &str) -> bool {
        let Some(live) = &mut self.comp.live else { return false };
        if live.reading != reading || self.comp.live_english {
            return false;
        }
        self.comp.refined = Some(LiveConversion { reading: reading.to_string(), surface: surface.to_string() });
        if live.surface == surface {
            return false;
        }
        live.surface = surface.to_string();
        true
    }

    /// Recomputes live conversion and suggestions after a composing edit.
    fn refresh(&mut self, conv: &Converter) {
        let c = &mut self.comp;
        c.live = None;
        c.live_english = false;
        c.suggestions.clear();
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
        let suggest = self.options.suggestions && c.after.is_empty() && settled.chars().count() >= 2;
        let unresolved = typo::has_unresolved_romaji(&settled);
        let mut correction = None;
        if (suggest || (live && unresolved)) && c.composer.pending().is_empty() {
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
        let english = if live && typo::has_unresolved_romaji(&kana) {
            let bundled = c.bundled_candidates(&kana).map(|b| b[0].clone());
            let typo_possible = c.typo_input(&settled).is_some() && !c.composer.pending().is_empty();
            bundled.or_else(|| c.english(&kana, correction.is_some()).filter(|_| unresolved && !typo_possible))
        } else {
            None
        };
        let mut live_best = None;
        if let Some(text) = english {
            c.live = Some(LiveConversion { reading: settled.clone(), surface: text });
            c.live_english = true;
            if let Some(fix) = correction.filter(|_| suggest) {
                c.suggestions.push(fix.surface);
            }
            return;
        }
        if live {
            // Lattice only: this runs on every keystroke.
            c.live = match correction.take_if(|_| unresolved) {
                Some(fix) => Some(LiveConversion { reading: fix.reading, surface: fix.surface }),
                None => {
                    live_best = conv.convert(&settled, &[], 1).into_iter().next();
                    live_best.as_ref().map(|r| LiveConversion {
                        reading: settled.clone(),
                        surface: stable_live(c.refined.as_ref(), &settled, r),
                    })
                }
            };
        }
        if suggest && self.options.calculator {
            if let Some(value) = special::calculator_candidates(&settled).into_iter().next() {
                c.suggestions.push(value);
            }
        }
        if suggest {
            let shown = c.live.as_ref().map_or(settled.as_str(), |live| live.surface.as_str());
            if let Some(fix) = correction.filter(|fix| fix.surface != shown) {
                c.suggestions.push(fix.surface);
            }
            let reading = c.live.as_ref().map_or(settled.as_str(), |live| live.reading.as_str());
            for suggestion in conv.predict_with_conversion(reading, SUGGESTION_LIMIT, live_best.as_ref()) {
                if suggestion != shown && !c.suggestions.contains(&suggestion) {
                    c.suggestions.push(suggestion);
                    if c.suggestions.len() == SUGGESTION_LIMIT {
                        break;
                    }
                }
            }
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

    /// Whether [`Key::UndoCommit`] would restore the last commit now.
    pub fn can_undo(&self) -> bool {
        self.options.undo_commit && self.last_commit.is_some() && !self.is_active()
    }

    /// Text to remove before the caret: the commit that
    /// [`Key::UndoCommit`] took back. The frontend deletes it (if the
    /// document still has it there) and shows the restored reading.
    pub fn take_undo(&mut self) -> Option<String> {
        self.undo.take()
    }

    /// Handles a key. Returns whether the key was consumed.
    pub fn handle(&mut self, conv: &Converter, key: Key) -> bool {
        // Only the key right after a commit can take it back.
        let last = self.last_commit.take();
        if key == Key::UndoCommit {
            let Some(last) = last.filter(|_| self.options.undo_commit && !self.is_active()) else { return false };
            self.comp = Composing::default();
            if !last.literal && romaji::to_hiragana(&last.raw) == last.kana {
                for c in last.raw.chars() {
                    self.comp.composer.push(c);
                }
                self.comp.raw = last.raw;
            } else {
                self.comp.before = last.kana;
                self.comp.literal = last.literal;
            }
            // Edit the reading, not the conversion it had.
            self.comp.live_off = true;
            self.undo = Some(last.text);
            self.refresh(conv);
            return true;
        }
        let before = (self.comp.kana(), self.comp.raw.clone(), self.comp.literal, self.commit.len());
        let consumed = self.handle_key(conv, key);
        if self.commit.len() > before.3 && !self.is_active() && !before.0.is_empty() {
            self.last_commit = Some(LastCommit {
                text: self.commit[before.3..].to_string(),
                kana: before.0,
                raw: before.1,
                literal: before.2,
            });
        }
        consumed
    }

    fn handle_key(&mut self, conv: &Converter, key: Key) -> bool {
        let key = if let Key::ShiftedLetter(ch) = key {
            if !self.comp.literal {
                if self.conv.is_some() {
                    self.commit_conversion(conv);
                } else if !self.comp.is_empty() {
                    self.handle_composing(conv, Key::Enter);
                }
                self.comp.literal = true;
            }
            Key::Char(ch)
        } else {
            key
        };
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
            Key::Char(ch) if self.comp.literal && !ch.is_control() => {
                self.comp.before.push(ch);
                true
            }
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
            Key::Space if self.comp.literal => {
                self.commit_all();
                self.commit.push(' ');
                true
            }
            Key::Down if self.comp.literal => false,
            Key::Space | Key::Down => {
                self.start_conversion(conv);
                true
            }
            Key::Enter => {
                let kana = self.comp.kana();
                let text = match &self.comp.live {
                    // Commit exactly what is shown.
                    Some(live) if self.comp.live_english || self.comp.composer.pending().is_empty() => {
                        live.surface.clone()
                    }
                    // Pending romaji resolves to more kana: convert again.
                    Some(_) => conv.convert(&kana, &[], 1).first().map(|r| r.surface()).unwrap_or(kana),
                    None => kana,
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
                    corrected: false,
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
                if self.comp.literal {
                    return false;
                }
                let kana = self.comp.kana();
                let raw = self.comp.raw.clone();
                // Collapse to a single segment showing the transliteration.
                let text = transliterate(key, &kana, &raw);
                self.conv = Some(Converting {
                    segs: vec![SegState { reading: kana, cands: vec![text], expanded: false, selected: 0, predicted: false }],
                    focus: 0,
                    window: false,
                    corrected: false,
                });
                true
            }
            _ => true,
        }
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

    fn start_conversion(&mut self, conv: &Converter) {
        let kana = self.comp.kana();
        if self.options.calculator {
            let cands = special::calculator_candidates(&kana);
            if !cands.is_empty() {
                self.conv = Some(Converting {
                    segs: vec![SegState { reading: kana, cands, expanded: true, selected: 0, predicted: true }],
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
                }],
                focus: 0,
                window: false,
                corrected: false,
            });
            return;
        }
        let correction = if typo::has_unresolved_romaji(&kana) {
            self.comp.typo_input(&kana).and_then(|raw| {
                typo::correct(conv, raw, 0, TYPO_CONVERSION_BUDGET)
            })
        } else {
            None
        };
        if typo::has_unresolved_romaji(&kana) {
            if let Some(word) = self.comp.english(&kana, correction.is_some()) {
                // Keys that are not romaji: the word as typed, capitalized,
                // then the kana reading's conversion.
                let mut cands = vec![word.clone()];
                let mut chars = word.chars();
                let capitalized: String =
                    chars.next().map(|c| c.to_ascii_uppercase()).into_iter().chain(chars).collect();
                let kana_text = conv.convert_best(&kana, &[], &self.left_context).first().map(|r| r.surface());
                for c in std::iter::once(capitalized).chain(kana_text) {
                    if !cands.contains(&c) {
                        cands.push(c);
                    }
                }
                self.conv = Some(Converting {
                    segs: vec![SegState { reading: kana, cands, expanded: false, selected: 0, predicted: false }],
                    focus: 0,
                    window: false,
                    corrected: false,
                });
                return;
            }
        }
        let reading = correction.as_ref().map_or(kana.as_str(), |fix| fix.reading.as_str());
        let result = conv.convert_best(reading, &[], &self.left_context);
        let mut segs = segs_from(result.first(), reading);
        // A special reading (えがお) stays one segment so its candidates show.
        if segs.len() > 1 && !self.special_candidates(reading).is_empty() {
            let first = conv.segment_candidates(reading, 1).into_iter().next().unwrap_or_else(|| reading.to_string());
            segs = vec![SegState { reading: reading.to_string(), cands: vec![first], expanded: false, selected: 0, predicted: false }];
        }
        self.conv = Some(Converting {
            segs,
            focus: 0,
            window: false,
            corrected: correction.is_some(),
        });
    }

    fn expand(&mut self, conv: &Converter) {
        let special = {
            let cv = self.conv.as_ref().unwrap();
            self.special_candidates(&cv.segs[cv.focus].reading)
        };
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
                if s.selected != 0 && !s.predicted && !self.private {
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
        self.conv = Some(Converting { segs, focus, window: false, corrected: old.corrected });
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
            Key::ToHiragana | Key::ToKatakana | Key::ToHalfKatakana | Key::ToFullAscii | Key::ToHalfAscii => {
                let raw = self.comp.raw.clone();
                let cv = self.conv.as_mut().unwrap();
                let single = cv.segs.len() == 1;
                let s = &mut cv.segs[cv.focus];
                let text = if single && cv.corrected && matches!(key, Key::ToFullAscii | Key::ToHalfAscii) {
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

fn is_plain_interjection(reading: &str) -> bool {
    // Keep standalone reactions intact in live conversion. Space still
    // offers kanji for the same reading.
    let reading = reading.trim_end_matches(['、', '。', '！', '？', '…', 'ー', '～']);
    matches!(
        reading,
        "おお" | "おおっ" | "ああ" | "あっ" | "おっ" | "へえ" | "へぇ" | "うわ" | "うわっ" | "わあ"
    )
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
        assert_eq!(o, Options { live_conversion: false, ..Options::default() });
        assert_eq!(Options::parse(""), Options::default());
        let o = Options { live_conversion: false, suggestions: true, neural: false, learning: true, mode_indicator: false, theme: crate::Theme::Sakura, update_check: false, dates: false, emoji: true, calculator: false, undo_commit: false, private_fields: false };
        assert_eq!(Options::parse(&o.to_text()), o);
        assert_eq!(Options::parse("theme = midnight").theme, crate::Theme::Midnight);
        assert_eq!(Options::parse("theme = unknown").theme, crate::Theme::Default);
    }
}

#[cfg(test)]
mod live_tests {
    use super::{LiveConversion, stable_live};
    use crate::{Conversion, Segment};

    fn conv(segs: &[(&str, &str)]) -> Conversion {
        Conversion {
            segments: segs
                .iter()
                .map(|(r, s)| Segment { reading: r.to_string(), surface: s.to_string(), words: Vec::new() })
                .collect(),
            cost: 0,
        }
    }

    #[test]
    fn keeps_refined_text_while_typing() {
        let r = LiveConversion { reading: "きかい".into(), surface: "機械".into() };
        // The lattice agrees with the refined text: use it.
        assert_eq!(stable_live(Some(&r), "きかいが", &conv(&[("きかい", "機械"), ("が", "が")])), "機械が");
        // The lattice would flip it back: keep it, then the lattice's rest.
        let flip = conv(&[("きかい", "機会"), ("がこわれた", "が壊れた")]);
        assert_eq!(stable_live(Some(&r), "きかいがこわれた", &flip), "機械が壊れた");
        assert_eq!(stable_live(Some(&r), "きかい", &conv(&[("きかい", "機会")])), "機械");
        // No segment boundary there: the lattice result.
        let joined = conv(&[("きかいが", "機会が")]);
        assert_eq!(stable_live(Some(&r), "きかいが", &joined), "機会が");
        // A different reading (e.g. after Backspace): the lattice result.
        assert_eq!(stable_live(Some(&r), "きか", &conv(&[("きか", "時価")])), "時価");
        assert_eq!(stable_live(None, "きかい", &conv(&[("きかい", "機会")])), "機会");
    }
}
