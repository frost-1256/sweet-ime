//! Compares against ids produced by the HF `tokenizers` reference
//! (ByteLevel BPE, add_prefix_space=false). Skipped without the model files.

use sweet_zenz::Tokenizer;

#[test]
fn matches_reference() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../third_party/zenz/v3.1-xsmall");
    let Ok(t) = Tokenizer::load(dir) else { return };
    let cases: &[(&str, &[u32])] = &[
        (
            "\u{EE00}キョウハイイテンキデスネ\u{EE01}今日はいい天気ですね",
            &[172, 120, 202, 436, 504, 400, 623, 280, 280, 367, 259, 436, 406, 271, 545, 172, 120, 203, 490, 304, 253, 242, 242, 865, 442, 246, 255, 529],
        ),
        ("記者の汽車が帰社", &[577, 429, 240, 3262, 660, 254, 1138, 487]),
        ("介護は本当に大変です", &[782, 924, 253, 358, 491, 243, 331, 581, 246, 255]),
        ("明日は音楽の公演", &[634, 304, 253, 854, 601, 240, 688, 841]),
    ];
    for (text, want) in cases {
        assert_eq!(t.encode(text), *want, "{text}");
        assert_eq!(t.decode(want), *text);
    }
}

#[test]
fn trie_scoring_matches_independent() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../third_party/zenz/v3.1-xsmall");
    let Ok(z) = sweet_zenz::Zenz::load(dir) else { return };
    let p = sweet_zenz::Zenz::prompt("きしゃのきしゃ", "");
    let cands: Vec<String> = ["記者の汽車", "記者の記者", "汽車の記者", "きしゃの汽車", "記者"].iter().map(|s| s.to_string()).collect();
    let together = z.score(&p, &cands);
    for (i, c) in cands.iter().enumerate() {
        let alone = z.score(&p, std::slice::from_ref(c))[0];
        assert!((alone - together[i]).abs() < 1e-3, "{c}: {alone} vs {}", together[i]);
    }
}
