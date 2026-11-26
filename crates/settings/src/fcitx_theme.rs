//! fcitx5 draws the candidate window on Linux, styled by its classicui
//! theme. This writes the Sweet IME color themes as classicui themes and
//! selects them in `classicui.conf`.

#![cfg_attr(windows, allow(dead_code))]

use std::path::{Path, PathBuf};
use sweet_engine::{Theme, UserTheme};
use sweet_engine::theme::{Colors, Rgb};

/// One classicui theme to write.
pub struct Spec {
    pub name: String,
    pub colors: Colors,
    /// Shaped like a GTK popover (Adwaita): larger corners, a drop shadow
    /// and a plain selected row.
    pub gtk: bool,
}

/// The classicui theme name of a theme. A theme that follows the system is
/// a light and a dark one; the default theme's are `sweet-ime-light` and
/// `sweet-ime-dark`.
pub fn theme_name(theme: Theme, system_light: bool) -> String {
    match theme {
        Theme::Default if system_light => "sweet-ime-light".into(),
        Theme::Default => "sweet-ime-dark".into(),
        t if t.follows_system() && !system_light => format!("sweet-ime-{}-dark", t.id()),
        t => format!("sweet-ime-{}", t.id()),
    }
}

/// The themes `theme` uses: both variants of one that follows the system.
fn themes_for(theme: Theme, user_theme: &UserTheme) -> Vec<Spec> {
    if theme == Theme::Custom {
        return vec![Spec {
            name: theme_name(Theme::Custom, true),
            colors: user_theme.colors(),
            gtk: false,
        }];
    }
    let variants: &[bool] = if theme.follows_system() { &[true, false] } else { &[true] };
    variants
        .iter()
        .map(|&light| Spec { name: theme_name(theme, light), colors: theme.colors(light), gtk: theme == Theme::Adwaita })
        .collect()
}

/// Every built-in classicui theme: the package installs them all. The default
/// theme's variants are the light and dark themes.
pub fn all_themes() -> Vec<Spec> {
    let default_user = UserTheme::default();
    Theme::BUILTINS
        .into_iter()
        .filter(|&t| t != Theme::Default)
        .flat_map(|t| themes_for(t, &default_user))
        .collect()
}

/// Selection bar color when the theme follows the system accent (as the
/// settings preview draws it).
fn accent(c: &Colors) -> Rgb {
    c.accent.unwrap_or(if c.dark { Rgb(0x60, 0xcd, 0xff) } else { Rgb(0x00, 0x5f, 0xb8) })
}

fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

