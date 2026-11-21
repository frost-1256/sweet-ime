//! Session behaviour against the real system dictionary.
//! Skipped when data/system.dict has not been built.

use sweet_engine::{Converter, Dictionary, Key, Session, SpanAttr};

fn converter() -> Option<Converter> {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/system.dict");
    Dictionary::open(p).ok().map(Converter::new)
}

fn type_str(s: &mut Session, c: &Converter, text: &str) {
    for ch in text.chars() {
        assert!(s.handle(c, Key::Char(ch)));
    }
}

#[test]
fn compose_convert_commit() {
    let Some(c) = converter() else { return };
    let mut s = Session::new();
    type_str(&mut s, &c, "kyouhaiitenkidesune");
    assert_eq!(s.preedit().text, "きょうはいいてんきですね");
    s.handle(&c, Key::Space);
    let p = s.preedit();
    assert_eq!(p.text, "今日はいい天気ですね");
    assert!(p.spans.len() >= 2);
    assert_eq!(p.spans[0].2, SpanAttr::Focused);
    assert!(s.candidates().is_none());
    s.handle(&c, Key::Enter);
    assert_eq!(s.take_commit(), "今日はいい天気ですね");
    assert!(!s.is_active());
}

#[test]
fn candidate_window_and_selection() {
    let Some(c) = converter() else { return };
    let mut s = Session::new();
    type_str(&mut s, &c, "kisha");
    s.handle(&c, Key::Space);
    s.handle(&c, Key::Space);
    let list = s.candidates().expect("window open");
    assert!(list.items.len() > 3, "{:?}", list.items);
    assert_eq!(list.selected, 1);
    s.handle(&c, Key::Char('3'));
    let committed = s.take_commit();
    assert_eq!(committed, list.items[2]);
}

#[test]
fn resize_segment() {
    let Some(c) = converter() else { return };
    let mut s = Session::new();
    type_str(&mut s, &c, "watashihanihonjindesu");
    s.handle(&c, Key::Space);
    let before = s.preedit().spans.len();
    s.handle(&c, Key::ShrinkSegment);
    let p = s.preedit();
    assert!(p.spans.len() >= before, "{p:?}");
    s.handle(&c, Key::ExtendSegment);
    s.handle(&c, Key::Enter);
    assert!(!s.take_commit().is_empty());
}

#[test]
fn caret_editing_and_escape() {
    let Some(c) = converter() else { return };
    let mut s = Session::new();
    type_str(&mut s, &c, "kaki");
    s.handle(&c, Key::Left);
    type_str(&mut s, &c, "n");
    s.handle(&c, Key::End);
    assert_eq!(s.preedit().text, "かんき");
    s.handle(&c, Key::Space);
    s.handle(&c, Key::Escape);
    assert_eq!(s.preedit().text, "かんき");
    s.handle(&c, Key::Escape);
    assert!(!s.is_active());
}

#[test]
fn transliterate_keys() {
    let Some(c) = converter() else { return };
    let mut s = Session::new();
    type_str(&mut s, &c, "ko-hi-");
    s.handle(&c, Key::ToKatakana);
    assert_eq!(s.preedit().text, "コーヒー");
    s.handle(&c, Key::ToHalfAscii);
    assert_eq!(s.preedit().text, "ko-hi-");
    s.handle(&c, Key::Enter);
    assert_eq!(s.take_commit(), "ko-hi-");
}

#[test]
fn learns_selected_candidate() {
    let Some(c) = converter() else { return };
    let mut s = Session::new();
    type_str(&mut s, &c, "kisha");
    s.handle(&c, Key::Space);
    let first = s.preedit().text;
    s.handle(&c, Key::Space); // open window, select 2nd
    let second = s.candidates().unwrap().items[1].clone();
    assert_ne!(first, second);
    s.handle(&c, Key::Enter);
    assert_eq!(s.take_commit(), second);

    // Now the learned word wins, alone and inside a sentence.
    type_str(&mut s, &c, "kisha");
    s.handle(&c, Key::Space);
    assert_eq!(s.preedit().text, second);
    s.handle(&c, Key::Enter);
    s.take_commit();
    type_str(&mut s, &c, "ashitakishaninarimasu");
    s.handle(&c, Key::Space);
    assert!(s.preedit().text.contains(&second), "{}", s.preedit().text);
}
