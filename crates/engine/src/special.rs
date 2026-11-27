//! Candidates computed from special readings: dates and times (きょう ->
//! 2026/11/22), punctuation (なかぐろ -> ・), emoji (えがお -> 😊),
//! kaomoji (かお) and arithmetic
//! (123・456= -> 0.2697…).

pub(crate) fn punctuation_candidates(reading: &str) -> &'static [&'static str] {
    match reading {
        "なかぐろ" | "なかてん" | "てん" => &["・", "･"],
        "さんてんりーだー" | "さんてん" | "てんてんてん" => &["…"],
        "かっこ" => &["()", "（）", "「」"],
        "おどりじ" | "くりかえし" | "おなじ" | "のま" => &["々"],
        _ => &[],
    }
}

/// Local date and time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DateTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
}

impl DateTime {
    /// The current local time.
    pub fn now() -> Self {
        local_now()
    }

    /// The date `days` days later (earlier if negative), same time.
    fn add_days(self, days: i64) -> Self {
        let (year, month, day) = civil_from_days(days_from_civil(self.year, self.month, self.day) + days);
        Self { year, month, day, ..self }
    }

    fn weekday(self) -> &'static str {
        // 1970-01-01 was a Thursday.
        let i = (days_from_civil(self.year, self.month, self.day) + 4).rem_euclid(7);
        ["日", "月", "火", "水", "木", "金", "土"][i as usize]
    }
}

/// Days since 1970-01-01 (proleptic Gregorian).
fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = i64::from(y) - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (i64::from(m) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((yoe + era * 400 + i64::from(m <= 2)) as i32, m, d)
}

#[cfg(windows)]
fn local_now() -> DateTime {
    #[repr(C)]
    #[derive(Default)]
    struct SystemTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        milliseconds: u16,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetLocalTime(st: *mut SystemTime);
    }
    let mut st = SystemTime::default();
    unsafe { GetLocalTime(&mut st) };
    DateTime {
        year: i32::from(st.year),
        month: u32::from(st.month),
        day: u32::from(st.day),
        hour: u32::from(st.hour),
        minute: u32::from(st.minute),
    }
}

#[cfg(unix)]
fn local_now() -> DateTime {
    // struct tm starts with nine ints on every Unix libc.
    #[repr(C)]
    struct Tm {
        sec: i32,
        min: i32,
        hour: i32,
        mday: i32,
        mon: i32,
        year: i32,
        wday: i32,
        yday: i32,
        isdst: i32,
        // tm_gmtoff and tm_zone (glibc, BSD); room to spare.
        _rest: [u64; 4],
    }
    unsafe extern "C" {
        fn time(t: *mut i64) -> i64;
        fn localtime_r(t: *const i64, out: *mut Tm) -> *mut Tm;
    }
    let mut tm = Tm { sec: 0, min: 0, hour: 0, mday: 1, mon: 0, year: 70, wday: 0, yday: 0, isdst: 0, _rest: [0; 4] };
    unsafe {
        let t = time(std::ptr::null_mut());
        localtime_r(&t, &mut tm);
    }
    DateTime {
        year: tm.year + 1900,
        month: (tm.mon + 1) as u32,
        day: tm.mday as u32,
        hour: tm.hour as u32,
        minute: tm.min as u32,
    }
}

#[cfg(not(any(windows, unix)))]
fn local_now() -> DateTime {
    DateTime { year: 1970, month: 1, day: 1, hour: 0, minute: 0 }
}

/// 令和 year for dates from 2019-05-01.
fn reiwa(t: DateTime) -> Option<i32> {
    (t.year > 2019 || (t.year == 2019 && t.month >= 5)).then(|| t.year - 2018)
}

fn date_forms(t: DateTime) -> Vec<String> {
    let mut v = vec![
        format!("{}/{:02}/{:02}", t.year, t.month, t.day),
        format!("{}年{}月{}日", t.year, t.month, t.day),
        format!("{}月{}日({})", t.month, t.day, t.weekday()),
        format!("{}-{:02}-{:02}", t.year, t.month, t.day),
    ];
    if let Some(r) = reiwa(t) {
        v.push(format!("令和{}年{}月{}日", if r == 1 { "元".to_string() } else { r.to_string() }, t.month, t.day));
    }
    v.push(format!("{}曜日", t.weekday()));
    v
}

