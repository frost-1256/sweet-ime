//! The user's romaji table replaces the built-in one for the whole process.
//! Kept in its own test binary because the table is process-wide.

use sweet_engine::romaji::{self, Composer, Rule};

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

    /// Removes the directory even when an assertion fails.
    struct TempDir(std::path::PathBuf);
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = TempDir(
        std::env::temp_dir().join(format!("sweet-ime-romaji-{}-{nanos}", std::process::id())),
    );
    std::fs::create_dir_all(&dir.0).unwrap();
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
    // Exactly at the limit is read; one byte more is not.
    let mut full = "ca\tか\n".repeat(romaji::MAX_TABLE_BYTES as usize / 7);
    full.push_str(&"\n".repeat(romaji::MAX_TABLE_BYTES as usize - full.len()));
    std::fs::write(&path, &full).unwrap();
    assert!(read_romaji_table(&path).is_ok(), "at the limit");
    full.push('\n');
    std::fs::write(&path, &full).unwrap();
    assert!(read_romaji_table(&path).is_err(), "larger than the limit");
}
