//! Measures keystroke typo correction on generated typos:
//! `typo_eval <dict> <eval.tsv>... [--per N] [--budget MS]`.
//!
//! Each expected reading is typed as romaji with one random slip (a
//! neighbouring key, a dropped, doubled or swapped key) and corrected as a
//! conversion would be. Also counts corrections made to inputs typed
//! correctly, which a fix must not break.
use std::collections::HashMap;
use std::time::{Duration, Instant};

use sweet_engine::{Converter, Dictionary, romaji, typo};

/// The margin the input session asks of a suggested fix.
const SUGGESTION_MARGIN: i32 = 1000;

const ROWS: &[&str] = &["qwertyuiop", "asdfghjkl", "zxcvbnm"];

/// Keys next to `c` on the same row (as on a phone keyboard, where slips
/// along the row are the common ones) and the keys above and below.
fn neighbours(c: char) -> Vec<char> {
    let mut out = Vec::new();
    for (r, row) in ROWS.iter().enumerate() {
        let Some(i) = row.find(c) else { continue };
        let row: Vec<char> = row.chars().collect();
        if i > 0 {
            out.push(row[i - 1]);
        }
        if i + 1 < row.len() {
            out.push(row[i + 1]);
        }
        for other in [r.wrapping_sub(1), r + 1] {
            if let Some(o) = ROWS.get(other) {
                let o: Vec<char> = o.chars().collect();
                for d in [i.wrapping_sub(1), i] {
                    if let Some(&k) = o.get(d) {
                        out.push(k);
                    }
                }
            }
        }
    }
    out
}

/// Kana -> romaji as typed with common Hepburn spellings.
fn kana_table() -> HashMap<String, String> {
    let mut sylls: Vec<String> = Vec::new();
    let vowels = ["a", "i", "u", "e", "o"];
    for c in ["", "k", "s", "t", "n", "h", "m", "y", "r", "w", "g", "z", "d", "b", "p", "f", "v", "j"] {
        for v in vowels {
            sylls.push(format!("{c}{v}"));
        }
    }
    for c in ["ky", "sh", "ch", "ny", "hy", "my", "ry", "gy", "by", "py", "ts", "th", "dh", "x", "j"] {
        for v in vowels {
            sylls.push(format!("{c}{v}"));
        }
    }
    for s in ["xtu", "xya", "xyu", "xyo", "xwa", "wo", "nn", "-"] {
        sylls.push(s.to_owned());
    }
    // Preferred spellings first: shi, chi, tsu, fu, ji over si, ti, tu, hu, zi.
    sylls.sort_by_key(|s| match s.as_str() {
        "shi" | "chi" | "tsu" | "fu" | "ji" | "sha" | "shu" | "sho" | "cha" | "chu" | "cho" | "ja" | "ju" | "jo" => 0,
        _ => 1,
    });
    let mut map = HashMap::new();
    for s in sylls {
        let k = romaji::to_hiragana(&s);
        if !k.bytes().any(|b| b.is_ascii_alphabetic()) {
            map.entry(k).or_insert(s);
        }
    }
    map
}

fn to_romaji(kana: &str, table: &HashMap<String, String>) -> Option<String> {
    let chars: Vec<char> = kana.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut sokuon = false;
    while i < chars.len() {
        if chars[i] == 'っ' {
            sokuon = true;
            i += 1;
            continue;
        }
        let two: String = chars[i..(i + 2).min(chars.len())].iter().collect();
        let one = chars[i].to_string();
        let (r, n) = match table.get(&two).filter(|_| two.chars().count() == 2) {
            Some(r) => (r.clone(), 2),
            None => (table.get(&one)?.clone(), 1),
        };
        if sokuon {
            out.push(r.chars().next().filter(|c| !"aiueon".contains(*c))?);
            sokuon = false;
        }
        out.push_str(&r);
        i += n;
    }
    // A single n before a vowel or y would read differently: nn is typed.
    (romaji::to_hiragana(&out) == kana).then_some(out)
}

struct Rng(u64);
impl Rng {
    fn next(&mut self, n: usize) -> usize {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % n
    }
}

