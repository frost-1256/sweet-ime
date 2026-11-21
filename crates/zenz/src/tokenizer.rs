//! GPT-2 byte-level BPE tokenizer (vocab.json + merges.txt).

use std::collections::HashMap;
use std::io;
use std::path::Path;

pub struct Tokenizer {
    encoder: HashMap<String, u32>,
    decoder: Vec<String>,
    ranks: HashMap<(String, String), u32>,
    byte_to_char: [char; 256],
    char_to_byte: HashMap<char, u8>,
    pub bos: u32,
    pub eos: u32,
}

fn bytes_to_unicode() -> [char; 256] {
    let mut bs: Vec<u32> = (b'!' as u32..=b'~' as u32)
        .chain(0xA1..=0xAC)
        .chain(0xAE..=0xFF)
        .collect();
    let mut cs = bs.clone();
    let mut n = 0;
    for b in 0..256u32 {
        if !bs.contains(&b) {
            bs.push(b);
            cs.push(256 + n);
            n += 1;
        }
    }
    let mut table = ['\0'; 256];
    for (b, c) in bs.iter().zip(cs) {
        table[*b as usize] = char::from_u32(c).unwrap();
    }
    table
}

#[derive(PartialEq, Clone, Copy)]
enum Class {
    Letter,
    Number,
    Space,
    Other,
}

fn class(c: char) -> Class {
    if c.is_whitespace() {
        Class::Space
    } else if c.is_alphabetic() {
        Class::Letter
    } else if c.is_numeric() {
        Class::Number
    } else {
        Class::Other
    }
}

/// GPT-2 pre-tokenization, equivalent to
/// `'s|'t|'re|'ve|'m|'ll|'d| ?\p{L}+| ?\p{N}+| ?[^\s\p{L}\p{N}]+|\s+(?!\S)|\s+`.
pub fn pre_tokenize(text: &str) -> Vec<&str> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let byte_at = |i: usize| if i < chars.len() { chars[i].0 } else { text.len() };
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i].1;
        if c == '\'' {
            let rest = &text[chars[i].0..];
            if let Some(l) = ["'s", "'t", "'re", "'ve", "'m", "'ll", "'d"]
                .iter()
                .find(|p| rest.starts_with(**p))
                .map(|p| p.chars().count())
            {
                out.push(&text[chars[i].0..byte_at(i + l)]);
                i += l;
                continue;
            }
        }
        let mut j = i;
        let cls = if c == ' ' && i + 1 < chars.len() && class(chars[i + 1].1) != Class::Space {
            j += 1;
            class(chars[i + 1].1)
        } else {
            class(c)
        };
        if cls == Class::Space {
            while j < chars.len() && class(chars[j].1) == Class::Space {
                j += 1;
            }
            if j < chars.len() && j - i > 1 {
                j -= 1; // leave the last whitespace for the next token
            }
        } else {
            while j < chars.len() && class(chars[j].1) == cls {
                j += 1;
            }
        }
        out.push(&text[chars[i].0..byte_at(j)]);
        i = j;
    }
    out
}

impl Tokenizer {
    pub fn load(dir: impl AsRef<Path>) -> io::Result<Self> {
        let dir = dir.as_ref();
        Self::from_strings(
            &std::fs::read_to_string(dir.join("vocab.json"))?,
            &std::fs::read_to_string(dir.join("merges.txt"))?,
        )
    }

    pub fn from_strings(vocab_json: &str, merges: &str) -> io::Result<Self> {
        let vocab: HashMap<String, u32> =
            serde_json::from_str(vocab_json).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let mut ranks = HashMap::new();
        for (r, line) in merges.lines().filter(|l| !l.starts_with("#version")).enumerate() {
            if let Some((a, b)) = line.split_once(' ') {
                ranks.insert((a.to_string(), b.to_string()), r as u32);
            }
        }
        let mut decoder = vec![String::new(); vocab.values().max().map_or(0, |m| *m as usize + 1)];
        for (k, v) in &vocab {
            decoder[*v as usize] = k.clone();
        }
        let byte_to_char = bytes_to_unicode();
        let char_to_byte = byte_to_char.iter().enumerate().map(|(b, c)| (*c, b as u8)).collect();
        let bos = *vocab.get("<s>").unwrap_or(&1);
        let eos = *vocab.get("</s>").unwrap_or(&2);
        Ok(Self { encoder: vocab, decoder, ranks, byte_to_char, char_to_byte, bos, eos })
    }

    pub fn vocab_size(&self) -> usize {
        self.decoder.len()
    }

    fn bpe(&self, word: &str, out: &mut Vec<u32>) {
        let mut parts: Vec<String> = word.chars().map(|c| c.to_string()).collect();
        loop {
            let mut best: Option<(u32, usize)> = None;
            for i in 0..parts.len().saturating_sub(1) {
                if let Some(&r) = self.ranks.get(&(parts[i].clone(), parts[i + 1].clone())) {
                    if best.is_none_or(|(br, _)| r < br) {
                        best = Some((r, i));
                    }
                }
            }
            let Some((_, i)) = best else { break };
            let merged = format!("{}{}", parts[i], parts[i + 1]);
            parts[i] = merged;
            parts.remove(i + 1);
        }
        for p in parts {
            match self.encoder.get(&p) {
                Some(&id) => out.push(id),
                None => {
                    // Fall back to single byte-chars; unknown -> [UNK] (0).
                    for c in p.chars() {
                        out.push(*self.encoder.get(&c.to_string()).unwrap_or(&0));
                    }
                }
            }
        }
    }

    pub fn encode(&self, text: &str) -> Vec<u32> {
        let mut out = Vec::new();
        for piece in pre_tokenize(text) {
            let mapped: String = piece.bytes().map(|b| self.byte_to_char[b as usize]).collect();
            self.bpe(&mapped, &mut out);
        }
        out
    }

    pub fn decode(&self, ids: &[u32]) -> String {
        let mut bytes = Vec::new();
        for &id in ids {
            if let Some(tok) = self.decoder.get(id as usize) {
                for c in tok.chars() {
                    if let Some(&b) = self.char_to_byte.get(&c) {
                        bytes.push(b);
                    }
                }
            }
        }
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pretokenize() {
        assert_eq!(pre_tokenize("hello world"), vec!["hello", " world"]);
        assert_eq!(pre_tokenize("今日は\u{3000}晴れ"), vec!["今日は", "\u{3000}", "晴れ"]);
        assert_eq!(pre_tokenize("a  b"), vec!["a", " ", " b"]);
        assert_eq!(pre_tokenize("\u{EE00}カタ\u{EE01}"), vec!["\u{EE00}", "カタ", "\u{EE01}"]);
    }
}
