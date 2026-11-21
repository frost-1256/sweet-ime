use sweet_zenz::Zenz;
fn main() {
    let dir = std::env::args().nth(1).unwrap();
    let z = Zenz::load(&dir).unwrap();
    for t in [1usize, 5, 15, 30] {
        let toks: Vec<u32> = (0..t as u32).map(|i| 300 + i).collect();
        let iters = 50;
        let s = std::time::Instant::now();
        for _ in 0..iters {
            let mut c = z.model.new_cache();
            std::hint::black_box(z.model.forward(&toks, &mut c, true));
        }
        let el = s.elapsed() / iters;
        println!("T={t:>2}: {el:>9.2?}  ({:.2?}/token)", el / t as u32);
    }
    let p = Zenz::prompt("きょうはいいてんきですね", "");
    let c: Vec<String> = ["今日はいい天気ですね", "今日は良い天気ですね", "京はいい天気ですね", "今日はいい転機ですね", "教派いい天気ですね"].iter().map(|s| s.to_string()).collect();
    let s = std::time::Instant::now();
    for _ in 0..20 { std::hint::black_box(z.score(&p, &c)); }
    println!("score 5 cands: {:.2?}", s.elapsed() / 20);
}
