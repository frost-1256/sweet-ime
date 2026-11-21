//! Scores candidate conversions: `zenz_score <model> <reading> <cand>...`
//! (prefix `ctx=<text>` for left context). Also prints greedy decoding.
use sweet_zenz::Zenz;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let z = Zenz::load(&args[1]).expect("load");
    let mut rest = &args[2..];
    let mut ctx = String::new();
    if let Some(c) = rest.first().and_then(|a| a.strip_prefix("ctx=")) {
        ctx = c.to_string();
        rest = &rest[1..];
    }
    let reading = &rest[0];
    let cands: Vec<String> = rest[1..].to_vec();
    let p = Zenz::prompt(reading, &ctx);
    println!("greedy: {}", z.generate(&p, 40));
    for (c, s) in cands.iter().zip(z.score(&p, &cands)) {
        println!("{s:>9.3}  {c}");
    }
}
