//! User dictionary: words the user registered by hand. Unlike the learning
//! history these are kept until the user deletes them, and they carry a part
//! of speech so they connect to the surrounding words.
//!
//! Persisted as TSV, one word per line: `reading \t word \t pos \t comment`.
//! This is also the export format of the Mozc and Google Japanese Input user
//! dictionary tools, so their files can be imported as is. Lines starting
//! with `#` or `!` (Microsoft IME headers) are ignored.

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Parts of speech offered for user words: (label, id.def feature). The
/// labels follow Mozc's user dictionary so imported files keep their POS.
pub const POS: &[(&str, &str)] = &[
    ("名詞", "名詞,一般,*,*,*,*,*"),
    ("固有名詞", "名詞,固有名詞,一般,*,*,*,*"),
    ("人名", "名詞,固有名詞,人名,一般,*,*,*"),
    ("姓", "名詞,固有名詞,人名,姓,*,*,*"),
    ("名", "名詞,固有名詞,人名,名,*,*,*"),
    ("地名", "名詞,固有名詞,地域,一般,*,*,*"),
    ("組織", "名詞,固有名詞,組織,*,*,*,*"),
    ("名詞サ変", "名詞,サ変接続,*,*,*,*,*"),
    ("形容動詞", "名詞,形容動詞語幹,*,*,*,*,*"),
    ("感動詞", "感動詞,*,*,*,*,*,*"),
    ("記号", "記号,一般,*,*,*,*,*"),
    ("顔文字", "記号,一般,*,*,*,*,*"),
    ("短縮よみ", "名詞,一般,*,*,*,*,*"),
    ("サジェストのみ", ""),
];

/// The default part of speech.
pub const DEFAULT_POS: &str = "名詞";

/// Part-of-speech labels used by other IMEs, mapped to ours.
fn canonical_pos(label: &str) -> &'static str {
    let label = label.trim();
    if let Some(&(l, _)) = POS.iter().find(|(l, _)| *l == label) {
        return l;
    }
    match label {
        "人名姓" | "姓のみ" => "姓",
        "人名名" | "名のみ" => "名",
        "地名その他" | "地域" => "地名",
        "組織名" | "会社名" => "組織",
        "さ変名詞" | "サ変名詞" | "名詞サ変接続" => "名詞サ変",
        "形動名詞" | "形容動詞語幹" => "形容動詞",
        "感動詞" | "間投詞" => "感動詞",
        "抑制単語" | "サジェストのみ" => "サジェストのみ",
        _ => DEFAULT_POS,
    }
}

/// The id.def feature for a POS label, or `None` for words that are only
/// suggested (never placed in the conversion lattice).
pub fn pos_feature(label: &str) -> Option<&'static str> {
    let label = canonical_pos(label);
    POS.iter()
        .find(|(l, _)| *l == label)
        .map(|(_, f)| *f)
        .filter(|f| !f.is_empty())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserWord {
    pub reading: String,
    pub surface: String,
    pub pos: String,
    pub comment: String,
}

/// Why a word could not be added.
#[derive(Debug, PartialEq, Eq)]
pub enum AddError {
    EmptyReading,
    EmptySurface,
    /// The reading has characters other than kana, digits or symbols typed
    /// from the keyboard.
    BadReading,
    /// Tabs and newlines cannot be stored.
    BadCharacter,
    Duplicate,
    SaveFailed,
}

impl std::fmt::Display for AddError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            AddError::EmptyReading => "読みを入力してください",
            AddError::EmptySurface => "単語を入力してください",
            AddError::BadReading => "読みはひらがなで入力してください",
            AddError::BadCharacter => "タブや改行は使えません",
            AddError::Duplicate => "同じ読みと単語がすでに登録されています",
            AddError::SaveFailed => "ユーザー辞書を保存できませんでした",
        })
    }
}

/// Normalizes a reading as typed: katakana becomes hiragana, and full-width
/// ASCII becomes half-width (the converter sees half-width ASCII).
pub fn normalize_reading(reading: &str) -> String {
    reading
        .trim()
        .chars()
        .map(|c| match c {
            'ァ'..='ヶ' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
            '！'..='～' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            '\u{3000}' => ' ',
            _ => c,
        })
        .collect()
}

fn valid_reading(reading: &str) -> bool {
    reading.chars().all(|c| {
        matches!(
            c,
            'ぁ'..='ゖ' | 'ー' | 'ゝ' | 'ゞ' | '、' | '。' | '「' | '」' | '・' | '〜'
        ) || c.is_ascii_graphic()
    })
}

#[derive(Default)]
pub struct UserDictionary {
    words: Vec<UserWord>,
    /// reading -> indices into `words`.
    index: HashMap<String, Vec<usize>>,
    max_reading_len: usize,
    path: Option<PathBuf>,
    mtime: Option<SystemTime>,
}

impl UserDictionary {
    /// An empty dictionary kept in memory (tests, or no storage).
    pub fn new() -> Self {
        Self::default()
    }

