//! Input settings and their configuration-file format.

/// Default settle delay before running background neural live refinement (0 = off).
pub const DEFAULT_LIVE_DEBOUNCE_MS: u32 = 0;
/// Upper bound accepted for `live_debounce_ms`.
pub const MAX_LIVE_DEBOUNCE_MS: u32 = 500;

/// User-facing behaviour switches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// Show the converted text while typing; Enter commits it.
    pub live_conversion: bool,
    /// Wait time in milliseconds before running background live refinement (0 disables debounce).
    pub live_debounce_ms: u32,
    /// Show predictions while typing.
    pub suggestions: bool,
    /// Insert an ASCII space when Space is pressed with no active reading.
    pub halfwidth_space: bool,
    /// Keep showing predictive suggestions even on 1-char readings or when
    /// dictionary completions are sparse.
    pub always_show_suggestions: bool,
    /// Add Simeji kaomoji candidates to desktop Space conversions.
    pub simeji_cloud: bool,
    /// Show the original input alongside the Windows composition.
    pub show_typed_input: bool,
    /// Rerank conversions with the neural model when it is installed.
    pub neural: bool,
    /// Learn from chosen candidates and prefer them later.
    pub learning: bool,
    /// Show "あ" / "A" near the caret when the input mode changes.
    pub mode_indicator: bool,
    /// Colors of the candidate window and the mode badge.
    pub theme: crate::Theme,
    /// Fade the candidate window in and out when appearing/disappearing.
    pub window_fade: bool,
    /// Animate a rainbow border on the Windows candidate window.
    pub gaming_mode: bool,
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
    /// Recognize common English words typed as their spelling, also inside
    /// a sentence, and learn words fixed with F10.
    pub english_words: bool,
    /// Show such a word as its katakana loanword first (best -> ベスト),
    /// with the spelling as the next candidate.
    pub english_katakana: bool,
    /// A word started with Shift ends by itself once Japanese follows it
    /// ("Googlehakensaku" -> Google, then はけんさく).
    pub shift_one_word: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            live_conversion: true,
            live_debounce_ms: DEFAULT_LIVE_DEBOUNCE_MS,
            suggestions: true,
            always_show_suggestions: false,
            // Android's Japanese keyboard already uses ASCII space by default.
            halfwidth_space: cfg!(target_os = "android"),
            simeji_cloud: false,
            show_typed_input: false,
            neural: true,
            learning: true,
            mode_indicator: true,
            theme: crate::Theme::Default,
            window_fade: true,
            gaming_mode: false,
            update_check: true,
            dates: true,
            emoji: true,
            calculator: true,
            undo_commit: true,
            private_fields: true,
            english_words: true,
            english_katakana: true,
            shift_one_word: true,
        }
    }
}

impl Options {
    /// Maximum supported value for [`Options::live_debounce_ms`].
    pub const MAX_LIVE_DEBOUNCE_MS: u32 = MAX_LIVE_DEBOUNCE_MS;

    /// Parses `key = value` lines (`#` comments). Unknown keys are ignored.
    pub fn parse(text: &str) -> Self {
        let mut o = Self::default();
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let on = matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "on" | "yes"
            );
            match k.trim() {
                "live_conversion" => o.live_conversion = on,
                "live_debounce_ms" => {
                    if let Ok(ms) = v.trim().parse::<u32>() {
                        o.live_debounce_ms = ms.min(MAX_LIVE_DEBOUNCE_MS);
                    }
                }
                "suggestions" => o.suggestions = on,
                "halfwidth_space" => o.halfwidth_space = on,
                "always_show_suggestions" => o.always_show_suggestions = on,
                "simeji_cloud" => o.simeji_cloud = on,
                "show_typed_input" => o.show_typed_input = on,
                "neural" => o.neural = on,
                "learning" => o.learning = on,
                "mode_indicator" => o.mode_indicator = on,
                "theme" => o.theme = crate::Theme::from_id(v).unwrap_or_default(),
                "window_fade" => o.window_fade = on,
                "gaming_mode" => o.gaming_mode = on,
                "update_check" => o.update_check = on,
                "dates" => o.dates = on,
                "emoji" => o.emoji = on,
                "calculator" => o.calculator = on,
                "undo_commit" => o.undo_commit = on,
                "private_fields" => o.private_fields = on,
                "english_words" => o.english_words = on,
                "english_katakana" => o.english_katakana = on,
                "shift_one_word" => o.shift_one_word = on,
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
             live_debounce_ms = {}
             suggestions = {}
             halfwidth_space = {}
             always_show_suggestions = {}
             simeji_cloud = {}
             show_typed_input = {}
             neural = {}
             learning = {}
             mode_indicator = {}
             theme = {}
             window_fade = {}
             gaming_mode = {}
             update_check = {}
             dates = {}
             emoji = {}
             calculator = {}
             undo_commit = {}
             private_fields = {}
             english_words = {}
             english_katakana = {}
             shift_one_word = {}
",
            b(self.live_conversion),
            self.live_debounce_ms.min(MAX_LIVE_DEBOUNCE_MS),
            b(self.suggestions),
            b(self.halfwidth_space),
            b(self.always_show_suggestions),
            b(self.simeji_cloud),
            b(self.show_typed_input),
            b(self.neural),
            b(self.learning),
            b(self.mode_indicator),
            self.theme.id(),
            b(self.window_fade),
            b(self.gaming_mode),
            b(self.update_check),
            b(self.dates),
            b(self.emoji),
            b(self.calculator),
            b(self.undo_commit),
            b(self.private_fields),
            b(self.english_words),
            b(self.english_katakana),
            b(self.shift_one_word),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Options;

    #[test]
    fn parse_options() {
        let o = Options::parse(
            "# comment\nlive_conversion = off\n suggestions=1 # trailing\nunknown = 3\n",
        );
        assert_eq!(
            o,
            Options {
                live_conversion: false,
                ..Options::default()
            }
        );
        assert_eq!(Options::parse(""), Options::default());
        let o = Options {
            live_conversion: false,
            live_debounce_ms: 140,
            suggestions: true,
            always_show_suggestions: true,
            halfwidth_space: true,
            simeji_cloud: true,
            show_typed_input: true,
            neural: false,
            learning: true,
            mode_indicator: false,
            theme: crate::Theme::Sakura,
            window_fade: false,
            gaming_mode: true,
            update_check: false,
            dates: false,
            emoji: true,
            calculator: false,
            undo_commit: false,
            private_fields: false,
            english_words: true,
            english_katakana: false,
            shift_one_word: false,
        };
        assert_eq!(Options::parse(&o.to_text()), o);
        assert_eq!(Options::parse("live_debounce_ms = 0").live_debounce_ms, 0);
        assert_eq!(
            Options::parse("live_debounce_ms = 9999").live_debounce_ms,
            super::MAX_LIVE_DEBOUNCE_MS
        );
        assert_eq!(
            Options::parse("live_debounce_ms = invalid").live_debounce_ms,
            super::DEFAULT_LIVE_DEBOUNCE_MS
        );
        assert_eq!(
            Options::parse("theme = midnight").theme,
            crate::Theme::Midnight
        );
        assert_eq!(
            Options::parse("theme = unknown").theme,
            crate::Theme::Default
        );
    }
}
