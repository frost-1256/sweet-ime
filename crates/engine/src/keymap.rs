//! User-configurable key bindings.
//!
//! A binding maps a key chord (a Windows virtual-key code plus modifiers)
//! to an action, optionally only while composing or only while not
//! composing. Keys without a binding fall back to typing their character.
//!
//! File format (`keymap.txt`), one binding per line, `#` comments:
//!
//! ```text
//! Ctrl+Space   ime_toggle
//! Henkan       convert     composing
//! Henkan       ime_on
//! ```

use crate::session::Key;

/// What a key does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    ImeToggle,
    ImeOn,
    ImeOff,
    /// An editing key passed to the session (never `Key::Char`).
    Key(Key),
}

/// Format version written to files (see [`Keymap::parse`]).
pub const KEYMAP_VERSION: u32 = 3;

/// When a binding applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum When {
    #[default]
    Always,
    /// Only while text is being composed or converted.
    Composing,
    /// Only while nothing is being composed.
    Idle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    /// Windows virtual-key code (normalized, see [`normalize_vk`]).
    pub vk: u16,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub chord: Chord,
    pub action: Action,
    pub when: When,
}

/// (file name, display name, virtual-key code) of every bindable key.
pub const KEYS: &[(&str, &str, u16)] = &[
    ("Space", "Space", 0x20),
    ("LeftShift", "左 Shift", 0xA0),
    ("RightShift", "右 Shift", 0xA1),
    ("LeftCtrl", "左 Ctrl", 0xA2),
    ("RightCtrl", "右 Ctrl", 0xA3),
    ("LeftAlt", "左 Alt / Option", 0xA4),
    ("RightAlt", "右 Alt / Option", 0xA5),
    ("Enter", "Enter", 0x0D),
    ("Escape", "Esc", 0x1B),
    ("Tab", "Tab", 0x09),
    ("Backspace", "Backspace", 0x08),
    ("Delete", "Delete", 0x2E),
    ("Insert", "Insert", 0x2D),
    ("Left", "←", 0x25),
    ("Right", "→", 0x27),
    ("Up", "↑", 0x26),
    ("Down", "↓", 0x28),
    ("Home", "Home", 0x24),
    ("End", "End", 0x23),
    ("PageUp", "PageUp", 0x21),
    ("PageDown", "PageDown", 0x22),
    ("Kanji", "半角/全角", 0x19),
    ("Henkan", "変換", 0x1C),
    ("Muhenkan", "無変換", 0x1D),
    ("Kana", "ひらがな", 0xF2),
    ("Eisu", "英数", 0xF0),
    ("ImeOn", "IME オン", 0x16),
    ("ImeOff", "IME オフ", 0x1A),
    ("Backquote", "` (日本語配列では @)", 0xC0),
    ("F1", "F1", 0x70),
    ("F2", "F2", 0x71),
    ("F3", "F3", 0x72),
    ("F4", "F4", 0x73),
    ("F5", "F5", 0x74),
    ("F6", "F6", 0x75),
    ("F7", "F7", 0x76),
    ("F8", "F8", 0x77),
    ("F9", "F9", 0x78),
    ("F10", "F10", 0x79),
    ("F11", "F11", 0x7A),
    ("F12", "F12", 0x7B),
    ("A", "A", 0x41),
    ("B", "B", 0x42),
    ("C", "C", 0x43),
    ("D", "D", 0x44),
    ("E", "E", 0x45),
    ("F", "F", 0x46),
    ("G", "G", 0x47),
    ("H", "H", 0x48),
    ("I", "I", 0x49),
    ("J", "J", 0x4A),
    ("K", "K", 0x4B),
    ("L", "L", 0x4C),
    ("M", "M", 0x4D),
    ("N", "N", 0x4E),
    ("O", "O", 0x4F),
    ("P", "P", 0x50),
    ("Q", "Q", 0x51),
    ("R", "R", 0x52),
    ("S", "S", 0x53),
    ("T", "T", 0x54),
    ("U", "U", 0x55),
    ("V", "V", 0x56),
    ("W", "W", 0x57),
    ("X", "X", 0x58),
    ("Y", "Y", 0x59),
    ("Z", "Z", 0x5A),
    ("0", "0", 0x30),
    ("1", "1", 0x31),
    ("2", "2", 0x32),
    ("3", "3", 0x33),
    ("4", "4", 0x34),
    ("5", "5", 0x35),
    ("6", "6", 0x36),
    ("7", "7", 0x37),
    ("8", "8", 0x38),
    ("9", "9", 0x39),
];

