//! Platform-independent input session: turns key events into preedit,
//! candidate list and commit text. Frontends (TSF, fcitx5) only translate
//! keys and render the resulting state.

use crate::converter::Converter;
use crate::english;
use crate::romaji::{self, Composer};
use crate::width;

mod conversion;
mod live;
mod options;
mod url;

use conversion::{Converting, SegState};
use live::LiveConversion;
pub use options::Options;

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
    /// Right after a commit: report it as the conversion wanted for its
    /// reading (see [`Session::take_report`]).
    ReportCommit,
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

impl CandidateList {
    pub fn page_start(&self) -> usize {
        self.selected / self.page_size * self.page_size
    }
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
    /// `live` already includes the pending keys: they end an English word
    /// ("kyouhalinux" shows 今日はLinux, not 今日はぃぬx).
    live_pending: bool,
    /// Live conversion turned off for this composition (Esc / caret moves).
    live_off: bool,
    /// The last background-refined live conversion. While the reading only
    /// grows, its text is kept so the display doesn't flip between the
    /// lattice and the refined result on every keystroke.
    refined: Option<LiveConversion>,
    suggestions: Vec<String>,
    typo_suggestion: Option<crate::typo::Correction>,
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
        // "amazon" is also the reading あまぞん: katakana first there.
        if !self.raw_spells(reading) || english::katakana_name(reading).is_some() {
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

/// Particles that end a word typed after Shift at Space or Enter
/// ("Googleha" -> Google + は).
const PARTICLES: &[&str] = &[
    "は", "が", "を", "に", "で", "の", "と", "も", "へ", "や", "か", "って", "から", "まで",
    "より",
];

/// Other short endings that end a word typed after Shift at Space or Enter
/// ("Macdayo" -> Mac + だよ, "Applesan" -> Apple + さん).
const ENDINGS: &[&str] = &[
    "です", "でした", "だ", "だよ", "だね", "だった", "じゃ", "さん", "くん", "ちゃん", "など", "とか",
    "なら", "だけ", "しか", "でも", "には", "では", "とは", "にも", "ね", "よ",
];

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
    /// A commit just reported (see [`Session::take_report`]).
    report: Option<crate::reports::Report>,
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
            report: None,
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
        self.last_commit = None;
        self.undo = None;
        self.report = None;
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
            let pending = if c.live_english || c.live_pending { "" } else { c.composer.pending() };
            let text = format!("{}{}", live.surface, pending);
            let spans = if text.is_empty() { vec![] } else { vec![(0, text.len(), SpanAttr::Input)] };
            return Preedit { caret: text.len(), text, spans };
        }
        let text = format!("{}{}{}", c.before, c.composer.preedit(), c.after);
        let caret = c.before.len() + c.composer.preedit().len();
        let spans = if text.is_empty() {
            vec![]
        } else {
            vec![(0, text.len(), SpanAttr::Input)]
        };
        Preedit { text, caret, spans }
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
        let before = self.commit_state();
        let changed = if self.conv.is_none() {
            // Suggestion list: commit the prediction directly.
            match self.comp.suggestions.get(index).cloned() {
                Some(text) => {
                    self.learn_selected_typo(conv, &text);
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
        if changed {
            self.remember_commit(before);
        }
        self.refresh(conv);
        changed
    }

    /// Commits the frontend's choice of surface for the current reading.
    pub fn commit_as(&mut self, text: &str) {
        let before = self.commit_state();
        self.commit.push_str(text);
        self.reset();
        self.remember_commit(before);
    }

    fn learn_selected_typo(&self, conv: &Converter, selected: &str) {
        if let Some(fix) = self
            .comp
            .typo_suggestion
            .as_ref()
            .filter(|fix| !self.private && fix.surface == selected)
        {
            conv.learn(&fix.reading, selected);
        }
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

    /// Opens the candidate list of the focused segment while converting,
    /// keeping the selection (touch keyboards show it at once). Returns
    /// whether it opened.
    pub fn open_candidates(&mut self, conv: &Converter) -> bool {
        if self.conv.as_ref().is_none_or(|cv| cv.window) {
            return false;
        }
        self.expand(conv);
        self.conv.as_mut().unwrap().window = true;
        true
    }

    /// Whether [`Key::UndoCommit`] would restore the last commit now.
    pub fn can_undo(&self) -> bool {
        self.options.undo_commit && self.last_commit.is_some() && !self.is_active()
    }

    /// The exact committed text the frontend must verify before restoring the reading.
    pub fn undoable_commit(&self) -> Option<&str> {
        self.last_commit
            .as_ref()
            .filter(|_| self.can_undo())
            .map(|last| last.text.as_str())
    }

    /// The last commit, while [`Key::ReportCommit`] would report it now.
    /// Nothing is reported from private fields.
    pub fn reportable_commit(&self) -> Option<&str> {
        self.last_commit
            .as_ref()
            .filter(|last| !self.private && !self.is_active() && !last.kana.is_empty())
            .map(|last| last.text.as_str())
    }

    /// The commit that [`Key::ReportCommit`] reported, for the frontend to
    /// store (see [`crate::reports::add`]).
    pub fn take_report(&mut self) -> Option<crate::reports::Report> {
        self.report.take()
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
        if key == Key::ReportCommit {
            let Some(last) = self.last_commit.as_ref().filter(|_| self.reportable_commit().is_some()) else {
                return false;
            };
            // The commit can still be taken back after reporting it.
            self.report = Some(crate::reports::Report { reading: last.kana.clone(), text: last.text.clone() });
            return true;
        }
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
        let before = self.commit_state();
        let consumed = self.handle_key(conv, key);
        self.remember_commit(before);
        consumed
    }

    fn commit_state(&self) -> (String, String, bool, usize) {
        (
            self.comp.kana(),
            self.comp.raw.clone(),
            self.comp.literal,
            self.commit.len(),
        )
    }

    fn remember_commit(&mut self, (kana, raw, literal, commit_start): (String, String, bool, usize)) {
        if self.commit.len() > commit_start && !self.is_active() && !kana.is_empty() {
            self.last_commit = Some(LastCommit {
                text: self.commit[commit_start..].to_string(),
                kana,
                raw,
                literal,
            });
        }
    }

    fn handle_key(&mut self, conv: &Converter, key: Key) -> bool {
        let commit_start = self.commit.len();
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
        // A single key can commit the previous text and begin a new reading
        // (for example when Shift-started English gives way to Japanese).
        if self.is_active() && self.commit.len() > commit_start {
            self.left_context.push_str(&self.commit[commit_start..]);
        }
        self.refresh(conv);
        consumed
    }

    /// Keeps a dedicated symbol key in an active URL or bundled spelling.
    /// The frontend handles other symbols as usual.
    pub fn handle_symbol(&mut self, conv: &Converter, ch: char) -> bool {
        let c = &self.comp;
        if self.conv.is_some() {
            return false;
        }
        let continues_url = ch.is_ascii_punctuation()
            && c.literal
            && (url::recognizes(&c.before) || url::recognizes(&c.kana()));
        let starts_scheme = ch == ':'
            && c.after.is_empty()
            && if c.literal {
                url::scheme(&c.before)
            } else {
                c.before.is_empty() && url::scheme(&c.raw)
            };
        let spelling = !c.literal
            && c.before.is_empty()
            && c.after.is_empty()
            && english::accepts_spelling_symbol(&c.raw, ch);
        (continues_url || starts_scheme || spelling) && self.handle(conv, Key::Char(ch))
    }

    /// Ends a word typed after Shift once Japanese follows it: when the
    /// letters are a known word followed by romaji for three or more kana
    /// ("Googlehaken" -> Google + はけん), or, at Space or Enter (`ending`),
    /// by a particle ("Googleha"), commits the word and goes on in Japanese
    /// with the rest. Returns whether it did.
    fn end_shift_word(&mut self, ending: bool) -> bool {
        let c = &self.comp;
        if !self.options.shift_one_word
            || !c.literal
            || !c.after.is_empty()
            || !c.before.is_ascii()
            || url::recognizes(&c.before)
        {
            return false;
        }
        let text = c.before.clone();
        if english::is_known_word(&text) {
            return false;
        }
        // A known word with only romaji after it, once enough Japanese
        // follows: the longest one with a particle after it ("Chromedehiraku"
        // is Chrome + でひらく, "Hellowosa" Hello + をさ, not Hell + おをさ),
        // or else the longest. Capitals before the romaji end a word too
        // ("AIwotukau", "Xdekaku").
        let particle = |kana: &str| PARTICLES.iter().chain(ENDINGS).any(|p| kana.starts_with(p));
        let romaji_after = |end: usize| {
            let rest = &text[end..];
            if !rest.bytes().all(|b| b.is_ascii_lowercase() || b == b'-') {
                return None;
            }
            let mut composer = romaji::Composer::new();
            for ch in rest.chars() {
                composer.push(ch);
            }
            let kana = composer.kana();
            if kana.bytes().any(|b| b.is_ascii_alphabetic()) {
                return None;
            }
            // A last n is ん at Space or Enter ("Applesan").
            Some(match composer.pending() {
                "n" => (format!("{kana}ん"), true),
                pending => (kana.to_string(), pending.is_empty()),
            })
        };
        let splits: Vec<(usize, (String, bool))> = (1..text.len())
            .rev()
            .filter_map(|end| romaji_after(end).map(|r| (end, r)))
            .filter(|(end, (kana, _))| {
                let w = &text[..*end];
                english::is_known_word(w) || (w.ends_with(|c: char| c.is_ascii_uppercase()) && particle(kana))
            })
            .collect();
        let Some((end, (kana, whole))) =
            splits.iter().find(|(_, (kana, _))| particle(kana)).or(splits.first()).cloned()
        else {
            return false;
        };
        let japanese = if ending {
            whole && (PARTICLES.contains(&kana.as_str()) || ENDINGS.contains(&kana.as_str()))
        } else {
            kana.chars().count() >= 3
        };
        if !japanese {
            return false;
        }
        self.commit.push_str(&text[..end]);
        self.comp = Composing::default();
        for ch in text[end..].chars() {
            self.comp.composer.push(ch);
            self.comp.raw.push(ch);
        }
        true
    }

    fn handle_composing(&mut self, conv: &Converter, key: Key) -> bool {
        let empty = self.comp.is_empty();
        match key {
            Key::Char(ch) if self.comp.literal && !ch.is_control() => {
                self.comp.before.push(ch);
                self.end_shift_word(false);
                true
            }
            Key::Char(c) if !c.is_control() => {
                self.comp.composer.push(c);
                self.comp.raw.push(c);
                if self.comp.before.is_empty()
                    && self.comp.after.is_empty()
                    && matches!(c, ':' | '.' | 'a'..='z')
                    && url::recognizes(&self.comp.raw)
                {
                    self.comp.before = std::mem::take(&mut self.comp.raw);
                    self.comp.composer.clear();
                    self.comp.literal = true;
                }
                true
            }
            Key::Space if empty => {
                self.commit.push('\u{3000}');
                true
            }
            _ if empty => false,
            // Space or Enter right after one Japanese kana ("Googleha").
            Key::Space | Key::Enter if self.comp.literal && self.end_shift_word(true) => {
                self.refresh(conv);
                self.handle_composing(conv, key)
            }
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
                    Some(live)
                        if self.comp.live_english
                            || self.comp.live_pending
                            || self.comp.composer.pending().is_empty() =>
                    {
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
