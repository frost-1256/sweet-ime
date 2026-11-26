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
