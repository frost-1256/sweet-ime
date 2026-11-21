//! User learning: remembers the candidates the user picked and prefers them
//! in later conversions. Persisted as TSV so every process using the IME
//! shares it (reloaded when the file changes).
//!
//! Line format: `reading \t surface \t lid \t rid \t count \t last_used_unix`

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Maximum number of remembered entries (least recently used are dropped).
const MAX_ENTRIES: usize = 20_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Learned {
    pub surface: String,
    pub lid: u16,
    pub rid: u16,
    pub count: u32,
    pub last: u64,
}

#[derive(Default)]
pub struct UserHistory {
    entries: HashMap<String, Vec<Learned>>,
    max_reading_len: usize,
    path: Option<PathBuf>,
    mtime: Option<SystemTime>,
}

/// Milliseconds since the Unix epoch.
fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

impl UserHistory {
    /// In-memory history (tests, or when no storage is available).
    pub fn new() -> Self {
        Self::default()
    }

    /// History persisted at `path` (created on first write).
    pub fn with_file(path: impl Into<PathBuf>) -> Self {
        let mut h = Self { path: Some(path.into()), ..Self::default() };
        h.reload_if_changed();
        h
    }

    pub fn len(&self) -> usize {
        self.entries.values().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn file_mtime(path: &Path) -> Option<SystemTime> {
        std::fs::metadata(path).and_then(|m| m.modified()).ok()
    }

    /// Re-reads the file if another process changed it.
    pub fn reload_if_changed(&mut self) {
        let Some(path) = self.path.clone() else { return };
        let mtime = Self::file_mtime(&path);
        if mtime.is_none() || mtime == self.mtime {
            return;
        }
        if let Ok(text) = std::fs::read_to_string(&path) {
            self.entries.clear();
            self.max_reading_len = 0;
            for line in text.lines() {
                let f: Vec<&str> = line.split('\t').collect();
                if f.len() < 6 {
                    continue;
                }
                let (Ok(lid), Ok(rid), Ok(count), Ok(last)) =
                    (f[2].parse(), f[3].parse(), f[4].parse(), f[5].parse())
                else {
                    continue;
                };
                self.insert(f[0], Learned { surface: f[1].to_string(), lid, rid, count, last });
            }
            self.mtime = mtime;
        }
    }

    fn insert(&mut self, reading: &str, e: Learned) {
        self.max_reading_len = self.max_reading_len.max(reading.len());
        let v = self.entries.entry(reading.to_string()).or_default();
        v.retain(|x| x.surface != e.surface);
        // Newest first; on equal timestamps the just-inserted entry wins
        // because the sort is stable.
        v.insert(0, e);
        v.sort_by(|a, b| b.last.cmp(&a.last));
    }

    /// Records that the user chose `surface` for `reading`.
    pub fn learn(&mut self, reading: &str, surface: &str, lid: u16, rid: u16) {
        if reading.is_empty() || surface.is_empty() || reading.contains(['\t', '\n']) || surface.contains(['\t', '\n']) {
            return;
        }
        self.reload_if_changed();
        let count = self
            .entries
            .get(reading)
            .and_then(|v| v.iter().find(|x| x.surface == surface))
            .map_or(0, |x| x.count);
        self.insert(reading, Learned { surface: surface.to_string(), lid, rid, count: count.saturating_add(1), last: now() });
        self.evict();
        if let Err(_e) = self.save() {
            // Learning is best-effort; conversion keeps working without it.
        }
    }

    /// Forgets a learned surface (e.g. the user deleted a wrong suggestion).
    pub fn forget(&mut self, reading: &str, surface: &str) {
        self.reload_if_changed();
        if let Some(v) = self.entries.get_mut(reading) {
            v.retain(|x| x.surface != surface);
            if v.is_empty() {
                self.entries.remove(reading);
            }
            let _ = self.save();
        }
    }

    fn evict(&mut self) {
        let n = self.len();
        if n <= MAX_ENTRIES {
            return;
        }
        let mut all: Vec<u64> = self.entries.values().flatten().map(|e| e.last).collect();
        all.sort_unstable();
        let cutoff = all[n - MAX_ENTRIES];
        for v in self.entries.values_mut() {
            v.retain(|e| e.last >= cutoff);
        }
        self.entries.retain(|_, v| !v.is_empty());
    }

    fn save(&mut self) -> io::Result<()> {
        let Some(path) = self.path.clone() else { return Ok(()) };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension(format!("tmp{}", std::process::id()));
        {
            let mut f = io::BufWriter::new(std::fs::File::create(&tmp)?);
            for (reading, v) in &self.entries {
                for e in v {
                    writeln!(f, "{reading}\t{}\t{}\t{}\t{}\t{}", e.surface, e.lid, e.rid, e.count, e.last)?;
                }
            }
            f.flush()?;
        }
        std::fs::rename(&tmp, &path)?;
        self.mtime = Self::file_mtime(&path);
        Ok(())
    }

    /// Every learned entry as (reading, entry), most recent first.
    pub fn all(&mut self) -> Vec<(String, Learned)> {
        self.reload_if_changed();
        let mut v: Vec<(String, Learned)> = self
            .entries
            .iter()
            .flat_map(|(r, es)| es.iter().map(move |e| (r.clone(), e.clone())))
            .collect();
        v.sort_by(|a, b| b.1.last.cmp(&a.1.last));
        v
    }

    /// Forgets everything.
    pub fn clear(&mut self) -> io::Result<()> {
        self.entries.clear();
        self.max_reading_len = 0;
        self.save()
    }

    /// Learned surfaces for exactly `reading`, most recent first.
    pub fn lookup(&self, reading: &str) -> &[Learned] {
        self.entries.get(reading).map_or(&[], Vec::as_slice)
    }

    /// Learned entries whose reading strictly extends `prefix`, most recent first.
    pub fn predictive(&self, prefix: &str) -> Vec<(&str, &Learned)> {
        let mut v: Vec<(&str, &Learned)> = self
            .entries
            .iter()
            .filter(|(r, _)| r.len() > prefix.len() && r.starts_with(prefix))
            .flat_map(|(r, es)| es.iter().map(move |e| (r.as_str(), e)))
            .collect();
        v.sort_by(|a, b| b.1.last.cmp(&a.1.last));
        v
    }

    /// Calls `f(len_bytes, entry)` for learned readings that are a prefix of `text`.
    pub fn common_prefix(&self, text: &str, mut f: impl FnMut(usize, &Learned)) {
        if self.entries.is_empty() {
            return;
        }
        for (i, c) in text.char_indices() {
            let end = i + c.len_utf8();
            if end > self.max_reading_len {
                break;
            }
            if let Some(v) = self.entries.get(&text[..end]) {
                for e in v {
                    f(end, e);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learn_persist_reload() {
        let dir = std::env::temp_dir().join(format!("sweet-ime-test-{}", std::process::id()));
        let path = dir.join("history.tsv");
        let _ = std::fs::remove_file(&path);
        let mut h = UserHistory::with_file(&path);
        h.learn("きしゃ", "汽車", 1, 1);
        h.learn("きしゃ", "汽車", 1, 1);
        h.learn("きしゃ", "帰社", 2, 2);
        assert_eq!(h.lookup("きしゃ")[0].surface, "帰社");
        let h2 = UserHistory::with_file(&path);
        let v = h2.lookup("きしゃ");
        assert_eq!(v.len(), 2);
        assert_eq!(v.iter().find(|e| e.surface == "汽車").unwrap().count, 2);
        let mut seen = Vec::new();
        h2.common_prefix("きしゃのきしゃ", |l, e| seen.push((l, e.surface.clone())));
        assert_eq!(seen.len(), 2);
        assert!(seen.iter().all(|(l, _)| *l == "きしゃ".len()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
