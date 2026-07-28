//! The app's own UI metrics, over the shared ones in [`tango_ui`] —
//! re-exported here, so `crate::ui::style::*` is the one place app code
//! reads a metric from whichever side of the boundary owns it.

pub use tango_ui::style::*;

/// Splash titles ("Welcome to Tango") — the top of the typographic
/// scale in [`tango_ui::style`], which the rest of the app draws from.
pub const TEXT_DISPLAY: f32 = 22.0;

// Standard button padding. Standard body text comes from iced's
// `default_text_size` (set in `run_app`), so there's no standalone
// STANDARD_TEXT_SIZE constant — widgets that don't pass an explicit
// size inherit the app default.
pub const STANDARD_PADDING: [f32; 2] = [6.0, 14.0];

/// The height a [`crate::ui::widgets::picker`] lays out to: its text's
/// line box (iced's default 1.3 relative line height over the app's
/// body size) plus its vertical padding. Borders draw inside the
/// bounds, so they don't add to it.
///
/// Anything swapped INTO a picker's slot should carry this height, so
/// the row it sits in measures the same either way and a live-state
/// flip can't shift the layout around it.
pub const PICKER_HEIGHT: f32 = TEXT_BODY * 1.3 + STANDARD_PADDING[0] * 2.0;

/// Pinned inner-control height for the play-tab link-code bar
/// and the session media-controls bar — every button / picker
/// in both strips is sized to this so the bars come out the
/// same height naturally (no outer container pinning needed).
pub const BAR_CONTROL_HEIGHT: f32 = 40.0;

/// The app's registered default UI font (see `default_font` / the
/// `.font(...)` calls in `main`). Most widgets inherit it for free, but
/// a few build their own text styles and must name it explicitly —
/// notably the markdown widget, whose `Style` otherwise defaults to the
/// system sans-serif instead of our bundled Noto face.
pub const DEFAULT_FONT: iced::Font = iced::Font::with_name("Noto Sans");
