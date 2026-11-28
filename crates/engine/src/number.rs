//! Numbers read out in kana ("さんぜんにひゃく") parsed into values, so the
//! converter can offer 三千二百 and 3200 even when the dictionary has no
//! entry for the whole reading.

use std::sync::OnceLock;

/// A number reading found at the start of a text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NumberReading {
    /// Length of the reading in bytes.
    pub len: usize,
    pub value: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tok {
    /// A digit; `sokuon` forms (いっ, はっ) only come before a unit or a counter.
    Digit { v: u64, sokuon: bool },
    /// 十, 百 or 千; `sokuon` forms (じゅっ, ひゃっ) end the number.
    Unit { v: u64, sokuon: bool },
    /// 万, 億 or 兆.
    Big(u64),
}

const TOKENS: &[(&str, Tok)] = {
    use Tok::*;
    &[
        (
            "いち",
            Digit {
                v: 1,
                sokuon: false,
            },
        ),
        ("いっ", Digit { v: 1, sokuon: true }),
        (
            "に",
            Digit {
                v: 2,
                sokuon: false,
            },
        ),
        (
            "さん",
            Digit {
                v: 3,
                sokuon: false,
            },
        ),
        (
            "よん",
            Digit {
                v: 4,
                sokuon: false,
            },
        ),
        (
            "ご",
            Digit {
                v: 5,
                sokuon: false,
            },
        ),
        (
            "ろく",
            Digit {
                v: 6,
                sokuon: false,
            },
        ),
        ("ろっ", Digit { v: 6, sokuon: true }),
        (
            "なな",
            Digit {
                v: 7,
                sokuon: false,
            },
        ),
        (
            "しち",
            Digit {
                v: 7,
                sokuon: false,
            },
        ),
        (
            "はち",
            Digit {
                v: 8,
                sokuon: false,
            },
        ),
        ("はっ", Digit { v: 8, sokuon: true }),
        (
            "きゅう",
            Digit {
                v: 9,
                sokuon: false,
            },
        ),
        (
            "じゅう",
            Unit {
                v: 10,
                sokuon: false,
            },
        ),
        (
            "じゅっ",
            Unit {
                v: 10,
                sokuon: true,
            },
        ),
        (
            "じっ",
            Unit {
                v: 10,
                sokuon: true,
            },
        ),
        (
            "ひゃく",
            Unit {
                v: 100,
                sokuon: false,
            },
        ),
        (
            "びゃく",
            Unit {
                v: 100,
                sokuon: false,
            },
        ),
        (
            "ぴゃく",
            Unit {
                v: 100,
                sokuon: false,
            },
        ),
        (
            "ひゃっ",
            Unit {
                v: 100,
                sokuon: true,
            },
        ),
        (
            "ぴゃっ",
            Unit {
                v: 100,
                sokuon: true,
            },
        ),
        (
            "せん",
            Unit {
                v: 1000,
                sokuon: false,
            },
        ),
        (
            "ぜん",
            Unit {
                v: 1000,
                sokuon: false,
            },
        ),
        ("まん", Big(10_000)),
        ("おく", Big(100_000_000)),
        ("ちょう", Big(1_000_000_000_000)),
    ]
};

