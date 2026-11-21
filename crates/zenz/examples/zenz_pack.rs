//! Pack a Hugging Face zenz model directory into a single mmap-able file:
//!   zenz_pack <hf model dir> <out.swz>
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let t = std::time::Instant::now();
    sweet_zenz::Zenz::pack(a[1].as_ref(), a[2].as_ref()).expect("pack");
    let t2 = std::time::Instant::now();
    let z = sweet_zenz::Zenz::load(&a[2]).expect("reload");
    let p = sweet_zenz::Zenz::prompt("きょうはいいてんきですね", "");
    println!("packed in {:.2?}, reloaded in {:.2?}: {}", t2 - t, t2.elapsed(), z.generate(&p, 30));
}
