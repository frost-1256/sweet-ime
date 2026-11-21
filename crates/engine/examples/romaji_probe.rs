//! Finds keystroke typos of a romaji word that produce a given preedit:
//! `romaji_probe <correct romaji> <observed preedit>`.
use sweet_engine::romaji::Composer;

fn preedit(s: &str) -> String {
    let mut c = Composer::new();
    c.push_str(s);
    c.preedit()
}

fn edits(s: &str) -> Vec<String> {
    let keys = "abcdefghijklmnopqrstuvwxyz";
    let b: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    for i in 0..=b.len() {
        for k in keys.chars() {
            let mut v = b.clone();
            v.insert(i, k);
            out.push(v.iter().collect());
        }
        if i < b.len() {
            let mut v = b.clone();
            v.remove(i);
            out.push(v.iter().collect());
            for k in keys.chars() {
                let mut v = b.clone();
                v[i] = k;
                out.push(v.iter().collect());
            }
        }
        if i + 1 < b.len() {
            let mut v = b.clone();
            v.swap(i, i + 1);
            out.push(v.iter().collect());
        }
    }
    out
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let want = &a[2];
    for e1 in edits(&a[1]) {
        if preedit(&e1) == *want {
            println!("1 edit: {e1}");
        }
        for e2 in edits(&e1) {
            if preedit(&e2) == *want {
                println!("2 edits: {e2}");
            }
        }
    }
}
