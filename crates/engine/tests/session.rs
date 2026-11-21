//! Session behaviour against the real system dictionary.
//! Skipped when data/system.dict has not been built.

use sweet_engine::{Converter, Dictionary, Key, ListKind, Options, Session, SpanAttr};

/// Session with live conversion and suggestions off (classic behaviour).
fn plain() -> Session {
    let mut s = Session::new();
    s.options = Options { live_conversion: false, suggestions: false, ..Options::default() };
    s
}

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
fn shift_starts_english_until_space_or_enter() {
    let Some(c) = converter() else { return };
    let mut s = Session::new();
    type_str(&mut s, &c, "kana");
    assert!(s.handle(&c, Key::ShiftedLetter('G')));
    assert_eq!(s.take_commit(), "かな");
    type_str(&mut s, &c, "it");
    assert!(s.handle(&c, Key::ShiftedLetter('H')));
    type_str(&mut s, &c, "ub");
    assert_eq!(s.preedit().text, "GitHub");
    assert!(s.candidates().is_none());
    assert!(s.live_request().is_none());
    assert!(s.handle(&c, Key::Space));
    assert_eq!(s.take_commit(), "GitHub ");
    assert!(!s.is_active());

    type_str(&mut s, &c, "kana");
    assert_eq!(s.preedit().text, "かな");
    assert!(s.handle(&c, Key::Escape));
    assert!(s.handle(&c, Key::ShiftedLetter('I')));
    assert!(s.handle(&c, Key::ShiftedLetter('M')));
    assert!(s.handle(&c, Key::ShiftedLetter('E')));
    assert!(s.handle(&c, Key::Enter));
    assert_eq!(s.take_commit(), "かなIME");
    assert!(!s.is_active());
}

#[test]
fn temporary_english_supports_editing_and_cancellation() {
    let Some(c) = converter() else { return };
    let mut s = plain();
    assert!(s.handle(&c, Key::ShiftedLetter('T')));
    type_str(&mut s, &c, "oo");
    assert!(s.handle(&c, Key::Left));
    assert!(s.handle(&c, Key::Delete));
    type_str(&mut s, &c, "ol");
    assert_eq!(s.preedit().text, "Tool");
    assert!(s.handle(&c, Key::Escape));
    assert_eq!(s.take_commit(), "");

    assert!(s.handle(&c, Key::ShiftedLetter('A')));
    assert!(s.handle(&c, Key::Backspace));
    assert!(!s.is_active());
    type_str(&mut s, &c, "a");
    assert_eq!(s.preedit().text, "あ");
}

#[test]
fn japanese_readings_convert_to_case_sensitive_technical_terms() {
    let Some(c) = converter() else { return };
    let mut s = plain();
    for (reading, spelling) in [("aiemui-", "IME"), ("gittohabu", "GitHub")] {
        type_str(&mut s, &c, reading);
        assert!(s.handle(&c, Key::Space));
        assert_eq!(s.preedit().text, spelling);
        assert!(s.handle(&c, Key::Enter));
        assert_eq!(s.take_commit(), spelling);
    }
}

#[test]
fn lowercase_technical_terms_convert_to_their_canonical_spelling() {
    let Some(c) = converter() else { return };
    let mut s = Session::new();
    for (typed, expected) in [("ime", "IME"), ("ieee", "IEEE"), ("github", "GitHub")] {
        type_str(&mut s, &c, typed);
        assert!(s.handle(&c, Key::Space));
        assert_eq!(s.preedit().text, expected);
        assert!(s.handle(&c, Key::Enter));
        assert_eq!(s.take_commit(), expected);
    }

    type_str(&mut s, &c, "ime-ji");
    assert!(s.handle(&c, Key::Space));
    assert_eq!(s.preedit().text, "イメージ");
}

