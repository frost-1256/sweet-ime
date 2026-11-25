//! Conversions the user reported, to improve the dictionary.
//!
//! A report is a reading and the text the user wanted for it (what they
//! committed after fixing the conversion). The file stays on the device;
//! the settings apps show it and let the user send it on as a GitHub issue.
//! Each report is a line of Mozc's evaluation.tsv format, so the file can
//! be checked with `sweet-cli <dict> eval reports.tsv` and its lines added
//! to `eval/everyday.tsv`.

use std::io;
use std::path::Path;

const HEADER: &str = "# Conversions reported in Sweet IME. Check them with `sweet-cli <dict> eval <this file>`.\n";

/// Where new issues for reports are filed.
pub const ISSUE_URL: &str = "https://github.com/frost-1256/sweet-ime/issues/new";

/// Most characters in a reported reading or text.
const MAX_CHARS: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    /// The reading in hiragana.
    pub reading: String,
    /// The text the user wanted.
    pub text: String,
}

impl Report {
    fn line(&self) -> String {
        format!("OK:\t{}\t\tConversion Expected\t{}", self.reading, self.text)
    }

    fn parse(line: &str) -> Option<Self> {
        let f: Vec<&str> = line.split('\t').collect();
        match f[..] {
            [_, reading, _, "Conversion Expected", text] => {
                Some(Self { reading: reading.to_string(), text: text.to_string() })
            }
            _ => None,
        }
    }

    /// Whether the report can be stored: one line, not empty, not too long.
    fn is_valid(&self) -> bool {
        [&self.reading, &self.text].iter().all(|s| {
            !s.trim().is_empty() && s.chars().count() <= MAX_CHARS && !s.chars().any(char::is_control)
        })
    }
}

/// The reports in `path`, oldest first; none if the file is missing.
pub fn load(path: &Path) -> io::Result<Vec<Report>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(text.lines().filter(|l| !l.starts_with('#')).filter_map(Report::parse).collect()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// Writes `reports` to `path` (none removes the file).
pub fn save(path: &Path, reports: &[Report]) -> io::Result<()> {
    if reports.is_empty() {
        return match std::fs::remove_file(path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, to_text(reports))
}

/// Adds a report to `path`. Returns whether it was new.
pub fn add(path: &Path, report: Report) -> io::Result<bool> {
    if !report.is_valid() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a reportable conversion"));
    }
    let mut reports = load(path)?;
    // The same reading again: the latest choice counts.
    if let Some(i) = reports.iter().position(|r| r.reading == report.reading) {
        if reports[i] == report {
            return Ok(false);
        }
        reports.remove(i);
    }
    reports.push(report);
    save(path, &reports)?;
    Ok(true)
}

/// The file text for `reports`.
pub fn to_text(reports: &[Report]) -> String {
    let mut out = HEADER.to_string();
    for r in reports {
        out.push_str(&r.line());
        out.push('\n');
    }
    out
}

/// The new-issue URL for sending `reports`, with a title and a body the
/// user reviews before submitting it.
pub fn issue_url(reports: &[Report]) -> String {
    let mut body = String::from("Sweet IME で報告した変換です(読み → 期待する変換)。\n\n```tsv\n");
    for r in reports {
        body.push_str(&r.line());
        body.push('\n');
    }
    body.push_str("```\n");
    format!(
        "{ISSUE_URL}?template=in_app_conversion_report.md&title={}&body={}",
        encode("変換の報告"),
        encode(&body)
    )
}

/// Percent-encodes a URL query value.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(reading: &str, text: &str) -> Report {
        Report { reading: reading.into(), text: text.into() }
    }

    #[test]
    fn keeps_one_report_per_reading() {
        let dir = std::env::temp_dir().join(format!("sweet-reports-{}", std::process::id()));
        let path = dir.join("reports.tsv");
        let _ = std::fs::remove_file(&path);
        assert!(add(&path, report("ほかにあんある", "他に案ある")).unwrap());
        assert!(!add(&path, report("ほかにあんある", "他に案ある")).unwrap());
        assert!(add(&path, report("きかい", "機会")).unwrap());
        assert!(add(&path, report("ほかにあんある", "ほかに案ある")).unwrap());
        assert_eq!(load(&path).unwrap(), [report("きかい", "機会"), report("ほかにあんある", "ほかに案ある")]);
        assert!(add(&path, report("a\tb", "x")).is_err());
        save(&path, &[]).unwrap();
        assert!(!path.exists());
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn lines_are_evaluation_rows() {
        let text = to_text(&[report("きかい", "機会")]);
        assert!(text.ends_with("OK:\tきかい\t\tConversion Expected\t機会\n"));
        let url = issue_url(&[report("きかい", "機会")]);
        assert!(url.starts_with(concat!(
            "https://github.com/frost-1256/sweet-ime/issues/new",
            "?template=in_app_conversion_report.md&title=%E5%A4%89"
        )));
        assert!(url.contains("Conversion%20Expected%09%E6%A9%9F%E4%BC%9A"));
        assert!(!url.contains(' ') && !url.contains('\n'));
    }
}