/// `hex` with an alpha byte, as classicui reads it (#rrggbbaa).
fn hexa(c: Rgb, alpha: f32) -> String {
    format!("{}{:02x}", hex(c), (alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// The files of one classicui theme, as (file name, contents).
pub fn theme_files(spec: &Spec) -> Vec<(&'static str, String)> {
    let (c, name) = (&spec.colors, spec.name.as_str());
    let (bg, border, text, sub) = (hex(c.bg), hex(c.border), hex(c.text), hex(c.sub));
    let hover = hex(c.hover);
    let bar = hex(accent(c));
    // 9-slice images: the corners (Margin) keep their size, the rest stretches.
    // The GTK shape draws a shadow in a transparent border (ShadowMargin).
    let (shadow, panel_margin, row_radius) = if spec.gtk { (12, 26, 6) } else { (0, 8, 4) };
    let bg_alpha = c.bg_alpha.clamp(sweet_engine::theme::MIN_BG_ALPHA, 1.0);
    let enable_blur = if bg_alpha < 0.99 { "True" } else { "False" };
    let panel = if spec.gtk {
        let opacity = if c.dark { 0.5 } else { 0.22 };
        format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="80"><defs><filter id="s" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="4"/></filter></defs><rect x="12" y="14" width="56" height="56" rx="12" fill="#000000" fill-opacity="{opacity}" filter="url(#s)"/><rect x="12.5" y="12.5" width="55" height="55" rx="11.5" fill="{bg}" fill-opacity="{bg_alpha:.2}" stroke="{border}"/></svg>"##
        )
    } else {
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect x="0.5" y="0.5" width="31" height="31" rx="7.5" fill="{bg}" fill-opacity="{bg_alpha:.2}" stroke="{border}"/></svg>"#
        )
    };
    let menu_highlight = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect width="32" height="32" rx="{row_radius}" fill="{hover}" fill-opacity="{:.2}"/></svg>"#,
        c.hover_alpha
    );
    // A tinted pill; except in the GTK shape, with a short accent bar at the
    // left, like Windows.
    let highlight = if spec.gtk {
        menu_highlight.clone()
    } else {
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect width="32" height="32" rx="4" fill="{hover}" fill-opacity="{:.2}"/><rect x="0" y="9" width="3" height="14" rx="1.5" fill="{bar}"/></svg>"#,
            c.hover_alpha
        )
    };
    let arrow = |points: &str| {
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><polyline points="{points}" fill="none" stroke="{sub}" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/></svg>"#
        )
    };
    let line = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="1"><rect width="8" height="1" fill="{border}"/></svg>"#);
    let check = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><circle cx="8" cy="8" r="3.5" fill="{bar}"/></svg>"#
    );
    let margin = |section: &str, l: u32, r: u32, t: u32, b: u32| {
        format!("[{section}]\nLeft={l}\nRight={r}\nTop={t}\nBottom={b}\n\n")
    };
    let mut conf = format!(
        "# Written by the Sweet IME settings app; changes are overwritten.\n\
         [Metadata]\nName=Sweet IME ({name})\nVersion=1\nAuthor=Sweet IME\n\
         Description=Sweet IME candidate window colors\nScaleWithDPI=True\n\n\
         [InputPanel]\nNormalColor={text}\nHighlightCandidateColor={text}\n\
         HighlightColor={text}\nHighlightBackgroundColor={}\nSpacing=0\n\
         EnableBlur={enable_blur}\nFullWidthHighlight=True\n\n\
         [InputPanel/Background]\nImage=panel.svg\n\n",
        hexa(accent(c), 0.25)
    );
    let pad_v = u32::from(
        c.panel_padding
            .clamp(sweet_engine::theme::MIN_PANEL_PADDING, sweet_engine::theme::MAX_PANEL_PADDING),
    );
    let pad_h = pad_v + 4;
    let (m, content) = (panel_margin, shadow + pad_v.saturating_sub(1).max(2));
    conf += &margin("InputPanel/Background/Margin", m, m, m, m);
    conf += &margin("InputPanel/ContentMargin", content, content, content, content);
    if shadow > 0 {
        conf += &margin("InputPanel/ShadowMargin", shadow, shadow, shadow, shadow);
    }
    conf += &margin("InputPanel/TextMargin", pad_h, pad_h, pad_v, pad_v);
    conf += "[InputPanel/Highlight]\nImage=highlight.svg\n\n";
    conf += &margin("InputPanel/Highlight/Margin", 8, 8, 8, 8);
    conf += "[InputPanel/PrevPage]\nImage=prev.svg\n\n";
    conf += &margin("InputPanel/PrevPage/ClickMargin", 4, 4, 4, 4);
    conf += "[InputPanel/NextPage]\nImage=next.svg\n\n";
    conf += &margin("InputPanel/NextPage/ClickMargin", 4, 4, 4, 4);
    conf += &format!("[Menu]\nNormalColor={text}\nHighlightCandidateColor={text}\nSpacing=0\n\n");
    conf += "[Menu/Background]\nImage=panel.svg\n\n";
    conf += &margin("Menu/Background/Margin", m, m, m, m);
    conf += &margin("Menu/ContentMargin", content, content, content, content);
    if shadow > 0 {
        conf += &margin("Menu/ShadowMargin", shadow, shadow, shadow, shadow);
    }
    conf += "[Menu/Highlight]\nImage=menu-highlight.svg\n\n";
    conf += &margin("Menu/Highlight/Margin", 8, 8, 8, 8);
    conf += "[Menu/Separator]\nImage=line.svg\n\n";
    conf += "[Menu/CheckBox]\nImage=check.svg\n\n";
    conf += "[Menu/SubMenu]\nImage=next.svg\n\n";
    conf += &margin("Menu/TextMargin", 8, 8, 5, 5);
    vec![
        ("theme.conf", conf),
        ("panel.svg", panel),
        ("highlight.svg", highlight),
        ("menu-highlight.svg", menu_highlight),
        ("prev.svg", arrow("10,3 5,8 10,13")),
        ("next.svg", arrow("6,3 11,8 6,13")),
        ("line.svg", line),
        ("check.svg", check),
    ]
}

