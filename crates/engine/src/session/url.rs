//! Recognizes URL input early enough to preserve the literal keys.

pub(super) fn scheme(text: &str) -> bool {
    text.eq_ignore_ascii_case("http") || text.eq_ignore_ascii_case("https")
}

pub(super) fn recognizes(text: &str) -> bool {
    if text
        .split_once(':')
        .is_some_and(|(prefix, _)| scheme(prefix))
    {
        return true;
    }
    let host = text.split(['/', ':', '?', '#']).next().unwrap_or("");
    if host
        .get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("www."))
        && host[4..].starts_with(|c: char| c.is_ascii_alphanumeric())
    {
        return true;
    }
    let Some((name, suffix)) = host.rsplit_once('.') else {
        return false;
    };
    // A bare domain needs stronger evidence than a single period, which
    // normally ends Japanese text. An explicit scheme allows any hostname.
    const SUFFIXES: &[&str] = &[
        "ai", "app", "co", "com", "dev", "edu", "gov", "info", "io", "jp", "me", "net", "org",
        "xyz",
    ];
    SUFFIXES
        .iter()
        .any(|ending| suffix.eq_ignore_ascii_case(ending))
        && name.bytes().any(|b| b.is_ascii_alphabetic())
        && name.split('.').all(|label| {
            label.starts_with(|c: char| c.is_ascii_alphanumeric())
                && label.ends_with(|c: char| c.is_ascii_alphanumeric())
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}
