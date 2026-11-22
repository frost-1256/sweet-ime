//! Builds the English word list index at compile time, so the IME doesn't
//! build it in every process that loads it (see `english::word_index`).

#![allow(dead_code)]

#[path = "src/romaji.rs"]
mod romaji;
#[path = "src/word_keys.rs"]
mod word_keys;

use std::collections::HashSet;
use std::path::PathBuf;

fn main() {
    let src = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("src");
    let read = |name: &str| {
        println!("cargo:rerun-if-changed=src/{name}");
        std::fs::read_to_string(src.join(name)).unwrap()
    };
    let (tech, vrchat, english, words) =
        (read("tech_terms.tsv"), read("vrchat.tsv"), read("english.tsv"), read("english_words.txt"));
    println!("cargo:rerun-if-changed=src/romaji.rs");
    println!("cargo:rerun-if-changed=src/word_keys.rs");

    let bundled = word_keys::bundled_readings(word_keys::bundled_spellings(&tech, &vrchat, &english));
    let bundled: HashSet<&str> = bundled.iter().map(|(r, _)| r.as_str()).collect();
    let (readings, entries) = word_keys::build_index(&words, &bundled);
    let max_len = entries.iter().map(|e| e.1 as usize).max().unwrap_or(0);

    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    std::fs::write(out.join("word_readings.txt"), readings).unwrap();
    std::fs::write(out.join("word_entries.bin"), word_keys::encode(&entries)).unwrap();
    std::fs::write(out.join("word_max_len.rs"), max_len.to_string()).unwrap();
}