/// `classicui.conf` with the Sweet IME themes selected and every other line
/// kept. The default theme follows the desktop's light/dark setting.
pub fn select_in_classicui(conf: &str, theme: Theme) -> String {
    let (light, dark) = (theme_name(theme, true), theme_name(theme, false));
    let mut values = vec![("Theme", light.as_str()), ("DarkTheme", dark.as_str())];
    if theme.follows_system() {
        values.push(("UseDarkTheme", "True"));
    }
    set_keys(conf, values)
}

/// Sets `key=value` lines, keeping every other line.
fn set_keys(conf: &str, values: Vec<(&str, &str)>) -> String {
    let mut values: Vec<_> = values
        .into_iter()
        .map(|(key, value)| (key, Some(value)))
        .collect();
    let mut out = String::new();
    let mut lines = conf.lines();
    let mut section = None;
    for line in lines.by_ref() {
        if is_section(line) {
            section = Some(line);
            break;
        }
        let key = line.split('=').next().unwrap_or("").trim();
        if let Some((key, value)) = values.iter_mut().find(|(k, _)| *k == key) {
            if let Some(value) = value.take() {
                out += &format!("{key}={value}\n");
            }
        } else {
            out += line;
            out.push('\n');
        }
    }
    for (k, v) in values
        .into_iter()
        .filter_map(|(key, value)| Some((key, value?)))
    {
        out += &format!("{k}={v}\n");
    }
    for line in section.into_iter().chain(lines) {
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn is_section(line: &str) -> bool {
    let line = line.trim();
    line.starts_with('[') && line.ends_with(']')
}

fn font_parts(conf: &str) -> (&str, &str) {
    let desc = conf
        .lines()
        .take_while(|line| !is_section(line))
        .filter_map(|line| line.split_once('='))
        .filter_map(|(key, value)| (key.trim() == "Font").then_some(value.trim()))
        .last()
        .unwrap_or("Sans 10");
    let desc = desc
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(desc);
    let core = desc.split_once(" @").map_or(desc, |(core, _)| core);
    let core = core.split_once(" #").map_or(core, |(core, _)| core);
    if let Some((family, size)) = core.rsplit_once(char::is_whitespace)
        && size
            .strip_suffix("px")
            .unwrap_or(size)
            .parse::<f32>()
            .is_ok_and(|n| n.is_finite() && n > 0.0)
    {
        return (family.trim_end().trim_end_matches(','), size);
    }
    (core, "10")
}

#[cfg(not(windows))]
pub fn candidate_font() -> String {
    let conf = xdg("XDG_CONFIG_HOME", ".config")
        .and_then(|dir| std::fs::read_to_string(dir.join("fcitx5/conf/classicui.conf")).ok())
        .unwrap_or_default();
    let (font, _) = font_parts(&conf);
    if font == "Sans" {
        String::new()
    } else {
        font.replace("\\\"", "\"").replace("\\\\", "\\")
    }
}

fn select_font(conf: &str, family: &str) -> String {
    let (_, size) = font_parts(conf);
    let family = if family.is_empty() { "Sans" } else { family }
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    // A trailing comma separates the family from Pango's style keywords.
    let value = format!("\"{family}, {size}\"");
    set_keys(conf, vec![("Font", &value)])
}

fn raw_font_desc(conf: &str) -> &str {
    let desc = conf
        .lines()
        .take_while(|line| !is_section(line))
        .filter_map(|line| line.split_once('='))
        .filter_map(|(key, value)| (key.trim() == "Font").then_some(value.trim()))
        .last()
        .unwrap_or("Sans 10");
    desc.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(desc)
}

fn font_suffix(desc: &str) -> &str {
    let idx = [desc.find(" @"), desc.find(" #")]
        .into_iter()
        .flatten()
        .min();
    idx.map_or("", |i| &desc[i..])
}

fn select_font_size(conf: &str, size: u32) -> String {
    let desc = raw_font_desc(conf);
    let suffix = font_suffix(desc);
    let core = &desc[..desc.len() - suffix.len()];
    let size = size.clamp(8, 24);
    let replaced = if let Some((prefix, old_size)) = core.rsplit_once(char::is_whitespace)
        && old_size
            .strip_suffix("px")
            .unwrap_or(old_size)
            .parse::<f32>()
            .is_ok_and(|n| n.is_finite() && n > 0.0)
    {
        let unit = if old_size.ends_with("px") { "px" } else { "" };
        let prefix = prefix.trim_end();
        format!("{prefix} {size}{unit}{suffix}")
    } else if core.trim().is_empty() {
        format!("Sans {size}{suffix}")
    } else {
        format!("{} {size}{suffix}", core.trim_end())
    };
    let value = format!("\"{replaced}\"");
    set_keys(conf, vec![("Font", &value)])
}

fn parse_font_size(conf: &str) -> (u32, bool) {
    let (_, size) = font_parts(conf);
    let is_px = size.ends_with("px");
    let n = size
        .strip_suffix("px")
        .unwrap_or(size)
        .parse::<f32>()
        .ok()
        .filter(|n| n.is_finite())
        .map(|n| n.round().clamp(8.0, 24.0) as u32)
        .unwrap_or(10);
    (n, is_px)
}

fn parse_vertical(conf: &str) -> bool {
    conf.lines()
        .take_while(|line| !is_section(line))
        .filter_map(|line| line.split_once('='))
        .filter_map(|(k, v)| (k.trim() == "Vertical Candidate List").then_some(v.trim()))
        .last()
        .is_some_and(|v| v.eq_ignore_ascii_case("true"))
}

fn select_vertical(conf: &str, vertical: bool) -> String {
    set_keys(
        conf,
        vec![("Vertical Candidate List", if vertical { "True" } else { "False" })],
    )
}

#[cfg(not(windows))]
pub fn candidate_font_size() -> (u32, bool) {
    let conf = xdg("XDG_CONFIG_HOME", ".config")
        .and_then(|dir| std::fs::read_to_string(dir.join("fcitx5/conf/classicui.conf")).ok())
        .unwrap_or_default();
    parse_font_size(&conf)
}

#[cfg(not(windows))]
pub fn apply_candidate_font_size(size: u32) -> Result<(), String> {
    let config = xdg("XDG_CONFIG_HOME", ".config").ok_or("ホームフォルダが見つかりません")?;
    let size = size.clamp(8, 24);
    write_classicui(
        &config,
        |conf| select_font_size(conf, size),
        &format!("Font size={size}"),
    )
}

#[cfg(not(windows))]
pub fn vertical_candidate_list() -> bool {
    let conf = xdg("XDG_CONFIG_HOME", ".config")
        .and_then(|dir| std::fs::read_to_string(dir.join("fcitx5/conf/classicui.conf")).ok())
        .unwrap_or_default();
    parse_vertical(&conf)
}

#[cfg(not(windows))]
pub fn apply_vertical_candidate_list(vertical: bool) -> Result<(), String> {
    let config = xdg("XDG_CONFIG_HOME", ".config").ok_or("ホームフォルダが見つかりません")?;
    write_classicui(
        &config,
        |conf| select_vertical(conf, vertical),
        if vertical {
            "Vertical Candidate List=True"
        } else {
            "Vertical Candidate List=False"
        },
    )
}

#[cfg(not(windows))]
pub fn apply_candidate_font(family: &str) -> Result<(), String> {
    let config = xdg("XDG_CONFIG_HOME", ".config").ok_or("ホームフォルダが見つかりません")?;
    let hint = if family.is_empty() { "Sans" } else { family };
    write_classicui(
        &config,
        |conf| select_font(conf, family),
        &format!("Font={hint}"),
    )
}

fn xdg(var: &str, fallback: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(fallback)))
}

