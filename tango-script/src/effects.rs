//! Transactional requests for host work. Scripts never touch the clipboard.
use mlua::{Lua, Value};
use serde::{Deserialize, Serialize};

use crate::{decode, invalid, ui::canvas, Result};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Effect {
    /// Request one of the embedder's enabled actions after this update commits.
    Invoke {
        id: String,
    },
    Edit {
        command: crate::EditCommand,
    },
    CopyText {
        text: String,
    },
    CopyHtml {
        text: String,
        html: String,
    },
    CopyImage {
        image: canvas::ImageSource,
    },
    /// Relative offsets in 0..=1; the renderer confines IDs to this document.
    ScrollTo {
        id: String,
        x: f32,
        y: f32,
    },
}

pub(crate) fn read(lua: &Lua, value: Value) -> Result<Vec<Effect>> {
    if value.is_nil() {
        return Ok(Vec::new());
    }
    let mut effects: Vec<Effect> = decode::from_value(
        lua,
        value,
        decode::Limits {
            values: 524_288,
            bytes: 64 * 1024 * 1024,
            depth: 64,
            string: 4 * 1024 * 1024,
            root: decode::Root::Sequence,
        },
        true,
    )?;
    if effects.len() > 16 {
        return Err(invalid("an update may return at most 16 host effects"));
    }
    let mut budget = canvas::Budget::default();
    for effect in &mut effects {
        match effect {
            Effect::Invoke { id } => crate::editor::embedding::validate_action_id(id)?,
            Effect::CopyImage { image } => image.validate(&mut budget)?,
            Effect::ScrollTo { id, x, y } => {
                if id.is_empty() || id.len() > 1024 {
                    return Err(invalid("invalid scroll target ID"));
                }
                for n in [x, y] {
                    crate::ui::style::bounded("scroll offset", *n, 0.0, 1.0)?;
                }
            }
            _ => {}
        }
    }
    Ok(effects)
}
