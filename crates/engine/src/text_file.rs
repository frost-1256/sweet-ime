//! Decoding of text files that other input methods export.

/// Text from a file: UTF-16 with a byte order mark (Microsoft IME and
/// Google Japanese Input exports), UTF-16 without one (an ASCII first
/// character next to a zero byte), or else UTF-8 with or without a BOM.
pub fn decode_text(bytes: &[u8]) -> String {
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
    match bytes {
        [0xFF, 0xFE, rest @ ..] => utf16(rest, true),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, false),
        [a, 0, ..] if *a != 0 && a.is_ascii() => utf16(bytes, true),
        [0, b, ..] if *b != 0 && b.is_ascii() => utf16(bytes, false),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
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
    fn decodes_utf8_and_utf16_with_or_without_a_bom() {
        let text = "kya\tきゃ\ntt\tっ\tt\n";
        assert_eq!(decode_text(text.as_bytes()), text);
        assert_eq!(
            decode_text(&[b"\xEF\xBB\xBF".as_slice(), text.as_bytes()].concat()),
            text
        );
        for le in [true, false] {
            for bom in [true, false] {
                assert_eq!(
                    decode_text(&utf16(text, le, bom)),
                    text,
                    "le={le} bom={bom}"
                );
            }
        }
    }
}