fn time_forms(t: DateTime) -> Vec<String> {
    let (ampm, h12) = if t.hour < 12 { ("午前", t.hour) } else { ("午後", t.hour - 12) };
    vec![
        format!("{}:{:02}", t.hour, t.minute),
        format!("{}時{}分", t.hour, t.minute),
        format!("{ampm}{h12}時{}分", t.minute),
    ]
}

fn is_date_reading(reading: &str) -> bool {
    matches!(
        reading,
        "きょう"
            | "ほんじつ"
            | "ひづけ"
            | "あした"
            | "あす"
            | "みょうにち"
            | "あさって"
            | "きのう"
            | "さくじつ"
            | "おととい"
            | "いま"
            | "じこく"
            | "げんざい"
            | "ことし"
            | "こんねん"
            | "こんげつ"
            | "ようび"
    )
}

fn is_ascii_digits(s: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit())
}

fn is_date_form(s: &str) -> bool {
    // YYYY/MM/DD or YYYY-MM-DD
    for sep in ['/', '-'] {
        let mut parts = s.split(sep);
        if let (Some(y), Some(m), Some(d), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
            && is_ascii_digits(y, 4, 4)
            && is_ascii_digits(m, 2, 2)
            && is_ascii_digits(d, 2, 2)
        {
            return true;
        }
    }
    // YYYY年M月D日 or 令和(元|N)年M月D日
    if let Some((year, rest)) = s.split_once('年')
        && let Some((month, rest)) = rest.split_once('月')
        && let Some((day, "")) = rest.split_once('日')
        && is_ascii_digits(month, 1, 2)
        && is_ascii_digits(day, 1, 2)
    {
        if is_ascii_digits(year, 4, 4) {
            return true;
        }
        if let Some(era) = year.strip_prefix("令和")
            && (era == "元" || is_ascii_digits(era, 1, 3))
        {
            return true;
        }
    }
    // M月D日(曜)
    if let Some((month, rest)) = s.split_once('月')
        && let Some((day, rest)) = rest.split_once("日(")
        && let Some(wd) = rest.strip_suffix(')')
        && is_ascii_digits(month, 1, 2)
        && is_ascii_digits(day, 1, 2)
        && matches!(wd, "日" | "月" | "火" | "水" | "木" | "金" | "土")
    {
        return true;
    }
    // X曜日
    matches!(
        s,
        "日曜日" | "月曜日" | "火曜日" | "水曜日" | "木曜日" | "金曜日" | "土曜日"
    )
}

fn is_time_form(s: &str) -> bool {
    // H:MM or HH:MM
    if let Some((h, m)) = s.split_once(':')
        && is_ascii_digits(h, 1, 2)
        && is_ascii_digits(m, 2, 2)
    {
        return true;
    }
    // H時M分 or (午前|午後)H時M分
    let rest = s
        .strip_prefix("午前")
        .or_else(|| s.strip_prefix("午後"))
        .unwrap_or(s);
    if let Some((h, rest)) = rest.split_once('時')
        && let Some((m, "")) = rest.split_once('分')
        && is_ascii_digits(h, 1, 2)
        && is_ascii_digits(m, 1, 2)
    {
        return true;
    }
    false
}

fn is_year_or_month_form(s: &str) -> bool {
    // YYYY年 or 令和(元|N)年
    if let Some(year) = s.strip_suffix('年') {
        if is_ascii_digits(year, 4, 4) {
            return true;
        }
        if let Some(era) = year.strip_prefix("令和")
            && (era == "元" || is_ascii_digits(era, 1, 3))
        {
            return true;
        }
    }
    // M月 or YYYY年M月
    if let Some(rest) = s.strip_suffix('月') {
        if is_ascii_digits(rest, 1, 2) {
            return true;
        }
        if let Some((y, m)) = rest.split_once('年')
            && is_ascii_digits(y, 4, 4)
            && is_ascii_digits(m, 1, 2)
        {
            return true;
        }
    }
    false
}

/// True if `(reading, surface)` is a time- or calculation-dependent candidate
/// that should not be saved to user learning history.
pub fn is_ephemeral_candidate(reading: &str, surface: &str) -> bool {
    if is_date_reading(reading) {
        if is_date_form(surface) || is_time_form(surface) || is_year_or_month_form(surface) {
            return true;
        }
        // "YYYY/MM/DD HH:MM"
        if let Some((d, t)) = surface.split_once(' ')
            && is_date_form(d)
            && is_time_form(t)
        {
            return true;
        }
        return false;
    }
    calculator_candidates(reading).iter().any(|c| c == surface)
}

/// Date and time candidates for `reading` at time `now`.
pub fn date_candidates(reading: &str, now: DateTime) -> Vec<String> {
    let day = |n| date_forms(now.add_days(n));
    match reading {
        "きょう" | "ほんじつ" | "ひづけ" => day(0),
        "あした" | "あす" | "みょうにち" => day(1),
        "あさって" => day(2),
        "きのう" | "さくじつ" => day(-1),
        "おととい" => day(-2),
        "いま" | "じこく" | "げんざい" => {
            let mut v = time_forms(now);
            v.push(format!("{} {}", date_forms(now)[0], time_forms(now)[0]));
            v
        }
        "ことし" | "こんねん" => {
            let mut v = vec![format!("{}年", now.year)];
            if let Some(r) = reiwa(now) {
                v.push(format!("令和{r}年"));
            }
            v
        }
        "こんげつ" => vec![format!("{}月", now.month), format!("{}年{}月", now.year, now.month)],
        "ようび" => vec![format!("{}曜日", now.weekday())],
        _ => Vec::new(),
    }
}

const EMOJI: &str = include_str!("emoji.tsv");

/// Emoji and kaomoji for `reading` (えがお, はーと, かお, かおもじ).
pub fn emoji_candidates(reading: &str) -> Vec<String> {
    EMOJI
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once('\t'))
        .filter(|(r, _)| r.split(',').any(|r| r == reading))
        .flat_map(|(_, list)| list.split(' ').filter(|s| !s.is_empty()).map(str::to_string))
        .collect()
}

