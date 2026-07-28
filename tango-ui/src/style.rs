//! Shared UI metrics: the typographic scale, row/pane spacing, and the
//! monospace face — the layout constants both the app and the
//! save-editor layer measure against, so a row built on either side of
//! that boundary comes out the same height. Metrics only one of them
//! uses live in its own `style` module, which re-exports this one.

// Typographic scale. Everything that renders text picks from this
// list; one-off sizes outside it tend to look like UI bugs
// (random 12px next to 11px next to 13px). If you need a new
// size, add it here and update the audit.
//
//   TITLE   — section headers ("tab-settings", empty-state cards).
//   HEADING — sub-section labels (nickname on side cards).
//   BODY    — default body copy. Same value as the iced default.
//   CAPTION — muted hints, status lines, metadata labels.
pub const TEXT_TITLE: f32 = 18.0;
pub const TEXT_HEADING: f32 = 15.0;
pub const TEXT_BODY: f32 = 13.0;
pub const TEXT_CAPTION: f32 = 11.0;

/// List rows and whole-row buttons (library entries, zebra rows).
pub const ROW_PADDING: [f32; 2] = [6.0, 10.0];

/// Standard internal padding for [`crate::widgets::pane`] containers.
/// Use this on `.padding(...)` so every demarcation pane has the same
/// gap between its edge and its content.
pub const PANE_PADDING: f32 = 12.0;
/// Standard outer gap (column spacing / row spacing / outer padding)
/// between sibling panes.
pub const PANE_GAP: f32 = 8.0;

/// The bundled monospace face. Most widgets inherit the app's default
/// font for free; the ones that build their own text styles have to
/// name a face explicitly, and anything tabular wants this one.
pub const MONOSPACE_FONT: iced::Font = iced::Font::with_name("Noto Sans Mono");
