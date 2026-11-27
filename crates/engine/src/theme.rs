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
    /// The Dracula color scheme (draculatheme.com).
    Dracula,
    /// GNOME's Adwaita (GTK) colors; follows light/dark like the default.
    Adwaita,
    /// User-defined colors and background opacity (`user_theme.txt`).
    Custom,
}

/// Colors used to draw one theme.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Colors {
    /// Window background.
    pub bg: Rgb,
    /// Window background opacity (`0.35..=1.0`); built-in themes use `1.0`.
    pub bg_alpha: f32,
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
    /// Candidate row vertical padding in pixels (used by fcitx5 classicui; default 6).
    pub panel_padding: u8,
    /// True when the accent color should be lightened (dark backgrounds).
    pub dark: bool,
}

/// Minimum allowed background opacity so candidate text remains legible.
pub const MIN_BG_ALPHA: f32 = 0.35;
/// Default vertical padding for candidate rows (pixels).
pub const DEFAULT_PANEL_PADDING: u8 = 6;
pub const MIN_PANEL_PADDING: u8 = 2;
pub const MAX_PANEL_PADDING: u8 = 18;

const LIGHT: Colors = Colors {
    bg: Rgb(0xf9, 0xf9, 0xf9),
    bg_alpha: 1.0,
    text: Rgb(0x1a, 0x1a, 0x1a),
    sub: Rgb(0x5f, 0x5f, 0x5f),
    hover: Rgb(0x00, 0x00, 0x00),
    hover_alpha: 0.06,
    accent: None,
    border: Rgb(0xe5, 0xe5, 0xe5),
    badge_border: Rgb(200, 200, 200),
    panel_padding: DEFAULT_PANEL_PADDING,
    dark: false,
};

const DARK: Colors = Colors {
    bg: Rgb(0x2c, 0x2c, 0x2c),
    bg_alpha: 1.0,
    text: Rgb(0xff, 0xff, 0xff),
    sub: Rgb(0xc5, 0xc5, 0xc5),
    hover: Rgb(0xff, 0xff, 0xff),
    hover_alpha: 0.08,
    accent: None,
    border: Rgb(0x40, 0x40, 0x40),
    badge_border: Rgb(90, 90, 90),
    panel_padding: DEFAULT_PANEL_PADDING,
    dark: true,
};

impl Theme {
    /// Built-in themes shown in the theme gallery.
    pub const BUILTINS: [Theme; 9] = [
        Theme::Default,
        Theme::Light,
        Theme::Dark,
        Theme::Sakura,
        Theme::Matcha,
        Theme::Midnight,
        Theme::Lavender,
        Theme::Dracula,
        Theme::Adwaita,
    ];