/// The value of an arithmetic expression typed as kana input ending in
/// `=` ("123・456=" from 123/456=), as candidates: the value, then the
/// expression with it.
pub fn calculator_candidates(reading: &str) -> Vec<String> {
    let Some(expr) = reading.strip_suffix('=').or_else(|| reading.strip_suffix('＝')) else { return Vec::new() };
    // Kana input turns some operators into Japanese punctuation.
    let ascii: String = expr
        .chars()
        .map(|c| match c {
            '・' | '／' | '÷' => '/',
            'ー' | '－' | '−' => '-',
            '。' | '．' => '.',
            '＋' => '+',
            '＊' | '×' => '*',
            '（' => '(',
            '）' => ')',
            '０'..='９' => char::from_u32(c as u32 - '０' as u32 + '0' as u32).unwrap_or(c),
            c => c,
        })
        .filter(|c| !c.is_whitespace())
        .collect();
    // An expression, not just a number: at least one operator after a digit.
    if ascii.is_empty()
        || !ascii.chars().all(|c| c.is_ascii_digit() || "+-*/().^%".contains(c))
        || !ascii.chars().skip(1).any(|c| "+-*/^%".contains(c))
    {
        return Vec::new();
    }
    let Some(v) = Parser { s: ascii.as_bytes(), i: 0 }.parse() else { return Vec::new() };
    if !v.is_finite() {
        return Vec::new();
    }
    let value = format_number(v);
    vec![value.clone(), format!("{ascii}={value}")]
}

