//! Opt-in Alpha feedback on Windows and Android. Unsent conversions live only
//! in memory.

use serde_json::{Value, json};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const MAX_RECORDS: usize = 32;
const BATCH_WINDOW: Duration = Duration::from_secs(60);

/// Only a compiled-in HTTPS Worker destination can turn on collection.
pub fn endpoint() -> Option<&'static str> {
    feedback_endpoint(VERSION, option_env!("SWEET_IME_ALPHA_FEEDBACK_URL")?)
}

fn feedback_endpoint<'a>(version: &str, url: &'a str) -> Option<&'a str> {
    let (_, prerelease) = version.split_once('-')?;
    if prerelease != "alpha"
        && !prerelease
            .strip_prefix("alpha.")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let host = url.strip_prefix("https://")?.strip_suffix("/v1/batches")?;
    (!host.is_empty()
        && host.contains('.')
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-')))
    .then_some(url)
}

fn consent_path() -> Option<PathBuf> {
    Some(crate::paths::config_dir()?.join("alpha-feedback-consent.txt"))
}

fn valid_invite(invite: &str) -> bool {
    (32..=128).contains(&invite.len())
        && invite
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

/// An opaque consent receipt. New consent invalidates batches from the
/// previous enrollment, including when the same invite is entered again.
#[derive(Clone, PartialEq, Eq)]
pub struct Consent(String);

pub fn consent() -> Option<Consent> {
    consent_at(&consent_path()?)
}

/// The consent stored in `path`, for frontends whose settings screen does not
/// share the input method's settings directory.
pub fn consent_at(path: &Path) -> Option<Consent> {
    let endpoint = endpoint()?;
    let mut receipt = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(513)
        .read_to_string(&mut receipt)
        .ok()?;
    valid_receipt(&receipt, endpoint).then_some(Consent(receipt))
}

fn valid_receipt(receipt: &str, url: &str) -> bool {
    if receipt.len() > 512 {
        return false;
    }
    let mut lines = receipt.split('\n');
    lines.next().is_some_and(valid_invite)
        && lines.next() == Some(url)
        && lines
            .next()
            .is_some_and(|stamp| stamp.parse::<u128>().is_ok())
        && lines.next().is_none()
}

pub fn enable(invite: &str) -> io::Result<()> {
    let path = consent_path().ok_or_else(|| io::Error::other("settings directory unavailable"))?;
    enable_at(&path, invite)
}

pub fn enable_at(path: &Path, invite: &str) -> io::Result<()> {
    let Some(url) = endpoint().filter(|_| valid_invite(invite)) else {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    };
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, format!("{invite}\n{url}\n{}", stamp.as_nanos()))
}

pub fn disable() -> io::Result<()> {
    match consent_path() {
        Some(path) => disable_at(&path),
        None => Ok(()),
    }
}

