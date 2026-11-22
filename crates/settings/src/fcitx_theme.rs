//! fcitx5 draws the candidate window on Linux, styled by its classicui
//! theme. This writes the Sweet IME color themes as classicui themes and
//! selects them in `classicui.conf`.

#![cfg_attr(windows, allow(dead_code))]

use std::path::{Path, PathBuf};
use sweet_engine::Theme;
use sweet_engine::theme::{Colors, Rgb};

/// Theme directory names under `$XDG_DATA_HOME/fcitx5/themes`.
const LIGHT_NAME: &str = "sweet-ime";
const DARK_NAME: &str = "sweet-ime-dark";

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
pub fn theme_files(c: &Colors, name: &str) -> Vec<(&'static str, String)> {
    let (bg, border, text, sub) = (hex(c.bg), hex(c.border), hex(c.text), hex(c.sub));
    let hover = hex(c.hover);
    let bar = hex(accent(c));
    // 9-slice images: the corners (Margin) keep their size, the rest stretches.
    let panel = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect x="0.5" y="0.5" width="31" height="31" rx="7.5" fill="{bg}" stroke="{border}"/></svg>"#
    );
    // A tinted pill with a short accent bar at the left, like Windows.
    let highlight = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect width="32" height="32" rx="4" fill="{hover}" fill-opacity="{:.2}"/><rect x="0" y="9" width="3" height="14" rx="1.5" fill="{bar}"/></svg>"#,
        c.hover_alpha
    );
    let menu_highlight = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect width="32" height="32" rx="4" fill="{hover}" fill-opacity="{:.2}"/></svg>"#,
        c.hover_alpha
    );
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
         EnableBlur=False\nFullWidthHighlight=True\n\n\
         [InputPanel/Background]\nImage=panel.svg\n\n",
        hexa(accent(c), 0.25)
    );
    conf += &margin("InputPanel/Background/Margin", 8, 8, 8, 8);
    conf += &margin("InputPanel/ContentMargin", 5, 5, 5, 5);
    conf += &margin("InputPanel/TextMargin", 10, 10, 6, 6);
    conf += "[InputPanel/Highlight]\nImage=highlight.svg\n\n";
    conf += &margin("InputPanel/Highlight/Margin", 8, 8, 8, 8);
    conf += "[InputPanel/PrevPage]\nImage=prev.svg\n\n";
    conf += &margin("InputPanel/PrevPage/ClickMargin", 4, 4, 4, 4);
    conf += "[InputPanel/NextPage]\nImage=next.svg\n\n";
    conf += &margin("InputPanel/NextPage/ClickMargin", 4, 4, 4, 4);
    conf += &format!("[Menu]\nNormalColor={text}\nHighlightCandidateColor={text}\nSpacing=0\n\n");
    conf += "[Menu/Background]\nImage=panel.svg\n\n";
    conf += &margin("Menu/Background/Margin", 8, 8, 8, 8);
    conf += &margin("Menu/ContentMargin", 5, 5, 5, 5);
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
    let (light, dark) = if theme == Theme::Default { (LIGHT_NAME, DARK_NAME) } else { (LIGHT_NAME, LIGHT_NAME) };
    let mut values = vec![("Theme", light), ("DarkTheme", dark)];
    if theme == Theme::Default {
        values.push(("UseDarkTheme", "True"));
    }
    set_keys(conf, values)
}

/// Sets `key=value` lines, keeping every other line.
fn set_keys(conf: &str, mut values: Vec<(&str, &str)>) -> String {
    let mut out = String::new();
    for line in conf.lines() {
        let key = line.split('=').next().unwrap_or("").trim();
        if let Some(i) = values.iter().position(|(k, _)| *k == key) {
            let (k, v) = values.remove(i);
            out += &format!("{k}={v}\n");
        } else {
            out += line;
            out.push('\n');
        }
    }
    for (k, v) in values {
        out += &format!("{k}={v}\n");
    }
    out
}

fn xdg(var: &str, fallback: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(fallback)))
}

fn write_theme(dir: &Path, c: &Colors, name: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for (file, contents) in theme_files(c, name) {
        std::fs::write(dir.join(file), contents)?;
    }
    Ok(())
}

/// Writes the themes, selects them, and asks fcitx5 to reload classicui.
pub fn apply(theme: Theme) -> Result<(), String> {
    let data = xdg("XDG_DATA_HOME", ".local/share").ok_or("ホームフォルダが見つかりません")?;
    let config = xdg("XDG_CONFIG_HOME", ".config").ok_or("ホームフォルダが見つかりません")?;
    let themes = data.join("fcitx5/themes");
    let label = theme.id();
    write_theme(&themes.join(LIGHT_NAME), &theme.colors(true), label)
        .and_then(|()| if theme == Theme::Default { write_theme(&themes.join(DARK_NAME), &theme.colors(false), "dark") } else { Ok(()) })
        .map_err(|e| format!("テーマを書き込めませんでした: {e}"))?;

    write_classicui(&config, |old| select_in_classicui(old, theme))
}

/// Selects fcitx5's own themes again.
pub fn restore() -> Result<(), String> {
    let config = xdg("XDG_CONFIG_HOME", ".config").ok_or("ホームフォルダが見つかりません")?;
    write_classicui(&config, |old| set_keys(old, vec![("Theme", "default"), ("DarkTheme", "default-dark")]))
}

fn write_classicui(config: &Path, edit: impl FnOnce(&str) -> String) -> Result<(), String> {
    let path = config.join("fcitx5/conf/classicui.conf");
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    let new = edit(&old);
    if new != old {
        // Home Manager links a managed file into the Nix store.
        if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(format!(
                "{} は別のツール (Home Manager など) が管理しています。そちらで Theme={LIGHT_NAME} を設定してください。",
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

/// Asks the running fcitx5 to reload the classicui theme.
fn reload() {
    use std::process::{Command, Stdio};
    let dbus = Command::new("dbus-send")
        .args([
            "--session",
            "--print-reply",
            "--dest=org.fcitx.Fcitx5",
            "/controller",
            "org.fcitx.Fcitx.Controller1.ReloadAddonConfig",
            "string:classicui",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if !dbus.is_ok_and(|s| s.success()) {
        let _ = Command::new("fcitx5-remote").arg("-r").stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_uses_the_theme_colors() {
        let c = Theme::Sakura.colors(true);
        let files = theme_files(&c, "sakura");
        let conf = &files[0].1;
        assert!(conf.contains("NormalColor=#4a2332"), "{conf}");
        assert!(files.iter().any(|(f, s)| *f == "panel.svg" && s.contains("#fff4f7") && s.contains("#f5c6d6")));
        assert!(files.iter().any(|(f, s)| *f == "highlight.svg" && s.contains("#e85a8a")));
        // Every image the theme names is written.
        for image in conf.lines().filter_map(|l| l.strip_prefix("Image=")) {
            assert!(files.iter().any(|(f, _)| *f == image), "{image}");
        }
    }

    #[test]
    fn classicui_keeps_other_settings() {
        let old = "Vertical Candidate List=True\nTheme=default\nFont=\"Sans 12\"\n";
        let new = select_in_classicui(old, Theme::Matcha);
        assert_eq!(new, "Vertical Candidate List=True\nTheme=sweet-ime\nFont=\"Sans 12\"\nDarkTheme=sweet-ime\n");
        let new = select_in_classicui("", Theme::Default);
        assert_eq!(new, "Theme=sweet-ime\nDarkTheme=sweet-ime-dark\nUseDarkTheme=True\n");
        // Applying twice changes nothing.
        assert_eq!(select_in_classicui(&new, Theme::Default), new);
    }
}
