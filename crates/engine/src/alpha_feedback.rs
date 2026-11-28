//! Opt-in Windows Alpha feedback. Unsent conversions live only in memory.

use serde_json::{Value, json};
use std::io::{self, Read, Write};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;
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
#[derive(PartialEq, Eq)]
pub struct Consent(String);

pub fn consent() -> Option<Consent> {
    let endpoint = endpoint()?;
    let path = consent_path()?;
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
    let Some(url) = endpoint().filter(|_| valid_invite(invite)) else {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    };
    let path = consent_path().ok_or_else(|| io::Error::other("settings directory unavailable"))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?;
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(path, format!("{invite}\n{url}\n{}", stamp.as_nanos()))
}

pub fn disable() -> io::Result<()> {
    let Some(path) = consent_path() else {
        return Ok(());
    };
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    Ok(())
}

struct Batch {
    consent: Consent,
    records: Vec<Value>,
}

static PENDING: Mutex<Option<Batch>> = Mutex::new(None);

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
    let Ok(mut pending) = PENDING.try_lock() else {
        return;
    };
    let needs_worker = pending.is_none();
    let batch = pending.get_or_insert_with(|| Batch {
        consent: Consent(String::new()),
        records: Vec::new(),
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

fn flush() {
    std::thread::sleep(BATCH_WINDOW);
    let Some(batch) = PENDING.lock().unwrap_or_else(|e| e.into_inner()).take() else {
        return;
    };
    if batch.records.is_empty() || consent().as_ref() != Some(&batch.consent) {
        return;
    }
    let Some(url) = endpoint() else { return };
    let Some((invite, _)) = batch.consent.0.split_once('\n') else {
        return;
    };
    let body = json!({ "schema": 1, "version": VERSION, "records": batch.records }).to_string();
    let _ = post(url, invite, &body);
}

fn post(url: &str, invite: &str, body: &str) -> io::Result<()> {
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
