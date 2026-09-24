//! Window sizing policy: the resolutions the window offers and the
//! smallest it may be.

/// Standard windowed resolutions surfaced in the graphics settings
/// pick-list. Selecting one resizes the live window and updates
/// `config.last_window_size`. The first is the window's minimum size.
pub const STANDARD_RESOLUTIONS: &[(u32, u32)] = &[
    (960, 720),
    (1280, 720),
    (1280, 800),
    (1366, 768),
    (1440, 900),
    (1600, 900),
    (1680, 1050),
    (1920, 1080),
    (2560, 1440),
    (3840, 2160),
];

/// The smallest the window may be, enforced at creation and on restore.
pub const MINIMUM_RESOLUTION: (u32, u32) = STANDARD_RESOLUTIONS[0];
