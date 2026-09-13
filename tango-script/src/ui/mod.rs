//! Game-neutral UI descriptions, decoded by Serde and validated before rendering.
pub mod action;
pub mod canvas;
pub mod motion;
pub mod style;
#[cfg(test)]
mod tests;
pub use action::{Action, Event, Modifiers, PointerButton, PointerPhase};
pub use canvas::{Draw, Font, LineCap, PointerCapture, Raster, TextSize, Wrapping};
pub use motion::Motion;
pub use style::{Align, Appearance, Color, Cursor, Layout, Padding, Size, Tone};

use std::collections::BTreeSet;

use mlua::{Lua, Table, Value};
use serde::{Deserialize, Serialize};

use crate::{decode, invalid, Result};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ButtonFeedback {
    pub child: Box<Node>,
    #[serde(default)]
    pub tooltip: Option<Tooltip>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Choice {
    pub value: String,
    pub label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged, deny_unknown_fields)]
pub enum Tooltip {
    Text(String),
    Rich {
        content: Box<Node>,
        #[serde(default)]
        position: TooltipPosition,
        #[serde(default)]
        gap: f32,
    },
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TooltipPosition {
    #[default]
    Top,
    Bottom,
    Left,
    Right,
    FollowCursor,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Node {
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub motion: Option<Motion>,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub tooltip: Option<Tooltip>,
    #[serde(default)]
    pub cursor: Option<Cursor>,
    #[serde(flatten)]
    pub layout: Layout,
    #[serde(default)]
    pub tone: Tone,
    #[serde(default)]
    pub appearance: Appearance,
    #[serde(flatten)]
    pub kind: Kind,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Kind {
    Column {
        children: Vec<Node>,
    },
    Row {
        children: Vec<Node>,
    },
    Wrap {
        children: Vec<Node>,
    },
    Stack {
        children: Vec<Node>,
    },
    Container {
        child: Box<Node>,
    },
    Scroll {
        child: Box<Node>,
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        on_scroll: bool,
        #[serde(default)]
        scrollbar: style::ScrollbarVisibility,
        #[serde(default)]
        horizontal: bool,
    },
    List {
        #[serde(default)]
        id: Option<String>,
        children: Vec<Node>,
        #[serde(default)]
        reorder: bool,
    },
    Text {
        text: String,
        #[serde(default)]
        size: TextSize,
        #[serde(default)]
        font: Font,
        #[serde(default)]
        color: Option<Color>,
        #[serde(default)]
        strikethrough: bool,
        #[serde(default)]
        underline: bool,
        #[serde(default)]
        wrapping: Wrapping,
    },
    Image {
        image: Raster,
        #[serde(default = "yes")]
        nearest: bool,
        #[serde(default = "one_f32")]
        opacity: f32,
    },
    Input {
        id: String,
        #[serde(default)]
        text: String,
        #[serde(default)]
        value: String,
        #[serde(default)]
        placeholder: String,
        #[serde(default)]
        secure: bool,
        #[serde(default)]
        content_padding: Option<Padding>,
        #[serde(default)]
        size: TextSize,
    },
    Select {
        id: String,
        #[serde(default)]
        text: String,
        value: String,
        choices: Vec<Choice>,
        #[serde(default)]
        content_padding: Option<Padding>,
        #[serde(default)]
        size: TextSize,
    },
    Button {
        id: String,
        #[serde(default = "yes")]
        action_enabled: bool,
        #[serde(default)]
        value: String,
        #[serde(default)]
        text: String,
        #[serde(default)]
        child: Option<Box<Node>>,
        #[serde(default)]
        feedback: Option<ButtonFeedback>,
        #[serde(default)]
        size: TextSize,
        #[serde(default)]
        font: Font,
        #[serde(default)]
        content_padding: Option<Padding>,
        #[serde(default)]
        hovered: Appearance,
        #[serde(default)]
        pressed: Appearance,
        #[serde(default)]
        disabled: Appearance,
    },
    Checkbox {
        id: String,
        text: String,
        checked: bool,
        #[serde(default)]
        size: Option<f32>,
        #[serde(default)]
        text_size: Option<TextSize>,
    },
    Slider {
        id: String,
        #[serde(rename = "number")]
        value: f64,
        #[serde(default)]
        min: f64,
        #[serde(default = "one")]
        max: f64,
        #[serde(default = "one")]
        step: f64,
    },
    Canvas {
        #[serde(default)]
        id: Option<String>,
        #[serde(rename = "canvas_width")]
        width: f32,
        #[serde(rename = "canvas_height")]
        height: f32,
        #[serde(default)]
        pointer: bool,
        #[serde(default)]
        pointer_capture: PointerCapture,
        #[serde(default)]
        keyboard: bool,
        #[serde(default)]
        rasterize: Option<canvas::Rasterize>,
        #[serde(default = "yes")]
        scale_to_fit: bool,
        commands: Vec<Draw>,
    },
    Space,
}

fn yes() -> bool {
    true
}
fn one_f32() -> f32 {
    1.0
}
fn one() -> f64 {
    1.0
}

// Includes offscreen rows and rich tooltip content in large editor catalogs.
const MAX_UI_NODES: usize = 32_768;

struct Budget {
    nodes: usize,
    drawing: canvas::Budget,
    ids: BTreeSet<String>,
    keys: BTreeSet<String>,
    motions: BTreeSet<String>,
}

impl Budget {
    fn id(&mut self, id: &str) -> Result<()> {
        if id.is_empty() || !self.ids.insert(id.to_owned()) {
            return Err(invalid("interactive UI IDs must be nonempty and unique"));
        }
        Ok(())
    }
}

impl Node {
    pub(crate) fn read(lua: &Lua, table: Table) -> Result<Self> {
        let mut node: Self = decode::from_value(
            lua,
            Value::Table(table),
            decode::Limits {
                values: 524_288,
                bytes: 64 * 1024 * 1024,
                depth: 96,
                string: 16 * 1024,
                root: decode::Root::Map,
            },
            true,
        )?;
        node.validate(
            0,
            &mut Budget {
                nodes: MAX_UI_NODES,
                drawing: canvas::Budget::default(),
                ids: BTreeSet::new(),
                keys: BTreeSet::new(),
                motions: BTreeSet::new(),
            },
        )?;
        Ok(node)
    }

    fn validate(&mut self, depth: usize, budget: &mut Budget) -> Result<()> {
        if depth > 32 {
            return Err(invalid("UI tree exceeds its depth limit"));
        }
        if budget.nodes == 0 {
            return Err(invalid("UI tree exceeds its node limit"));
        }
        budget.nodes -= 1;
        if let Some(key) = &self.key {
            if key.is_empty() || key.len() > 1024 || !budget.keys.insert(key.clone()) {
                return Err(invalid("UI keys must be nonempty and unique"));
            }
        }
        self.layout.validate()?;
        if let Some(motion) = &self.motion {
            motion.validate()?;
            if !budget.motions.insert(motion.key.clone()) || budget.motions.len() > 1024 {
                return Err(invalid("a view may contain at most 1024 unique motion keys"));
            }
        }
        self.tone.validate()?;
        self.appearance.validate()?;
        if let Some(Tooltip::Rich { content, gap, .. }) = &mut self.tooltip {
            style::bounded("tooltip gap", *gap, 0.0, 1024.0)?;
            content.validate(depth + 1, budget)?;
        }
        match &mut self.kind {
            Kind::Column { children } | Kind::Row { children } | Kind::Wrap { children } | Kind::Stack { children } => {
                for child in children {
                    child.validate(depth + 1, budget)?;
                }
            }
            Kind::List { id, children, reorder } => {
                if *reorder && id.is_none() {
                    return Err(invalid("reorderable lists require an ID"));
                }
                if let Some(id) = id {
                    budget.id(id)?;
                }
                for child in children {
                    child.validate(depth + 1, budget)?;
                }
            }
            Kind::Container { child } => child.validate(depth + 1, budget)?,
            Kind::Scroll {
                child, id, on_scroll, ..
            } => {
                if *on_scroll && id.is_none() {
                    return Err(invalid("scroll events require an ID"));
                }
                if let Some(id) = id {
                    budget.id(id)?;
                }
                child.validate(depth + 1, budget)?;
            }
            Kind::Text { size, color, .. } => {
                size.validate()?;
                if let Some(color) = color {
                    color.validate()?;
                }
            }
            Kind::Image { image, opacity, .. } => {
                image.validate(&mut budget.drawing.images)?;
                style::bounded("image opacity", *opacity, 0.0, 1.0)?;
            }
            Kind::Checkbox {
                id, size, text_size, ..
            } => {
                budget.id(id)?;
                if let Some(size) = size {
                    style::bounded("checkbox size", *size, 1.0, 256.0)?;
                }
                if let Some(size) = text_size {
                    size.validate()?;
                }
            }
            Kind::Input {
                id,
                size,
                content_padding,
                ..
            } => {
                budget.id(id)?;
                size.validate()?;
                if let Some(padding) = content_padding {
                    padding.validate()?;
                }
            }
            Kind::Button {
                id,
                child,
                size,
                content_padding,
                hovered,
                pressed,
                disabled,
                feedback,
                ..
            } => {
                budget.id(id)?;
                size.validate()?;
                if let Some(padding) = content_padding {
                    padding.validate()?;
                }
                for appearance in [hovered, pressed, disabled] {
                    appearance.validate()?;
                }
                if let Some(child) = child {
                    child.validate(depth + 1, budget)?;
                }
                if let Some(feedback) = feedback {
                    feedback.child.validate(depth + 1, budget)?;
                    if let Some(Tooltip::Rich { content, gap, .. }) = &mut feedback.tooltip {
                        style::bounded("tooltip gap", *gap, 0.0, 1024.0)?;
                        content.validate(depth + 1, budget)?;
                    }
                }
            }
            Kind::Select {
                id,
                value,
                choices,
                size,
                content_padding,
                ..
            } => {
                budget.id(id)?;
                size.validate()?;
                if let Some(padding) = content_padding {
                    padding.validate()?;
                }
                if choices.len() > 1024 {
                    return Err(invalid("too many choices"));
                }
                let mut values = BTreeSet::new();
                for choice in choices {
                    if !values.insert(&choice.value) {
                        return Err(invalid("duplicate choice value"));
                    }
                }
                if !values.contains(value) {
                    return Err(invalid("selected value is missing from choices"));
                }
            }
            Kind::Slider {
                id,
                value,
                min,
                max,
                step,
            } => {
                budget.id(id)?;
                if [*min, *max, *value, *step]
                    .iter()
                    .any(|n| !n.is_finite() || n.abs() > 1e9)
                    || min >= max
                    || *step <= 0.0
                    || value < min
                    || value > max
                {
                    return Err(invalid(
                        "slider requires finite min < max, a value in range, and positive step",
                    ));
                }
            }
            Kind::Canvas {
                id,
                width,
                height,
                pointer,
                pointer_capture,
                keyboard,
                rasterize,
                commands,
                ..
            } => {
                if (*pointer || *keyboard) && id.is_none() {
                    return Err(invalid("interactive canvases require an ID"));
                }
                pointer_capture.validate()?;
                if let Some(id) = id {
                    budget.id(id)?;
                }
                canvas::validate(*width, *height, commands, *rasterize, &mut budget.drawing)?;
            }
            Kind::Space => {}
        }
        Ok(())
    }

    /// Structural children, including passive tooltip and feedback content.
    /// Traversal does not imply that these children can receive actions.
    pub fn children(&self) -> impl Iterator<Item = &Self> {
        let (many, single, feedback) = match &self.kind {
            Kind::Column { children }
            | Kind::Row { children }
            | Kind::Wrap { children }
            | Kind::Stack { children }
            | Kind::List { children, .. } => (children.as_slice(), None, None),
            Kind::Container { child } | Kind::Scroll { child, .. } => (&[][..], Some(child.as_ref()), None),
            Kind::Button { child, feedback, .. } => (&[][..], child.as_deref(), feedback.as_ref()),
            _ => (&[][..], None, None),
        };
        fn content(tooltip: Option<&Tooltip>) -> Option<&Node> {
            match tooltip {
                Some(Tooltip::Rich { content, .. }) => Some(content),
                _ => None,
            }
        }
        many.iter()
            .chain(single)
            .chain(content(self.tooltip.as_ref()))
            .chain(feedback.map(|f| f.child.as_ref()))
            .chain(content(feedback.and_then(|f| f.tooltip.as_ref())))
    }

    /// Validate against the current tree, including disabled ancestor widgets.
    pub fn accepts(&self, action: &Action) -> bool {
        if !self.enabled || !action.bounded() {
            return false;
        }
        match &self.kind {
            Kind::Column { children } | Kind::Row { children } | Kind::Wrap { children } | Kind::Stack { children } => {
                children.iter().any(|n| n.accepts(action))
            }
            Kind::Container { child } => child.accepts(action),
            Kind::Scroll {
                child, id, on_scroll, ..
            } => {
                (*on_scroll && id.as_deref() == Some(&action.id) && matches!(action.event, Event::Scroll { .. }))
                    || child.accepts(action)
            }
            Kind::Input { id, .. } => id == &action.id && matches!(action.event, Event::Change { .. }),
            Kind::Button {
                id,
                value,
                child,
                action_enabled,
                ..
            } => {
                (*action_enabled
                    && id == &action.id
                    && matches!(&action.event, Event::Activate { value: v } if v == value))
                    || child.as_ref().is_some_and(|child| child.accepts(action))
            }
            Kind::Select { id, choices, .. } => {
                id == &action.id
                    && matches!(&action.event, Event::Change { value } if choices.iter().any(|c| c.value == *value))
            }
            Kind::Checkbox { id, .. } => id == &action.id && matches!(action.event, Event::Toggle { .. }),
            Kind::Slider { id, min, max, step, .. } => {
                id == &action.id
                    && matches!(&action.event, Event::Slide { number } if number >= min && number <= max
                        && (number == max || (((number - min) / step).round() - (number - min) / step).abs() < 1e-4))
            }
            Kind::List { id, children, reorder } => {
                (*reorder
                    && id.as_deref() == Some(&action.id)
                    && matches!(action.event, Event::Reorder { from, to }
                        if (1..=children.len()).contains(&from) && (1..=children.len()).contains(&to)))
                    || children.iter().any(|n| n.accepts(action))
            }
            Kind::Canvas {
                id, pointer, keyboard, ..
            } => {
                id.as_deref() == Some(&action.id)
                    && match action.event {
                        Event::Pointer { .. } => *pointer,
                        Event::Key { .. } => *keyboard,
                        _ => false,
                    }
            }
            _ => false,
        }
    }
}
