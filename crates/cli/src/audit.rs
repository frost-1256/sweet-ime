//! `sweet-cli <dict> audit [readings]`: finds words whose dictionary cost
//! looks wrong, for `crates/dictc/src/cost_adjustments.tsv`.
//!
//! Short readings with several common nouns (not names, which the model
//! knows less well), one in kanji, are put into a few neutral phrases and converted with the lattice alone. Where the zenz
//! model (`SWEET_ZENZ`) clearly prefers another lattice candidate that
//! differs at the word, the pair is counted. Pairs that the model corrects
//! in most phrases are printed, most phrases first, with the cost offset
//! that would make the model's word win. The model is a hint, not ground
//! truth: check each row before adding it.

use anyhow::{Context, Result};
use std::collections::{BTreeMap, HashMap};
use sweet_engine::Converter;
use sweet_zenz::Zenz;

/// Phrases a noun reading is put into ({} is the reading).
const FRAMES: [&str; 5] = ["その{}", "{}がある", "{}について", "{}のはなし", "いい{}だね"];
/// How much more likely (in nats) the model's sentence must be.
const MARGIN: f32 = 1.5;
/// How many of the phrases the model must correct.
const MIN_FRAMES: usize = 3;
/// Readings whose cheapest noun costs more than this are left out (rare).
const MAX_COST: i16 = 6500;
/// Lattice candidates scored per phrase.
const CANDIDATES: usize = 8;
/// Parts of speech of the nouns compared.
const COMMON_NOUNS: [&str; 4] = ["名詞,一般,", "名詞,サ変接続,", "名詞,形容動詞語幹,", "名詞,副詞可能,"];

struct Finding {
    frames: usize,
    margin: f32,
}

pub fn audit(conv: &Converter, limit: Option<usize>) -> Result<()> {
    let path = std::env::var("SWEET_ZENZ").context("set SWEET_ZENZ to a zenz model")?;
    let zenz = Zenz::load(&path)?;
    let nouns = nouns(conv);
    let readings: Vec<&String> = nouns.keys().take(limit.unwrap_or(usize::MAX)).collect();
    eprintln!("auditing {} readings", readings.len());
    let mut found: HashMap<(String, String, String), Finding> = HashMap::new();
    let t = std::time::Instant::now();
    for (n, reading) in readings.iter().enumerate() {
        if n % 500 == 0 && n > 0 {
            eprintln!("{n} readings in {:.0?}, {} pairs", t.elapsed(), found.len());
        }
        for frame in FRAMES {
            let start = frame.find("{}").unwrap();
            let text = frame.replace("{}", reading);
            let span = (start, start + reading.len());
            let cands = conv.convert(&text, &[], CANDIDATES);
            if cands.len() < 2 {
                continue;
            }
            let surfaces: Vec<String> = cands.iter().map(|c| c.surface()).collect();
            let scores = zenz.score(&Zenz::prompt(&text, ""), &surfaces);
            let best = (0..scores.len()).max_by(|&a, &b| scores[a].total_cmp(&scores[b])).unwrap();
            let margin = scores[best] - scores[0];
            if best == 0 || margin < MARGIN {
                continue;
            }
            let (Some(loser), Some(winner)) = (word_at(&cands[0], span), word_at(&cands[best], span)) else {
                continue;
            };
            if loser == winner || ![&loser, &winner].iter().all(|w| nouns[*reading].contains(w)) {
                continue;
            }
            let f = found.entry(((*reading).clone(), loser, winner)).or_insert(Finding { frames: 0, margin: 0.0 });
            f.frames += 1;
            f.margin += margin;
        }
    }
    let mut rows: Vec<_> = found.into_iter().filter(|(_, f)| f.frames >= MIN_FRAMES).collect();
    rows.sort_by(|a, b| b.1.frames.cmp(&a.1.frames).then(b.1.margin.total_cmp(&a.1.margin)));
    println!("# reading\tdictionary's word\tsuggested offset\t# model's word, phrases, mean margin");
    let dict = conv.dictionary();
    for ((reading, loser, winner), f) in rows {
        let cost = |surface: &str| dict.lookup(&reading).iter().filter(|e| e.surface == surface).map(|e| e.cost).min();
        let offset = match (cost(&loser), cost(&winner)) {
            (Some(l), Some(w)) => format!("{}", (i32::from(w) - i32::from(l) + 300).max(300)),
            _ => "?".into(),
        };
        println!("{reading}\t{loser}\t{offset}\t# {winner}, {}/{}, {:.1}", f.frames, FRAMES.len(), f.margin / f.frames as f32);
    }
    eprintln!("done in {:.0?}", t.elapsed());
    Ok(())
}

/// Readings of two to four kana with their common noun spellings, where
/// there are two or more, one in kanji.
fn nouns(conv: &Converter) -> BTreeMap<String, Vec<String>> {
    let dict = conv.dictionary();
    let mut nouns: BTreeMap<String, (Vec<String>, i16)> = BTreeMap::new();
    for (reading, e) in dict.predictive("", usize::MAX) {
        let n = reading.chars().count();
        if !(2..=4).contains(&n) || !reading.chars().all(|c| matches!(c, 'ぁ'..='ゖ' | 'ー')) {
            continue;
        }
        if !dict.feature(e.lid).is_some_and(|f| COMMON_NOUNS.iter().any(|p| f.starts_with(p))) {
            continue;
        }
        let (surfaces, cost) = nouns.entry(reading).or_insert((Vec::new(), i16::MAX));
        if !surfaces.iter().any(|s| s == e.surface) {
            surfaces.push(e.surface.to_string());
        }
        *cost = (*cost).min(e.cost);
    }
    let kanji = |s: &str| s.chars().any(|c| matches!(c, '\u{4e00}'..='\u{9fff}'));
    nouns
        .into_iter()
        .filter(|(_, (surfaces, cost))| *cost <= MAX_COST && surfaces.len() >= 2 && surfaces.iter().any(|s| kanji(s)))
        .map(|(reading, (surfaces, _))| (reading, surfaces))
        .collect()
}

/// The surface of the word in `c` whose reading is exactly `span`.
fn word_at(c: &sweet_engine::Conversion, span: (usize, usize)) -> Option<String> {
    let mut at = 0;
    for w in c.segments.iter().flat_map(|s| &s.words) {
        let end = at + w.reading.len();
        if (at, end) == span {
            return Some(w.surface.clone());
        }
        at = end;
    }
    None
}
