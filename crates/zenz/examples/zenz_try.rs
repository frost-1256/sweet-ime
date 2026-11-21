//! Greedy-decode a few readings: `zenz_try <model dir> [bos|nobos]`.
use sweet_zenz::Zenz;
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let t = std::time::Instant::now();
    let mut z = Zenz::load(&args[1]).expect("load");
    println!("loaded in {:.2?}  bos={} eos={}", t.elapsed(), z.tokenizer.bos, z.tokenizer.eos);
    z.add_bos = args.get(2).map(|s| s != "nobos").unwrap_or(true);
    for (r, ctx) in [
        ("きょうはいいてんきですね", ""),
        ("かいごはほんとうにたいへんです", ""),
        ("ほしゅてきなめんもあるね", ""),
        ("きしゃのきしゃがきしゃできしゃした", ""),
        ("あせをすいそうなので", ""),
        ("しあげはあしたにしよう", ""),
        ("こうえん", "明日は音楽の"),
    ] {
        let p = Zenz::prompt(r, ctx);
        let t = std::time::Instant::now();
        let out = z.generate(&p, 40);
        println!("{:>9.2?}  {r} -> {out}", t.elapsed());
    }
}
