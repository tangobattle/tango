//! This layer's own UI metrics, over the shared ones in [`tango_ui`] —
//! re-exported here, so `crate::style::*` is the one place editor code
//! reads a metric from whichever side of the boundary owns it.

pub use tango_ui::style::*;

/// Header strip across the top of a pane (editor headers, settings
/// rows): a bit taller than a list row, flush with PANE_PADDING
/// horizontally.
pub const HEADER_PADDING: [f32; 2] = [8.0, 12.0];

/// Compact inline controls — the filter inputs and sort pick-lists
/// that sit inside pane headers. Tighter than a standard button's
/// padding so they don't blow up the header height.
pub const CONTROL_PADDING: [f32; 2] = [5.0, 10.0];
