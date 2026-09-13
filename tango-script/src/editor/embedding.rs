//! Host-owned capabilities. Packages choose how to present them, never grant them.
use std::collections::BTreeMap;

use crate::{invalid, Effect, Result};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct HostAction {
    pub enabled: bool,
    /// Some actions can operate on a staged document; launches usually cannot.
    pub while_editing: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Embedding {
    /// Presentation preference; the package owns its cover and reveal flow.
    pub streamer_mode: bool,
    /// False when the embedding UI supplies edit and contextual export controls.
    pub inline_actions: bool,
    /// Application-defined IDs, with no game or application action enum in the VM.
    pub actions: BTreeMap<String, HostAction>,
}

impl Default for Embedding {
    fn default() -> Self {
        Self {
            streamer_mode: false,
            inline_actions: true,
            actions: BTreeMap::new(),
        }
    }
}

pub(crate) fn validate_action_id(id: &str) -> Result<()> {
    if id.is_empty() || id.len() > 256 || id.chars().any(char::is_control) {
        return Err(invalid(
            "host action IDs must contain 1..=256 bytes without control characters",
        ));
    }
    Ok(())
}

impl Embedding {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.actions.len() > 64 {
            return Err(invalid("an embed may expose at most 64 host actions"));
        }
        for id in self.actions.keys() {
            validate_action_id(id)?;
        }
        Ok(())
    }
}

impl super::EditorContext {
    pub(crate) fn action_enabled(&self, id: &str) -> bool {
        self.embedding.actions.get(id).is_some_and(|action| {
            action.enabled
                && self
                    .session
                    .is_none_or(|session| !session.saving && (!session.editing || action.while_editing))
        })
    }

    pub(crate) fn validate_effects(&self, effects: &[Effect]) -> Result<()> {
        for effect in effects {
            if let Effect::Invoke { id } = effect {
                if !self.action_enabled(id) {
                    return Err(invalid(format!("host action is not enabled: {id}")));
                }
            }
        }
        Ok(())
    }
}
