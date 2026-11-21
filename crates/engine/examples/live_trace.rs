//! Prints the live preedit after each key.
use sweet_engine::{Converter, Dictionary, Key, Session};

fn main() {
    let c = Converter::new(Dictionary::open("data/system.dict").unwrap());
    for text in std::env::args().skip(1) {
        let mut s = Session::new();
        for ch in text.chars() {
            s.handle(&c, Key::Char(ch));
            println!("{ch} {}", s.preedit().text);
        }
        println!();
    }
}
