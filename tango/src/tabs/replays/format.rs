//! Replay metadata formatting shared by the list and the detail panel.

use super::*;

/// A replay's millis-since-epoch timestamp, formatted per `fmt` in
/// local time; `"(?)"` when the value is out of range.
pub(super) fn format_ts(ms: u64, fmt: &str) -> String {
    std::time::UNIX_EPOCH
        .checked_add(std::time::Duration::from_millis(ms))
        .map(|t| chrono::DateTime::<chrono::Local>::from(t).format(fmt).to_string())
        .unwrap_or_else(|| "(?)".to_string())
}

/// Display label for the `link_code` field of a replay's
/// metadata. Direct-TCP sessions leave the field blank in the
/// metadata (see `netplay::take_pre_match`); render those as a
/// localized "(direct)" marker so the row / detail panel still
/// has something where the link code would be.
pub(super) fn link_code_display<'a>(lang: &LanguageIdentifier, code: &'a str) -> std::borrow::Cow<'a, str> {
    if code.is_empty() {
        std::borrow::Cow::Owned(t!(lang, "replays-direct-marker"))
    } else {
        std::borrow::Cow::Borrowed(code)
    }
}