    pub const ALL: [Theme; 10] = [
        Theme::Default,
        Theme::Light,
        Theme::Dark,
        Theme::Sakura,
        Theme::Matcha,
        Theme::Midnight,
        Theme::Lavender,
        Theme::Dracula,
        Theme::Adwaita,
        Theme::Custom,
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
            Theme::Dracula => "dracula",
            Theme::Adwaita => "adwaita",
            Theme::Custom => "custom",
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
            Theme::Dracula => "Dracula",
            Theme::Adwaita => "GTK (Adwaita)",
            Theme::Custom => "ユーザーテーマ",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|t| t.id().eq_ignore_ascii_case(id.trim()))
    }

    /// Whether the colors follow the system's light/dark setting.
    pub fn follows_system(self) -> bool {
        matches!(self, Theme::Default | Theme::Adwaita)
    }

    /// Colors for this theme; `system_light` is the system app theme and
    /// only matters when [`Theme::follows_system`]. For [`Theme::Custom`],
    /// callers should prefer [`UserTheme::colors`]; this returns the default
    /// user theme colors as a fallback.
    pub fn colors(self, system_light: bool) -> Colors {
        match self {
            Theme::Default if system_light => LIGHT,
            Theme::Default => DARK,
            Theme::Light => LIGHT,
            Theme::Dark => DARK,
            Theme::Sakura => Colors {
                bg: Rgb(0xff, 0xf4, 0xf7),
                bg_alpha: 1.0,
                text: Rgb(0x4a, 0x23, 0x32),
                sub: Rgb(0xa0, 0x6a, 0x7d),
                hover: Rgb(0xe8, 0x5a, 0x8a),
                hover_alpha: 0.14,
                accent: Some(Rgb(0xe8, 0x5a, 0x8a)),
                border: Rgb(0xf5, 0xc6, 0xd6),
                badge_border: Rgb(0xee, 0xa8, 0xc0),
                panel_padding: DEFAULT_PANEL_PADDING,
                dark: false,
            },
            Theme::Matcha => Colors {
                bg: Rgb(0xf4, 0xf7, 0xee),
                bg_alpha: 1.0,
                text: Rgb(0x26, 0x33, 0x1c),
                sub: Rgb(0x6b, 0x7a, 0x5c),
                hover: Rgb(0x5a, 0x8a, 0x3c),
                hover_alpha: 0.13,
                accent: Some(Rgb(0x4f, 0x80, 0x32)),
                border: Rgb(0xd3, 0xdf, 0xc3),
                badge_border: Rgb(0xb4, 0xc9, 0x9c),
                panel_padding: DEFAULT_PANEL_PADDING,
                dark: false,
            },
            Theme::Midnight => Colors {
                bg: Rgb(0x1b, 0x20, 0x33),
                bg_alpha: 1.0,
                text: Rgb(0xe6, 0xea, 0xf5),
                sub: Rgb(0x93, 0x9c, 0xb8),
                hover: Rgb(0x7a, 0xa2, 0xff),
                hover_alpha: 0.16,
                accent: Some(Rgb(0x7a, 0xa2, 0xff)),
                border: Rgb(0x34, 0x3b, 0x55),
                badge_border: Rgb(0x4a, 0x53, 0x72),
                panel_padding: DEFAULT_PANEL_PADDING,
                dark: true,
            },
            Theme::Dracula => Colors {
                bg: Rgb(0x28, 0x2a, 0x36),
                bg_alpha: 1.0,
                text: Rgb(0xf8, 0xf8, 0xf2),
                sub: Rgb(0x8b, 0x96, 0xc4),
                hover: Rgb(0xbd, 0x93, 0xf9),
                hover_alpha: 0.18,
                accent: Some(Rgb(0xff, 0x79, 0xc6)),
                border: Rgb(0x44, 0x47, 0x5a),
                badge_border: Rgb(0x62, 0x72, 0xa4),
                panel_padding: DEFAULT_PANEL_PADDING,
                dark: true,
            },
            // libadwaita's popover colors and blue accent.
            Theme::Adwaita if system_light => Colors {
                bg: Rgb(0xff, 0xff, 0xff),
                bg_alpha: 1.0,
                text: Rgb(0x32, 0x32, 0x36),
                sub: Rgb(0x7f, 0x7f, 0x83),
                hover: Rgb(0x00, 0x00, 0x06),
                hover_alpha: 0.08,
                accent: Some(Rgb(0x35, 0x84, 0xe4)),
                border: Rgb(0xdb, 0xdb, 0xdc),
                badge_border: Rgb(0xc0, 0xc0, 0xc2),
                panel_padding: DEFAULT_PANEL_PADDING,
                dark: false,
            },
            Theme::Adwaita => Colors {
                bg: Rgb(0x36, 0x36, 0x3a),
                bg_alpha: 1.0,
                text: Rgb(0xff, 0xff, 0xff),
                sub: Rgb(0xa8, 0xa8, 0xab),
                hover: Rgb(0xff, 0xff, 0xff),
                hover_alpha: 0.10,
                accent: Some(Rgb(0x78, 0xae, 0xed)),
                border: Rgb(0x4a, 0x4a, 0x4e),
                badge_border: Rgb(0x5a, 0x5a, 0x5e),
                panel_padding: DEFAULT_PANEL_PADDING,
                dark: true,
            },
            Theme::Lavender => Colors {
                bg: Rgb(0xf6, 0xf3, 0xfc),
                bg_alpha: 1.0,
                text: Rgb(0x2e, 0x24, 0x45),
                sub: Rgb(0x7d, 0x70, 0x99),
                hover: Rgb(0x8b, 0x6c, 0xd9),
                hover_alpha: 0.14,
                accent: Some(Rgb(0x7f, 0x5c, 0xd6)),
                border: Rgb(0xdd, 0xd3, 0xf2),
                badge_border: Rgb(0xc4, 0xb5, 0xe8),
                panel_padding: DEFAULT_PANEL_PADDING,
                dark: false,
            },
            Theme::Custom => UserTheme::default().colors(),
        }
    }
}

