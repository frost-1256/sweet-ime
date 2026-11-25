//! Update check against GitHub Releases and installation on Windows and macOS.
//!
//! Requests go through `curl`, which ships with Windows 10 and later and is
//! on the PATH of the Linux package.

// Elsewhere updates come from the package manager: only the check is used.
#![cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use sweet_engine::paths;
use sweet_ui::eframe::egui;

const REPO: &str = "frost-1256/sweet-ime";
#[cfg(target_os = "macos")]
const INSTALLER: &str = "SweetIME-macOS.zip";
#[cfg(not(target_os = "macos"))]
const INSTALLER: &str = "SweetIME-Setup.exe";
/// The running version. Debug builds can pretend to be another one with
/// `SWEET_UPDATE_CURRENT` to try the update flow.
pub fn current() -> String {
    #[cfg(debug_assertions)]
    if let Ok(v) = std::env::var("SWEET_UPDATE_CURRENT") {
        return v;
    }
    env!("CARGO_PKG_VERSION").to_string()
}

/// A published release.
#[derive(Clone, Debug)]
pub struct Release {
    /// Version without the leading `v`.
    pub version: String,
    pub notes: String,
    pub page: String,
    pub installer: Option<Asset>,
}

#[derive(Clone, Debug)]
pub struct Asset {
    pub url: String,
    pub size: u64,
    pub sha256: Option<String>,
}

/// `1.2.3` style version as numbers; a pre-release suffix is ignored.
fn parse_version(v: &str) -> Vec<u64> {
    let v = v.trim().trim_start_matches('v');
    let core = v.split(['-', '+']).next().unwrap_or("");
    core.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

/// Whether `latest` is a newer version than `current`.
pub fn is_newer(latest: &str, current: &str) -> bool {
    let (mut a, mut b) = (parse_version(latest), parse_version(current));
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    a > b || (a == b && !is_prerelease(latest) && is_prerelease(current))
}

fn is_prerelease(version: &str) -> bool {
    version.split('+').next().unwrap_or_default().contains('-')
}

fn curl() -> std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // The system copy, not whatever is first on the PATH.
        let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| r"C:\Windows".into());
        let mut c = std::process::Command::new(root.join(r"System32\curl.exe"));
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        c
    }
    #[cfg(target_os = "macos")]
    return std::process::Command::new("/usr/bin/curl");
    #[cfg(not(any(windows, target_os = "macos")))]
    std::process::Command::new("curl")
}

fn run(c: &mut std::process::Command) -> Result<Vec<u8>, String> {
    let out = c.output().map_err(|e| format!("curl を実行できません: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("通信に失敗しました: {}", err.trim()));
    }
    Ok(out.stdout)
}

/// Parses the `releases/latest` API response.
fn parse_release(json: &[u8]) -> Result<Release, String> {
    let v: serde_json::Value = serde_json::from_slice(json).map_err(|e| format!("応答を読めません: {e}"))?;
    let tag = v["tag_name"].as_str().ok_or("応答にバージョンがありません")?;
    let expected_url = format!("https://github.com/{REPO}/releases/download/{tag}/{INSTALLER}");
    let installer = v["assets"].as_array().into_iter().flatten().find_map(|a| {
        let url = a["browser_download_url"].as_str()?;
        if a["name"].as_str()? != INSTALLER || url != expected_url {
            return None;
        }
        Some(Asset {
            url: url.to_string(),
            size: a["size"].as_u64().unwrap_or(0),
            sha256: a["digest"].as_str().and_then(|d| d.strip_prefix("sha256:")).map(str::to_ascii_lowercase),
        })
    });
    Ok(Release {
        version: tag.trim_start_matches('v').to_string(),
        notes: v["body"].as_str().unwrap_or("").trim().to_string(),
        page: v["html_url"].as_str().unwrap_or("").to_string(),
        installer,
    })
}

/// Fetches the latest (non-pre-release) release.
pub fn fetch_latest() -> Result<Release, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let body = run(curl().args([
        "-q",
        "-fsSL",
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
        "--max-time",
        "20",
        "-H",
        "Accept: application/vnd.github+json",
        "-H",
        "User-Agent: sweet-ime-updater",
        &url,
    ]))?;
    parse_release(&body)
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Downloads the installer into the temporary directory and checks it.
fn download(asset: &Asset, dest: &Path) -> Result<PathBuf, String> {
    let _ = std::fs::remove_file(dest);
    run(curl()
        .args([
            "-q", "-fsSL", "--proto", "=https", "--proto-redir", "=https", "--max-time", "900", "-o",
        ])
        .arg(dest)
        .arg(&asset.url))?;
    let size = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
    if asset.size != 0 && size != asset.size {
        let _ = std::fs::remove_file(dest);
        return Err(format!("ダウンロードしたファイルのサイズが違います({size} / {} バイト)", asset.size));
    }
    if let Some(want) = &asset.sha256 {
        let got = sha256_file(dest).map_err(|e| e.to_string())?;
        if &got != want {
            let _ = std::fs::remove_file(dest);
            return Err("ダウンロードしたファイルのハッシュが一致しません".into());
        }
    }
    Ok(dest.to_path_buf())
}

/// Starts the downloaded installer in update mode. It asks for administrator
/// rights itself.
#[cfg(windows)]
fn launch(installer: &Path) -> Result<(), String> {
    std::process::Command::new(installer)
        .arg("--update")
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("インストーラーを起動できません: {e}"))
}