pub fn disable_at(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Sends one batch: `(url, invite, JSON body)`. Frontends without a system
/// HTTP client register one; the Windows build uses curl.
pub type Uploader = fn(&str, &str, &str) -> io::Result<()>;

static UPLOADER: OnceLock<Uploader> = OnceLock::new();

pub fn set_uploader(uploader: Uploader) {
    let _ = UPLOADER.set(uploader);
}

struct Batch {
    consent: Consent,
    records: Vec<Value>,
    /// Set when the host is shutting down the input service; the worker then
    /// sends without waiting for the rest of the batch window.
    urgent: bool,
}

static PENDING: Mutex<Option<Batch>> = Mutex::new(None);
static WAKE: Condvar = Condvar::new();

/// Delays before each upload attempt after the first one fails.
const RETRY_DELAYS: [Duration; 3] = [
    Duration::from_secs(15),
    Duration::from_secs(60),
    Duration::from_secs(300),
];

fn japanese(text: &str, reading: bool) -> bool {
    !text.trim().is_empty()
        && text.chars().count() <= 80
        && text.chars().all(|c| {
            matches!(c, ' ' | '\u{3000}'..='\u{3003}' | '\u{3008}'..='\u{301f}'
                | '\u{3041}'..='\u{309f}' | '\u{30a0}'..='\u{30ff}')
                || (!reading
                    && matches!(c, '々' | '〇' | '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}'))
        })
}

/// Enqueues a confirmed conversion after the frontend has checked field
/// privacy. The queue is bounded and never waits for I/O on the input thread.
pub fn enqueue(receipt: Consent, reading: &str, text: &str, original: Option<&str>) {
    if !japanese(reading, true)
        || !japanese(text, false)
        || original.is_some_and(|s| !japanese(s, false))
    {
        return;
    }
    let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    let needs_worker = pending.is_none();
    let batch = pending.get_or_insert_with(|| Batch {
        consent: Consent(String::new()),
        records: Vec::new(),
        urgent: false,
    });
    if batch.consent != receipt {
        batch.consent = receipt;
        batch.records.clear();
    }
    if batch.records.len() < MAX_RECORDS {
        let mut record = json!({ "reading": reading, "text": text });
        if let Some(original) = original {
            record["original"] = json!(original);
        }
        batch.records.push(record);
    }
    if needs_worker
        && std::thread::Builder::new()
            .name("sweet-ime-feedback".into())
            .spawn(flush)
            .is_err()
    {
        *pending = None;
    }
}

/// Asks the worker to send the pending batch now, without blocking. Called
/// when the input service is deactivated, which usually precedes process exit.
/// Once curl has read the body it finishes the upload even if this process
/// exits first.
pub fn flush_soon() {
    let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(batch) = pending.as_mut() {
        batch.urgent = true;
        WAKE.notify_all();
    }
}

fn flush() {
    let mut delay = BATCH_WINDOW;
    let mut retries = RETRY_DELAYS.iter();
    loop {
        let pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
        let (mut pending, _) = WAKE
            .wait_timeout_while(pending, delay, |p| !p.as_ref().is_some_and(|b| b.urgent))
            .unwrap_or_else(|e| e.into_inner());
        let Some(batch) = pending.take() else {
            return;
        };
        drop(pending);
        if batch.records.is_empty() || consent().as_ref() != Some(&batch.consent) {
            return;
        }
        let Some(url) = endpoint() else { return };
        let Some((invite, _)) = batch.consent.0.split_once('\n') else {
            return;
        };
        let body = json!({ "schema": 1, "version": VERSION, "records": batch.records }).to_string();
        if post(url, invite, &body).is_ok() {
            return;
        }
        let Some(&next) = retries.next() else { return };
        // Requeue the failed records ahead of anything typed meanwhile. If a
        // newer worker already owns the queue, it sends them with its batch.
        let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
        match pending.as_mut() {
            None => {
                *pending = Some(Batch {
                    urgent: false,
                    ..batch
                });
                delay = next;
            }
            Some(newer) => {
                if newer.consent == batch.consent {
                    let mut records = batch.records;
                    records.append(&mut newer.records);
                    records.truncate(MAX_RECORDS);
                    newer.records = records;
                }
                return;
            }
        }
    }
}

fn post(url: &str, invite: &str, body: &str) -> io::Result<()> {
    if let Some(upload) = UPLOADER.get() {
        return upload(url, invite, body);
    }
    system_post(url, invite, body)
}

#[cfg(not(windows))]
fn system_post(_: &str, _: &str, _: &str) -> io::Result<()> {
    Err(io::Error::other("no uploader registered"))
}

#[cfg(windows)]
fn system_post(url: &str, invite: &str, body: &str) -> io::Result<()> {
    use std::io::Write;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    // Reuse the system curl without putting the invite or text in argv,
    // following redirects, or importing the user's curl configuration.
    let system = std::env::var_os("SystemRoot").ok_or_else(|| io::Error::other("no SystemRoot"))?;
    let mut child = Command::new(PathBuf::from(system).join(r"System32\curl.exe"))
        .args([
            "--disable",
            "--silent",
            "--fail",
            "--proto",
            "=https",
            "--max-time",
            "10",
            "--config",
            "-",
            url,
        ])
        .creation_flags(0x0800_0000)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let escaped = body.replace('\\', "\\\\").replace('"', "\\\"");
    let result = child.stdin.take().unwrap().write_all(format!(
        "header = \"Authorization: Bearer {invite}\"\nheader = \"Content-Type: application/json\"\ndata-binary = \"{escaped}\"\n"
    ).as_bytes());
    let status = child.wait()?;
    result?;
    if !status.success() {
        return Err(io::Error::other("feedback upload failed"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_alpha_builds_can_use_the_https_endpoint() {
        let url = "https://alpha.example.com/v1/batches";
        assert_eq!(feedback_endpoint("1.0.0-alpha.5", url), Some(url));
        assert!(feedback_endpoint("1.0.0-beta.1", url).is_none());
        assert!(feedback_endpoint("1.0.0", url).is_none());
        assert!(
            feedback_endpoint("1.0.0-alpha.5", "http://alpha.example.com/v1/batches").is_none()
        );
    }

    #[test]
    fn consent_is_specific_to_the_destination() {
        let url = "https://alpha.example.com/v1/batches";
        let receipt = format!("{}\n{url}\n1234", "p".repeat(32));
        assert!(valid_receipt(&receipt, url));
        assert!(!valid_receipt(
            &receipt,
            "https://other.example.com/v1/batches"
        ));
        assert!(!valid_receipt(&format!("{receipt}\n"), url));
    }

    #[test]
    fn limits_records_to_japanese_text() {
        assert!(japanese("きょうは", true));
        assert!(japanese("今日は", false));
        assert!(!japanese("今日は", true));
        for excluded in [
            "mail@example.com",
            "ぱすわーど１２３",
            "秘密\n",
            "　",
            &"あ".repeat(81),
        ] {
            assert!(!japanese(excluded, false));
        }
    }
}
