//! Presentation timelines are declarative; clocks and redraws belong to the renderer.
use serde::{Deserialize, Serialize};

use crate::{invalid, Result};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Easing {
    Linear,
    EaseInCubic,
    #[default]
    EaseOutCubic,
    EaseInOutCubic,
}

/// Draw an arriving subtree displaced from its resting position. Layout stays
/// at rest; pointer handling follows the drawing. Descriptor changes restart time.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Motion {
    /// Unique within a view, independent of widget/action IDs.
    pub key: String,
    /// Package-owned change token, not a timestamp. Repeated renders retain it.
    pub revision: String,
    pub from: [f32; 2],
    #[serde(default = "duration")]
    pub duration_ms: u32,
    #[serde(default)]
    pub delay_ms: u32,
    #[serde(default)]
    pub easing: Easing,
}

fn duration() -> u32 {
    160
}

impl Motion {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.key.is_empty() || self.key.len() > 1024 || self.revision.len() > 1024 {
            return Err(invalid(
                "motion keys and revisions are limited to 1024 bytes; keys must be nonempty",
            ));
        }
        if self.duration_ms > 60_000 || self.delay_ms > 60_000 {
            return Err(invalid("motion duration and delay are limited to 60 seconds each"));
        }
        for value in self.from {
            super::style::bounded("motion offset", value, -16_384.0, 16_384.0)?;
        }
        Ok(())
    }
}