fn format_number(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let s = format!("{v:.10}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_string()
}

/// Recursive descent over + - * / % ^ and parentheses.
struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn parse(mut self) -> Option<f64> {
        let v = self.sum()?;
        (self.i == self.s.len()).then_some(v)
    }
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }
    fn sum(&mut self) -> Option<f64> {
        let mut v = self.product()?;
        while let Some(op @ (b'+' | b'-')) = self.peek() {
            self.i += 1;
            let r = self.product()?;
            v = if op == b'+' { v + r } else { v - r };
        }
        Some(v)
    }
    fn product(&mut self) -> Option<f64> {
        let mut v = self.power()?;
        while let Some(op @ (b'*' | b'/' | b'%')) = self.peek() {
            self.i += 1;
            let r = self.power()?;
            v = match op {
                b'*' => v * r,
                b'/' => v / r,
                _ => v % r,
            };
        }
        Some(v)
    }
    fn power(&mut self) -> Option<f64> {
        let base = self.unary()?;
        if self.peek() == Some(b'^') {
            self.i += 1;
            return Some(base.powf(self.power()?));
        }
        Some(base)
    }
    fn unary(&mut self) -> Option<f64> {
        if self.peek() == Some(b'-') {
            self.i += 1;
            return Some(-self.unary()?);
        }
        self.atom()
    }
    fn atom(&mut self) -> Option<f64> {
        if self.peek() == Some(b'(') {
            self.i += 1;
            let v = self.sum()?;
            if self.peek() != Some(b')') {
                return None;
            }
            self.i += 1;
            return Some(v);
        }
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit() || c == b'.') {
            self.i += 1;
        }
        std::str::from_utf8(&self.s[start..self.i]).ok()?.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: DateTime = DateTime { year: 2026, month: 11, day: 22, hour: 14, minute: 5 };

    #[test]
    fn dates_relative_to_today() {
        assert_eq!(date_candidates("きょう", T)[..3], ["2026/11/22", "2026年11月22日", "11月22日(日)"]);
        assert!(date_candidates("きょう", T).contains(&"令和8年11月22日".to_string()));
        assert_eq!(date_candidates("あした", T)[0], "2026/11/23");
        assert_eq!(date_candidates("きのう", DateTime { day: 1, month: 3, ..T })[0], "2026/02/28");
        assert_eq!(date_candidates("いま", T)[..3], ["14:05", "14時5分", "午後2時5分"]);
        assert!(date_candidates("ねこ", T).is_empty());
    }

    #[test]
    fn calculator_reads_kana_operators() {
        assert_eq!(calculator_candidates("123・456="), ["0.2697368421", "123/456=0.2697368421"]);
        assert_eq!(calculator_candidates("1+2*3="), ["7", "1+2*3=7"]);
        assert_eq!(calculator_candidates("(1+2)*3="), ["9", "(1+2)*3=9"]);
        assert_eq!(calculator_candidates("10ー4="), ["6", "10-4=6"]);
        assert_eq!(calculator_candidates("2^10="), ["1024", "2^10=1024"]);
        assert!(calculator_candidates("123=").is_empty());
        assert!(calculator_candidates("1・0=").is_empty());
        assert!(calculator_candidates("あ=").is_empty());
    }

    #[test]
    fn emoji_by_reading() {
        assert!(emoji_candidates("えがお").contains(&"😊".to_string()));
        assert!(!emoji_candidates("かお").is_empty());
        assert!(emoji_candidates("ねこぜ").is_empty());
    }

    #[test]
    fn ephemeral_candidates_are_detected() {
        for r in [
            "きょう",
            "ほんじつ",
            "ひづけ",
            "あした",
            "あす",
            "みょうにち",
            "あさって",
            "きのう",
            "さくじつ",
            "おととい",
            "いま",
            "じこく",
            "げんざい",
            "ことし",
            "こんねん",
            "こんげつ",
            "ようび",
        ] {
            for c in date_candidates(r, T) {
                assert!(is_ephemeral_candidate(r, &c), "{r} -> {c}");
            }
        }
        let morning = DateTime { hour: 9, minute: 5, ..T };
        for c in date_candidates("いま", morning) {
            assert!(is_ephemeral_candidate("いま", &c), "いま (9:05) -> {c}");
        }
        assert!(is_ephemeral_candidate("ことし", "令和元年"));
        assert!(!is_ephemeral_candidate("いま", "今"));
        assert!(!is_ephemeral_candidate("いま", "今が1番"));
        assert!(!is_ephemeral_candidate("ことし", "令和の時代"));
        assert!(!is_ephemeral_candidate("きょう", "今日"));
        assert!(!is_ephemeral_candidate("ようび", "曜日"));
        assert!(is_ephemeral_candidate("1+2=", "3"));
        assert!(is_ephemeral_candidate("1+2=", "1+2=3"));
        assert!(!is_ephemeral_candidate("えがお", "😊"));
    }
}
