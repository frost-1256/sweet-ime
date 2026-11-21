//! Per-user file locations shared by the text service and the settings app.

use std::path::PathBuf;

#[cfg(windows)]
fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(|d| PathBuf::from(d).join("SweetIME"))
}

/// `$var/sweet-ime`, or `$HOME/<fallback>/sweet-ime` (XDG base directories).
#[cfg(not(windows))]
fn xdg_dir(var: &str, fallback: &str) -> Option<PathBuf> {
    let base = std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(fallback)))?;
    Some(base.join("sweet-ime"))
}

/// Settings directory: `%APPDATA%\SweetIME` on Windows,
/// `$XDG_CONFIG_HOME/sweet-ime` elsewhere.
pub fn config_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    return env_dir("APPDATA");
    #[cfg(not(windows))]
    return xdg_dir("XDG_CONFIG_HOME", ".config");
}

/// Directory for large per-user data (the neural model):
/// `%LOCALAPPDATA%\SweetIME` on Windows, `$XDG_DATA_HOME/sweet-ime` elsewhere.
pub fn data_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    return env_dir("LOCALAPPDATA");
    #[cfg(not(windows))]
    return xdg_dir("XDG_DATA_HOME", ".local/share");
}

/// Settings file (`key = value` lines, see `Options`).
pub fn config_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join("config.txt"))
}

/// Learning history.
pub fn history_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join("history.tsv"))
}

/// Words the user registered by hand.
pub fn user_dict_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join("user_dict.tsv"))
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
