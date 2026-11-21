//! Converts a Hugging Face zenz model directory (config.json,
//! model.safetensors, vocab.json, merges.txt) into the single mmap-able
//! `.swz` file the IME loads.
//!
//!   sweet-zenz-pack <hf model dir> <out.swz>

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: sweet-zenz-pack <hf model dir> <out.swz>");
        return ExitCode::from(2);
    }
    let (src, out) = (&args[1], &args[2]);
    let t = std::time::Instant::now();
    if let Err(e) = sweet_zenz::Zenz::pack(src.as_ref(), out.as_ref()) {
        eprintln!("pack failed: {e}");
        return ExitCode::FAILURE;
    }
    // Sanity check: reload and convert a sentence.
    match sweet_zenz::Zenz::load(out) {
        Ok(z) => {
            let p = sweet_zenz::Zenz::prompt("きょうはいいてんきですね", "");
            println!("packed {out} in {:.2?}; test: {}", t.elapsed(), z.generate(&p, 30));
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("reload failed: {e}");
            ExitCode::FAILURE
        }
    }
}