    /// The dictionary stored at `path` (created on first save).
    pub fn with_file(path: impl Into<PathBuf>) -> Self {
        let mut d = Self {
            path: Some(path.into()),
            ..Self::default()
        };
        d.reload_if_changed();
        d
    }

    pub fn len(&self) -> usize {
        self.words.len()
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// All words in file order.
    pub fn words(&self) -> &[UserWord] {
        &self.words
    }

    fn file_mtime(path: &Path) -> Option<SystemTime> {
        std::fs::metadata(path).and_then(|m| m.modified()).ok()
    }

    /// Re-reads the file if it changed (another process, or a hand edit).
    pub fn reload_if_changed(&mut self) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let mtime = Self::file_mtime(&path);
        if mtime == self.mtime {
            return;
        }
        let text = if mtime.is_some() {
            std::fs::read_to_string(&path).ok()
        } else {
            Some(String::new())
        };
        if let Some(text) = text {
            self.words.clear();
            self.parse_into(&text);
            self.rebuild_index();
            self.mtime = mtime;
        }
    }

    /// Parses TSV lines, skipping comments, malformed rows and duplicates.
    /// Returns how many words were added.
    fn parse_into(&mut self, text: &str) -> usize {
        let mut added = 0;
        for line in text.lines() {
            let line = line.trim_start_matches('\u{feff}');
            if line.trim().is_empty() || line.starts_with('#') || line.starts_with('!') {
                continue;
            }
            let mut f = line.split('\t');
            let (Some(reading), Some(surface)) = (f.next(), f.next()) else {
                continue;
            };
            let pos = f.next().unwrap_or(DEFAULT_POS);
            let comment = f.next().unwrap_or("");
            let word = UserWord {
                reading: normalize_reading(reading),
                surface: surface.trim().to_string(),
                pos: canonical_pos(pos).to_string(),
                comment: comment.trim().to_string(),
            };
            if Self::check(&word).is_ok() && !self.contains(&word.reading, &word.surface) {
                self.words.push(word);
                added += 1;
            }
        }
        added
    }

    fn contains(&self, reading: &str, surface: &str) -> bool {
        self.words
            .iter()
            .any(|w| w.reading == reading && w.surface == surface)
    }

    fn rebuild_index(&mut self) {
        self.index.clear();
        self.max_reading_len = 0;
        for (i, w) in self.words.iter().enumerate() {
            self.max_reading_len = self.max_reading_len.max(w.reading.len());
            self.index.entry(w.reading.clone()).or_default().push(i);
        }
    }

    fn check(w: &UserWord) -> Result<(), AddError> {
        if w.reading.is_empty() {
            return Err(AddError::EmptyReading);
        }
        if w.surface.is_empty() {
            return Err(AddError::EmptySurface);
        }
        let bad = |s: &str| s.contains(['\t', '\n', '\r']);
        if bad(&w.reading) || bad(&w.surface) || bad(&w.comment) {
            return Err(AddError::BadCharacter);
        }
        if !valid_reading(&w.reading) {
            return Err(AddError::BadReading);
        }
        Ok(())
    }

    /// Adds a word and saves. The reading is normalized first.
    pub fn add(
        &mut self,
        reading: &str,
        surface: &str,
        pos: &str,
        comment: &str,
    ) -> Result<(), AddError> {
        self.reload_if_changed();
        let word = UserWord {
            reading: normalize_reading(reading),
            surface: surface.trim().to_string(),
            pos: canonical_pos(pos).to_string(),
            comment: comment.trim().to_string(),
        };
        Self::check(&word)?;
        if self.contains(&word.reading, &word.surface) {
            return Err(AddError::Duplicate);
        }
        self.words.push(word);
        self.rebuild_index();
        if self.save().is_err() {
            self.words.pop();
            self.rebuild_index();
            return Err(AddError::SaveFailed);
        }
        Ok(())
    }

    /// Replaces the word at `index` (as in [`UserDictionary::words`]).
    pub fn replace(
        &mut self,
        index: usize,
        reading: &str,
        surface: &str,
        pos: &str,
        comment: &str,
    ) -> Result<(), AddError> {
        let word = UserWord {
            reading: normalize_reading(reading),
            surface: surface.trim().to_string(),
            pos: canonical_pos(pos).to_string(),
            comment: comment.trim().to_string(),
        };
        Self::check(&word)?;
        if self
            .words
            .iter()
            .enumerate()
            .any(|(i, w)| i != index && w.reading == word.reading && w.surface == word.surface)
        {
            return Err(AddError::Duplicate);
        }
        let Some(slot) = self.words.get_mut(index) else {
            return Ok(());
        };
        let original = std::mem::replace(slot, word);
        self.rebuild_index();
        if self.save().is_err() {
            self.words[index] = original;
            self.rebuild_index();
            return Err(AddError::SaveFailed);
        }
        Ok(())
    }