#[test]
fn technical_spellings_offer_ordered_kana_candidates() {
    let Some(c) = converter() else { return };
    let mut s = Session::new();
    for (typed, expected) in [
        ("nginx", ["Nginx", "ﾝｷﾞｯｸｽ", "エンジンエックス"]),
        ("ieee", ["IEEE", "アイエエエ", "アイトリプルイー"]),
        (
            "kubectl",
            ["kubectl", "キューブコントロール", "キューブシーティーエル"],
        ),
        ("sql", ["SQL", "エスキューエル", "シークェル"]),
    ] {
        type_str(&mut s, &c, typed);
        assert!(s.handle(&c, Key::Space));
        assert_eq!(s.preedit().text, expected[0]);
        assert!(s.handle(&c, Key::Space));
        let list = s.candidates().expect("technical spelling candidates");
        assert_eq!(list.items[..expected.len()], expected, "{typed}");
        assert_eq!(s.preedit().text, expected[1]);
        assert!(s.handle(&c, Key::Enter));
        assert_eq!(s.take_commit(), expected[1]);
    }
}

#[test]
fn technical_spellings_convert_from_kana_aliases() {
    let Some(c) = converter() else { return };
    let mut s = plain();
    for (typed, expected) in [
        ("ngikkusu", "Nginx"),
        ("ンギックス", "Nginx"),
        ("ﾝｷﾞｯｸｽ", "Nginx"),
        ("ンギンクス", "Nginx"),
        ("ennjinnekkusu", "Nginx"),
        ("aieee", "IEEE"),
        ("アイエエエ", "IEEE"),
        ("しーくぇる", "SQL"),
        ("ぽすぐれ", "PostgreSQL"),
        ("クベコン", "kubectl"),
    ] {
        type_str(&mut s, &c, typed);
        assert!(s.handle(&c, Key::Space));
        assert_eq!(s.preedit().text, expected, "{typed}");
        assert!(s.handle(&c, Key::Enter));
        assert_eq!(s.take_commit(), expected);
    }

    for (key, kana) in [
        (Key::ToKatakana, "ンギックス"),
        (Key::ToHalfKatakana, "ﾝｷﾞｯｸｽ"),
    ] {
        type_str(&mut s, &c, "ngikkusu");
        assert!(s.handle(&c, key));
        assert_eq!(s.preedit().text, kana);
        assert!(s.handle(&c, Key::Space));
        assert_eq!(s.preedit().text, "Nginx");
        assert!(s.handle(&c, Key::Enter));
        assert_eq!(s.take_commit(), "Nginx");
    }
}

#[test]
fn standalone_interjections_stay_in_hiragana_until_explicit_conversion() {
    let Some(c) = converter() else { return };
    let mut s = Session::new();
    for (typed, expected) in [
        ("oo", "おお"),
        ("oo,", "おお、"),
        ("oo!", "おお！"),
        ("oo-", "おおー"),
        ("aa", "ああ"),
    ] {
        type_str(&mut s, &c, typed);
        assert_eq!(s.preedit().text, expected);
        assert!(s.live_request().is_none());
        assert!(s.handle(&c, Key::Enter));
        assert_eq!(s.take_commit(), expected);
    }

    type_str(&mut s, &c, "oo");
    assert!(s.handle(&c, Key::Space));
    assert!(s.handle(&c, Key::Space));
    let list = s.candidates().expect("explicit conversion candidates");
    assert!(list.items.iter().any(|item| item == "大"));
    assert!(s.select_candidate(
        &c,
        list.items.iter().position(|item| item == "おお").unwrap()
    ));
    assert_eq!(s.take_commit(), "おお");

    type_str(&mut s, &c, "ookii");
    assert_eq!(s.preedit().text, "大きい");
}