fn slip(raw: &str, rng: &mut Rng) -> String {
    let b: Vec<char> = raw.chars().collect();
    loop {
        let i = rng.next(b.len());
        if !b[i].is_ascii_lowercase() {
            continue;
        }
        let mut v = b.clone();
        match rng.next(4) {
            0 => {
                let n = neighbours(b[i]);
                v[i] = n[rng.next(n.len())];
            }
            1 => {
                v.remove(i);
            }
            2 => {
                let n = neighbours(b[i]);
                v.insert(i + rng.next(2), n[rng.next(n.len())]);
            }
            _ => {
                if i + 1 >= b.len() || b[i] == b[i + 1] {
                    continue;
                }
                v.swap(i, i + 1);
            }
        }
        let s: String = v.into_iter().collect();
        if romaji::to_hiragana(&s) != romaji::to_hiragana(raw) {
            return s;
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut files = Vec::new();
    let mut per = 3;
    let mut budget = 12;
    let mut it = args[2..].iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--per" => per = it.next().unwrap().parse().unwrap(),
            "--budget" => budget = it.next().unwrap().parse().unwrap(),
            f => files.push(f.to_owned()),
        }
    }
    let conv = Converter::new(Dictionary::open(&args[1]).expect("dict"));
    let table = kana_table();
    let mut rng = Rng(0x5eed);
    let budget = Duration::from_millis(budget);
    // [stray letters, kana only] x [fixed, wrong, missed]
    let mut counts = [[0usize; 3]; 2];
    let (mut broken, mut clean) = (0, 0);
    let verbose = std::env::var_os("TYPO_VERBOSE").is_some();
    let mut worst = Duration::ZERO;
    let mut time = Duration::ZERO;
    for f in &files {
        for line in std::fs::read_to_string(f).expect("eval file").lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let cols: Vec<&str> = line.split('\t').collect();
            let Some(reading) = cols.get(1) else { continue };
            let Some(raw) = to_romaji(reading, &table) else { continue };
            if !(5..=64).contains(&raw.len()) {
                continue;
            }
            // Fully kana input only ever gets a suggestion, with a margin.
            clean += 1;
            if let Some(c) = typo::correct(&conv, &raw, SUGGESTION_MARGIN, budget) {
                broken += 1;
                if verbose {
                    println!("broke  {raw} -> {} ({reading})", c.reading);
                }
            }
            for _ in 0..per {
                let typed = slip(&raw, &mut rng);
                let kana = romaji::to_hiragana(&typed);
                let letters = kana.bytes().any(|b| b.is_ascii_alphabetic());
                // Stray letters are fixed on conversion; kana is suggested.
                let margin = if letters { 0 } else { SUGGESTION_MARGIN };
                let t = Instant::now();
                let c = typo::correct(&conv, &typed, margin, budget);
                let e = t.elapsed();
                time += e;
                worst = worst.max(e);
                let row = &mut counts[usize::from(!letters)];
                match c {
                    Some(c) if c.reading == *reading => row[0] += 1,
                    Some(c) => {
                        row[1] += 1;
                        if verbose {
                            println!("wrong{}  {typed} -> {} (want {reading})", u8::from(letters), c.reading);
                        }
                    }
                    None => {
                        row[2] += 1;
                        if verbose {
                            println!("missed{} {typed} {kana} (want {reading})", u8::from(letters));
                        }
                    }
                }
            }
        }
    }
    let pct = |a: usize, b: usize| 100.0 * a as f64 / b.max(1) as f64;
    for (name, [f, w, m]) in ["stray letters (fixed on conversion)", "kana only (suggested)"].iter().zip(counts) {
        let n = f + w + m;
        println!("{name}: {n} typos, right {f} ({:.1}%), wrong {w} ({:.1}%), none {m}", pct(f, n), pct(w, n));
    }
    println!("clean {clean}: suggested a change for {broken} ({:.1}%)", pct(broken, clean));
    let total: usize = counts.iter().flatten().sum();
    println!("time: mean {:.1?}, worst {worst:.1?}", time / total.max(1) as u32);
}