    /// Removes the word at `index` and saves.
    pub fn remove(&mut self, index: usize) -> io::Result<()> {
        if index < self.words.len() {
            let word = self.words.remove(index);
            self.rebuild_index();
            if let Err(error) = self.save() {
                self.words.insert(index, word);
                self.rebuild_index();
                return Err(error);
            }
        }
        Ok(())
    }

    /// Adds every valid word in `text` (TSV as written by [`Self::export`],
    /// Mozc, Google Japanese Input or Microsoft IME) and saves. Returns how
    /// many were new.
    pub fn import(&mut self, text: &str) -> io::Result<usize> {
        self.reload_if_changed();
        let n = self.parse_into(text);
        if n > 0 {
            self.rebuild_index();
            self.save()?;
        }
        Ok(n)
    }

    /// The dictionary as TSV.
    pub fn export(&self) -> String {
        let mut s =
            String::from("# Sweet IME user dictionary: reading\tword\tpart of speech\tcomment\n");
        for w in &self.words {
            s.push_str(&format!(
                "{}\t{}\t{}\t{}\n",
                w.reading, w.surface, w.pos, w.comment
            ));
        }
        s
    }

    fn save(&mut self) -> io::Result<()> {
        let Some(path) = self.path.clone() else {
            return Ok(());
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension(format!("tmp{}", std::process::id()));
        {
            let mut f = io::BufWriter::new(std::fs::File::create(&tmp)?);
            f.write_all(self.export().as_bytes())?;
            f.flush()?;
        }
        std::fs::rename(&tmp, &path)?;
        self.mtime = Self::file_mtime(&path);
        Ok(())
    }

    /// Words with exactly `reading`.
    pub fn lookup(&self, reading: &str) -> impl Iterator<Item = &UserWord> {
        self.index
            .get(reading)
            .into_iter()
            .flatten()
            .map(|&i| &self.words[i])
    }

    /// Calls `f(len_bytes, word)` for words whose reading is a prefix of `text`.
    pub fn common_prefix(&self, text: &str, mut f: impl FnMut(usize, &UserWord)) {
        if self.words.is_empty() {
            return;
        }
        for (i, c) in text.char_indices() {
            let end = i + c.len_utf8();
            if end > self.max_reading_len {
                break;
            }
            for w in self.lookup(&text[..end]) {
                f(end, w);
            }
        }
    }

    /// Words whose reading starts with `prefix` (including equal), shortest
    /// reading first.
    pub fn predictive(&self, prefix: &str) -> Vec<&UserWord> {
        let mut v: Vec<&UserWord> = self
            .words
            .iter()
            .filter(|w| w.reading.starts_with(prefix))
            .collect();
        v.sort_by_key(|w| w.reading.len());
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_save_reload_and_import() {
        let dir = std::env::temp_dir().join(format!("sweet-ime-userdict-{}", std::process::id()));
        let path = dir.join("user_dict.tsv");
        let _ = std::fs::remove_dir_all(&dir);
        let mut d = UserDictionary::with_file(&path);
        assert!(d.add("スイート", "Sweet", "固有名詞", "").is_ok());
        assert_eq!(
            d.add("すいーと", "Sweet", "名詞", ""),
            Err(AddError::Duplicate)
        );
        assert_eq!(d.add("abc漢字", "x", "名詞", ""), Err(AddError::BadReading));
        assert_eq!(d.add("", "x", "名詞", ""), Err(AddError::EmptyReading));
        let d2 = UserDictionary::with_file(&path);
        assert_eq!(d2.words()[0].reading, "すいーと");
        assert_eq!(d2.words()[0].pos, "固有名詞");
        let mut d3 = UserDictionary::with_file(&path);
        let n = d3
            .import("!Microsoft IME Dictionary Tool\nかおもじ\t(^^)\t顔文字\t\nたなか\t田中\t人名姓\nすいーと\tSweet\t名詞\t\n")
            .unwrap();
        assert_eq!(n, 2);
        assert_eq!(d3.lookup("たなか").next().unwrap().pos, "姓");
        let mut seen = Vec::new();
        d3.common_prefix("たなかさん", |l, w| seen.push((l, w.surface.clone())));
        assert_eq!(seen, vec![("たなか".len(), "田中".to_string())]);
        d3.replace(0, "スイート", "Sweet IME", "固有名詞", "").unwrap();
        assert_eq!(
            UserDictionary::with_file(&path).words()[0].surface,
            "Sweet IME"
        );
        d3.remove(0).unwrap();
        assert_eq!(UserDictionary::with_file(&path).len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pos_labels_map_to_features() {
        assert_eq!(pos_feature("人名"), Some("名詞,固有名詞,人名,一般,*,*,*"));
        assert_eq!(pos_feature("さ変名詞"), Some("名詞,サ変接続,*,*,*,*,*"));
        assert_eq!(pos_feature("サジェストのみ"), None);
        assert_eq!(pos_feature("???"), Some("名詞,一般,*,*,*,*,*"));
    }
}