#[cfg(target_os = "macos")]
fn install_macos(asset: &Asset, archive: &Path) -> Result<(), String> {
    if !asset
        .sha256
        .as_ref()
        .is_some_and(|sha| sha.len() == 64 && sha.bytes().all(|c| c.is_ascii_hexdigit()))
    {
        return Err("このリリースのダウンロードを検証できません。".into());
    }
    let work = archive.parent().unwrap();
    std::fs::create_dir(work).map_err(|e| format!("更新用フォルダーを作れません: {e}"))?;
    let result = download(asset, archive).and_then(|archive| {
        let output = std::process::Command::new("/bin/bash")
            .arg("-c")
            .arg(include_str!("../../../macos/install.sh"))
            .args(["sweet-ime-install", "--archive"])
            .arg(archive)
            .output()
            .map_err(|e| format!("インストーラーを起動できません: {e}"))?;
        if output.status.success() {
            Ok(())
        } else {
            let message = String::from_utf8_lossy(&output.stderr);
            Err(format!("アップデートに失敗しました: {}", message.trim()))
        }
    });
    let _ = std::fs::remove_dir_all(work);
    result
}

// --- Remembered state (last check, skipped version) ---

fn state_file() -> Option<PathBuf> {
    paths::config_dir().map(|d| d.join("update.txt"))
}

#[derive(Default)]
pub struct Remembered {
    pub last_check: u64,
    pub skip: String,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Remembered {
    pub fn load() -> Self {
        let mut r = Self::default();
        let text = state_file().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        for line in text.lines() {
            match line.split_once('=').map(|(k, v)| (k.trim(), v.trim())) {
                Some(("last_check", v)) => r.last_check = v.parse().unwrap_or(0),
                Some(("skip", v)) => r.skip = v.to_string(),
                _ => {}
            }
        }
        r
    }

    pub fn save(&self) {
        if let Some(p) = state_file() {
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(p, format!("last_check = {}\nskip = {}\n", self.last_check, self.skip));
        }
    }

    /// Whether a background check is due (at most about once a day).
    #[cfg(not(target_os = "macos"))]
    pub fn due(&self) -> bool {
        now().saturating_sub(self.last_check) >= Duration::from_secs(20 * 3600).as_secs()
    }

    pub fn checked(&mut self) {
        self.last_check = now();
        self.save();
    }
}

// --- UI state machine ---

pub enum State {
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Downloading(Release),
    /// The installer is running; the caller should close.
    #[cfg(not(target_os = "macos"))]
    Launched,
    #[cfg(target_os = "macos")]
    Installed,
    Failed(String),
}

enum Event {
    Checked(Result<Release, String>),
    #[cfg(not(target_os = "macos"))]
    Downloaded(Result<PathBuf, String>),
    #[cfg(target_os = "macos")]
    Installed(Result<(), String>),
}

pub struct Updater {
    pub state: State,
    rx: Option<Receiver<Event>>,
    download_path: Option<(PathBuf, u64)>,
}

impl Updater {
    pub fn new() -> Self {
        Self { state: State::Idle, rx: None, download_path: None }
    }

    /// Starts with a known release (from a check done before the window opened).
    #[cfg(not(target_os = "macos"))]
    pub fn with_release(r: Release) -> Self {
        Self { state: State::Available(r), rx: None, download_path: None }
    }

    pub fn busy(&self) -> bool {
        matches!(self.state, State::Checking | State::Downloading(_))
    }