/// (file name, display name, action) of every action.
pub const ACTIONS: &[(&str, &str, Action)] = &[
    ("ime_toggle", "IME オン/オフ切り替え", Action::ImeToggle),
    ("ime_on", "IME オン", Action::ImeOn),
    ("ime_off", "IME オフ", Action::ImeOff),
    ("convert", "変換 / 次の候補", Action::Key(Key::Space)),
    ("half_space", "半角スペースを入力", Action::Key(Key::HalfSpace)),
    ("commit", "確定", Action::Key(Key::Enter)),
    ("cancel", "取り消し", Action::Key(Key::Escape)),
    ("backspace", "前の文字を削除", Action::Key(Key::Backspace)),
    ("delete", "後ろの文字を削除", Action::Key(Key::Delete)),
    ("left", "左へ / 前の文節", Action::Key(Key::Left)),
    ("right", "右へ / 次の文節", Action::Key(Key::Right)),
    ("up", "前の候補", Action::Key(Key::Up)),
    ("down", "次の候補", Action::Key(Key::Down)),
    ("home", "先頭へ", Action::Key(Key::Home)),
    ("end", "末尾へ", Action::Key(Key::End)),
    ("page_up", "前の候補ページ", Action::Key(Key::PageUp)),
    ("page_down", "次の候補ページ", Action::Key(Key::PageDown)),
    ("shrink", "文節を縮める", Action::Key(Key::ShrinkSegment)),
    ("extend", "文節を伸ばす", Action::Key(Key::ExtendSegment)),
    ("hiragana", "ひらがなに変換", Action::Key(Key::ToHiragana)),
    ("katakana", "全角カタカナに変換", Action::Key(Key::ToKatakana)),
    ("half_katakana", "半角カタカナに変換", Action::Key(Key::ToHalfKatakana)),
    ("full_ascii", "全角英数に変換", Action::Key(Key::ToFullAscii)),
    ("half_ascii", "半角英数に変換", Action::Key(Key::ToHalfAscii)),
    ("suggest", "予測候補を選ぶ", Action::Key(Key::Tab)),
    ("undo_commit", "確定を取り消す", Action::Key(Key::UndoCommit)),
    ("report_commit", "直前の変換を報告", Action::Key(Key::ReportCommit)),
];

pub const WHENS: &[(&str, &str, When)] = &[
    ("always", "常に", When::Always),
    ("composing", "入力中のみ", When::Composing),
    ("idle", "入力していない時のみ", When::Idle),
];

/// Maps keys that have several codes to one (半角/全角 sends 0xF3 / 0xF4
/// alternately on Japanese keyboards).
pub fn normalize_vk(vk: u16) -> u16 {
    match vk {
        0xF3 | 0xF4 => 0x19,
        v => v,
    }
}

/// Keys that exist only for the IME (半角/全角, 変換, 無変換, ひらがな,
/// 英数, IME on/off). Layouts produce some of them with a modifier held:
/// the Japanese layout for 101-key keyboards (kbd101) sends 半角/全角 for
/// Alt+`. With no binding of its own, such a chord uses the key's
/// unmodified binding.
pub fn is_ime_key(vk: u16) -> bool {
    matches!(normalize_vk(vk), 0x19 | 0x16 | 0x1A | 0x1C | 0x1D | 0xF0 | 0xF2)
}

/// Whether the key types a character (letters, digits, punctuation).
/// Shift is part of the character for these, so Shift chords do not fall
/// back to the unshifted binding.
pub fn is_character_key(vk: u16) -> bool {
    matches!(vk, 0x30..=0x39 | 0x41..=0x5A | 0xBA..=0xC0 | 0xDB..=0xDF | 0xE2)
}