/// Writes `themes` as theme directories under `dir`.
pub fn write_themes(dir: &Path, themes: &[Spec]) -> std::io::Result<()> {
    for spec in themes {
        write_theme(&dir.join(&spec.name), spec)?;
    }
    Ok(())
}

fn write_theme(dir: &Path, spec: &Spec) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for (file, contents) in theme_files(spec) {
        std::fs::write(dir.join(file), contents)?;
    }
    Ok(())
}

/// Writes the themes, selects them, and asks fcitx5 to reload classicui.
pub fn apply(theme: Theme, user_theme: &UserTheme) -> Result<(), String> {
    let data = xdg("XDG_DATA_HOME", ".local/share").ok_or("ホームフォルダが見つかりません")?;
    let config = xdg("XDG_CONFIG_HOME", ".config").ok_or("ホームフォルダが見つかりません")?;
    write_themes(&data.join("fcitx5/themes"), &themes_for(theme, user_theme))
        .map_err(|e| format!("テーマを書き込めませんでした: {e}"))?;
    let (light, dark) = (theme_name(theme, true), theme_name(theme, false));
    let hint = if light == dark { format!("Theme={light}") } else { format!("Theme={light}, DarkTheme={dark}") };
    write_classicui(&config, |old| select_in_classicui(old, theme), &hint)
}

