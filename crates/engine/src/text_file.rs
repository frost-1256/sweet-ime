//! Decoding of text files that other input methods export.

/// Text from a file: UTF-16 with a byte order mark (Microsoft IME and
/// Google Japanese Input exports), UTF-16 without one (see
/// [`utf16_without_bom`]), or else UTF-8 with or without a BOM.
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
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        // Text files here are tab- or line-separated, so a UTF-16 reading
        // without a single tab or line end is a misreading of UTF-8 text.
        _ => utf16_without_bom(bytes)
            .map(|le| utf16(bytes, le))
            .filter(|text| text.contains(['\t', '\n']))
            .unwrap_or_else(|| String::from_utf8_lossy(bytes).into_owned()),
    }
}

/// Byte order of UTF-16 text without a byte order mark: `Some(true)` for
/// little endian. UTF-8 text has no zero bytes, while every ASCII character
/// in UTF-16 (tabs and line ends at least) has one, on the odd bytes in
/// little endian and on the even bytes in big endian. The zeros must lean
/// clearly to one side and make up at least one character in sixteen, so a
/// stray NUL in UTF-8 text does not count.
fn utf16_without_bom(bytes: &[u8]) -> Option<bool> {
    if bytes.len() < 2 || bytes.len() % 2 != 0 {
        return None;
    }
    let zeros = |start: usize| {
        bytes
            .iter()
            .skip(start)
            .step_by(2)
            .filter(|&&b| b == 0)
            .count()
    };
    let (even, odd) = (zeros(0), zeros(1));
    let (most, least, le) = if odd > even {
        (odd, even, true)
    } else {
        (even, odd, false)
    };
    let units = bytes.len() / 2;
    (most >= 2 * least && most * 16 >= units).then_some(le)
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
        // A table whose first character is not ASCII.
        let japanese_first = "表\t\na\tあ\n";
        for text in [text, japanese_first] {
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
        assert_eq!(decode_text(japanese_first.as_bytes()), japanese_first);
        // A stray NUL in a longer UTF-8 file stays UTF-8.
        let stray = format!("{}a\0", "あ\tい\n".repeat(8));
        assert_eq!(decode_text(stray.as_bytes()), stray);
        // A short UTF-8 file whose zeros would pass for UTF-16.
        let short = "xxx\0\tい\nka\tか\n";
        assert_eq!(decode_text(short.as_bytes()), short);
    }
}