#[test]
fn vrchat_terms_convert_from_short_readings_and_ascii() {
    let Some(c) = converter() else { return };
    let mut s = Session::new();
    for (typed, expected) in [
        ("funyaosu", "ふにゃオス"),
        ("fizubo", "PhysBone"),
        ("mojuaba", "Modular Avatar"),
        ("abaopu", "Avatar Optimizer"),
        ("physbone", "PhysBone"),
        ("vrchat", "VRChat"),
        ("liltoon", "lilToon"),
    ] {
        type_str(&mut s, &c, typed);
        assert!(s.handle(&c, Key::Space));
        assert_eq!(s.preedit().text, expected, "{typed}");
        assert!(s.handle(&c, Key::Enter));
        assert_eq!(s.take_commit(), expected);
    }

    type_str(&mut s, &c, "fizubowoireru");
    assert!(s.handle(&c, Key::Space));
    assert!(s.preedit().text.starts_with("PhysBoneを"));
    assert!(s.handle(&c, Key::Enter));
    assert!(s.take_commit().starts_with("PhysBoneを"));

    type_str(&mut s, &c, "buicha");
    assert!(s.handle(&c, Key::Space));
    assert!(s.handle(&c, Key::Space));
    let list = s.candidates().expect("VRChat conversion candidates");
    assert!(list.items.iter().any(|item| item == "VRChat"));
    assert!(list.items.iter().any(|item| item == "ぶいちゃ"));
}

#[test]
fn net_slang_converts_from_meme_readings() {
    let Some(c) = converter() else { return };
    let mut s = plain();
    for (typed, expected) in [
        ("yarimasunexe", "やりますねぇ！"),
        ("soudayobinjou", "そうだよ（便乗）"),
        ("iizo-kore", "いいゾ～これ"),
        ("iiyokoiyo", "いいよ！こいよ！"),
        ("sui-toaiemui-", "Sweet IME"),
    ] {
        type_str(&mut s, &c, typed);
        assert!(s.handle(&c, Key::Space));
        assert_eq!(s.preedit().text, expected, "{typed}");
        assert!(s.handle(&c, Key::Enter));
        assert_eq!(s.take_commit(), expected);
    }

    // Ordinary readings keep their ordinary first candidate.
    for (typed, first) in [("iizokore", "いいぞこれ"), ("yarimasune", "やりますね"), ("hakusin", "迫真")] {
        type_str(&mut s, &c, typed);
        assert!(s.handle(&c, Key::Space));
        assert_eq!(s.preedit().text, first, "{typed}");
        s.reset();
    }

    // 男の子 stays first; 男の娘 is the next candidate.
    type_str(&mut s, &c, "otokonoko");
    assert!(s.handle(&c, Key::Space));
    assert_eq!(s.preedit().text, "男の子");
    assert!(s.handle(&c, Key::Space));
    assert_eq!(s.preedit().text, "男の娘");
    s.reset();

    type_str(&mut s, &c, "koko");
    assert!(s.handle(&c, Key::Space));
    assert!(s.handle(&c, Key::Space));
    let list = s.candidates().expect("ここ candidates");
    assert!(list.items.iter().any(|item| item == "こ↑こ↓"), "{:?}", list.items);
}

