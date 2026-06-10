//! Shared UI metrics: the typographic scale, control/row paddings,
//! pane spacing, and the registered fonts. Anything layout-ish that
//! more than one module needs lives here — previously these were
//! split between `app.rs` and `widgets.rs` and every view module
//! imported from both (or just inlined the numbers).

// Typographic scale. Everything that renders text picks from this
// list; one-off sizes outside it tend to look like UI bugs
// (random 12px next to 11px next to 13px). If you need a new
// size, add it here and update the audit.
//
//   DISPLAY — splash titles ("Welcome to Tango").
//   TITLE   — section headers ("tab-settings", empty-state cards).
//   HEADING — sub-section labels (nickname on side cards).
//   BODY    — default body copy. Same value as the iced default.
//   CAPTION — muted hints, status lines, metadata labels.
pub const TEXT_DISPLAY: f32 = 22.0;
pub const TEXT_TITLE: f32 = 18.0;
pub const TEXT_HEADING: f32 = 15.0;
pub const TEXT_BODY: f32 = 13.0;
pub const TEXT_CAPTION: f32 = 11.0;

// Button sizing constants. `PRIMARY` is the big call-to-action
// (Play); `STANDARD` is everything else. Standard body text comes
// from iced's `default_text_size` (set in `run_app`), so there's
// no standalone STANDARD_TEXT_SIZE constant — widgets that don't
// pass an explicit size inherit the app default.
pub const PRIMARY_PADDING: [f32; 2] = [6.0, 14.0];
pub const STANDARD_PADDING: [f32; 2] = [6.0, 14.0];

/// Header strip across the top of a pane (editor headers, settings
/// rows): a bit taller than a list row, flush with PANE_PADDING
/// horizontally.
pub const HEADER_PADDING: [f32; 2] = [8.0, 12.0];

/// Compact inline controls — the filter inputs and sort pick-lists
/// that sit inside pane headers. Tighter than STANDARD_PADDING so
/// they don't blow up the header height.
pub const CONTROL_PADDING: [f32; 2] = [5.0, 10.0];

/// List rows and whole-row buttons (library entries, zebra rows).
pub const ROW_PADDING: [f32; 2] = [6.0, 10.0];

/// Pinned inner-control height for the play-tab link-code bar
/// and the session media-controls bar — every button / picker
/// in both strips is sized to this so the bars come out the
/// same height naturally (no outer container pinning needed).
pub const BAR_CONTROL_HEIGHT: f32 = 40.0;

/// Standard internal padding for [`crate::widgets::pane`] containers.
/// Use this on `.padding(...)` so every demarcation pane has the same
/// gap between its edge and its content.
pub const PANE_PADDING: f32 = 12.0;
/// Standard outer gap (column spacing / row spacing / outer padding)
/// between sibling panes.
pub const PANE_GAP: f32 = 8.0;

/// The app's registered UI fonts (see `default_font` / the `.font(...)`
/// calls in `main`). Most widgets inherit the default font for free, but a
/// few build their own text styles and must name it explicitly — notably
/// the markdown widget, whose `Style` otherwise defaults to the system
/// sans-serif / monospace instead of our bundled Noto faces.
pub const DEFAULT_FONT: iced::Font = iced::Font::with_name("Noto Sans");
pub const MONOSPACE_FONT: iced::Font = iced::Font::with_name("Noto Sans Mono");