fn tokens_starting_with(text: &str) -> &'static [(&'static str, Tok)] {
    const KANA_COUNT: usize = 'ゖ' as usize - 'ぁ' as usize + 1;
    static BY_INITIAL: OnceLock<[Vec<(&'static str, Tok)>; KANA_COUNT]> = OnceLock::new();
    let Some(first @ 'ぁ'..='ゖ') = text.chars().next() else {
        return &[];
    };
    &BY_INITIAL.get_or_init(|| {
        let mut buckets = std::array::from_fn(|_| Vec::new());
        for &(reading, token) in TOKENS {
            let initial = reading.chars().next().expect("number token has a reading");
            buckets[initial as usize - 'ぁ' as usize].push((reading, token));
        }
        buckets
    })[first as usize - 'ぁ' as usize]
}

/// Whether `unit` may follow `prev` (sound changes: さんびゃく, ろっぴゃく,
/// さんぜん; and the plain forms elsewhere).
fn unit_follows(prev: Option<&str>, unit: &str) -> bool {
    match unit {
        "びゃく" => prev == Some("さん"),
        "ぴゃく" | "ぴゃっ" => matches!(prev, Some("ろっ" | "はっ")),
        "ぜん" => prev == Some("さん"),
        "ひゃく" | "ひゃっ" => !matches!(prev, Some("さん" | "ろっ" | "はっ")),
        "せん" => prev != Some("さん"),
        _ => true,
    }
}

/// Every number reading at the start of `text` made of at least two parts
/// (single words such as じゅう or まん are left to the dictionary), longest
/// last.
pub fn parse_prefixes(text: &str) -> Vec<NumberReading> {
    let mut out = Vec::new();
    let mut state = State::default();
    walk(text, 0, &mut state, &mut out);
    out.sort_by_key(|n| n.len);
    out.dedup_by_key(|n| n.len);
    out
}

#[derive(Clone, Copy, Default)]
struct State {
    /// Value of completed 万/億/兆 groups.
    total: u64,
    /// Value of the current group below 万.
    group: u64,
    /// Pending digit waiting for a unit.
    digit: Option<u64>,
    /// Smallest unit used in this group (units must decrease).
    last_unit: u64,
    /// Smallest big unit used (big units must decrease).
    last_big: u64,
    parts: usize,
    /// The previous token's reading, for sound changes.
    prev: Option<&'static str>,
    /// The number cannot continue (it ended in a sokuon unit).
    closed: bool,
}

fn walk(text: &str, pos: usize, st: &mut State, out: &mut Vec<NumberReading>) {
    if st.parts >= 2 {
        // The number may end here (a sokuon digit, as in じゅういっこ, ends
        // it before a counter).
        let value = st.total + st.group + st.digit.unwrap_or(0);
        if value > 0 {
            out.push(NumberReading { len: pos, value });
        }
    }
    if st.closed || st.parts >= 12 {
        return;
    }
    let rest = &text[pos..];
    for &(kana, tok) in tokens_starting_with(rest) {
        if !rest.starts_with(kana) {
            continue;
        }
        let saved = *st;
        let ok = match tok {
            Tok::Digit { v, .. } => {
                if st.digit.is_some() {
                    false
                } else {
                    st.digit = Some(v);
                    true
                }
            }
            Tok::Unit { v, sokuon } => {
                let allowed =
                    (st.last_unit == 0 || v < st.last_unit) && unit_follows(st.prev, kana);
                if allowed {
                    st.group += st.digit.take().unwrap_or(1) * v;
                    st.last_unit = v;
                    st.closed = sokuon;
                }
                allowed
            }
            Tok::Big(v) => {
                let group = st.group + st.digit.take().unwrap_or(0);
                let allowed = group > 0
                    && (st.last_big == 0 || v < st.last_big)
                    && !st.prev.is_some_and(|p| p.ends_with('っ'));
                if allowed {
                    st.total += group * v;
                    st.group = 0;
                    st.last_unit = 0;
                    st.last_big = v;
                }
                allowed
            }
        };
        // A sokuon digit must be followed by a unit or end the number.
        let sokuon_digit_ok = !matches!(saved.digit, Some(_))
            || !saved.prev.is_some_and(|p| p.ends_with('っ'))
            || matches!(tok, Tok::Unit { .. });
        if ok && sokuon_digit_ok {
            st.parts += 1;
            st.prev = Some(kana);
            walk(text, pos + kana.len(), st, out);
        }
        *st = saved;
    }
}

const KANJI_DIGITS: [&str; 10] = ["〇", "一", "二", "三", "四", "五", "六", "七", "八", "九"];

/// 0..=9999 in kanji (二千三十).
fn kanji_group(n: u64) -> String {
    let mut s = String::new();
    for (unit, label) in [(1000, "千"), (100, "百"), (10, "十")] {
        let d = (n / unit) % 10;
        if d > 0 {
            if d > 1 {
                s.push_str(KANJI_DIGITS[d as usize]);
            }
            s.push_str(label);
        }
    }
    let d = n % 10;
    if d > 0 {
        s.push_str(KANJI_DIGITS[d as usize]);
    }
    s
}

const BIG: [(u64, &str); 3] = [
    (1_000_000_000_000, "兆"),
    (100_000_000, "億"),
    (10_000, "万"),
];

/// A value in kanji numerals: 三千二百, 十万, 一億二千万.
pub fn to_kanji(mut n: u64) -> String {
    if n == 0 {
        return "〇".into();
    }
    let mut s = String::new();
    for (v, label) in BIG {
        let g = n / v;
        if g > 0 {
            s.push_str(&kanji_group(g));
            s.push_str(label);
            n %= v;
        }
    }
    s.push_str(&kanji_group(n));
    s
}

/// A value in Arabic digits with 万/億/兆 (10万, 1億2000万), or `None` below
/// 10000 (where it equals the plain digits).
pub fn to_arabic_big(mut n: u64) -> Option<String> {
    if n < 10_000 {
        return None;
    }
    let mut s = String::new();
    for (v, label) in BIG {
        let g = n / v;
        if g > 0 {
            s.push_str(&g.to_string());
            s.push_str(label);
            n %= v;
        }
    }
    if n > 0 {
        s.push_str(&n.to_string());
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(text: &str) -> Vec<(usize, u64)> {
        parse_prefixes(text)
            .into_iter()
            .map(|n| (text[..n.len].chars().count(), n.value))
            .collect()
    }

    #[test]
    fn parses_kana_numbers() {
        assert_eq!(values("さんぜんえん"), vec![(4, 3000)]);
        assert_eq!(values("にじゅうごびょう"), vec![(4, 20), (5, 25)]);
        assert_eq!(values("じゅうまんえん"), vec![(5, 100_000)]);
        assert_eq!(values("はっせんえん"), vec![(4, 8000)]);
        assert_eq!(values("ろっぴゃくにん"), vec![(5, 600), (6, 602)]);
        assert_eq!(
            values("いちおくにせんまん"),
            vec![
                (4, 100_000_000),
                (5, 100_000_002),
                (7, 100_002_000),
                (9, 120_000_000)
            ]
        );
        assert_eq!(values("さんじゅっぷん"), vec![(5, 30)]);
        assert_eq!(values("じゅうはちねん"), vec![(5, 18)]);
        // Wrong sound changes and lone words are not numbers.
        assert!(values("さんせん").is_empty());
        assert!(values("じゅうしょ").is_empty());
        assert!(values("いった").is_empty());
        assert!(
            values("にひゃくさんびゃく")
                .iter()
                .all(|&(_, v)| v == 200 || v == 203)
        );
    }

    #[test]
    fn formats_numbers() {
        assert_eq!(to_kanji(3000), "三千");
        assert_eq!(to_kanji(25), "二十五");
        assert_eq!(to_kanji(100_000), "十万");
        assert_eq!(to_kanji(10_000), "一万");
        assert_eq!(to_kanji(120_000_000), "一億二千万");
        assert_eq!(to_arabic_big(100_000).as_deref(), Some("10万"));
        assert_eq!(to_arabic_big(120_003_000).as_deref(), Some("1億2000万3000"));
        assert_eq!(to_arabic_big(9999), None);
    }
}