#[test]
fn compose_convert_commit() {
    let Some(c) = converter() else { return };
    let mut s = plain();
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
fn best_path_matches_the_first_nbest_candidate() {
    let Some(c) = converter() else { return };
    for (reading, boundaries) in [
        ("きしゃのきしゃ", &[][..]),
        ("きょうはいいてんきですね", &[9, 12][..]),
        ("ぶいちゃであそぶ", &[][..]),
        ("きのうabcのかいぎ", &[12][..]),
    ] {
        let best = c.convert(reading, boundaries, 1);
        let ranked = c.convert(reading, boundaries, 5);
        assert_eq!(best[0].cost, ranked[0].cost, "{reading}");
        assert_eq!(best[0].segments, ranked[0].segments, "{reading}");
    }
}

#[test]
fn candidate_window_and_selection() {
    let Some(c) = converter() else { return };
    let mut s = plain();
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
    let mut s = plain();
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
    let mut s = plain();
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
    let mut s = plain();
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
    let mut s = plain();
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

#[test]
fn live_conversion() {
    let Some(c) = converter() else { return };
    let mut s = Session::new();
    s.options.suggestions = false;
    type_str(&mut s, &c, "kyouhaiitenkidesune");
    assert_eq!(s.preedit().text, "今日はいい天気ですね");
    // Pending romaji is shown after the converted part.
    s.handle(&c, Key::Char('k'));
    assert_eq!(s.preedit().text, "今日はいい天気ですねk");
    s.handle(&c, Key::Backspace);
    // First Esc reveals the reading, the second cancels.
    s.handle(&c, Key::Escape);
    assert_eq!(s.preedit().text, "きょうはいいてんきですね");
    s.handle(&c, Key::Escape);
    assert!(!s.is_active());
    // Enter commits the live result.
    type_str(&mut s, &c, "kyouhaiitenkidesune");
    s.handle(&c, Key::Enter);
    assert_eq!(s.take_commit(), "今日はいい天気ですね");
}

#[test]
fn suggestions_tab_select() {
    let Some(c) = converter() else { return };
    let mut s = Session::new();
    s.options.live_conversion = false;
    type_str(&mut s, &c, "arigat");
    let list = s.candidates().expect("suggestions");
    assert_eq!(list.kind, ListKind::Suggestion);
    assert!(!list.items.is_empty());
    let first = list.items[0].clone();
    s.handle(&c, Key::Tab);
    let list = s.candidates().expect("conversion window");
    assert_eq!(list.kind, ListKind::Conversion);
    s.handle(&c, Key::Enter);
    assert_eq!(s.take_commit(), first);
}

#[test]
fn click_selects_candidate() {
    let Some(c) = converter() else { return };
    let mut s = plain();
    type_str(&mut s, &c, "kisha");
    s.handle(&c, Key::Space);
    s.handle(&c, Key::Space);
    let third = s.candidates().unwrap().items[2].clone();
    assert!(s.select_candidate(&c, 2));
    assert_eq!(s.take_commit(), third);
    assert!(!s.is_active());

    let mut s = Session::new();
    s.options.live_conversion = false;
    type_str(&mut s, &c, "arigat");
    let first = s.candidates().unwrap().items[0].clone();
    assert!(s.select_candidate(&c, 0));
    assert_eq!(s.take_commit(), first);
    assert!(!s.select_candidate(&c, 0));
}

#[test]
fn live_refinement_applies_only_to_current_reading() {
    let Some(c) = converter() else { return };
    let mut s = Session::new();
    s.options.suggestions = false;
    type_str(&mut s, &c, "kinounokaigi");
    let (reading, _) = s.live_request().expect("live");
    assert_eq!(reading, "きのうのかいぎ");
    assert!(!s.apply_live("ちがうよみ", "違う"));
    assert!(s.apply_live(&reading, "昨日の会議"));
    assert_eq!(s.preedit().text, "昨日の会議");
    s.handle(&c, Key::Enter);
    assert_eq!(s.take_commit(), "昨日の会議");
}

#[test]
fn typo_correction_on_space_and_escape() {
    let Some(c) = converter() else { return };
    let mut s = plain();
    for keys in ["saisiylo", "saisiyxo"] {
        type_str(&mut s, &c, keys);
        assert_eq!(s.preedit().text, "さいしyぉ");
        s.handle(&c, Key::Space);
        assert_eq!(s.preedit().text, "最初");
        s.handle(&c, Key::Escape);
        assert_eq!(s.preedit().text, "さいしyぉ");
        s.handle(&c, Key::Space);
        s.handle(&c, Key::Enter);
        assert_eq!(s.take_commit(), "最初");
    }
}

#[test]
fn live_typo_correction_uses_the_corrected_reading() {
    let Some(c) = converter() else { return };
    let mut s = Session::new();
    type_str(&mut s, &c, "saisiylo");
    assert_eq!(s.preedit().text, "最初");
    assert_eq!(s.live_request().unwrap().0, "さいしょ");
    assert!(!s.apply_live("さいしyぉ", "際しyぉ"));
    s.handle(&c, Key::Enter);
    assert_eq!(s.take_commit(), "最初");

    type_str(&mut s, &c, "saisiylo");
    s.handle(&c, Key::Escape);
    assert_eq!(s.preedit().text, "さいしyぉ");
}

#[test]
fn valid_kana_typo_is_offered_as_a_suggestion() {
    let Some(c) = converter() else { return };
    let mut s = Session::new();
    s.options.live_conversion = false;
    type_str(&mut s, &c, "konyaku");
    assert_eq!(s.preedit().text, "こにゃく");
    assert_eq!(s.candidates().unwrap().items[0], "婚約");
    s.handle(&c, Key::Tab);
    s.handle(&c, Key::Enter);
    assert_eq!(s.take_commit(), "婚約");

    type_str(&mut s, &c, "konyaku");
    s.handle(&c, Key::Enter);
    assert_eq!(s.take_commit(), "こにゃく");
}

/// Always prefers the last candidate, to make reranking observable.
struct PickLast;

impl sweet_engine::Reranker for PickLast {
    fn choose(&self, _r: &str, _l: &str, c: &[sweet_engine::Conversion]) -> Option<usize> {
        Some(c.len() - 1)
    }
}

/// A slow reranker that counts its runs.
struct Slow(std::sync::Arc<std::sync::atomic::AtomicUsize>);

impl sweet_engine::Reranker for Slow {
    fn choose(&self, _r: &str, _l: &str, c: &[sweet_engine::Conversion]) -> Option<usize> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        std::thread::sleep(std::time::Duration::from_millis(200));
        Some(c.len() - 1)
    }
}

#[test]
fn identical_rerank_runs_at_once_share_one_model_run() {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/system.dict");
    let Ok(d) = Dictionary::open(p) else { return };
    let runs = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let c = Converter::new(d).with_reranker(Box::new(Slow(runs.clone())));
    // A background refinement and the following Space convert the same reading.
    let results: Vec<String> = std::thread::scope(|s| {
        let a = s.spawn(|| c.convert_best("きしゃ", &[], "")[0].surface());
        std::thread::sleep(std::time::Duration::from_millis(50));
        let b = s.spawn(|| c.convert_best("きしゃ", &[], "")[0].surface());
        vec![a.join().unwrap(), b.join().unwrap()]
    });
    assert_eq!(results[0], results[1]);
    assert_eq!(runs.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn neural_and_learning_switches() {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/system.dict");
    let Ok(d) = Dictionary::open(p) else { return };
    let c = Converter::new(d).with_reranker(Box::new(PickLast));
    let lattice_best = c.convert("きしゃ", &[], 1)[0].surface();
    assert_ne!(c.convert_best("きしゃ", &[], "")[0].surface(), lattice_best);
    c.configure(false, true);
    assert!(!c.has_reranker());
    assert_eq!(c.convert_best("きしゃ", &[], "")[0].surface(), lattice_best);

    // Learning off: nothing is learned and nothing learned is used.
    c.configure(false, false);
    let other = c.segment_candidates("きしゃ", 5)[2].clone();
    c.learn("きしゃ", &other);
    assert_eq!(c.convert("きしゃ", &[], 1)[0].surface(), lattice_best);
    c.configure(false, true);
    c.learn("きしゃ", &other);
    assert_eq!(c.convert("きしゃ", &[], 1)[0].surface(), other);
}

#[test]
fn user_dictionary_words_convert_and_connect() {
    let Some(c) = converter() else { return };
    let dir = std::env::temp_dir().join(format!("sweet-ime-userdict-session-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("user_dict.tsv");
    let mut d = sweet_engine::UserDictionary::with_file(&path);
    d.add("ぽむ", "ぽむ太郎", "人名", "").unwrap();
    d.add("さとう", "佐東", "姓", "").unwrap();
    d.add("すいあい", "Sweet IME の設定", "サジェストのみ", "").unwrap();
    let c = c.with_user_dict(sweet_engine::UserDictionary::with_file(&path));

    let best = |s: &str| c.convert_best(s, &[], "")[0].surface();
    assert_eq!(best("ぽむがきた"), "ぽむ太郎が来た");
    assert!(best("さとうさんにあう").starts_with("佐東さんに"));
    assert!(c.segment_candidates("さとう", 10).contains(&"砂糖".to_string()));
    assert_eq!(c.segment_candidates("さとう", 10)[0], "佐東");
    // Suggestion-only words are offered while typing but never converted.
    assert!(c.predict("すいあ", 5).contains(&"Sweet IME の設定".to_string()));
    assert_ne!(best("すいあい"), "Sweet IME の設定");

    // Edits from another process (the settings app) are picked up.
    let mut d = sweet_engine::UserDictionary::with_file(&path);
    d.remove(0).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    let _ = std::fs::OpenOptions::new().append(true).open(&path).and_then(|f| f.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(5)));
    assert_ne!(best("ぽむがきた"), "ぽむ太郎が来た");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn english_words_typed_by_spelling_show_as_words() {
    let Some(c) = converter() else { return };
    for (typed, expected) in [
        // Bundled spellings, also while the last key is still pending.
        ("github", "GitHub"),
        ("linux", "Linux"),
        ("iphone", "iPhone"),
        ("vscode", "VS Code"),
        // Keys that are not romaji, shown as typed.
        ("discord", "Discord"),
        ("server", "server"),
        // Inside a sentence.
        ("vrchatdeasobu", "VRChatで遊ぶ"),
        ("githubnipusshusuru", "GitHubにプッシュする"),
        ("kyouhadiscorddehanasu", "今日はDiscordで話す"),
        // Japanese is unchanged, typos included.
        ("kyouhaiitenkidesu", "今日はいい天気です"),
        ("arigatpu", "ありがとう"),
    ] {
        let mut s = Session::new();
        type_str(&mut s, &c, typed);
        assert_eq!(s.preedit().text, expected, "live {typed}");
        assert!(s.handle(&c, Key::Enter));
        assert_eq!(s.take_commit(), expected, "enter {typed}");

        type_str(&mut s, &c, typed);
        assert!(s.handle(&c, Key::Space));
        assert_eq!(s.preedit().text, expected, "space {typed}");
        assert!(s.handle(&c, Key::Escape));
        assert!(s.handle(&c, Key::Escape));
        s.take_commit();
    }
    // Typing on past an unresolved letter keeps the kana while a typo fix is possible.
    let mut s = Session::new();
    type_str(&mut s, &c, "kak");
    assert_eq!(s.preedit().text, "かk");
    // A word being typed after Japanese doesn't turn the sentence into keys.
    let mut s = Session::new();
    for (i, ch) in "kyouhadiscorddehanasu".chars().enumerate() {
        assert!(s.handle(&c, Key::Char(ch)));
        assert!(i < 5 || s.preedit().text.starts_with("今日は"), "{}", s.preedit().text);
    }
}

#[test]
fn special_readings_offer_dates_emoji_and_arithmetic() {
    let Some(c) = converter() else { return };
    // The candidate list for `typed` (Space twice) with `options`.
    let list = |typed: &str, options: Options| {
        let mut s = Session::new();
        s.options = options;
        type_str(&mut s, &c, typed);
        assert!(s.handle(&c, Key::Space));
        assert!(s.handle(&c, Key::Space));
        s.candidates().map(|l| l.items).unwrap_or_default()
    };
    let commit = |typed: &str, options: Options| {
        let mut s = Session::new();
        s.options = options;
        type_str(&mut s, &c, typed);
        assert!(s.handle(&c, Key::Space));
        assert!(s.handle(&c, Key::Enter));
        s.take_commit()
    };
    let on = Options { live_conversion: false, suggestions: false, ..Options::default() };
    // A date follows the dictionary's 今日 in the candidate list.
    let today = list("kyou", on);
    assert_eq!(today[0], "今日");
    assert!(today[1].starts_with("20") && today[1].contains('/'), "{today:?}");
    assert!(list("egao", on).contains(&"😊".to_string()));
    // Arithmetic: Space gives the value; 123/456 types as 123・456.
    assert_eq!(commit("1+2*3=", on), "7");
    // ... and the value is the first prediction while typing.
    let mut live = Session::new();
    type_str(&mut live, &c, "123/456=");
    assert_eq!(live.candidates().unwrap().items[0], "0.2697368421");

    // Each can be turned off.
    let off = Options { dates: false, emoji: false, calculator: false, ..on };
    assert!(!list("kyou", off).iter().any(|c| c.contains('/')));
    assert!(!list("egao", off).contains(&"😊".to_string()));
    assert_ne!(commit("1+2*3=", off), "7");
}
