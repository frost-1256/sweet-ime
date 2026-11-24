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
fn backspace_reopens_romaji_after_a_mistyped_key() {
    let Some(c) = converter() else { return };
    for (mistyped, deletes, rest, expected) in [
        ("githyb", 2, "ub,", "GitHub、"),
        ("anthorpic", 5, "ropic,", "Anthropic、"),
        ("konnbanwq", 1, "a", "こんばんは"),
    ] {
        let mut s = Session::new();
        type_str(&mut s, &c, mistyped);
        for _ in 0..deletes {
            assert!(s.handle(&c, Key::Backspace));
        }
        type_str(&mut s, &c, rest);
        assert_eq!(s.preedit().text, expected);
        assert!(s.handle(&c, Key::Enter));
        assert_eq!(s.take_commit(), expected);
    }

    let mut s = plain();
    type_str(&mut s, &c, "kya");
    assert!(s.handle(&c, Key::Backspace));
    assert_eq!(s.preedit().text, "き");
}

#[test]
fn listed_english_word_preserves_a_following_japanese_ending() {
    let Some(c) = converter() else { return };
    for (input, expected) in [("busydesu", "busyです"), ("busy", "busy")] {
        for convert in [false, true] {
            let mut s = Session::new();
            type_str(&mut s, &c, input);
            if convert {
                assert!(s.handle(&c, Key::Space));
            }
            assert_eq!(s.preedit().text, expected);
            assert!(s.handle(&c, Key::Enter));
            assert_eq!(s.take_commit(), expected);
        }
    }
}

#[test]
fn lattice_prefers_currency_and_weather_homophones_in_context() {
    let Some(c) = converter() else { return };
    for (input, expected) in [
        ("1000enn", "1000円"),
        ("1,000ennwoshiharau", "1,000円を支払う"),
        ("kyouhaamegafutteiru", "今日は雨が降っている"),
        ("yukigafutteita", "雪が降っていた"),
        ("hatawofutteiru", "旗を振っている"),
    ] {
        let mut s = plain();
        type_str(&mut s, &c, input);
        assert!(s.handle(&c, Key::Space));
        assert!(s.handle(&c, Key::Enter));
        assert_eq!(s.take_commit(), expected);
    }
}