/// User-customizable theme colors and background opacity, saved in `user_theme.txt`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AcrylicBlur {
    Fine,
    #[default]
    Standard,
    Coarse,
}

impl AcrylicBlur {
    fn id(self) -> &'static str {
        match self {
            Self::Fine => "fine",
            Self::Standard => "standard",
            Self::Coarse => "coarse",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UserTheme {
    pub bg: [u8; 3],
    pub text: [u8; 3],
    pub sub: [u8; 3],
    pub hover: [u8; 3],
    pub hover_alpha: f32,
    pub accent: [u8; 3],
    pub border: [u8; 3],
    pub bg_alpha: f32,
    pub acrylic: bool,
    pub acrylic_blur: AcrylicBlur,
    pub panel_padding: u8,
    pub dark: bool,
}

impl Default for UserTheme {
    fn default() -> Self {
        Self {
            bg: [0x1e, 0x20, 0x28],
            text: [0xf2, 0xf4, 0xf8],
            sub: [0xa6, 0xac, 0xbe],
            hover: [0xff, 0xff, 0xff],
            hover_alpha: 0.12,
            accent: [0x60, 0xa5, 0xfa],
            border: [0x3c, 0x42, 0x54],
            bg_alpha: 0.88,
            acrylic: false,
            acrylic_blur: AcrylicBlur::default(),
            panel_padding: DEFAULT_PANEL_PADDING,
            dark: true,
        }
    }
}

fn is_dark_rgb(c: [u8; 3]) -> bool {
    let lin = |v: u8| {
        let s = v as f32 / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2]) < 0.179
}

impl UserTheme {
    /// Initializes a user theme by copying a built-in theme's colors.
    pub fn from_colors(c: Colors) -> Self {
        let accent = c.accent.unwrap_or(if c.dark {
            Rgb(0x60, 0xcd, 0xff)
        } else {
            Rgb(0x00, 0x5f, 0xb8)
        });
        let bg = [c.bg.0, c.bg.1, c.bg.2];
        Self {
            bg,
            text: [c.text.0, c.text.1, c.text.2],
            sub: [c.sub.0, c.sub.1, c.sub.2],
            hover: [c.hover.0, c.hover.1, c.hover.2],
            hover_alpha: c.hover_alpha.clamp(0.0, 1.0),
            accent: [accent.0, accent.1, accent.2],
            border: [c.border.0, c.border.1, c.border.2],
            bg_alpha: c.bg_alpha.clamp(MIN_BG_ALPHA, 1.0),
            acrylic: false,
            acrylic_blur: AcrylicBlur::default(),
            panel_padding: c.panel_padding.clamp(MIN_PANEL_PADDING, MAX_PANEL_PADDING),
            dark: is_dark_rgb(bg),
        }
    }

