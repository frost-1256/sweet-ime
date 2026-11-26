//! Per-user file locations shared by the text service and the settings app.

use std::path::PathBuf;

#[cfg(windows)]
fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(|d| PathBuf::from(d).join("SweetIME"))
}

/// `$var/sweet-ime`, or `$HOME/<fallback>/sweet-ime` (XDG base directories).
#[cfg(not(any(windows, target_os = "macos")))]
fn xdg_dir(var: &str, fallback: &str) -> Option<PathBuf> {
    let base = std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(fallback)))?;
    Some(base.join("sweet-ime"))
}

/// Settings directory: `%APPDATA%\SweetIME` on Windows,
/// `~/Library/Application Support/SweetIME` on macOS, XDG elsewhere.
pub fn config_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    return env_dir("APPDATA");
    #[cfg(target_os = "macos")]
    return std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join("Library/Application Support/SweetIME"));
    #[cfg(not(any(windows, target_os = "macos")))]
    return xdg_dir("XDG_CONFIG_HOME", ".config");
}

/// Directory for large per-user data (the neural model):
/// `%LOCALAPPDATA%\SweetIME` on Windows, the settings directory on macOS, XDG elsewhere.
pub fn data_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    return env_dir("LOCALAPPDATA");
    #[cfg(target_os = "macos")]
    return config_dir();
    #[cfg(not(any(windows, target_os = "macos")))]
    return xdg_dir("XDG_DATA_HOME", ".local/share");
}

/// Settings file (`key = value` lines, see `Options`).
pub fn config_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join("config.txt"))
}

/// User-defined theme colors and opacity (`user_theme.txt`).
pub fn user_theme_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join("user_theme.txt"))
}

pub fn load_user_theme() -> crate::UserTheme {
    user_theme_file()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| crate::UserTheme::parse(&t))
        .unwrap_or_default()
}

pub fn save_user_theme(theme: &crate::UserTheme) -> std::io::Result<()> {
    let p = user_theme_file().ok_or_else(|| std::io::Error::other("no settings directory"))?;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(p, theme.to_text())
}

/// Named user-theme presets (`user_themes.txt`).
pub fn saved_user_themes_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join("user_themes.txt"))
}

pub fn load_saved_user_themes() -> std::io::Result<Vec<crate::theme::SavedUserTheme>> {
    let Some(p) = saved_user_themes_file() else {
        return Ok(Vec::new());
    };
    match std::fs::read_to_string(p) {
        Ok(text) => Ok(crate::theme::parse_saved_user_themes(&text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

pub fn save_saved_user_themes(themes: &[crate::theme::SavedUserTheme]) -> std::io::Result<()> {
    let p =
        saved_user_themes_file().ok_or_else(|| std::io::Error::other("no settings directory"))?;
    let dir = p
        .parent()
        .ok_or_else(|| std::io::Error::other("no settings directory"))?;
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!("user_themes.{}.tmp", std::process::id()));
    let content = crate::theme::format_saved_user_themes(themes);
    if let Err(e) = std::fs::write(&tmp, content) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp, &p) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// The Windows candidate window's font family, one name per file.
#[cfg(windows)]
pub fn candidate_font_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join("candidate-font.txt"))
}

#[cfg(windows)]
pub fn load_candidate_font() -> String {
    candidate_font_file()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|text| text.lines().next().map(str::trim).map(str::to_owned))
        .unwrap_or_default()
}

#[cfg(windows)]
pub fn save_candidate_font(family: &str) -> std::io::Result<()> {
    let path =
        candidate_font_file().ok_or_else(|| std::io::Error::other("no settings directory"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, family)
}

/// Learning history.
pub fn history_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join("history.tsv"))
}

/// Conversions the user reported (see [`crate::reports`]).
pub fn reports_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join("reports.tsv"))
}

/// Words the user registered by hand.
pub fn user_dict_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join("user_dict.tsv"))
}

/// Attaches the learning history and user dictionary in the settings directory.
pub fn with_user_data(mut converter: crate::Converter) -> crate::Converter {
    if let Some(path) = history_file() {
        converter = converter.with_history(crate::UserHistory::with_file(path));
    }
    if let Some(path) = user_dict_file() {
        converter = converter.with_user_dict(crate::UserDictionary::with_file(path));
    }
    converter
}

/// Last modification time of a user file, if available.
pub fn modified(path: Option<PathBuf>) -> Option<std::time::SystemTime> {
    std::fs::metadata(path?).ok()?.modified().ok()
}

/// User-installed neural model (`zenz.swz` in [`data_dir`]).
pub fn zenz_file() -> Option<PathBuf> {
    data_dir().map(|d| d.join("zenz.swz"))
}

/// Reads the settings file; defaults when missing or unreadable.
pub fn load_options() -> crate::Options {
    config_file()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| crate::Options::parse(&t))
        .unwrap_or_default()
}

/// Writes the settings file.
pub fn save_options(o: &crate::Options) -> std::io::Result<()> {
    let p = config_file().ok_or_else(|| std::io::Error::other("no settings directory"))?;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(p, o.to_text())
}

/// Per-application modes (see [`crate::app_modes`]).
pub fn apps_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join("apps.txt"))
}

/// Reads the per-application modes; none when the file is missing.
pub fn load_app_modes() -> crate::app_modes::AppModes {
    apps_file()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| crate::app_modes::AppModes::parse(&t))
        .unwrap_or_default()
}

/// Writes the per-application modes.
pub fn save_app_modes(m: &crate::app_modes::AppModes) -> std::io::Result<()> {
    let p = apps_file().ok_or_else(|| std::io::Error::other("no settings directory"))?;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(p, m.to_text())
}

/// Key bindings (see [`crate::keymap`]).
pub fn keymap_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join("keymap.txt"))
}

/// Reads the key bindings; the built-in layout when the file is missing.
pub fn load_keymap() -> crate::Keymap {
    keymap_file()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| crate::Keymap::parse(&t))
        .unwrap_or_default()
}

/// Writes the key bindings.
pub fn save_keymap(k: &crate::Keymap) -> std::io::Result<()> {
    let p = keymap_file().ok_or_else(|| std::io::Error::other("no settings directory"))?;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(p, k.to_text())
}

/// Removes the key bindings file (back to the built-in layout).
pub fn reset_keymap() -> std::io::Result<()> {
    match keymap_file().map(std::fs::remove_file) {
        Some(Err(e)) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}
