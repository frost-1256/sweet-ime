//! Transport for desktop kaomoji lookups.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering::Relaxed};

const ENDPOINT: &str =
    "https://cloud.simeji.me/py?ol=1&switch=2&section=0&ver=10.7&api_version=2&web=1";
const MAX_BYTES: usize = 1024 * 1024;
const MAX_CANDIDATES: usize = 12;
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static ACTIVE: AtomicUsize = AtomicUsize::new(0);

pub struct Response {
    pub id: u64,
    pub items: Vec<String>,
}

struct WorkerSlot;

impl Drop for WorkerSlot {
    fn drop(&mut self) {
        ACTIVE.fetch_sub(1, Relaxed);
    }
}

pub(crate) fn submit(
    reading: String,
    ready: impl FnOnce(Response) + Send + 'static,
) -> Option<u64> {
    let active = ACTIVE.fetch_add(1, Relaxed);
    let slot = WorkerSlot;
    if active >= 4 {
        return None;
    }
    let id = NEXT_ID.fetch_add(1, Relaxed);
    std::thread::Builder::new()
        .name("sweet-ime-simeji".into())
        .spawn(move || {
            let items = lookup(&reading).unwrap_or_default();
            drop(slot);
            ready(Response { id, items });
        })
        .ok()?;
    Some(id)
}

fn curl() -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
        let mut command = Command::new(std::path::PathBuf::from(root).join("System32/curl.exe"));
        command.creation_flags(0x0800_0000);
        command
    }
    #[cfg(target_os = "macos")]
    return Command::new("/usr/bin/curl");
    #[cfg(not(any(windows, target_os = "macos")))]
    Command::new(option_env!("SWEET_IME_CURL").unwrap_or("curl"))
}

fn lookup(reading: &str) -> Option<Vec<String>> {
    let mut child = curl()
        .args([
            "-q",
            "--silent",
            "--fail",
            "--proto",
            "=https",
            "--connect-timeout",
            "2",
            "--max-time",
            "4",
            "--get",
            "--data-urlencode",
            "py@-",
            ENDPOINT,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // Keep the reading out of the process command line.
    let mut body = Vec::new();
    let read = (|| {
        child.stdin.take()?.write_all(reading.as_bytes()).ok()?;
        child
            .stdout
            .take()?
            .take(MAX_BYTES as u64 + 1)
            .read_to_end(&mut body)
            .ok()?;
        Some(())
    })();
    if read.is_none() || body.len() > MAX_BYTES {
        let _ = child.kill();
    }
    if child.wait().ok()?.success() && read.is_some() && body.len() <= MAX_BYTES {
        Some(parse(reading, &body))
    } else {
        None
    }
}

fn parse(reading: &str, body: &[u8]) -> Vec<String> {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return Vec::new();
    };
    let mut items = Vec::new();
    for group in value["data"].as_array().into_iter().flatten() {
        if group["hira"].as_str() != Some(reading) {
            continue;
        }
        for candidate in group["candidates"].as_array().into_iter().flatten() {
            if !matches!(candidate["type"].as_u64(), Some(9 | 10)) {
                continue;
            }
            let Some(word) = candidate["word"].as_str() else {
                continue;
            };
            if word.trim().is_empty()
                || word.encode_utf16().count() > 160
                || word.chars().any(char::is_control)
                || items.iter().any(|item| item == word)
            {
                continue;
            }
            items.push(word.to_owned());
            if items.len() == MAX_CANDIDATES {
                return items;
            }
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_distinct_kaomoji_for_the_requested_reading() {
        let body = r#"{"data":[
            {"hira":"ねこ","candidates":[{"type":9,"word":"other"}]},
            {"hira":"かお","candidates":[{"type":1,"word":"普通の候補"},
             {"type":9,"word":"(^^)"},{"type":10,"word":"(^^)"},
             {"type":10,"word":"(・ω・)"},{"type":9,"word":"\n"},
             {"type":9,"word":123}]}
        ]}"#;
        assert_eq!(parse("かお", body.as_bytes()), ["(^^)", "(・ω・)"]);
    }
}