    pub fn check(&mut self, ctx: &egui::Context) {
        if self.busy() {
            return;
        }
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Event::Checked(fetch_latest()));
            ctx.request_repaint();
        });
        self.rx = Some(rx);
        self.state = State::Checking;
    }

    /// Downloads and launches the installer of the available release.
    #[cfg(any(windows, target_os = "macos"))]
    pub fn install(&mut self, ctx: &egui::Context) {
        let State::Available(r) = &self.state else { return };
        let Some(asset) = r.installer.clone() else {
            self.state = State::Failed("このリリースにはインストーラーがありません".into());
            return;
        };
        let r = r.clone();
        #[cfg(windows)]
        let dest = std::env::temp_dir().join(format!("SweetIME-Setup-{}.exe", r.version));
        #[cfg(target_os = "macos")]
        let dest = std::env::temp_dir()
            .join(format!("sweet-ime-update-{}-{}", std::process::id(), now()))
            .join(INSTALLER);
        self.download_path = Some((dest.clone(), asset.size));
        let (tx, rx) = channel();
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            #[cfg(windows)]
            let _ = tx.send(Event::Downloaded(download(&asset, &dest)));
            #[cfg(target_os = "macos")]
            let _ = tx.send(Event::Installed(install_macos(&asset, &dest)));
            ctx2.request_repaint();
        });
        self.rx = Some(rx);
        self.state = State::Downloading(r);
    }

    /// Download progress as a fraction.
    pub fn progress(&self) -> Option<f32> {
        let (p, total) = self.download_path.as_ref()?;
        let have = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        (*total > 0).then(|| (have as f32 / *total as f32).min(1.0))
    }

    /// Handles finished background work.
    pub fn poll(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.rx else { return };
        if matches!(self.state, State::Downloading(_)) {
            // Keep the progress bar moving.
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        let Ok(ev) = rx.try_recv() else { return };
        self.rx = None;
        self.state = match ev {
            Event::Checked(Ok(r)) => {
                let mut m = Remembered::load();
                m.checked();
                if is_newer(&r.version, &current()) { State::Available(r) } else { State::UpToDate }
            }
            Event::Checked(Err(e)) => State::Failed(e),
            #[cfg(not(target_os = "macos"))]
            Event::Downloaded(Err(e)) => State::Failed(e),
            #[cfg(windows)]
            Event::Downloaded(Ok(path)) => match launch(&path) {
                Ok(()) => State::Launched,
                Err(e) => State::Failed(e),
            },
            #[cfg(not(any(windows, target_os = "macos")))]
            Event::Downloaded(Ok(_)) => State::Idle,
            #[cfg(target_os = "macos")]
            Event::Installed(Ok(())) => State::Installed,
            #[cfg(target_os = "macos")]
            Event::Installed(Err(e)) => State::Failed(e),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(is_newer("v0.1.3", "0.1.2"));
        assert!(is_newer("0.2.0", "0.1.9"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(is_newer("0.1.10", "0.1.9"));
        assert!(!is_newer("v0.1.2", "0.1.2"));
        assert!(!is_newer("0.1.1", "0.1.2"));
        assert!(!is_newer("0.1.2-rc1", "0.1.2"));
        assert!(is_newer("0.5.0", "0.5.0-alpha.1"));
        assert!(!is_newer("0.5.0+build-info", "0.5.0"));
    }

    #[test]
    fn parses_release_json() {
        let json = r###"{
            "tag_name": "v0.2.0",
            "html_url": "https://github.com/frost-1256/sweet-ime/releases/tag/v0.2.0",
            "body": "## What's Changed\n* Things\n",
            "assets": [
                {"name": "other.zip", "size": 1, "browser_download_url": "https://github.com/frost-1256/sweet-ime/releases/download/v0.2.0/other.zip"},
                {"name": "@installer@", "size": 123, "digest": "sha256:ABCDEF",
                 "browser_download_url": "https://github.com/frost-1256/sweet-ime/releases/download/v0.2.0/@installer@"}
            ]
        }"###.replace("@installer@", INSTALLER);
        let r = parse_release(json.as_bytes()).unwrap();
        assert_eq!(r.version, "0.2.0");
        assert_eq!(r.notes, "## What's Changed\n* Things");
        let a = r.installer.unwrap();
        assert_eq!(a.size, 123);
        assert_eq!(a.sha256.as_deref(), Some("abcdef"));
    }

    /// Talks to GitHub: `cargo test -p sweet-settings -- --ignored`.
    #[test]
    #[ignore]
    fn fetches_latest_release() {
        let r = fetch_latest().unwrap();
        assert!(!parse_version(&r.version).is_empty());
        let a = r.installer.expect("installer asset");
        assert!(a.size > 1_000_000);
        assert_eq!(a.sha256.as_ref().map(String::len), Some(64));
    }

    /// Downloads the current installer (about 30 MB) and checks its hash.
    #[test]
    #[ignore]
    fn downloads_and_verifies_installer() {
        let a = fetch_latest().unwrap().installer.unwrap();
        let dest = std::env::temp_dir().join(format!("sweet-update-test-{INSTALLER}"));
        download(&a, &dest).unwrap();
        let _ = std::fs::remove_file(&dest);
        let bad = Asset { sha256: Some("0".repeat(64)), ..a };
        assert!(download(&bad, &dest).is_err());
        assert!(!dest.exists());
    }

    #[test]
    fn ignores_installers_hosted_elsewhere() {
        let json = r#"{"tag_name": "v9.0.0", "assets": [
            {"name": "@installer@", "size": 1, "browser_download_url": "https://example.com/@installer@"}]}"#
            .replace("@installer@", INSTALLER);
        assert!(parse_release(json.as_bytes()).unwrap().installer.is_none());
    }
}
