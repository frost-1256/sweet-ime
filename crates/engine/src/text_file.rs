//! Decoding of text files that other input methods export.

/// Text from a file: UTF-16 with a byte order mark (Microsoft IME and
/// Google Japanese Input exports), or else UTF-8 with or without a BOM.
///
/// Without a byte order mark, UTF-16 and UTF-8 with NUL bytes cannot be told
/// apart reliably, and a wrong guess turns into wrong rows. Such a file is an
/// error, so the caller keeps what it had instead of saving garbage.
pub fn decode_text(bytes: &[u8]) -> std::io::Result<String> {
    let utf16 = |b: &[u8], le: bool| {
        let units: Vec<u16> = b
            .chunks_exact(2)
            .map(|c| {
                if le {
                    u16::from_le_bytes([c[0], c[1]])
                } else {
                    u16::from_be_bytes([c[0], c[1]])
                }
            })
            .collect();
        String::from_utf16_lossy(&units)
    };
    Ok(match bytes {
        [0xFF, 0xFE, rest @ ..] => utf16(rest, true),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, false),
        _ => {
            let text = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
            if text.contains(&0) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "the file has NUL bytes but no byte order mark; \
                     save it as UTF-8, or as UTF-16 with a byte order mark",
                ));
            }
            String::from_utf8_lossy(text).into_owned()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::decode_text;

    fn utf16(text: &str, le: bool, bom: bool) -> Vec<u8> {
        let mut out = Vec::new();
        for unit in bom.then_some(0xFEFF).into_iter().chain(text.encode_utf16()) {
            out.extend(if le {
                unit.to_le_bytes()
            } else {
                unit.to_be_bytes()
            });
        }
        out
    }

    #[test]
    fn decodes_utf8_and_utf16_with_a_bom() {
        let text = "kya\tきゃ\ntt\tっ\tt\n";
        let japanese_first = "表\t\na\tあ\n";
        for text in [text, japanese_first] {
            assert_eq!(decode_text(text.as_bytes()).unwrap(), text);
            let with_bom = [b"\xEF\xBB\xBF".as_slice(), text.as_bytes()].concat();
            assert_eq!(decode_text(&with_bom).unwrap(), text);
            for le in [true, false] {
                assert_eq!(
                    decode_text(&utf16(text, le, true)).unwrap(),
                    text,
                    "le={le}"
                );
            }
        }
    }

    #[test]
    fn rejects_text_that_cannot_be_told_apart_without_a_bom() {
        // UTF-16 without a byte order mark, including mostly non-ASCII text
        // whose zero bytes are few.
        let long = format!("表\t{}\na\tあ\n", "《あ〉い》".repeat(20));
        for text in ["kya\tきゃ\n", "表\t\na\tあ\n", long.as_str()] {
            for le in [true, false] {
                assert!(
                    decode_text(&utf16(text, le, false)).is_err(),
                    "{text:?} le={le}"
                );
            }
        }
        // UTF-8 with NUL bytes, which could also be read as UTF-16.
        for text in ["xxx\0\tい\nka\tか\n", "aa\t\0\tい\nka\tか\n"] {
            assert!(decode_text(text.as_bytes()).is_err(), "{text:?}");
        }
    }
}
