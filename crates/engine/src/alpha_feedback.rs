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
    let channel = |name: &str| {
        prerelease == name
            || prerelease
                .strip_prefix(name)
                .and_then(|rest| rest.strip_prefix('.'))
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    };
    if !channel("alpha") && !channel("beta") {
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

/// What the debug menu shows about this process's uploads. Holds no
/// conversion text.
struct Stats {
    queued: usize,
    sent_batches: usize,
    sent_records: usize,
    failures: usize,
    /// When, how many records, and the error if it failed.
    last_attempt: Option<(SystemTime, usize, Option<String>)>,
    last_skip: Option<(SystemTime, &'static str)>,
    /// When the last manual test was sent, and the error if it failed.
    last_test: Option<(SystemTime, Option<String>)>,
}

static STATS: Mutex<Stats> = Mutex::new(Stats {
    queued: 0,
    sent_batches: 0,
    sent_records: 0,
    failures: 0,
    last_attempt: None,
    last_skip: None,
    last_test: None,
});

fn stats() -> std::sync::MutexGuard<'static, Stats> {
    STATS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Records why a confirmed conversion was not queued, for the debug menu.
pub fn note_skip(reason: &'static str) {
    stats().last_skip = Some((SystemTime::now(), reason));
    log_event(reason.into());
}

/// Records whether the field a composition started in may send feedback.
/// Only changes are logged, so typing in one field adds one line.
pub fn note_field(status: &'static str) {
    static LAST: Mutex<&str> = Mutex::new("");
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if *last != status {
        *last = status;
        log_event(format!("入力欄: {status}"));
    }
}

/// Sends the test record with this machine's stored consent.
pub fn send_test() -> bool {
    consent_path().is_some_and(|path| send_test_at(&path))
}

fn ago(at: SystemTime) -> String {
    let secs = at.elapsed().map_or(0, |d| d.as_secs());
    if secs < 60 {
        format!("{secs}秒前")
    } else {
        format!("{}分前", secs / 60)
    }
}

/// The fixed record a manual test sends, easy to find and delete in storage.
pub const TEST_RECORD: (&str, &str) = ("そうしんてすと", "送信テスト");

/// Sends one batch holding only [`TEST_RECORD`] on a background thread, with
/// the consent at `path`. Returns false if there is no consent. The result
/// shows up in [`debug_report`].
pub fn send_test_at(path: &Path) -> bool {
    let Some(consent) = consent_at(path) else {
        return false;
    };
    let Some(url) = endpoint() else { return false };
    std::thread::Builder::new()
        .name("sweet-ime-feedback-test".into())
        .spawn(move || {
            let Some((invite, _)) = consent.0.split_once('\n') else {
                return;
            };
            let (reading, text) = TEST_RECORD;
            let body = json!({
                "schema": 1,
                "version": VERSION,
                "records": [{ "reading": reading, "text": text }],
            })
            .to_string();
            let result = post(url, invite, &body);
            log_event(match &result {
                Ok(()) => "テスト送信に成功しました".into(),
                Err(e) => format!("テスト送信に失敗しました ({e})"),
            });
            let mut stats = stats();
            stats.last_test = Some((SystemTime::now(), result.err().map(|e| e.to_string())));
        })
        .is_ok()
}

/// A plain-text summary of this process's feedback state for debugging.
pub fn debug_report(consented: bool) -> String {
    let pending = PENDING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map_or(0, |b| b.records.len());
    let stats = stats();
    let mut lines = vec![
        format!("同意: {}", if consented { "あり" } else { "なし" }),
        format!("送信待ち: {pending}件"),
        format!("記録した変換: {}件", stats.queued),
        format!(
            "送信成功: {}バッチ / {}件",
            stats.sent_batches, stats.sent_records
        ),
        format!("送信失敗: {}回", stats.failures),
    ];
    lines.push(match &stats.last_attempt {
        None => "最後の送信: まだありません".into(),
        Some((at, n, None)) => format!("最後の送信: {} {n}件 成功", ago(*at)),
        Some((at, n, Some(e))) => format!("最後の送信: {} {n}件 失敗 ({e})", ago(*at)),
    });
    match &stats.last_test {
        None => {}
        Some((at, None)) => lines.push(format!("テスト送信: {} 成功", ago(*at))),
        Some((at, Some(e))) => lines.push(format!("テスト送信: {} 失敗 ({e})", ago(*at))),
    }
    if let Some((at, reason)) = stats.last_skip {
        lines.push(format!("最後に記録しなかった理由: {} {reason}", ago(at)));
    }
    lines.join("\n")
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
        note_skip("日本語以外の文字を含むか、80文字を超えたため記録しませんでした");
        return;
    }
    stats().queued += 1;
    log_event(format!(
        "変換を記録しました{}",
        if original.is_some() {
            "(選び直しあり)"
        } else {
            ""
        }
    ));
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
        let started = SystemTime::now();
        let deadline = std::time::Instant::now() + delay;
        let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                break;
            }
            pending = WAKE
                .wait_timeout_while(pending, left.min(FLUSH_POLL), |p| {
                    !p.as_ref().is_some_and(|b| b.urgent)
                })
                .unwrap_or_else(|e| e.into_inner())
                .0;
            if pending.as_ref().is_some_and(|b| b.urgent) {
                break;
            }
            // Check the request file without holding the queue lock.
            drop(pending);
            let requested = flush_requested_since(started);
            pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
            if requested {
                log_event("今すぐ送信の要求を受け取りました".into());
                break;
            }
        }
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
        let count = batch.records.len();
        let result = post(url, invite, &body);
        {
            let mut stats = stats();
            stats.last_attempt = Some((
                SystemTime::now(),
                count,
                result.as_ref().err().map(|e| e.to_string()),
            ));
            if result.is_ok() {
                stats.sent_batches += 1;
                stats.sent_records += count;
            } else {
                stats.failures += 1;
            }
        }
        log_event(match &result {
            Ok(()) => format!("送信成功 {count}件"),
            Err(e) => format!("送信失敗 {count}件 ({e})"),
        });
        if result.is_ok() {
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

#[cfg(target_os = "android")]
fn system_post(_: &str, _: &str, _: &str) -> io::Result<()> {
    Err(io::Error::other("no uploader registered"))
}

#[cfg(not(target_os = "android"))]
fn system_post(url: &str, invite: &str, body: &str) -> io::Result<()> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    // Reuse the system curl without putting the invite or text in argv,
    // following redirects, or importing the user's curl configuration.
    #[cfg(windows)]
    let (mut command, null) = {
        use std::os::windows::process::CommandExt;
        let system =
            std::env::var_os("SystemRoot").ok_or_else(|| io::Error::other("no SystemRoot"))?;
        let mut c = Command::new(PathBuf::from(system).join(r"System32\curl.exe"));
        c.creation_flags(0x0800_0000);
        (c, "NUL")
    };
    #[cfg(not(windows))]
    let (mut command, null) = {
        let system = Path::new("/usr/bin/curl");
        (Command::new(if system.exists() { system } else { Path::new("curl") }), "/dev/null")
    };
    let mut child = command
        .args([
            "--disable",
            "--silent",
            "--proto",
            "=https",
            "--max-time",
            "10",
            "--output",
            null,
            "--write-out",
            "%{http_code}",
            "--config",
            "-",
            url,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| io::Error::other(format!("curl を実行できません: {e}")))?;
    let escaped = body.replace('\\', "\\\\").replace('"', "\\\"");
    let result = child.stdin.take().unwrap().write_all(format!(
        "header = \"Authorization: Bearer {invite}\"\nheader = \"Content-Type: application/json\"\ndata-binary = \"{escaped}\"\n"
    ).as_bytes());
    let mut code = String::new();
    if let Some(mut out) = child.stdout.take() {
        let _ = out.read_to_string(&mut code);
    }
    let status = child.wait()?;
    result?;
    match code.trim().parse::<u16>() {
        Ok(200..=299) => Ok(()),
        Ok(0) | Err(_) => Err(io::Error::other(format!(
            "接続できませんでした (curl {})",
            status.code().unwrap_or(-1)
        ))),
        Ok(code) => Err(io::Error::other(format!("HTTP {code}"))),
    }
}

/// Debug events shared between processes. On Windows and Linux the settings
/// app runs apart from the processes hosting the IME (applications, fcitx5),
/// so each process appends to a
/// small local log that the settings app reads. Events never hold conversion
/// text, and they are written on a background thread.
#[cfg(not(target_os = "android"))]
mod shared_log {
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::{OnceLock, mpsc};
    use std::time::{SystemTime, UNIX_EPOCH};

    const KEEP_LINES: usize = 100;
    const MAX_BYTES: u64 = 32 * 1024;

    pub fn path() -> Option<PathBuf> {
        Some(crate::paths::data_dir()?.join("alpha-feedback-debug.log"))
    }

    pub fn flush_request_path() -> Option<PathBuf> {
        Some(crate::paths::data_dir()?.join("alpha-feedback-flush"))
    }

    static TX: OnceLock<Option<mpsc::Sender<String>>> = OnceLock::new();

    pub fn push(event: String) {
        let tx = TX.get_or_init(|| {
            let (tx, rx) = mpsc::channel::<String>();
            let process = std::env::current_exe()
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_default();
            std::thread::Builder::new()
                .name("sweet-ime-feedback-log".into())
                .spawn(move || {
                    for event in rx {
                        let _ = append(&process, &event);
                    }
                })
                .ok()
                .map(|_| tx)
        });
        if let Some(tx) = tx {
            let _ = tx.send(event);
        }
    }

    fn append(process: &str, event: &str) -> std::io::Result<()> {
        let path = path().ok_or_else(|| std::io::Error::other("no data directory"))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let line = format!("{secs}\t{process}\t{}\n", event.replace(['\t', '\n'], " "));
        if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_BYTES) {
            let old = std::fs::read_to_string(&path).unwrap_or_default();
            let lines: Vec<&str> = old.lines().collect();
            let kept = lines[lines.len().saturating_sub(KEEP_LINES)..].join("\n");
            std::fs::write(&path, format!("{kept}\n{line}"))
        } else {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)?
                .write_all(line.as_bytes())
        }
    }
}

