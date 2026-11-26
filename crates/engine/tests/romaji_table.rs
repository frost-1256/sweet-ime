//! The user's romaji table replaces the built-in one for the whole process.
//! Kept in its own test binary because the table and the settings directory
//! are process-wide.

use sweet_engine::romaji::{self, Composer, Rule};

/// A fresh directory, removed even when an assertion fails.
struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("sweet-ime-{name}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn user_table_applies_to_composers_until_reset() {
    let mut rules = romaji::default_rules();
    rules.push(Rule::new("t'i", "てぃ", ""));
    rules.push(Rule::new("ca", "カ", ""));
    romaji::set_rules(Some(&rules));

    assert_eq!(romaji::to_hiragana("pa-t'i-"), "ぱーてぃー");
    let mut c = Composer::new();
    c.push_str("ca");
    assert_eq!(c.preedit(), "カ");

    romaji::set_rules(None);
    assert_eq!(romaji::to_hiragana("ca"), "cあ");
}

#[test]
fn reading_a_table_file_distinguishes_missing_empty_and_too_large() {
    use sweet_engine::paths::read_romaji_table;

    let dir = TempDir::new("romaji-read");
    let path = dir.0.join("romaji.tsv");

    assert!(read_romaji_table(&path).unwrap().is_none(), "missing");
    std::fs::write(&path, "no tabs\n").unwrap();
    assert_eq!(
        read_romaji_table(&path).unwrap(),
        Some((vec![], 0)),
        "no rows"
    );
    std::fs::write(&path, "ca\tか\n\tえ\n").unwrap();
    assert_eq!(
        read_romaji_table(&path).unwrap(),
        Some((vec![Rule::new("ca", "か", "")], 1))
    );
    // NUL bytes without a byte order mark could be UTF-16 or UTF-8: an error.
    std::fs::write(&path, "aa\t\0\tい\nka\tか\n").unwrap();
    assert!(read_romaji_table(&path).is_err(), "ambiguous encoding");
    let utf16: Vec<u8> = "ca\tか\n"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    std::fs::write(&path, utf16).unwrap();
    assert!(read_romaji_table(&path).is_err(), "UTF-16 without a BOM");
    // Exactly at the limit is read; one byte more is not.
    let mut full = "ca\tか\n".repeat(romaji::MAX_TABLE_BYTES as usize / 7);
    full.push_str(&"\n".repeat(romaji::MAX_TABLE_BYTES as usize - full.len()));
    std::fs::write(&path, &full).unwrap();
    assert!(read_romaji_table(&path).is_ok(), "at the limit");
    full.push('\n');
    std::fs::write(&path, &full).unwrap();
    assert!(read_romaji_table(&path).is_err(), "larger than the limit");
}

#[test]
fn saving_a_table_over_the_limit_keeps_the_saved_one() {
    use sweet_engine::paths::{load_romaji_rules, romaji_table_file, save_romaji_rules};

    let dir = TempDir::new("romaji-save");
    // Point the settings directory into the temporary one on every platform.
    unsafe {
        std::env::set_var("APPDATA", &dir.0);
        std::env::set_var("XDG_CONFIG_HOME", &dir.0);
        std::env::set_var("HOME", &dir.0);
    }
    assert!(romaji_table_file().unwrap().starts_with(&dir.0));

    let small = vec![Rule::new("ca", "か", "")];
    save_romaji_rules(&small).unwrap();
    // Valid rows whose combined UTF-8 size exceeds MAX_TABLE_BYTES.
    let row = Rule::new("ka", &"漢".repeat(romaji::MAX_OUTPUT_CHARS), "");
    let row_bytes = romaji::format_rules(std::slice::from_ref(&row)).len();
    let large = vec![row; (romaji::MAX_TABLE_BYTES as usize / row_bytes) + 1];
    assert!(save_romaji_rules(&large).is_err());
    assert_eq!(load_romaji_rules().unwrap(), Some(small));
    let leftovers: Vec<_> = std::fs::read_dir(romaji_table_file().unwrap().parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(leftovers, ["romaji.tsv"], "no temporary file is left");
}
