//! Tries typo correction: `typo_probe <dict> <romaji>...`
use std::time::{Duration, Instant};

use sweet_engine::{Converter, Dictionary, romaji, typo};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let conv = Converter::new(Dictionary::open(&args[1]).expect("dict"));
    for raw in &args[2..] {
        let typed = romaji::to_hiragana(raw);
        let orig = conv.convert(&typed, &[], 1);
        let t = Instant::now();
        let c = typo::correct(&conv, raw, 0, Duration::from_millis(200));
        let n = typo::variants(raw, 12).len();
        println!(
            "{raw:<22} {typed} -> {} (cost {})  fix: {:?}  [{n} variants, {:.1?}]",
            orig.first().map(|c| c.surface()).unwrap_or_default(),
            orig.first().map(|c| c.cost).unwrap_or(0),
            c.map(|c| format!("{} {} {}", c.raw, c.reading, c.surface)),
            t.elapsed()
        );
    }
}
