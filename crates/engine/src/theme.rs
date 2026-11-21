//! Color themes for the candidate window and the input mode badge.
//!
//! Platform-independent color data; the text service draws with it and the
//! settings app previews it.

/// An sRGB color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// Where a theme's colors come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Theme {
    /// Follows the Windows light/dark setting and accent color.
    #[default]
    Default,
    Light,
    Dark,
    Sakura,
    Matcha,
    Midnight,
    Lavender,
}

/// Colors used to draw one theme.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Colors {
    /// Window background.
    pub bg: Rgb,
    /// Candidate text.
    pub text: Rgb,
    /// Index numbers and footer.
    pub sub: Rgb,
    /// Selected row fill; drawn with `hover_alpha` over `bg`.
    pub hover: Rgb,
    pub hover_alpha: f32,
    /// Selection bar; `None` uses the Windows accent color.
    pub accent: Option<Rgb>,
    /// Candidate window border.
    pub border: Rgb,
    /// Mode badge border.
    pub badge_border: Rgb,
    /// True when the accent color should be lightened (dark backgrounds).
    pub dark: bool,
}

const LIGHT: Colors = Colors {
    bg: Rgb(0xf9, 0xf9, 0xf9),
    text: Rgb(0x1a, 0x1a, 0x1a),
    sub: Rgb(0x5f, 0x5f, 0x5f),
    hover: Rgb(0x00, 0x00, 0x00),
    hover_alpha: 0.06,
    accent: None,
    border: Rgb(0xe5, 0xe5, 0xe5),
    badge_border: Rgb(200, 200, 200),
    dark: false,
};

const DARK: Colors = Colors {
    bg: Rgb(0x2c, 0x2c, 0x2c),
    text: Rgb(0xff, 0xff, 0xff),
    sub: Rgb(0xc5, 0xc5, 0xc5),
    hover: Rgb(0xff, 0xff, 0xff),
    hover_alpha: 0.08,
    accent: None,
    border: Rgb(0x40, 0x40, 0x40),
    badge_border: Rgb(90, 90, 90),
    dark: true,
};

impl Theme {
    pub const ALL: [Theme; 7] = [
        Theme::Default,
        Theme::Light,
        Theme::Dark,
        Theme::Sakura,
        Theme::Matcha,
        Theme::Midnight,
        Theme::Lavender,
    ];

    /// Name used in `config.txt`.
    pub fn id(self) -> &'static str {
        match self {
            Theme::Default => "default",
            Theme::Light => "light",
            Theme::Dark => "dark",
            Theme::Sakura => "sakura",
            Theme::Matcha => "matcha",
            Theme::Midnight => "midnight",
            Theme::Lavender => "lavender",
        }
    }

    /// Name shown in the settings app.
    pub fn label(self) -> &'static str {
        match self {
            Theme::Default => "デフォルト(システムに合わせる)",
            Theme::Light => "ライト",
            Theme::Dark => "ダーク",
            Theme::Sakura => "さくら",
            Theme::Matcha => "抹茶",
            Theme::Midnight => "ミッドナイト",
            Theme::Lavender => "ラベンダー",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|t| t.id().eq_ignore_ascii_case(id.trim()))
    }

    /// Colors for this theme; `system_light` is the Windows app theme and
    /// only matters for [`Theme::Default`].
    pub fn colors(self, system_light: bool) -> Colors {
        match self {
            Theme::Default if system_light => LIGHT,
            Theme::Default => DARK,
            Theme::Light => LIGHT,
            Theme::Dark => DARK,
            Theme::Sakura => Colors {
                bg: Rgb(0xff, 0xf4, 0xf7),
                text: Rgb(0x4a, 0x23, 0x32),
                sub: Rgb(0xa0, 0x6a, 0x7d),
                hover: Rgb(0xe8, 0x5a, 0x8a),
                hover_alpha: 0.14,
                accent: Some(Rgb(0xe8, 0x5a, 0x8a)),
                border: Rgb(0xf5, 0xc6, 0xd6),
                badge_border: Rgb(0xee, 0xa8, 0xc0),
                dark: false,
            },
            Theme::Matcha => Colors {
                bg: Rgb(0xf4, 0xf7, 0xee),
                text: Rgb(0x26, 0x33, 0x1c),
                sub: Rgb(0x6b, 0x7a, 0x5c),
                hover: Rgb(0x5a, 0x8a, 0x3c),
                hover_alpha: 0.13,
                accent: Some(Rgb(0x4f, 0x80, 0x32)),
                border: Rgb(0xd3, 0xdf, 0xc3),
                badge_border: Rgb(0xb4, 0xc9, 0x9c),
                dark: false,
            },
            Theme::Midnight => Colors {
                bg: Rgb(0x1b, 0x20, 0x33),
                text: Rgb(0xe6, 0xea, 0xf5),
                sub: Rgb(0x93, 0x9c, 0xb8),
                hover: Rgb(0x7a, 0xa2, 0xff),
                hover_alpha: 0.16,
                accent: Some(Rgb(0x7a, 0xa2, 0xff)),
                border: Rgb(0x34, 0x3b, 0x55),
                badge_border: Rgb(0x4a, 0x53, 0x72),
                dark: true,
            },
            Theme::Lavender => Colors {
                bg: Rgb(0xf6, 0xf3, 0xfc),
                text: Rgb(0x2e, 0x24, 0x45),
                sub: Rgb(0x7d, 0x70, 0x99),
                hover: Rgb(0x8b, 0x6c, 0xd9),
                hover_alpha: 0.14,
                accent: Some(Rgb(0x7f, 0x5c, 0xd6)),
                border: Rgb(0xdd, 0xd3, 0xf2),
                badge_border: Rgb(0xc4, 0xb5, 0xe8),
                dark: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        for t in Theme::ALL {
            assert_eq!(Theme::from_id(t.id()), Some(t));
        }
        assert_eq!(Theme::from_id(" Sakura "), Some(Theme::Sakura));
        assert_eq!(Theme::from_id("nope"), None);
    }

    #[test]
    fn default_follows_system() {
        assert_eq!(Theme::Default.colors(true), Theme::Light.colors(true));
        assert_eq!(Theme::Default.colors(false), Theme::Dark.colors(true));
        assert!(Theme::Default.colors(true).accent.is_none());
    }
}
