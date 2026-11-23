//! The conversational ending 出たね in contexts where で + 種 is unlikely.

pub(super) const READING: &str = "でたね";
pub(super) const SURFACE: &str = "出たね";

fn ending(text: &str) -> Option<&str> {
    text.trim_end_matches(['。', '！', '？', '!', '?'])
        .strip_suffix(READING)
}

fn has_version_number(text: &str) -> bool {
    let tail = text.trim_end();
    let digits = tail
        .rsplit(|c: char| !c.is_ascii_digit() && c != '.')
        .next()
        .unwrap_or("");
    digits.contains('.')
        && digits.split('.').all(|part| !part.is_empty())
        && tail[..tail.len() - digits.len()]
            .bytes()
            .any(|b| b.is_ascii_alphabetic())
}

pub(super) fn span(text: &str) -> Option<(usize, usize)> {
    let before = ending(text)?;
    (before.is_empty() || before == "いま" || has_version_number(before))
        .then_some((before.len(), before.len() + READING.len()))
}

pub(super) fn follows_version(text: &str, left_context: &str) -> bool {
    ending(text).is_some_and(|before| {
        has_version_number(before) || before.is_empty() && has_version_number(left_context)
    })
}
