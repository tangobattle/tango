//! A bounded display list. The package owns geometry and interpretation.
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};

use super::style::{self, bounded, Color};
use crate::{invalid, Result};

/// Bake a canvas at its logical resolution before fitting it to the widget.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Rasterize {
    /// Hard alpha mask in source pixels, useful for pixel artwork.
    pub corner_radius: u32,
    pub nearest: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum PointerCapture {
    All(bool),
    Matching(Vec<PointerFilter>),
}

impl Default for PointerCapture {
    fn default() -> Self {
        Self::All(true)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PointerFilter {
    pub phase: super::PointerPhase,
    #[serde(default)]
    pub button: Option<super::PointerButton>,
}

impl PointerCapture {
    pub fn matches(&self, phase: super::PointerPhase, button: super::PointerButton) -> bool {
        match self {
            Self::All(capture) => *capture,
            Self::Matching(filters) => filters
                .iter()
                .any(|filter| filter.phase == phase && filter.button.is_none_or(|expected| expected == button)),
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if matches!(self, Self::Matching(filters) if filters.len() > 32) {
            return Err(invalid("too many pointer capture filters"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    #[serde(with = "tango_match::bytes")]
    pub rgba: Arc<[u8]>,
    #[serde(skip)]
    pub digest: [u8; 32],
}

/// A drawing without an interactive widget or view-state dependency.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Drawing {
    pub width: u32,
    pub height: u32,
    pub commands: Vec<Draw>,
    #[serde(default)]
    pub corner_radius: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ImageSource {
    Raster(Raster),
    Drawing(Drawing),
}

pub(crate) struct Budget {
    pub images: usize,
    raster_work: usize,
    commands: usize,
    points: usize,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            images: 64 * 1024 * 1024,
            raster_work: 64 * 1024 * 1024,
            commands: 8192,
            points: 32_768,
        }
    }
}

impl ImageSource {
    pub(crate) fn validate(&mut self, budget: &mut Budget) -> Result<()> {
        match self {
            Self::Raster(image) => image.validate(&mut budget.images),
            Self::Drawing(drawing) => validate(
                drawing.width as f32,
                drawing.height as f32,
                &mut drawing.commands,
                Some(Rasterize {
                    corner_radius: drawing.corner_radius,
                    nearest: false,
                }),
                budget,
            ),
        }
    }
}

pub(crate) fn validate(
    width: f32,
    height: f32,
    commands: &mut [Draw],
    rasterize: Option<Rasterize>,
    budget: &mut Budget,
) -> Result<()> {
    style::bounded("canvas width", width, 1.0, 8192.0)?;
    style::bounded("canvas height", height, 1.0, 8192.0)?;
    if let Some(options) = rasterize {
        style::bounded("rasterized canvas width", width, 1.0, 4096.0)?;
        style::bounded("rasterized canvas height", height, 1.0, 4096.0)?;
        if width.fract() != 0.0 || height.fract() != 0.0 || options.corner_radius > 4096 {
            return Err(invalid(
                "rasterized canvases require integer dimensions and a radius <= 4096",
            ));
        }
        budget.images = budget
            .images
            .checked_sub(width as usize * height as usize * 4)
            .ok_or_else(|| invalid("UI image budget exceeded"))?;
    }
    budget.commands = budget
        .commands
        .checked_sub(commands.len())
        .ok_or_else(|| invalid("canvas command budget exceeded"))?;
    for command in commands {
        command.validate(&mut budget.images, &mut budget.points)?;
        if rasterize.is_some() {
            budget.raster_work = budget
                .raster_work
                .checked_sub(command.raster_work(width, height))
                .ok_or_else(|| invalid("UI rasterization work budget exceeded"))?;
        }
    }
    if rasterize.is_some() {
        budget.raster_work = budget
            .raster_work
            .checked_sub(width as usize * height as usize)
            .ok_or_else(|| invalid("UI rasterization work budget exceeded"))?;
    }
    Ok(())
}

impl Raster {
    pub(crate) fn validate(&mut self, budget: &mut usize) -> Result<()> {
        if self.width == 0 || self.height == 0 || self.width > 4096 || self.height > 4096 {
            return Err(invalid("image dimensions must be in 1..=4096"));
        }
        let len = self.width as usize * self.height as usize * 4;
        if len != self.rgba.len() {
            return Err(invalid("RGBA image length does not match dimensions"));
        }
        *budget = budget
            .checked_sub(len)
            .ok_or_else(|| invalid("UI image budget exceeded"))?;
        let mut hash = Sha3_256::new();
        hash.update(self.width.to_le_bytes());
        hash.update(self.height.to_le_bytes());
        hash.update(&self.rgba);
        self.digest = hash.finalize().into();
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Font {
    #[default]
    Normal,
    Mono,
    Icons,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Wrapping {
    None,
    #[default]
    Word,
    Glyph,
    WordOrGlyph,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum TextSize {
    Named(TextSizeName),
    Pixels(f32),
}
impl Default for TextSize {
    fn default() -> Self {
        Self::Named(TextSizeName::Body)
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TextSizeName {
    Title,
    Heading,
    Body,
    Caption,
}
impl TextSize {
    pub(crate) fn validate(self) -> Result<()> {
        if let Self::Pixels(n) = self {
            bounded("text size", n, 1.0, 256.0)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Paint {
    pub fill: Color,
    pub stroke: Color,
    pub stroke_width: f32,
    pub line_cap: LineCap,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LineCap {
    #[default]
    Butt,
    Square,
    Round,
}
impl Default for Paint {
    fn default() -> Self {
        Self {
            fill: Color::transparent(),
            stroke: Color::transparent(),
            stroke_width: 1.0,
            line_cap: LineCap::default(),
        }
    }
}
impl Paint {
    fn validate(&self) -> Result<()> {
        self.fill.validate()?;
        self.stroke.validate()?;
        bounded("stroke width", self.stroke_width, 0.0, 256.0)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Draw {
    Rect {
        #[serde(default)]
        x: f32,
        #[serde(default)]
        y: f32,
        width: f32,
        height: f32,
        #[serde(default)]
        radius: f32,
        #[serde(flatten)]
        paint: Paint,
    },
    Ellipse {
        #[serde(default)]
        x: f32,
        #[serde(default)]
        y: f32,
        rx: f32,
        ry: f32,
        #[serde(flatten)]
        paint: Paint,
    },
    Path {
        points: Vec<[f32; 2]>,
        #[serde(default)]
        closed: bool,
        #[serde(flatten)]
        paint: Paint,
    },
    Text {
        #[serde(default)]
        x: f32,
        #[serde(default)]
        y: f32,
        text: String,
        #[serde(default)]
        size: TextSize,
        #[serde(default)]
        font: Font,
        #[serde(default)]
        color: Color,
    },
    Image {
        #[serde(default)]
        x: f32,
        #[serde(default)]
        y: f32,
        width: f32,
        height: f32,
        image: Raster,
        #[serde(default = "super::yes")]
        nearest: bool,
    },
}

impl Draw {
    /// Conservative covered-pixel estimate, after validating coordinates.
    /// Bounds CPU rasterization independently of retained image memory.
    pub(crate) fn raster_work(&self, canvas_width: f32, canvas_height: f32) -> usize {
        let (x1, y1, x2, y2, paint) = match self {
            Self::Rect {
                x,
                y,
                width,
                height,
                paint,
                ..
            } => (*x, *y, x + width, y + height, Some(paint)),
            Self::Ellipse { x, y, rx, ry, paint } => (x - rx, y - ry, x + rx, y + ry, Some(paint)),
            Self::Path { points, paint, .. } => {
                let Some([x, y]) = points.first().copied() else {
                    return 0;
                };
                let (mut x1, mut y1, mut x2, mut y2) = (x, y, x, y);
                for [x, y] in points {
                    x1 = x1.min(*x);
                    y1 = y1.min(*y);
                    x2 = x2.max(*x);
                    y2 = y2.max(*y);
                }
                (x1, y1, x2, y2, Some(paint))
            }
            Self::Image {
                x, y, width, height, ..
            } => (*x, *y, x + width, y + height, None),
            Self::Text { .. } => (0.0, 0.0, canvas_width, canvas_height, None),
        };
        // Account for both fill and stroke, including square caps and miters.
        let margin = paint.map_or(1.0, |p| p.stroke_width * 5.0 + 1.0);
        let width = ((x2 + margin).min(canvas_width) - (x1 - margin).max(0.0))
            .max(0.0)
            .ceil() as usize;
        let height = ((y2 + margin).min(canvas_height) - (y1 - margin).max(0.0))
            .max(0.0)
            .ceil() as usize;
        width * height * if paint.is_some() { 2 } else { 1 }
    }

    pub(crate) fn validate(&mut self, images: &mut usize, points_left: &mut usize) -> Result<()> {
        match self {
            Self::Rect {
                x,
                y,
                width,
                height,
                radius,
                paint,
            } => {
                position(*x, *y)?;
                dimensions(*width, *height)?;
                bounded("corner radius", *radius, 0.0, 8192.0)?;
                paint.validate()?;
            }
            Self::Ellipse { x, y, rx, ry, paint } => {
                position(*x, *y)?;
                bounded("ellipse radius", *rx, 0.0, 8192.0)?;
                bounded("ellipse radius", *ry, 0.0, 8192.0)?;
                paint.validate()?;
            }
            Self::Path { points, paint, .. } => {
                *points_left = points_left
                    .checked_sub(points.len())
                    .ok_or_else(|| invalid("canvas point budget exceeded"))?;
                for [x, y] in points {
                    position(*x, *y)?;
                }
                paint.validate()?;
            }
            Self::Text { x, y, size, color, .. } => {
                position(*x, *y)?;
                size.validate()?;
                color.validate()?;
            }
            Self::Image {
                x,
                y,
                width,
                height,
                image,
                ..
            } => {
                position(*x, *y)?;
                dimensions(*width, *height)?;
                image.validate(images)?;
            }
        }
        Ok(())
    }
}
fn position(x: f32, y: f32) -> Result<()> {
    bounded("canvas x", x, -16384.0, 16384.0)?;
    bounded("canvas y", y, -16384.0, 16384.0)
}
fn dimensions(width: f32, height: f32) -> Result<()> {
    bounded("canvas width", width, 0.0, 16384.0)?;
    bounded("canvas height", height, 0.0, 16384.0)
}