    pub fn colors(&self) -> Colors {
        Colors {
            bg: Rgb(self.bg[0], self.bg[1], self.bg[2]),
            bg_alpha: self.bg_alpha.clamp(MIN_BG_ALPHA, 1.0),
            text: Rgb(self.text[0], self.text[1], self.text[2]),
            sub: Rgb(self.sub[0], self.sub[1], self.sub[2]),
            hover: Rgb(self.hover[0], self.hover[1], self.hover[2]),
            hover_alpha: self.hover_alpha.clamp(0.0, 1.0),
            accent: Some(Rgb(self.accent[0], self.accent[1], self.accent[2])),
            border: Rgb(self.border[0], self.border[1], self.border[2]),
            badge_border: Rgb(self.border[0], self.border[1], self.border[2]),
            panel_padding: self.panel_padding.clamp(MIN_PANEL_PADDING, MAX_PANEL_PADDING),
            dark: is_dark_rgb(self.bg),
        }
    }

    pub fn parse(text: &str) -> Self {
        let mut t = Self::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else { continue };
            let k = k.trim();
            // Strip trailing `# comment` without removing a leading `#rrggbb`.
            let v = v.trim();
            let v = v
                .find(|c: char| c.is_whitespace())
                .map_or(v, |i| &v[..i]);
            match k {
                "bg" => if let Some(c) = parse_hex(v) { t.bg = c; },
                "text" => if let Some(c) = parse_hex(v) { t.text = c; },
                "sub" => if let Some(c) = parse_hex(v) { t.sub = c; },
                "hover" => if let Some(c) = parse_hex(v) { t.hover = c; },
                "accent" => if let Some(c) = parse_hex(v) { t.accent = c; },
                "border" => if let Some(c) = parse_hex(v) { t.border = c; },
                "bg_alpha" => {
                    if let Ok(a) = v.parse::<f32>() && a.is_finite() {
                        t.bg_alpha = a.clamp(MIN_BG_ALPHA, 1.0);
                    }
                }
                "hover_alpha" => {
                    if let Ok(a) = v.parse::<f32>() && a.is_finite() {
                        t.hover_alpha = a.clamp(0.0, 1.0);
                    }
                }
                "panel_padding" => {
                    if let Ok(p) = v.parse::<u8>() {
                        t.panel_padding = p.clamp(MIN_PANEL_PADDING, MAX_PANEL_PADDING);
                    }
                }
                "acrylic_blur" => {
                    t.acrylic_blur = match v {
                        "fine" => AcrylicBlur::Fine,
                        "coarse" => AcrylicBlur::Coarse,
                        _ => AcrylicBlur::Standard,
                    };
                }
                "acrylic" => {
                    if let Ok(enabled) = v.parse::<bool>() {
                        t.acrylic = enabled;
                    }
                }
                "dark" => match v {
                    "true" | "1" => t.dark = true,
                    "false" | "0" => t.dark = false,
                    _ => {}
                },
                _ => {}
            }
        }
        t
    }

    pub fn to_text(&self) -> String {
        format!(
            "bg = {}\ntext = {}\nsub = {}\nhover = {}\nhover_alpha = {:.2}\naccent = {}\nborder = {}\nbg_alpha = {:.2}\npanel_padding = {}\ndark = {}\nacrylic = {}\nacrylic_blur = {}\n",
            fmt_hex(self.bg),
            fmt_hex(self.text),
            fmt_hex(self.sub),
            fmt_hex(self.hover),
            self.hover_alpha.clamp(0.0, 1.0),
            fmt_hex(self.accent),
            fmt_hex(self.border),
            self.bg_alpha.clamp(MIN_BG_ALPHA, 1.0),
            self.panel_padding.clamp(MIN_PANEL_PADDING, MAX_PANEL_PADDING),
            self.dark,
            self.acrylic,
            self.acrylic_blur.id(),
        )
    }
}

