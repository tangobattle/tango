//! Types for the transport's scrub surface. The dedicated scrub-bar
//! widget that used to live here is gone — the analysis chart
//! ([`crate::widgets::hp_match_graph`] with a
//! [`ChartTransport`](crate::widgets::ChartTransport)) is the scrub
//! surface now.

/// Where the cursor is resting on the scrub surface, published through
/// the transport's hover callback. `x` is absolute (window space) — the
/// canvas is the only widget that knows where the bar landed in layout,
/// and the caller's floating preview is positioned in the session view's
/// full-window overlay stack, which shares that origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HoverInfo {
    /// The tick a click here would seek to (clamped to the prefetch
    /// watermark exactly like a press, so the preview never promises an
    /// unreachable frame).
    pub tick: u32,
    /// Cursor x, clamped into the bar.
    pub x: f32,
}
