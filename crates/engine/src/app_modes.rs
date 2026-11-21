//! Per-application input modes (`apps.txt`): start in hiragana, start in
//! direct (English) input, or leave the application alone entirely.

/// How the IME behaves in one application.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AppMode {
    /// Hiragana when the IME is switched to.
    #[default]
    Normal,
    /// Direct input when the IME is switched to; toggle keys still work.
    English,
    /// No input processing at all (remote desktops, games).
    Off,
}

impl AppMode {
    pub const ALL: [AppMode; 3] = [AppMode::Normal, AppMode::English, AppMode::Off];

    pub fn id(self) -> &'static str {
        match self {
            AppMode::Normal => "normal",
            AppMode::English => "english",
            AppMode::Off => "off",
        }
    }

    /// Name shown in the settings app.
    pub fn label(self) -> &'static str {
        match self {
            AppMode::Normal => "ひらがなで始める",
            AppMode::English => "英字で始める",
            AppMode::Off => "使わない",
        }
    }

    fn from_id(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.id().eq_ignore_ascii_case(s.trim()))
    }
}

/// Modes by executable file name (`code.exe`), matched ignoring case.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AppModes {
    pub entries: Vec<(String, AppMode)>,
}

impl AppModes {
    /// Parses `program.exe = mode` lines (`#` comments).
    pub fn parse(text: &str) -> Self {
        let entries = text
            .lines()
            .filter_map(|l| {
                let (exe, mode) = l.split('#').next()?.split_once('=')?;
                let exe = exe.trim();
                (!exe.is_empty()).then(|| Some((exe.to_string(), AppMode::from_id(mode)?)))?
            })
            .collect();
        Self { entries }
    }

    pub fn to_text(&self) -> String {
        let mut s = String::from("# Sweet IME per-application modes: program = normal | english | off\n");
        for (exe, mode) in &self.entries {
            s.push_str(&format!("{exe} = {}\n", mode.id()));
        }
        s
    }

    /// The mode for the program at `exe` (a path or a file name).
    pub fn mode_for(&self, exe: &str) -> AppMode {
        let name = exe.rsplit(['\\', '/']).next().unwrap_or(exe);
        self.entries.iter().rev().find(|(e, _)| e.eq_ignore_ascii_case(name)).map_or(AppMode::Normal, |e| e.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_match_by_file_name() {
        let m = AppModes::parse("# c\nmstsc.exe = off\nCode.exe=english\nbad line\nx.exe = unknown\n");
        assert_eq!(m.entries.len(), 2);
        assert_eq!(m.mode_for(r"C:\Windows\System32\MSTSC.EXE"), AppMode::Off);
        assert_eq!(m.mode_for("code.exe"), AppMode::English);
        assert_eq!(m.mode_for("notepad.exe"), AppMode::Normal);
        assert_eq!(AppModes::parse(&m.to_text()), m);
    }
}