pub const fn is_modifier_key(vk: u16) -> bool {
    matches!(vk, 0xA0..=0xA5)
}

/// Tracks an isolated modifier press across the platform's press and release events.
#[derive(Default)]
pub struct ModifierTap {
    down: u8,
    pending: Option<u16>,
}

impl ModifierTap {
    pub fn press(&mut self, vk: u16, alone: bool) {
        if !is_modifier_key(vk) {
            self.pending = None;
            return;
        }
        let bit = 1 << (vk - 0xA0);
        if self.down & bit == 0 {
            self.pending = (alone && self.down == 0).then_some(vk);
            self.down |= bit;
        }
    }

    pub fn will_release(&self, vk: u16) -> bool {
        self.pending == Some(vk)
    }

    pub fn release(&mut self, vk: u16) -> bool {
        if !is_modifier_key(vk) {
            return false;
        }
        self.down &= !(1 << (vk - 0xA0));
        self.pending.take() == Some(vk)
    }
}

pub fn key_name(vk: u16) -> Option<&'static str> {
    KEYS.iter().find(|k| k.2 == vk).map(|k| k.0)
}

pub fn key_label(vk: u16) -> String {
    KEYS.iter().find(|k| k.2 == vk).map_or_else(|| format!("0x{vk:02X}"), |k| k.1.to_string())
}

pub fn action_label(a: Action) -> &'static str {
    ACTIONS.iter().find(|x| x.2 == a).map_or("?", |x| x.1)
}

/// IME mode keys to register with the host. Include the raw layout aliases
/// because TSF matches preserved keys before our keymap normalizes them.
pub fn preserved_toggles(map: &Keymap) -> Vec<(Chord, Action)> {
    let mut out: Vec<(Chord, Action)> = Vec::new();
    for b in &map.bindings {
        let toggle = matches!(b.action, Action::ImeToggle | Action::ImeOn | Action::ImeOff);
        if !(toggle && b.when == When::Always) {
            continue;
        }
        let mut add = |c: Chord| {
            if map.bindings.iter().any(|x| x.chord == c && x.action != b.action) {
                return;
            }
            let keys: &[u16] = if c.vk == 0x19 { &[0x19, 0xF3, 0xF4] } else { &[c.vk] };
            for &vk in keys {
                let raw = Chord { vk, ..c };
                if !out.iter().any(|(x, _)| *x == raw) {
                    out.push((raw, b.action));
                }
            }
        };
        if b.chord.ctrl || b.chord.alt || is_ime_key(b.chord.vk) {
            add(b.chord);
        }
        if b.chord == Chord::key(0x19) {
            // kbd101 sends 半角/全角 for Alt+`.
            add(Chord { alt: true, ..b.chord });
        }
    }
    out
}

pub fn when_label(w: When) -> &'static str {
    WHENS.iter().find(|x| x.2 == w).map_or("?", |x| x.1)
}

impl Chord {
    pub const fn key(vk: u16) -> Self {
        Self { vk, ctrl: false, shift: false, alt: false }
    }

    pub const fn shift(vk: u16) -> Self {
        Self { vk, ctrl: false, shift: true, alt: false }
    }

    /// "Ctrl+Shift+変換"
    pub fn label(&self) -> String {
        let mut s = String::new();
        for (on, m) in [(self.ctrl, "Ctrl+"), (self.alt, "Alt+"), (self.shift, "Shift+")] {
            if on {
                s.push_str(m);
            }
        }
        s + &key_label(self.vk)
    }

    fn to_text(self) -> String {
        let mut s = String::new();
        for (on, m) in [(self.ctrl, "Ctrl+"), (self.alt, "Alt+"), (self.shift, "Shift+")] {
            if on {
                s.push_str(m);
            }
        }
        match key_name(self.vk) {
            Some(n) => s + n,
            None => s + &format!("0x{:02X}", self.vk),
        }
    }

