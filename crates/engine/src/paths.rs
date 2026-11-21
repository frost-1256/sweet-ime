//! Per-user file locations shared by the text service and the settings app.

use std::path::PathBuf;

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(|d| PathBuf::from(d).join("SweetIME"))
}

/// Roaming settings directory (`%APPDATA%\SweetIME`).
pub fn config_dir() -> Option<PathBuf> {
    env_dir("APPDATA")
}

/// Settings file (`key = value` lines, see `Options`).
pub fn config_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join("config.txt"))
}

/// Learning history.
pub fn history_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join("history.tsv"))
}

/// User-installed neural model (`%LOCALAPPDATA%\SweetIME\zenz.swz`).
pub fn zenz_file() -> Option<PathBuf> {
    env_dir("LOCALAPPDATA").map(|d| d.join("zenz.swz"))
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
    let p = config_file().ok_or_else(|| std::io::Error::other("APPDATA is not set"))?;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(p, o.to_text())
}