#[test]
fn exposes_exact_homophones_beyond_the_ranked_candidates() {
    let Some(c) = converter() else { return };
    let reading = "こう";
    let ranked = c.segment_candidates(reading, 40);
    let exact = c.dictionary().lookup(reading);
    let omitted = exact
        .iter()
        .find(|word| !ranked.iter().any(|candidate| candidate == word.surface))
        .expect("the bounded ranking should omit some exact matches");

    let mut s = plain();
    type_str(&mut s, &c, "kou");
    s.handle(&c, Key::Space);
    let first = s.preedit().text;
    assert!(s.open_candidates(&c));
    let list = s.candidates().unwrap();
    assert_eq!(list.items.first(), Some(&first));
    assert!(exact.iter().all(|word| list.items.iter().any(|item| item == word.surface)));
    let index = list.items.iter().position(|item| item == omitted.surface).unwrap();
    assert!(s.select_candidate(&c, index));
    assert_eq!(s.take_commit(), omitted.surface);

    type_str(&mut s, &c, "kou");
    assert!(s.expand_predictions(&c));
    let list = s.candidates().unwrap();
    assert_eq!(list.kind, ListKind::Conversion);
    assert!(exact.iter().all(|word| list.items.iter().any(|item| item == word.surface)));
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
fn japanese_readings_convert_to_game_names_and_music_genres() {
    let Some(c) = converter() else { return };
    for (romaji, katakana, expected) in [
        ("purojekutomu-n", "プロジェクトムーン", "ProjectMoon"),
        (
            "robotomi-ko-pore-syon",
            "ロボトミーコーポレーション",
            "LobotomyCorporation",
        ),
        (
            "raiburari-oburuina",
            "ライブラリーオブルイナ",
            "LibraryOfRuina",
        ),
        (
            "raiburarioburuina",
            "ライブラリオブルイナ",
            "LibraryOfRuina",
        ),
        ("rinbasukanpani-", "リンバスカンパニー", "LimbusCompany"),
        ("haipa-furippu", "ハイパーフリップ", "hyperflip"),
        ("dariakoa", "ダリアコア", "dariacore"),
        ("ji-zasukurabu", "ジーザスクラブ", "jesusclub"),
        ("ja-ji-kurabu", "ジャージークラブ", "jerseyclub"),
        ("ja-jikurabu", "ジャージクラブ", "jerseyclub"),
    ] {
        for typed in [romaji, katakana] {
            let mut s = plain();
            type_str(&mut s, &c, typed);
            assert!(s.handle(&c, Key::Space));
            assert_eq!(s.preedit().text, expected, "{typed}");
            assert!(s.handle(&c, Key::Enter));
            assert_eq!(s.take_commit(), expected);
        }
    }
}

#[test]
fn soundcloud_vocabulary_is_available_from_kana_and_romaji() {
    let Some(c) = converter() else { return };
    for (romaji, katakana, expected) in [
        ("saundokuraudo", "サウンドクラウド", "SoundCloud"),
        ("dejirukoa", "デジルコア", "digilcore"),
        ("shijirukoa", "シジルコア", "sigilcore"),
        ("bureikukoa", "ブレイクコア", "breakcore"),
        ("firi-kurabu", "フィリークラブ", "philly club"),
        ("puraguenubi-", "プラグエヌビー", "pluggnb"),
        ("dorifutofonku", "ドリフトフォンク", "drift phonk"),
        ("ha-dowe-bu", "ハードウェーブ", "hardwave"),
    ] {
        for typed in [romaji, katakana] {
            let mut s = plain();
            type_str(&mut s, &c, typed);
            assert!(s.handle(&c, Key::Space));
            assert_eq!(s.preedit().text, expected, "{typed}");
        }
    }

    for (typed, first, alternative) in [
        ("じゃーじーくらぶ", "jerseyclub", "jersey club"),
        ("ふっとわーく", "フットワーク", "footwork"),
        ("ぷらぐ", "プラグ", "plugg"),
    ] {
        let mut s = plain();
        type_str(&mut s, &c, typed);
        assert!(s.handle(&c, Key::Space));
        assert_eq!(s.preedit().text, first, "{typed}");
        assert!(s.open_candidates(&c));
        assert!(
            s.candidates()
                .unwrap()
                .items
                .iter()
                .any(|item| item == alternative),
            "{alternative}",
        );
    }
}

#[test]
fn converts_ai_avatar_and_pc_vocabulary() {
    let Some(c) = converter() else { return };
    for (typed, expected) in [
        ("kuensantenroku", "Qwen3.6"),
        ("ジーエルエムゴテンサン", "GLM-5.3"),
        ("ji-ji-yu-efu", "GGUF"),
        ("けーぶいきゃっしゅ", "KVキャッシュ"),
        ("エヌディーエムエフ", "NDMF"),
        ("ma-jia-machua", "Merge Armature"),
        ("きせかえぎみっく", "着せ替えギミック"),
        ("えーえむでぃーえきすぽ", "AMD EXPO"),
        ("enubuiemui-", "NVMe"),
        ("かんいすいれい", "簡易水冷"),
    ] {
        let mut s = plain();
        type_str(&mut s, &c, typed);
        assert!(s.handle(&c, Key::Space));
        assert_eq!(s.preedit().text, expected, "{typed}");
    }

    let mut s = plain();
    type_str(&mut s, &c, "ragu");
    assert!(s.handle(&c, Key::Space));
    assert_eq!(s.preedit().text, "ラグ");
    assert!(s.open_candidates(&c));
    assert!(
        s.candidates()
            .unwrap()
            .items
            .iter()
            .any(|item| item == "RAG")
    );
}

#[test]
fn bundled_spellings_convert_live_from_letters_digits_and_symbols() {
    let Some(c) = converter() else { return };
    for (typed, expected) in [
        ("m。2", "M.2"),
        ("m.2", "M.2"),
        ("m2", "M.2"),
        ("ｍ．２", "M.2"),
        ("glm-5.3", "GLM-5.3"),
        ("qwen3。6", "Qwen3.6"),
        ("gemma4", "Gemma 4"),
        ("q4_k_m", "Q4_K_M"),
        ("node。js", "Node.js"),
        ("miniーitx", "Mini-ITX"),
        ("c++", "C++"),
        ("ci/cd", "CI/CD"),
        ("ndmf", "NDMF"),
        ("vrcquesttools", "VRCQuestTools"),
        ("mergearmature", "Merge Armature"),
        ("gguf", "GGUF"),
        ("vllm", "vLLM"),
        ("comfyui", "ComfyUI"),
        ("nvme", "NVMe"),
        ("amdexpo", "AMD EXPO"),
        ("projectmoon", "ProjectMoon"),
    ] {
        let mut s = Session::new();
        type_str(&mut s, &c, typed);
        assert_eq!(s.preedit().text, expected, "{typed}");
        assert!(s.handle(&c, Key::Enter));
        assert_eq!(s.take_commit(), expected, "{typed}");

        let mut s = plain();
        type_str(&mut s, &c, typed);
        assert!(s.handle(&c, Key::Space));
        assert_eq!(s.preedit().text, expected, "{typed}");
    }
}

#[test]
fn dedicated_symbol_keys_stay_in_a_bundled_spelling() {
    let Some(c) = converter() else { return };
    let mut s = Session::new();
    type_str(&mut s, &c, "m");
    assert!(s.handle_symbol(&c, '。'));
    assert_eq!(s.take_commit(), "");
    type_str(&mut s, &c, "2");
    assert_eq!(s.preedit().text, "M.2");
    assert!(s.handle(&c, Key::Backspace));
    assert_ne!(s.preedit().text, "M.2");
    type_str(&mut s, &c, "2");
    assert_eq!(s.preedit().text, "M.2");

    for typed in ["arigatou", "m2", "gg"] {
        let mut s = Session::new();
        type_str(&mut s, &c, typed);
        assert!(!s.handle_symbol(&c, '。'), "{typed}");
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
fn an_unfinished_consonant_can_be_completed_after_moving_the_caret() {
    let Some(c) = converter() else { return };
    let mut s = Session::new();
    type_str(&mut s, &c, "zuttopattidakda");
    assert!(s.handle(&c, Key::Left));
    let preedit = s.preedit();
    assert_eq!(&preedit.text[..preedit.caret], "ずっとぱっちだk");
    assert_eq!(&preedit.text[preedit.caret..], "だ");

    type_str(&mut s, &c, "e");
    let preedit = s.preedit();
    assert_eq!(&preedit.text[..preedit.caret], "ずっとぱっちだけ");
    assert_eq!(&preedit.text[preedit.caret..], "だ");
    assert!(s.handle(&c, Key::Space));
    assert!(s.handle(&c, Key::Enter));
    assert_eq!(s.take_commit(), "ずっとパッチだけだ");
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
fn private_field_learns_nothing() {
    let Some(c) = converter() else { return };
    let mut s = plain();
    s.private = true;
    type_str(&mut s, &c, "kisha");
    s.handle(&c, Key::Space);
    let first = s.preedit().text;
    s.handle(&c, Key::Space);
    let second = s.candidates().unwrap().items[1].clone();
    s.handle(&c, Key::Enter);
    assert_eq!(s.take_commit(), second);

    type_str(&mut s, &c, "kisha");
    s.handle(&c, Key::Space);
    assert_eq!(s.preedit().text, first);
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
fn decimal_point_between_digits() {
    let Some(c) = converter() else { return };
    let mut s = plain();
    type_str(&mut s, &c, "5.5");
    assert_eq!(s.preedit().text, "5.5");
    s.handle(&c, Key::Enter);
    assert_eq!(s.take_commit(), "5.5");

    type_str(&mut s, &c, "1,000enn");
    s.handle(&c, Key::Space);
    s.handle(&c, Key::Enter);
    assert!(s.take_commit().starts_with("1,000"));

    type_str(&mut s, &c, "5.");
    s.handle(&c, Key::Enter);
    assert_eq!(s.take_commit(), "5。");

    // Live conversion keeps the number as typed.
    let mut s = Session::new();
    type_str(&mut s, &c, "5.5kai");
    s.handle(&c, Key::Enter);
    assert!(s.take_commit().starts_with("5.5"));
}

#[test]
fn url_keys_stay_ascii_in_japanese_mode() {
    let Some(c) = converter() else { return };
    let mut s = Session::new();
    type_str(&mut s, &c, "https");
    assert!(s.handle_symbol(&c, ':'));
    assert!(s.handle_symbol(&c, '/'));
    assert!(s.handle_symbol(&c, '/'));
    type_str(&mut s, &c, "example.com/");
    s.handle(&c, Key::ShiftedLetter('A'));
    type_str(&mut s, &c, "Pi?x=5.3&y=foo-bar#Part");
    let url = "https://example.com/APi?x=5.3&y=foo-bar#Part";
    assert_eq!(s.preedit().text, url);
    assert!(s.live_request().is_none());
    s.handle(&c, Key::Space);
    assert_eq!(s.take_commit(), format!("{url} "));
    type_str(&mut s, &c, "nihongo");
    s.handle(&c, Key::Enter);
    assert_eq!(s.take_commit(), "日本語");
}

#[test]
fn bare_domains_keep_their_typed_spelling() {
    let Some(c) = converter() else { return };
    let mut s = plain();
    for url in ["www.example.dev/a", "example.co.jp/wiki", "my-site.com"] {
        type_str(&mut s, &c, url);
        assert_eq!(s.preedit().text, url);
        s.handle(&c, Key::Enter);
        assert_eq!(s.take_commit(), url);
    }
    type_str(&mut s, &c, "arigatou.");
    assert!(!s.handle_symbol(&c, '/'));
    s.handle(&c, Key::Enter);
    assert_eq!(s.take_commit(), "ありがとう。");
}

#[test]
fn typo_early_in_a_long_reading_is_fixed() {
    let Some(c) = converter() else { return };
    let mut s = plain();
    // The stray "y" is far from the end, out of reach of the last keys.
    type_str(&mut s, &c, "kakuyninnshiteokimasu");
    assert_eq!(s.preedit().text, "かくyにんしておきます");
    s.handle(&c, Key::Space);
    s.handle(&c, Key::Enter);
    assert_eq!(s.take_commit(), "確認しておきます");
}

#[test]
fn a_better_early_typo_repair_beats_a_late_candidate() {
    let Some(c) = converter() else { return };
    let fix = sweet_engine::typo::correct(
        &c,
        "ariatougozaimasu",
        1000,
        std::time::Duration::from_millis(250),
    )
    .expect("the missing g can be repaired");
    assert_eq!(fix.reading, "ありがとうございます");
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
fn live_typo_correction_survives_more_keys_and_punctuation() {
    let Some(c) = converter() else { return };
    for convert_first in [false, true] {
        let mut s = Session::new();
        type_str(&mut s, &c, "saisiylo");
        assert_eq!(s.preedit().text, "最初");
        type_str(&mut s, &c, "n");
        assert_eq!(s.preedit().text, "最初n");
        assert_eq!(s.live_commit_request().unwrap().0, "さいしょん");
        type_str(&mut s, &c, "o,");
        assert_eq!(s.preedit().text, "最初の、");
        assert_eq!(s.live_request().unwrap().0, "さいしょの、");
        assert!(!s.apply_live("さいしょ", "最初"));
        if convert_first {
            assert!(s.handle(&c, Key::Space));
            assert_eq!(s.preedit().text, "最初の、");
        }
        assert!(s.handle(&c, Key::Enter));
        assert_eq!(s.take_commit(), "最初の、");
    }

    let mut s = Session::new();
    type_str(&mut s, &c, "kakuyninn");
    assert_eq!(s.preedit().text, "確認");
    for ch in "shiteokimasu.".chars() {
        assert!(s.handle(&c, Key::Char(ch)));
        assert!(s.preedit().text.starts_with("確認"), "{}", s.preedit().text);
    }
    assert!(s.handle(&c, Key::Enter));
    assert_eq!(s.take_commit(), "確認しておきます。");
}

#[test]
fn editing_or_dismissing_a_live_typo_preserves_the_original_keys() {
    let Some(c) = converter() else { return };
    let mut s = Session::new();
    type_str(&mut s, &c, "saisiylono,");
    assert!(s.handle(&c, Key::Backspace));
    assert_eq!(s.preedit().text, "最初の");
    assert!(s.handle(&c, Key::Backspace));
    assert_eq!(s.preedit().text, "最初n");
    assert!(s.handle(&c, Key::Backspace));
    assert_eq!(s.preedit().text, "最初");
    assert!(s.handle(&c, Key::Backspace));
    assert!(s.preedit().text.ends_with("yl"), "{}", s.preedit().text);

    s.reset();
    type_str(&mut s, &c, "saisiylono,");
    assert!(s.handle(&c, Key::Space));
    assert!(s.handle(&c, Key::Escape));
    assert_eq!(s.preedit().text, "さいしyぉの、");
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

/// Enough time for the search to finish on a slow CI runner; these tests check
/// what it finds, not how fast.
const TYPO_TEST_BUDGET: std::time::Duration = std::time::Duration::from_secs(5);

#[test]
fn typo_correction_restores_missing_consonants() {
    let Some(c) = converter() else { return };
    for (keys, reading) in [
        ("oyasuminaai", "おやすみなさい"),
        ("dennshagatomateiru", "でんしゃがとまっている"),
    ] {
        let fix = sweet_engine::typo::correct(&c, keys, 1000, TYPO_TEST_BUDGET).unwrap();
        assert_eq!(fix.reading, reading, "{keys}");
    }
}

#[test]
fn corrects_early_typos_even_when_the_wrong_reading_is_all_kana() {
    let Some(c) = converter() else { return };
    let fix = sweet_engine::typo::correct(&c, "raigatougozaimasu", 1000, TYPO_TEST_BUDGET).unwrap();
    assert_eq!(fix.reading, "ありがとうございます");
}

#[test]
fn learns_the_corrected_reading_only_when_the_user_picks_it() {
    for (use_tab, private) in [(false, false), (true, false), (true, true)] {
        let Some(c) = converter() else { return };
        let mut s = Session::new();
        s.options.live_conversion = false;
        s.private = private;
        type_str(&mut s, &c, "konyaku");
        assert_eq!(c.convert("こんやく", &[], 1)[0].learned_words(), 0);
        assert_eq!(s.candidates().unwrap().items[0], "婚約");
        if use_tab {
            s.handle(&c, Key::Tab);
            s.handle(&c, Key::Enter);
        } else {
            assert!(s.select_candidate(&c, 0));
        }
        assert_eq!(s.take_commit(), "婚約");
        assert_eq!(c.convert("こんやく", &[], 1)[0].learned_words(), usize::from(!private));
        assert_eq!(c.convert("こにゃく", &[], 1)[0].learned_words(), 0);
    }
}

/// Always prefers the last candidate, to make reranking observable.
struct PickLast;

impl sweet_engine::Reranker for PickLast {
    fn choose(&self, _r: &str, _l: &str, c: &[sweet_engine::Conversion]) -> Option<usize> {
        Some(c.len() - 1)
    }
}

#[test]
fn neural_reranking_keeps_learned_words_and_still_picks_other_words() {
    let Some(c) = converter() else { return };
    c.learn("きしゃ", "汽車");
    let reading = "あしたきしゃになります";
    let lattice_candidates = c.convert(reading, &[], 1);
    assert_eq!(lattice_candidates[0].learned_words(), 1, "{lattice_candidates:?}");
    let lattice = lattice_candidates[0].surface();
    let c = c.with_reranker(Box::new(PickLast));
    let best = &c.convert_best(reading, &[], "")[0];
    assert!(best.surface().contains("汽車"), "{}", best.surface());
    assert_ne!(best.surface(), lattice);
}

#[test]
fn live_conversion_uses_the_text_committed_before_the_new_reading() {
    let Some(c) = converter() else { return };
    let mut s = Session::new();
    s.set_left_context("先に");
    s.handle(&c, Key::ShiftedLetter('G'));
    type_str(&mut s, &c, "ooglehakensaku");
    assert_eq!(s.take_commit(), "Google");
    let (reading, context) = s.live_request().unwrap();
    assert_eq!(reading, "はけんさく");
    assert_eq!(context, "先にGoogle");
}

#[test]
fn distinguishes_release_announcements_from_seeds() {
    let Some(c) = converter() else { return };
    let mut s = Session::new();
    type_str(&mut s, &c, "imadetane");
    s.handle(&c, Key::Enter);
    assert_eq!(s.take_commit(), "今出たね");
    assert_eq!(c.convert("にわでたね", &[], 1)[0].surface(), "庭で種");
    assert_eq!(
        c.convert("はなをそだてるのでたねをかう", &[], 1)[0].surface(),
        "花を育てるので種を買う"
    );

    let c = c.with_reranker(Box::new(PickLast));
    for (reading, context, surface) in [
        ("GLM 5.3でたね", "", "GLM 5.3出たね"),
        ("でたね", "GLM 5.3", "出たね"),
    ] {
        assert_eq!(c.convert_best(reading, &[], context)[0].surface(), surface);
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
fn names_written_both_ways_offer_katakana_then_english() {
    let Some(c) = converter() else { return };
    for (typed, katakana, english) in [
        ("enubidhia", "エヌビディア", "NVIDIA"),
        ("amazon", "アマゾン", "Amazon"),
        ("gu-guru", "グーグル", "Google"),
        ("interu", "インテル", "Intel"),
    ] {
        let mut s = plain();
        type_str(&mut s, &c, typed);
        s.handle(&c, Key::Space);
        assert_eq!(s.preedit().text, katakana, "{typed}");
        s.handle(&c, Key::Space);
        let list = s.candidates().expect("candidate list");
        assert_eq!(&list.items[..2], [katakana, english], "{typed}");
        s.handle(&c, Key::Escape);
        s.handle(&c, Key::Escape);
        s.take_commit();
    }
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
        ("nvidia", "NVIDIA"),
        ("geforce", "GeForce"),
        ("xiaomi", "Xiaomi"),
        ("huggingface", "Hugging Face"),
        // Keys that are not romaji, shown as typed.
        ("discord", "Discord"),
        ("server", "server"),
        // Inside a sentence.
        ("vrchatdeasobu", "VRChatで遊ぶ"),
        ("githubnipusshusuru", "GitHubにプッシュする"),
        ("kyouhanvidianokabu", "今日はNVIDIAの株"),
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
fn english_words_stay_intact_before_punctuation() {
    let Some(c) = converter() else { return };
    for (typed, expected) in [
        ("anthropic,", "Anthropic、"),
        ("anthropic.", "Anthropic。"),
        ("korehaanthropic,", "これはAnthropic、"),
        ("anthropicdesu", "Anthropicです"),
        ("busy（", "busy（"),
        ("busy）", "busy）"),
    ] {
        let mut s = Session::new();
        type_str(&mut s, &c, typed);
        assert_eq!(s.preedit().text, expected, "{typed}");
        assert!(s.handle(&c, Key::Enter));
        assert_eq!(s.take_commit(), expected);

        let mut s = plain();
        type_str(&mut s, &c, typed);
        assert!(s.handle(&c, Key::Space));
        assert_eq!(s.preedit().text, expected, "{typed}");
    }
}

#[test]
fn listed_english_words_convert_inside_sentences() {
    let Some(c) = converter() else { return };
    let convert = |c: &Converter, typed: &str, on: bool| {
        let mut s = Session::new();
        s.options.english_words = on;
        type_str(&mut s, c, typed);
        let live = s.preedit().text;
        assert!(s.handle(c, Key::Space));
        (live, s.preedit().text)
    };
    for (typed, word) in [
        // A common loanword shows in katakana.
        ("korehabestdesu", "ベスト"),
        ("kyouhadesktopwokau", "デスクトップ"),
        // Others keep their spelling.
        ("watashihasoftwarewotsukuru", "software"),
        // Letters that spell a word are not a typo (not 部署).
        ("konnshuuhabusydesu", "busy"),
        ("nginxwotsukau", "Nginx"),
    ] {
        let (live, space) = convert(&c, typed, true);
        assert!(live.contains(word), "live {typed}: {live}");
        assert!(space.contains(word), "space {typed}: {space}");
    }
    // The spelling is the next candidate, also for a word typed alone.
    for typed in ["best", "korehabestdesu"] {
        let mut s = Session::new();
        type_str(&mut s, &c, typed);
        assert!(s.handle(&c, Key::Space));
        if typed != "best" {
            assert!(s.handle(&c, Key::Right));
        }
        assert!(s.preedit().text.contains("ベスト"), "{}", s.preedit().text);
        assert!(s.handle(&c, Key::Space));
        let items = s.candidates().unwrap().items;
        assert_eq!(items.get(1).map(|i| i.starts_with("best")), Some(true), "{typed}: {items:?}");
    }
    // Katakana off: the spelling.
    c.set_english_words(true, false);
    assert!(convert(&c, "korehabestdesu", true).1.contains("best"));
    c.set_english_words(true, true);
    // Romaji and typos stay Japanese.
    for (typed, expected) in [("tesuto", "テスト"), ("shigtoo", "仕事"), ("arigatpu", "ありがとう")] {
        assert_eq!(convert(&c, typed, true).1, expected, "{typed}");
    }
    // Off: only bundled spellings are words.
    c.set_english_words(false, true);
    let (_, space) = convert(&c, "korehabestdesu", false);
    assert!(!space.contains("best"), "{space}");
    c.set_english_words(true, true);
}

/// Prefers the first candidate that spells "best", like a model reading the
/// typed keys.
struct PickSpelling;

impl sweet_engine::Reranker for PickSpelling {
    fn choose(&self, _r: &str, _l: &str, c: &[sweet_engine::Conversion]) -> Option<usize> {
        c.iter().position(|c| c.surface().contains("best"))
    }
}

#[test]
fn reranking_keeps_the_katakana_loanword() {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/system.dict");
    let Ok(d) = Dictionary::open(p) else { return };
    let c = Converter::new(d).with_reranker(Box::new(PickSpelling));
    let reading = sweet_engine::romaji::to_hiragana("korehabestdesu");
    assert_eq!(c.convert_best(&reading, &[], "")[0].surface(), "これはベストです");
    c.set_english_words(true, false);
    assert_eq!(c.convert_best(&reading, &[], "")[0].surface(), "これはbestです");
}

#[test]
fn a_word_ending_in_n_takes_the_n_row_kana() {
    let Some(c) = converter() else { return };
    // "nn" reads ん, so "pythonno" would be Python + お.
    for (typed, want) in [("pythonnoscriptwokaita", "Pythonの"), ("designnoteian", "designの")] {
        let mut s = Session::new();
        type_str(&mut s, &c, typed);
        assert!(s.handle(&c, Key::Space));
        assert!(s.preedit().text.starts_with(want), "{typed}: {}", s.preedit().text);
    }
}

#[test]
fn english_words_not_on_the_list_are_learned() {
    let Some(c) = converter() else { return };
    let sentence = |c: &Converter| {
        let mut s = Session::new();
        type_str(&mut s, c, "kyouhazigbeewotsukau");
        assert!(s.handle(c, Key::Space));
        s.preedit().text
    };
    assert!(!sentence(&c).contains("zigbee"));
    let mut s = Session::new();
    // A typo fix reads it as 地下絵; F10 gives the keys back.
    type_str(&mut s, &c, "zigbee");
    assert!(s.handle(&c, Key::ToHalfAscii));
    assert!(s.handle(&c, Key::Enter));
    assert_eq!(s.take_commit(), "zigbee");
    assert!(sentence(&c).contains("zigbee"), "{}", sentence(&c));
    // Romaji fixed with F10 is not learned as English.
    let mut s = Session::new();
    type_str(&mut s, &c, "kore");
    assert!(s.handle(&c, Key::ToHalfAscii));
    assert!(s.handle(&c, Key::Enter));
    assert_eq!(s.take_commit(), "kore");
    type_str(&mut s, &c, "kore");
    assert!(s.handle(&c, Key::Space));
    assert_ne!(s.preedit().text, "kore");
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

#[test]
fn undo_commit_restores_the_reading() {
    let Some(c) = converter() else { return };
    let mut s = plain();
    type_str(&mut s, &c, "kyou");
    assert!(s.handle(&c, Key::Space));
    assert!(s.handle(&c, Key::Enter));
    assert_eq!(s.take_commit(), "今日");
    assert!(s.can_undo());
    assert!(s.handle(&c, Key::UndoCommit));
    assert_eq!(s.take_undo().as_deref(), Some("今日"));
    assert_eq!(s.preedit().text, "きょう");
    // Typing goes on from the reading.
    type_str(&mut s, &c, "ha");
    assert_eq!(s.preedit().text, "きょうは");
    assert!(s.handle(&c, Key::Enter));
    assert_eq!(s.take_commit(), "きょうは");

    // Only right after the commit.
    type_str(&mut s, &c, "a");
    assert!(s.handle(&c, Key::Enter));
    s.take_commit();
    type_str(&mut s, &c, "i");
    assert!(s.handle(&c, Key::Escape));
    assert!(!s.can_undo());
    assert!(!s.handle(&c, Key::UndoCommit));

    // Live conversion: the English word comes back as its keys.
    let mut s = Session::new();
    type_str(&mut s, &c, "github");
    assert!(s.handle(&c, Key::Enter));
    assert_eq!(s.take_commit(), "GitHub");
    assert!(s.handle(&c, Key::UndoCommit));
    assert_eq!(s.take_undo().as_deref(), Some("GitHub"));
    assert!(s.handle(&c, Key::ToHalfAscii));
    assert!(s.handle(&c, Key::Enter));
    assert_eq!(s.take_commit(), "github");

    // Off in the settings.
    let mut s = plain();
    s.options.undo_commit = false;
    type_str(&mut s, &c, "a");
    assert!(s.handle(&c, Key::Enter));
    assert!(!s.can_undo());
    assert!(!s.handle(&c, Key::UndoCommit));
}

#[test]
fn report_commit_records_the_reading_and_the_text() {
    let Some(c) = converter() else { return };
    let mut s = plain();
    type_str(&mut s, &c, "kyou");
    assert!(s.handle(&c, Key::Space));
    assert!(s.handle(&c, Key::Enter));
    assert_eq!(s.take_commit(), "今日");
    assert_eq!(s.reportable_commit(), Some("今日"));
    assert!(s.handle(&c, Key::ReportCommit));
    let report = s.take_report().unwrap();
    assert_eq!((report.reading.as_str(), report.text.as_str()), ("きょう", "今日"));
    // It can still be taken back.
    assert!(s.handle(&c, Key::UndoCommit));
    assert_eq!(s.take_undo().as_deref(), Some("今日"));

    // Only right after the commit, and not in private fields.
    assert!(!s.handle(&c, Key::ReportCommit));
    s.reset();
    s.private = true;
    type_str(&mut s, &c, "a");
    assert!(s.handle(&c, Key::Enter));
    assert!(s.reportable_commit().is_none());
    assert!(!s.handle(&c, Key::ReportCommit));
    assert!(s.take_report().is_none());
}

/// Live text after typing `typed`.
fn live_text(c: &Converter, typed: &str) -> String {
    let mut s = Session::new();
    type_str(&mut s, c, typed);
    s.preedit().text
}

#[test]
fn spellings_typed_as_romaji_convert_at_the_start() {
    let Some(c) = converter() else { return };
    // ime reads いめ: IME live too, and Enter commits it.
    assert_eq!(live_text(&c, "ime"), "IME");
    let mut s = Session::new();
    type_str(&mut s, &c, "ime");
    assert!(s.handle(&c, Key::Enter));
    assert_eq!(s.take_commit(), "IME");
    assert!(live_text(&c, "imewotsukau").starts_with("IMEを"));
    assert!(live_text(&c, "debiannoinsuto").starts_with("Debianの"));
    // Japanese that starts with the same kana stays Japanese.
    assert_eq!(live_text(&c, "ime-ji"), "イメージ");
    assert!(live_text(&c, "aaoishii").starts_with("ああ"));
}

#[test]
fn a_word_ending_in_pending_keys_converts_at_once() {
    let Some(c) = converter() else { return };
    // The last key ("x", "b") is still pending romaji.
    assert_eq!(live_text(&c, "kyouhalinux"), "今日はLinux");
    assert_eq!(live_text(&c, "kyouhagithub"), "今日はGitHub");
    assert_eq!(live_text(&c, "kyouhak"), "今日はk");
}

#[test]
fn a_typed_word_split_by_romaji_stays_whole() {
    let Some(c) = converter() else { return };
    // "systemctl" reads sysてmctl.
    assert_eq!(live_text(&c, "kyouhasystemctl"), "今日はsystemctl");
    assert!(live_text(&c, "systemctlwotsukau").starts_with("systemctlを"));
}

/// Types `text` with Shift on its first letter; returns what was committed
/// and the preedit.
fn shift_type(c: &Converter, s: &mut Session, text: &str) -> (String, String) {
    let mut committed = String::new();
    for (i, ch) in text.chars().enumerate() {
        let key = if i == 0 {
            Key::ShiftedLetter(ch)
        } else {
            Key::Char(ch)
        };
        assert!(s.handle(c, key));
        committed.push_str(&s.take_commit());
    }
    (committed, s.preedit().text)
}

#[test]
fn marked_spellings_convert_inside_japanese_sentences() {
    let Some(c) = converter() else { return };
    for (typed, expected) in [
        ("kyouhaqwen3.6wotsukau", "今日はQwen3.6を使う"),
        ("kyouham.2wotsukau", "今日はM.2を使う"),
        ("kyouhagm2wotsukau", "今日はgm2を使う"),
        ("kyouhaq4_kmwotsukau", "今日はQ4_K_Mを使う"),
        ("ci/cdde", "CI/CDで"),
        ("kyouhawi-fiwotsukau", "今日はWi-Fiを使う"),
    ] {
        let mut s = Session::new();
        type_str(&mut s, &c, typed);
        assert!(s.handle(&c, Key::Space));
        assert_eq!(s.preedit().text, expected, "{typed}");
    }
}

#[test]
fn a_shift_word_ends_when_japanese_follows() {
    let Some(c) = converter() else { return };
    let mut s = Session::new();
    let (committed, shown) = shift_type(&c, &mut s, "Googlehakensakuenzindesu");
    assert_eq!(committed, "Google");
    assert_eq!(shown, "は検索エンジンです");
    // The longest word: Hello + を, not Hell + おを.
    let mut s = Session::new();
    assert_eq!(shift_type(&c, &mut s, "Hellowosagasu").0, "Hello");
    // A particle ends it at Enter or Space.
    let mut s = Session::new();
    shift_type(&c, &mut s, "Googleha");
    assert!(s.handle(&c, Key::Enter));
    assert_eq!(s.take_commit(), "Googleは");
    // The word with a particle after it: Chrome + で, not Chromed + え.
    let mut s = Session::new();
    assert_eq!(shift_type(&c, &mut s, "Chromedehiraku").0, "Chrome");
    // Capitals end a word before a particle.
    for (text, word) in [("Xdekaku", "X"), ("AIwotukau", "AI")] {
        let mut s = Session::new();
        assert_eq!(shift_type(&c, &mut s, text).0, word);
    }
    for (text, word) in [
        ("Qwen3.6wotsukau", "Qwen3.6"),
        ("Projectmoonwotsukau", "ProjectMoon"),
        ("Zyph42wotsukau", "Zyph42"),
    ] {
        let mut s = Session::new();
        assert_eq!(shift_type(&c, &mut s, text).0, word, "{text}");
    }
    // Short endings end it at Enter too.
    for (text, out) in [("Applesan", "Appleさん"), ("Macdayo", "Macだよ"), ("GitHubdesu", "GitHubです")] {
        let mut s = Session::new();
        let mut committed = String::new();
        for (i, ch) in text.chars().enumerate() {
            let key = if i == 0 || ch.is_ascii_uppercase() { Key::ShiftedLetter(ch) } else { Key::Char(ch) };
            assert!(s.handle(&c, key));
            committed.push_str(&s.take_commit());
        }
        assert!(s.handle(&c, Key::Enter));
        committed.push_str(&s.take_commit());
        assert_eq!(committed, out);
    }
    // Names stay whole.
    for name in ["Tomoko", "Takeshi", "Superman", "Sweetime"] {
        let mut s = Session::new();
        shift_type(&c, &mut s, name);
        assert!(s.handle(&c, Key::Space));
        assert_eq!(s.take_commit(), format!("{name} "));
    }
    // A whole word, or no Japanese after it, stays English.
    let mut s = Session::new();
    shift_type(&c, &mut s, "Hello");
    assert!(s.handle(&c, Key::Space));
    assert_eq!(s.take_commit(), "Hello ");
    // Off: English until Space.
    let mut s = Session::new();
    s.options.shift_one_word = false;
    let (committed, shown) = shift_type(&c, &mut s, "Googlehakensaku");
    assert_eq!(
        (committed.as_str(), shown.as_str()),
        ("", "Googlehakensaku")
    );
}

#[test]
fn predictions_use_the_preceding_words_and_an_unfinished_tail() {
    let Some(c) = converter() else { return };
    for (typed, expected) in [
        ("しりょうをおく", "資料を送る"),
        ("おへんじあり", "お返事ありがとう"),
        ("きょうはらいぶらりーおぶる", "今日はLibraryOfRuina"),
    ] {
        let mut s = Session::new();
        type_str(&mut s, &c, typed);
        let candidates = s.candidates().expect("predictions for incomplete reading");
        assert_eq!(candidates.kind, ListKind::Suggestion);
        assert_eq!(candidates.items.first().map(String::as_str), Some(expected), "{typed}");
    }
}

#[test]
fn full_names_do_not_lead_predictions() {
    let Some(c) = converter() else { return };
    let predictions = c.predict("ほしいな", 5);
    assert_ne!(predictions.first().map(String::as_str), Some("星井七瀬"), "{predictions:?}");
}

#[test]
fn colloquial_long_vowels_stay_hiragana() {
    let Some(c) = converter() else { return };
    for (reading, expected) in [
        ("ちょーだい", "ちょーだい"),
        ("これちょーだい", "これちょーだい"),
        ("そーなんだ", "そーなんだ"),
        ("こーひー", "コーヒー"),
        ("ほーる", "ホール"),
        ("そーす", "ソース"),
        ("まなー", "マナー"),
    ] {
        assert_eq!(c.convert(reading, &[], 1)[0].surface(), expected, "{reading}");
    }
    let candidates = c.segment_candidates("ちょーだい", 5);
    assert_eq!(candidates[..2], ["ちょーだい", "チョーダイ"]);
}
