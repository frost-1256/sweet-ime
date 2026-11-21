//! Per-keystroke cost of live conversion and suggestions.
use std::time::Instant;
use sweet_engine::{Converter, Dictionary, Key, Options, Session};

fn main() {
    let dict = std::env::args().nth(1).unwrap_or("data/system.dict".into());
    let c = Converter::new(Dictionary::open(&dict).unwrap());
    for (live, sugg) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut s = Session::new();
        s.options = Options { live_conversion: live, suggestions: sugg };
        let t = Instant::now();
        let mut worst = 0f64;
        let text = "kyouhaiitenkidesunearigatougozaimasu";
        for ch in text.chars() {
            let k = Instant::now();
            s.handle(&c, Key::Char(ch));
            worst = worst.max(k.elapsed().as_secs_f64() * 1e3);
        }
        let n = text.len() as f64;
        println!("live={live:5} sugg={sugg:5}: mean {:.3}ms worst {:.3}ms", t.elapsed().as_secs_f64() * 1e3 / n, worst);
    }
    for p in ["あ", "あり", "ありが", "きょうは", "どうもありが", "わたしはでんしゃで", "きのうのかいぎのしりょ"] {
        let t = Instant::now();
        let r = c.predict_sentence(p, 5);
        println!("predict {p}: {:.3}ms {:?}", t.elapsed().as_secs_f64() * 1e3, r);
    }
}
