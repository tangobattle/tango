//! Transient "Copied!" feedback for clipboard buttons. One global
//! slot — only one copy can have *just* happened — keyed by a stable
//! per-button string, so exactly the button that fired flips its
//! glyph to a check and its tooltip to "Copied!" for a moment (see
//! [`crate::widgets::copy_icon_button`]).
//!
//! The view side reads [`is_lit`]; the update path that actually
//! lands the text on the clipboard calls [`flash`] with the same key.
//! [`flash`] rides [`crate::anim::kick`], so the frames subscription
//! keeps redraws coming until the flash expires and the tooltip
//! reverts on its own — even with the cursor parked on the button.

use std::sync::Mutex;

/// How long the "Copied!" state lingers after a copy.
const FLASH: std::time::Duration = std::time::Duration::from_millis(1500);

static LAST: Mutex<Option<(String, std::time::Instant)>> = Mutex::new(None);

/// Record that the copy behind `key` just landed on the clipboard.
pub fn flash(key: &str) {
    *LAST.lock().unwrap() = Some((key.to_string(), std::time::Instant::now()));
    crate::anim::kick(FLASH);
}

/// Whether `key`'s copy flash is still fresh.
pub fn is_lit(key: &str) -> bool {
    LAST.lock()
        .unwrap()
        .as_ref()
        .is_some_and(|(k, at)| k == key && at.elapsed() < FLASH)
}
