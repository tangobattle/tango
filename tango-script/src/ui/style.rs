use serde::{Deserialize, Serialize};

use crate::{invalid, Result};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Size {
    Named(SizeName),
    Pixels(f32),
    Portion { portion: u16 },
}
impl Default for Size {
    fn default() -> Self {
        Self::Named(SizeName::Shrink)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SizeName {
    Shrink,
    Fill,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged, deny_unknown_fields)]
pub enum Tone {
    Named(ToneName),
    Row {
        /// One-based row position, matching Luau array indices.
        row: u32,
        #[serde(default)]
        selected: bool,
    },
    Tinted {
        tint: Color,
    },
    Tab {
        tab: bool,
    },
}
impl Default for Tone {
    fn default() -> Self {
        Self::Named(ToneName::Neutral)
    }
}
impl Tone {
    pub(crate) fn validate(self) -> Result<()> {
        if let Self::Row { row: 0, .. } = self {
            return Err(invalid("row positions are 1-based"));
        }
        if let Self::Tinted { tint } = self {
            tint.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToneName {
    #[default]
    Neutral,
    Primary,
    Danger,
    Muted,
    Selected,
    Plate,
    Card,
    Transparent,
    Tooltip,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Padding {
    Uniform(f32),
    Sides([f32; 4]),
}
impl Default for Padding {
    fn default() -> Self {
        Self::Uniform(0.0)
    }
}
impl Padding {
    pub fn sides(self) -> [f32; 4] {
        match self {
            Self::Uniform(n) => [n; 4],
            Self::Sides(sides) => sides,
        }
    }

    pub(crate) fn validate(self) -> Result<()> {
        for side in self.sides() {
            bounded("padding", side, 0.0, 1024.0)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Layout {
    pub width: Size,
    pub height: Size,
    pub padding: Padding,
    pub spacing: f32,
    pub align_x: Align,
    pub align_y: Align,
    pub max_width: Option<f32>,
    pub max_height: Option<f32>,
    pub clip: bool,
}

/// Optional pointer affordance. It does not capture input or create an action.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Cursor {
    Default,
    Hidden,
    ContextMenu,
    Help,
    Pointer,
    Progress,
    Wait,
    Cell,
    Crosshair,
    Text,
    Alias,
    Copy,
    Move,
    NoDrop,
    NotAllowed,
    Grab,
    Grabbing,
    ResizeHorizontal,
    ResizeVertical,
    ResizeDiagonalUp,
    ResizeDiagonalDown,
    ResizeColumn,
    ResizeRow,
    AllScroll,
    ZoomIn,
    ZoomOut,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ThemeColor {
    #[default]
    Text,
    Muted,
    Primary,
    Danger,
    DangerStrong,
    Success,
    Warning,
    Background,
    Plate,
    Border,
    Transparent,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged, deny_unknown_fields)]
pub enum Color {
    Theme(ThemeColor),
    Rgba([f32; 4]),
    ThemeAlpha {
        theme: ThemeColor,
        alpha: f32,
    },
    ThemeTint {
        theme: ThemeColor,
        tint: [f32; 4],
        mix: f32,
    },
}
impl Default for Color {
    fn default() -> Self {
        Self::Theme(ThemeColor::Text)
    }
}
impl Color {
    pub(crate) fn transparent() -> Self {
        Self::Theme(ThemeColor::Transparent)
    }
    pub(crate) fn validate(self) -> Result<()> {
        match self {
            Self::Rgba(channels) => {
                for channel in channels {
                    bounded("RGBA channel", channel, 0.0, 1.0)?;
                }
            }
            Self::ThemeAlpha { alpha, .. } => bounded("color alpha", alpha, 0.0, 1.0)?,
            Self::ThemeTint { tint, mix, .. } => {
                Self::Rgba(tint).validate()?;
                bounded("color mix", mix, 0.0, 1.0)?;
            }
            Self::Theme(_) => {}
        }
        Ok(())
    }
}

/// Optional overrides on the shared theme style. Unspecified properties inherit
/// the preset, including its gradients, shadow and interaction state.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Appearance {
    pub background: Option<Background>,
    pub text_color: Option<Color>,
    pub border_color: Option<Color>,
    pub border_width: Option<f32>,
    pub radius: Option<f32>,
    pub shadow: Option<Shadow>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged, deny_unknown_fields)]
pub enum Background {
    Solid(Color),
    Linear { angle: f32, stops: Vec<GradientStop> },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GradientStop {
    pub offset: f32,
    pub color: Color,
}

impl Background {
    pub(crate) fn validate(&self) -> Result<()> {
        match self {
            Self::Solid(color) => color.validate(),
            Self::Linear { angle, stops } => {
                bounded("gradient angle", *angle, -36000.0, 36000.0)?;
                if stops.is_empty() || stops.len() > 8 {
                    return Err(invalid("gradients require 1–8 stops"));
                }
                let mut previous = -1.0;
                for stop in stops {
                    bounded("gradient offset", stop.offset, 0.0, 1.0)?;
                    if stop.offset <= previous {
                        return Err(invalid("gradient stops must have increasing offsets"));
                    }
                    previous = stop.offset;
                    stop.color.validate()?;
                }
                Ok(())
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScrollbarVisibility {
    #[default]
    Auto,
    Hover,
    Hidden,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Shadow {
    pub color: Color,
    #[serde(default)]
    pub offset: [f32; 2],
    #[serde(default)]
    pub blur: f32,
}
impl Appearance {
    pub(crate) fn validate(&self) -> Result<()> {
        if let Some(background) = &self.background {
            background.validate()?;
        }
        for color in [self.text_color, self.border_color].into_iter().flatten() {
            color.validate()?;
        }
        if let Some(width) = self.border_width {
            bounded("border width", width, 0.0, 1024.0)?;
        }
        if let Some(radius) = self.radius {
            bounded("border radius", radius, 0.0, 8192.0)?;
        }
        if let Some(shadow) = self.shadow {
            shadow.color.validate()?;
            bounded("shadow blur", shadow.blur, 0.0, 1024.0)?;
            for offset in shadow.offset {
                bounded("shadow offset", offset, -1024.0, 1024.0)?;
            }
        }
        Ok(())
    }
}

pub(crate) fn bounded(name: &str, value: f32, min: f32, max: f32) -> Result<()> {
    if !value.is_finite() || !(min..=max).contains(&value) {
        return Err(invalid(format!("{name} must be a finite number in {min}..={max}")));
    }
    Ok(())
}

impl Size {
    fn validate(self) -> Result<()> {
        match self {
            Self::Pixels(n) => bounded("layout size", n, 0.0, 16384.0),
            Self::Portion { portion: 0 } => Err(invalid("fill portion must be positive")),
            _ => Ok(()),
        }
    }
}

impl Layout {
    pub(crate) fn validate(&self) -> Result<()> {
        self.width.validate()?;
        self.height.validate()?;
        self.padding.validate()?;
        bounded("spacing", self.spacing, 0.0, 1024.0)?;
        for max in [self.max_width, self.max_height].into_iter().flatten() {
            bounded("maximum layout size", max, 0.0, 16384.0)?;
        }
        Ok(())
    }
}