fn log_event(event: String) {
    #[cfg(not(target_os = "android"))]
    shared_log::push(event);
    #[cfg(target_os = "android")]
    let _ = event;
}

/// The newest `limit` shared debug events, newest first, as
/// "N秒前 [process] event" lines. Empty when there is no log.
#[cfg(not(target_os = "android"))]
pub fn debug_log(limit: usize) -> Vec<String> {
    let Some(text) = shared_log::path().and_then(|p| std::fs::read_to_string(p).ok()) else {
        return Vec::new();
    };
    text.lines()
        .rev()
        .filter_map(|line| {
            let mut fields = line.splitn(3, '\t');
            let secs: u64 = fields.next()?.parse().ok()?;
            let process = fields.next()?;
            let event = fields.next()?;
            let at = UNIX_EPOCH + Duration::from_secs(secs);
            Some(format!("{} [{process}] {event}", ago(at)))
        })
        .take(limit)
        .collect()
}

/// Asks every IME process holding a batch to send it now (Windows and Linux:
/// the settings app cannot reach their memory, so the workers watch this file).
#[cfg(not(target_os = "android"))]
pub fn request_flush() -> io::Result<()> {
    let path =
        shared_log::flush_request_path().ok_or_else(|| io::Error::other("no data directory"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, b"")?;
    log_event("今すぐ送信を要求しました".into());
    Ok(())
}

/// Whether the settings app asked for a flush after `since`.
fn flush_requested_since(since: SystemTime) -> bool {
    #[cfg(not(target_os = "android"))]
    return shared_log::flush_request_path()
        .and_then(|p| std::fs::metadata(p).ok()?.modified().ok())
        .is_some_and(|at| at > since);
    #[cfg(target_os = "android")]
    {
        let _ = since;
        false
    }
}

/// How often a waiting worker checks for a flush request from another process.
#[cfg(not(target_os = "android"))]
const FLUSH_POLL: Duration = Duration::from_secs(2);
#[cfg(target_os = "android")]
const FLUSH_POLL: Duration = BATCH_WINDOW;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_alpha_and_beta_builds_can_use_the_https_endpoint() {
        let url = "https://alpha.example.com/v1/batches";
        assert_eq!(feedback_endpoint("1.0.0-alpha.5", url), Some(url));
        assert_eq!(feedback_endpoint("1.0.0-beta.1", url), Some(url));
        assert!(feedback_endpoint("1.0.0-rc.1", url).is_none());
        assert!(feedback_endpoint("1.0.0-betax", url).is_none());
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