    fn parse(text: &str) -> Option<Self> {
        let mut c = Chord::key(0);
        let mut parts: Vec<&str> = text.split('+').collect();
        let key = parts.pop()?.trim();
        for m in parts {
            match m.trim().to_ascii_lowercase().as_str() {
                "ctrl" => c.ctrl = true,
                "shift" => c.shift = true,
                "alt" => c.alt = true,
                _ => return None,
            }
        }
        c.vk = match KEYS.iter().find(|k| k.0.eq_ignore_ascii_case(key)) {
            Some(k) => k.2,
            None => u16::from_str_radix(key.strip_prefix("0x").or_else(|| key.strip_prefix("0X"))?, 16).ok()?,
        };
        c.vk = normalize_vk(c.vk);
        (!is_modifier_key(c.vk) || c == Chord::key(c.vk)).then_some(c)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keymap {
    pub bindings: Vec<Binding>,
}

impl Default for Keymap {
    /// The built-in layout (MS-IME like).
    fn default() -> Self {
        use Action::*;
        let b = |chord, action| Binding { chord, action, when: When::Always };
        let k = Chord::key;
        Self {
            bindings: vec![
                b(k(0x19), ImeToggle),
                // Alt+` (US keyboards), as in Microsoft IME.
                b(Chord { vk: 0xC0, ctrl: false, shift: false, alt: true }, ImeToggle),
                b(k(0x16), ImeOn),
                b(k(0x1C), ImeOn),
                b(k(0xF2), ImeOn),
                b(k(0x1A), ImeOff),
                b(k(0x1D), ImeOff),
                b(k(0xF0), ImeOff),
                b(k(0x20), Key(super::Key::Space)),
                b(Chord::shift(0x20), Key(super::Key::HalfSpace)),
                b(k(0x0D), Key(super::Key::Enter)),
                b(k(0x1B), Key(super::Key::Escape)),
                b(k(0x08), Key(super::Key::Backspace)),
                b(k(0x2E), Key(super::Key::Delete)),
                b(k(0x09), Key(super::Key::Tab)),
                b(k(0x25), Key(super::Key::Left)),
                b(k(0x27), Key(super::Key::Right)),
                b(k(0x26), Key(super::Key::Up)),
                b(k(0x28), Key(super::Key::Down)),
                b(k(0x24), Key(super::Key::Home)),
                b(k(0x23), Key(super::Key::End)),
                b(k(0x21), Key(super::Key::PageUp)),
                b(k(0x22), Key(super::Key::PageDown)),
                b(Chord::shift(0x25), Key(super::Key::ShrinkSegment)),
                b(Chord::shift(0x27), Key(super::Key::ExtendSegment)),
                b(k(0x75), Key(super::Key::ToHiragana)),
                b(k(0x76), Key(super::Key::ToKatakana)),
                b(k(0x77), Key(super::Key::ToHalfKatakana)),
                b(k(0x78), Key(super::Key::ToFullAscii)),
                b(k(0x79), Key(super::Key::ToHalfAscii)),
                // Right after committing: back to the reading.
                Binding {
                    chord: Chord { vk: 0x08, ctrl: true, shift: false, alt: false },
                    action: Key(super::Key::UndoCommit),
                    when: When::Idle,
                },
                // Right after committing: report the conversion.
                Binding {
                    chord: Chord { vk: 0x7B, ctrl: true, shift: false, alt: false },
                    action: Key(super::Key::ReportCommit),
                    when: When::Idle,
                },
            ],
        }
    }
}

impl Keymap {
    /// The action for `chord`, or `None` to type the key's character.
    ///
    /// A binding for the current state (composing / idle) wins over an
    /// `Always` one. A Shift chord of a non-character key with no binding
    /// of its own uses the unshifted binding.
    pub fn lookup(&self, chord: Chord, composing: bool) -> Option<Action> {
        let chord = Chord { vk: normalize_vk(chord.vk), ..chord };
        let find = |c: Chord| {
            let applies = |w: When| match w {
                When::Always => true,
                When::Composing => composing,
                When::Idle => !composing,
            };
            let mut best: Option<&Binding> = None;
            for b in self.bindings.iter().filter(|b| b.chord == c && applies(b.when)) {
                if best.is_none_or(|x| x.when == When::Always && b.when != When::Always) {
                    best = Some(b);
                }
            }
            best.map(|b| b.action)
        };
        find(chord)
            .or_else(|| {
                (chord.shift && !chord.ctrl && !chord.alt && !is_character_key(chord.vk))
                    .then(|| find(Chord { shift: false, ..chord }))
                    .flatten()
            })
            .or_else(|| is_ime_key(chord.vk).then(|| find(Chord::key(chord.vk))).flatten())
    }

    /// Parses the file format; bad lines are skipped. Files written by an
    /// older version get the default bindings added since then.
    pub fn parse(text: &str) -> Self {
        let mut bindings = Vec::new();
        let mut version = 1;
        for line in text.lines() {
            if let Some(v) = line.trim().strip_prefix("#! version") {
                version = v.trim().parse().unwrap_or(1);
                continue;
            }
            let line = line.split('#').next().unwrap_or("").trim();
            let mut f = line.split_whitespace();
            let (Some(chord), Some(action)) = (f.next(), f.next()) else { continue };
            let Some(chord) = Chord::parse(chord) else { continue };
            let Some(action) = ACTIONS.iter().find(|a| a.0 == action).map(|a| a.2) else { continue };
            let when = match f.next() {
                None => When::Always,
                Some(w) => match WHENS.iter().find(|x| x.0 == w) {
                    Some(x) => x.2,
                    None => continue,
                },
            };
            bindings.push(Binding { chord, action, when });
        }
        let mut map = Self { bindings };
        map.upgrade(version);
        map
    }

    /// Adds default bindings introduced after file `version`, unless the
    /// user already uses the chord.
    fn upgrade(&mut self, version: u32) {
        let added = [
            (2, Chord { vk: 0xC0, ctrl: false, shift: false, alt: true }, Action::ImeToggle),
            (3, Chord::shift(0x20), Action::Key(Key::HalfSpace)),
        ];
        for (since, chord, action) in added {
            if version < since && !self.bindings.iter().any(|b| b.chord == chord) {
                self.bindings.push(Binding { chord, action, when: When::Always });
            }
        }
    }

    pub fn to_text(&self) -> String {
        let mut s = format!(
            "#! version {KEYMAP_VERSION}\n# Sweet IME key bindings: <key> <action> [always|composing|idle]\n"
        );
        for b in &self.bindings {
            let action = ACTIONS.iter().find(|a| a.2 == b.action).map_or("?", |a| a.0);
            let mut line = format!("{:<18} {action}", b.chord.to_text());
            if b.when != When::Always {
                let w = WHENS.iter().find(|x| x.2 == b.when).map_or("always", |x| x.0);
                line = format!("{line:<40} {w}");
            }
            s.push_str(line.trim_end());
            s.push('\n');
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut m = Keymap::default();
        m.bindings.push(Binding {
            chord: Chord { vk: 0x20, ctrl: true, shift: false, alt: false },
            action: Action::ImeToggle,
            when: When::Always,
        });
        m.bindings.push(Binding { chord: Chord::key(0x1C), action: Action::Key(Key::Space), when: When::Composing });
        assert_eq!(Keymap::parse(&m.to_text()), m);
    }

    #[test]
    fn lookup_rules() {
        let mut m = Keymap::default();
        m.bindings.push(Binding { chord: Chord::key(0x1C), action: Action::Key(Key::Space), when: When::Composing });
        // State-specific binding wins while composing.
        assert_eq!(m.lookup(Chord::key(0x1C), true), Some(Action::Key(Key::Space)));
        assert_eq!(m.lookup(Chord::key(0x1C), false), Some(Action::ImeOn));
        // 半角/全角 codes are normalized.
        assert_eq!(m.lookup(Chord::key(0xF3), false), Some(Action::ImeToggle));
        assert_eq!(m.lookup(Chord::shift(0x20), true), Some(Action::Key(Key::HalfSpace)));
        assert_eq!(m.lookup(Chord::shift(0x25), true), Some(Action::Key(Key::ShrinkSegment)));
        // Letters are typed, Shift+letter too.
        assert_eq!(m.lookup(Chord::key(0x41), true), None);
        assert_eq!(m.lookup(Chord::shift(0x41), true), None);
        // Alt+` toggles.
        assert_eq!(m.lookup(Chord { alt: true, ..Chord::key(0xC0) }, false), Some(Action::ImeToggle));
        // kbd101: Alt+` arrives as Alt+半角/全角.
        assert_eq!(m.lookup(Chord { alt: true, ..Chord::key(0x19) }, false), Some(Action::ImeToggle));
        assert_eq!(m.lookup(Chord { alt: true, ..Chord::key(0x1D) }, true), Some(Action::ImeOff));
        // Ctrl chords are not bound by default.
        assert_eq!(m.lookup(Chord { ctrl: true, ..Chord::key(0x20) }, false), None);
    }

    #[test]
    fn old_files_get_new_defaults() {
        let alt_grave = Chord { vk: 0xC0, ctrl: false, shift: false, alt: true };
        let old = Keymap::parse("Kanji ime_toggle\n");
        assert!(old.bindings.iter().any(|b| b.chord == alt_grave));
        assert_eq!(old.lookup(Chord::shift(0x20), false), Some(Action::Key(Key::HalfSpace)));
        let custom = Keymap::parse("#! version 2\nShift+Space convert\n");
        assert_eq!(custom.lookup(Chord::shift(0x20), true), Some(Action::Key(Key::Space)));
        // A current file keeps the user's choice (Alt+` removed).
        let mut m = Keymap::default();
        m.bindings.retain(|b| b.chord != alt_grave);
        assert_eq!(Keymap::parse(&m.to_text()), m);
    }

    #[test]
    fn preserved_toggles() {
        let mut map = Keymap::default();
        map.bindings.push(Binding {
            chord: Chord::key(0x1C),
            action: Action::Key(Key::Space),
            when: When::Composing,
        });
        let keys = super::preserved_toggles(&map);
        for vk in [0x19, 0xF3, 0xF4] {
            assert!(keys.contains(&(Chord::key(vk), Action::ImeToggle)));
            assert!(keys.contains(&(Chord { alt: true, ..Chord::key(vk) }, Action::ImeToggle)));
        }
        assert!(keys.contains(&(Chord { alt: true, ..Chord::key(0xC0) }, Action::ImeToggle)));
        assert!(keys.contains(&(Chord::key(0xF0), Action::ImeOff)));
        assert!(!keys.iter().any(|(chord, _)| chord.vk == 0x1C));
    }

    #[test]
    fn parse_skips_bad_lines() {
        let m = Keymap::parse("#! version 3\nCtrl+Space ime_toggle\nBogus ime_on\nSpace nope\nHenkan convert composing # c\n");
        assert_eq!(m.bindings.len(), 2);
        assert_eq!(m.bindings[1].when, When::Composing);
    }
    #[test]
    fn modifier_taps_are_side_specific_and_cancelled_by_chords() {
        let mut tap = super::ModifierTap::default();
        tap.press(0xA5, true);
        tap.press(0xA5, true); // Duplicate test-down or autorepeat.
        assert!(tap.will_release(0xA5));
        assert!(!tap.will_release(0xA4));
        assert!(tap.release(0xA5));
        assert!(!tap.release(0xA5));

        tap.press(0xA5, true);
        tap.press(0x41, false);
        assert!(!tap.release(0xA5));

        tap.press(0xA0, true);
        tap.press(0xA1, true);
        assert!(!tap.release(0xA1));
        assert!(!tap.release(0xA0));
    }

    #[test]
    fn modifier_key_names_round_trip_as_standalone_bindings() {
        let map = super::Keymap::parse("#! version 3\nRightAlt ime_on\nCtrl+LeftShift ime_off\n");
        assert_eq!(
            map.lookup(super::Chord::key(0xA5), false),
            Some(super::Action::ImeOn)
        );
        assert_eq!(
            map.lookup(
                super::Chord {
                    ctrl: true,
                    ..super::Chord::key(0xA0)
                },
                false
            ),
            None
        );
        assert_eq!(Keymap::parse(&map.to_text()), map);
    }
}