fn fmt_hex(c: [u8; 3]) -> String {
    format!("{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// A named user-theme preset persisted in `user_themes.txt`.
#[derive(Clone, Debug, PartialEq)]
pub struct SavedUserTheme {
    pub name: String,
    pub theme: UserTheme,
}

pub fn sanitize_theme_name(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '[' | ']' | '\r' | '\n' => ' ',
            _ => c,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

pub fn parse_saved_user_themes(text: &str) -> Vec<SavedUserTheme> {
    let mut out = Vec::new();
    let mut current_name: Option<String> = None;
    let mut buf = String::new();
    let flush = |name: Option<String>, buf: &mut String, out: &mut Vec<SavedUserTheme>| {
        if let Some(name) = name
            && !name.is_empty()
        {
            out.push(SavedUserTheme {
                name,
                theme: UserTheme::parse(buf),
            });
        }
        buf.clear();
    };
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix('[')
            && let Some(name) = rest.strip_suffix(']')
        {
            flush(current_name.take(), &mut buf, &mut out);
            let name = sanitize_theme_name(name);
            if !name.is_empty() {
                current_name = Some(name);
            }
            continue;
        }
        if current_name.is_some() {
            buf.push_str(line);
            buf.push('\n');
        }
    }
    flush(current_name, &mut buf, &mut out);
    out
}

pub fn format_saved_user_themes(themes: &[SavedUserTheme]) -> String {
    let mut out = String::new();
    for entry in themes {
        let name = sanitize_theme_name(&entry.name);
        if name.is_empty() {
            continue;
        }
        out.push('[');
        out.push_str(&name);
        out.push_str("]\n");
        out.push_str(&entry.theme.to_text());
        out.push('\n');
    }
    out
}

fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 || !s.is_ascii() {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some([r, g, b])
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

    #[test]
    fn user_theme_round_trips() {
        let mut ut = UserTheme::from_colors(Theme::Sakura.colors(true));
        ut.bg_alpha = 0.72;
        ut.acrylic = true;
        ut.acrylic_blur = AcrylicBlur::Coarse;
        ut.hover_alpha = 0.25;
        let parsed = UserTheme::parse(&ut.to_text());
        assert_eq!(parsed.bg, ut.bg);
        assert_eq!(parsed.text, ut.text);
        assert_eq!(parsed.sub, ut.sub);
        assert_eq!(parsed.hover, ut.hover);
        assert_eq!(parsed.accent, ut.accent);
        assert_eq!(parsed.border, ut.border);
        assert_eq!(parsed.dark, ut.dark);
        assert!((parsed.bg_alpha - 0.72).abs() < 0.01);
        assert!(parsed.acrylic);
        assert_eq!(parsed.acrylic_blur, AcrylicBlur::Coarse);
        assert_eq!(
            UserTheme::parse("acrylic = true").acrylic_blur,
            AcrylicBlur::Standard
        );
        assert!(!UserTheme::parse("bg_alpha = 0.72").acrylic);
        assert!((parsed.hover_alpha - 0.25).abs() < 0.01);
        // Out-of-range alpha is clamped, and `#rrggbb` with trailing comment parses.
        let clamped = UserTheme::parse("bg = #112233 # comment\nbg_alpha = 0.05\n");
        assert_eq!(clamped.bg, [0x11, 0x22, 0x33]);
        assert_eq!(clamped.bg_alpha, MIN_BG_ALPHA);
        // Dark mode is automatically derived from bg luminance.
        assert!(clamped.colors().dark);
        assert!(!ut.colors().dark);
    }

    #[test]
    fn saved_user_themes_round_trip() {
        let list = vec![
            SavedUserTheme {
                name: "ガラス風".into(),
                theme: UserTheme::default(),
            },
            SavedUserTheme {
                name: "さくら透過".into(),
                theme: UserTheme::from_colors(Theme::Sakura.colors(true)),
            },
        ];
        let parsed = parse_saved_user_themes(&format_saved_user_themes(&list));
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].name, "ガラス風");
        assert_eq!(parsed[1].name, "さくら透過");
        assert_eq!(parsed[1].theme.bg, list[1].theme.bg);
    }
}
