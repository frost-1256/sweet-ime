//! sweet-cli: interactive conversion REPL and accuracy/latency evaluation.
//!
//!   sweet-cli <dict> repl
//!   sweet-cli <dict> eval <evaluation.tsv>
//!   sweet-cli <dict> bench

use anyhow::{Result, bail};
use std::io::{BufRead, Write};
use std::time::Instant;
use sweet_engine::{Converter, Dictionary, romaji};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        bail!("usage: sweet-cli <dict> (repl | eval <tsv> | bench)");
    }
    let t0 = Instant::now();
    let mut conv = Converter::new(Dictionary::open(&args[1])?);
    if let Ok(path) = std::env::var("SWEET_ZENZ") {
        let t = Instant::now();
        let mut r = sweet_zenz::ZenzReranker::new(sweet_zenz::Zenz::load(&path)?);
        if let Some(n) = std::env::var("SWEET_RERANK_N").ok().and_then(|v| v.parse().ok()) {
            r.n = n;
        }
        if let Some(m) = std::env::var("SWEET_MIX").ok().and_then(|v| v.parse().ok()) {
            r.mix = m;
        }
        eprintln!("zenz model {path} loaded in {:.2?}", t.elapsed());
        conv = conv.with_reranker(Box::new(r));
    }
    eprintln!(
        "loaded {} entries in {:.2?}",
        conv.dictionary().entry_count(),
        t0.elapsed()
    );
    match args[2].as_str() {
        "repl" => repl(&conv),
        "eval" => eval(&conv, args.get(3).map(String::as_str).unwrap_or("evaluation.tsv")),
        "bench" => bench(&conv),
        other => bail!("unknown command {other}"),
    }
}

fn input_to_kana(line: &str) -> String {
    if line.is_ascii() { romaji::to_hiragana(line) } else { line.to_string() }
}

fn repl(conv: &Converter) -> Result<()> {
    let stdin = std::io::stdin();
    print!("> ");
    std::io::stdout().flush()?;
    for line in stdin.lock().lines() {
        let line = line?;
        let kana = input_to_kana(line.trim());
        let t = Instant::now();
        let res = conv.convert_best(&kana, &[], "");
        let el = t.elapsed();
        println!("{kana}  ({el:.2?})");
        for (i, c) in res.iter().enumerate() {
            let segs: Vec<String> = c.segments.iter().map(|s| s.surface.clone()).collect();
            println!("  {i}: {}   [{}] cost={}", c.surface(), segs.join("|"), c.cost);
        }
        if let Some(best) = res.first() {
            for s in &best.segments {
                let cands = conv.segment_candidates(&s.reading, 8);
                println!("    {} -> {}", s.reading, cands.join(" / "));
            }
        }
        print!("> ");
        std::io::stdout().flush()?;
    }
    Ok(())
}

fn eval(conv: &Converter, path: &str) -> Result<()> {
    // evaluation.tsv columns: status (Mozc OSS result), input, Mozc output,
    // command, argument. Only conversion commands are scored.
    let text = std::fs::read_to_string(path)?;
    let (mut cases, mut ok, mut mozc_ok, mut agree) = (0, 0, 0, 0);
    // [top-1 tests, top-N tests] x [cases, sweet ok, mozc ok]
    let mut split = [[0i32; 3]; 2];
    let mut worst = std::time::Duration::ZERO;
    let mut sum = std::time::Duration::ZERO;
    let verbose = std::env::var_os("SWEET_VERBOSE").is_some();
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 5 || !f[3].starts_with("Conversion") {
            continue;
        }
        let (status, input, mozc_out, command, arg) = (f[0], f[1], f[2], f[3], f[4]);
        let top_n: usize = command
            .strip_prefix("Conversion Expected")
            .and_then(|r| r.trim().parse().ok())
            .unwrap_or(1);
        let t = Instant::now();
        let mut res = conv.convert_best(input, &[], "");
        if top_n > 1 {
            // Candidate-list tests: best first, then the lattice N-best.
            for c in conv.convert(input, &[], top_n.min(50)) {
                if !res.iter().any(|r| r.surface() == c.surface()) {
                    res.push(c);
                }
            }
        }
        let el = t.elapsed();
        if std::env::var_os("SWEET_PROF").is_some() { eprintln!("case {:.2?}", el); }
        sum += el;
        worst = worst.max(el);
        cases += 1;
        let surfaces: Vec<String> = res.iter().map(|c| c.surface()).collect();
        let top = surfaces.first().cloned().unwrap_or_default();
        if status == "OK:" {
            mozc_ok += 1;
        }
        if top == mozc_out {
            agree += 1;
        }
        let pass = if command.starts_with("Conversion Expected") {
            surfaces.iter().take(top_n).any(|s| s == arg)
        } else {
            let re = regex::Regex::new(&format!("^(?:{arg})$"))?;
            match command {
                "Conversion Match" => re.is_match(&top),
                "Conversion Not Match" => !re.is_match(&top),
                _ => continue,
            }
        };
        let bucket = &mut split[(top_n > 1) as usize];
        bucket[0] += 1;
        bucket[1] += pass as i32;
        bucket[2] += (status == "OK:") as i32;
        if pass {
            ok += 1;
        } else if verbose {
            println!("FAIL [{command}] {arg}  {input} -> {top}  (mozc {status} {mozc_out})");
        }
    }
    let pct = |a: i32| 100.0 * a as f64 / cases as f64;
    println!("sweet pass: {ok}/{cases} ({:.1}%)   mozc-oss pass: {mozc_ok}/{cases} ({:.1}%)   top1 == mozc: {:.1}%",
        pct(ok), pct(mozc_ok), pct(agree));
    for (name, b) in [("top-1 tests", split[0]), ("top-N tests", split[1])] {
        println!("  {name}: sweet {}/{} ({:.1}%)  mozc {}/{} ({:.1}%)", b[1], b[0],
            100.0 * b[1] as f64 / b[0] as f64, b[2], b[0], 100.0 * b[2] as f64 / b[0] as f64);
    }
    println!("latency: mean {:.2?}, max {:.2?}", sum / cases as u32, worst);
    Ok(())
}

fn bench(conv: &Converter) -> Result<()> {
    let inputs = [
        "きょうはいいてんきですね",
        "わたしはにほんごのにゅうりょくほうしきをつくっています",
        "こうそくでせいかくなかなかんじへんかんをめざす",
        "とうきょうとっきょきょかきょく",
    ];
    for s in inputs {
        let t = Instant::now();
        let iters = 200;
        let mut r = Vec::new();
        for _ in 0..iters {
            r = conv.convert(s, &[], 10);
        }
        let el = t.elapsed() / iters;
        println!("{el:>10.2?}  {} -> {}", s, r.first().map(|c| c.surface()).unwrap_or_default());
    }
    Ok(())
}