/// Selects fcitx5's own themes again.
pub fn restore() -> Result<(), String> {
    let config = xdg("XDG_CONFIG_HOME", ".config").ok_or("ホームフォルダが見つかりません")?;
    write_classicui(&config, |old| set_keys(old, vec![("Theme", "default"), ("DarkTheme", "default-dark")]), "Theme=default")
}

fn write_classicui(config: &Path, edit: impl FnOnce(&str) -> String, hint: &str) -> Result<(), String> {
    let path = config.join("fcitx5/conf/classicui.conf");
    let old = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("{} を読み込めませんでした: {e}", path.display())),
    };
    let new = edit(&old);
    if new != old {
        // Home Manager links a managed file into the Nix store.
        if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(format!(
                "{} は別のツール (Home Manager など) が管理しています。そちらで {hint} を設定してください。",
                path.display()
            ));
        }
        std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))
            .and_then(|()| std::fs::write(&path, new))
            .map_err(|e| format!("{} に書き込めませんでした: {e}", path.display()))?;
    }
    reload();
    Ok(())
}

/// Asks the running fcitx5 to reload the active UI addon (classicui / sweetui).
fn reload() {
    use std::process::{Command, Stdio};
    let reload_addon = |addon: &str| {
        Command::new("dbus-send")
            .args([
                "--session",
                "--print-reply",
                "--dest=org.fcitx.Fcitx5",
                "/controller",
                "org.fcitx.Fcitx.Controller1.ReloadAddonConfig",
                &format!("string:{addon}"),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    };
    let active = Command::new("dbus-send")
        .args([
            "--session",
            "--print-reply=literal",
            "--dest=org.fcitx.Fcitx5",
            "/controller",
            "org.fcitx.Fcitx.Controller1.CurrentUI",
        ])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());
    match active.as_deref() {
        Some("kimpanel") => {
            let _ = reload_addon("kimpanel");
            let _ = reload_addon("classicui");
        }
        Some(ui) => {
            let _ = reload_addon(ui);
        }
        None => {
            let _ = reload_addon("classicui");
            let _ = reload_addon("sweetui");
        }
    }
}

/// Reloads SweetUI's configuration without touching `classicui.conf`.
#[cfg(not(windows))]
pub fn reload_sweetui() {
    use std::process::{Command, Stdio};
    let _ = Command::new("dbus-send")
        .args([
            "--session",
            "--print-reply",
            "--dest=org.fcitx.Fcitx5",
            "/controller",
            "org.fcitx.Fcitx.Controller1.ReloadAddonConfig",
            "string:sweetui",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_uses_the_theme_colors() {
        let ut = UserTheme::default();
        let files = theme_files(&themes_for(Theme::Sakura, &ut)[0]);
        let conf = &files[0].1;
        assert!(conf.contains("NormalColor=#4a2332"), "{conf}");
        assert!(files.iter().any(|(f, s)| *f == "panel.svg" && s.contains("#fff4f7") && s.contains("#f5c6d6")));
        assert!(files.iter().any(|(f, s)| *f == "highlight.svg" && s.contains("#e85a8a")));
        // Every image the theme names is written.
        for image in conf.lines().filter_map(|l| l.strip_prefix("Image=")) {
            assert!(files.iter().any(|(f, _)| *f == image), "{image}");
        }
        // Translucent user theme enables blur and writes fill-opacity.
        let custom_files = theme_files(&themes_for(Theme::Custom, &ut)[0]);
        assert!(custom_files[0].1.contains("EnableBlur=True"));
        assert!(custom_files.iter().any(|(f, s)| *f == "panel.svg" && s.contains("fill-opacity=\"0.88\"")));
    }

    #[test]
    fn package_has_every_theme_the_app_selects() {
        let names: Vec<String> = all_themes().into_iter().map(|s| s.name).collect();
        for t in Theme::BUILTINS {
            for light in [true, false] {
                assert!(names.contains(&theme_name(t, light)), "{}", theme_name(t, light));
            }
        }
    }

    #[test]
    fn classicui_keeps_other_settings() {
        let old = "Vertical Candidate List=True\nTheme=default\nFont=\"Sans 12\"\n";
        let new = select_in_classicui(old, Theme::Matcha);
        assert_eq!(new, "Vertical Candidate List=True\nTheme=sweet-ime-matcha\nFont=\"Sans 12\"\nDarkTheme=sweet-ime-matcha\n");
        let new = select_in_classicui("", Theme::Default);
        assert_eq!(new, "Theme=sweet-ime-light\nDarkTheme=sweet-ime-dark\nUseDarkTheme=True\n");
        // Applying twice changes nothing.
        assert_eq!(select_in_classicui(&new, Theme::Default), new);
    }

    #[test]
    fn classicui_font_preserves_size_and_other_sections() {
        let old =
            "Theme=default\nFont=\"Sans Bold 13px\"\nMenuFont=\"Sans 10\"\n\n[Other]\nFont=Other\n";
        let new = select_font(old, "Noto Serif CJK JP");
        assert_eq!(
            new,
            "Theme=default\nFont=\"Noto Serif CJK JP, 13px\"\nMenuFont=\"Sans 10\"\n\n[Other]\nFont=Other\n"
        );
        assert_eq!(font_parts(&new), ("Noto Serif CJK JP", "13px"));
        assert_eq!(font_parts(&select_font(&new, "")), ("Sans", "13px"));
        assert_eq!(
            select_font("[Other]\nFont=Other\n", "Serif"),
            "Font=\"Serif, 10\"\n[Other]\nFont=Other\n"
        );
        assert_eq!(
            select_font("Font=Sans 12\nFont=Sans 14\n", "Serif"),
            "Font=\"Serif, 14\"\n"
        );
        let sized = select_font_size(&new, 16);
        assert_eq!(parse_font_size(&sized), (16, true));
        assert!(sized.contains("Font=\"Noto Serif CJK JP, 16px\""));
        let bold = select_font_size("Font=\"Sans Bold 13px\"\n", 16);
        assert_eq!(bold, "Font=\"Sans Bold 16px\"\n");
        let varied = select_font_size("Font=\"Cantarell 12 @wght=700\"\n", 14);
        assert_eq!(varied, "Font=\"Cantarell 14 @wght=700\"\n");
        assert_eq!(parse_font_size(&varied), (14, false));
        let vert = select_vertical(&sized, true);
        assert!(parse_vertical(&vert));
        assert!(!parse_vertical(&select_vertical(&vert, false)));
    }
}
