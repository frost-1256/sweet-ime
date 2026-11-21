use sweet_zenz::Zenz;
fn main() {
    let z = Zenz::load(std::env::args().nth(1).unwrap()).unwrap();
    let r = "にほんちーむのえんぎはこのあとすぐです";
    let p = Zenz::prompt(r, "");
    let c: Vec<String> = ["日本チームの演技はこのあとすぐです","日本チームの演技派このあとすぐです","日本チームの縁起はこのあとすぐです","日本チームの演技はこの後すぐです","日本チームの演技はこのあと直ぐです"].iter().map(|s| s.to_string()).collect();
    let pt = z.tokenizer.encode(&p).len();
    let ct: usize = c.iter().map(|s| z.tokenizer.encode(s).len()).sum();
    for _ in 0..3 {
        let t = std::time::Instant::now();
        let s = z.score(&p, &c);
        println!("prompt {pt} tok, cands {ct} tok: {:.2?} {:?}", t.elapsed(), s);
    }
    let mut cache = z.model.new_cache();
    let ids: Vec<u32> = (0..60).map(|i| 300 + i).collect();
    let t = std::time::Instant::now();
    z.model.forward(&ids, &mut cache, false);
    println!("seq forward 60 tok: {:.2?}", t.elapsed());
    let parents: Vec<Option<usize>> = (0..60).map(|i: usize| i.checked_sub(1)).collect();
    let mut cache = z.model.new_cache();
    let t = std::time::Instant::now();
    z.model.forward_tree(&ids, &parents, &mut cache, 59);
    println!("tree forward 60 tok: {:.2?}", t.elapsed());
}
