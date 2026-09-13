use std::collections::BTreeMap;

use crate::runtime::Runtime;
use crate::{invalid, Action, Node, Profile, Result};
use std::sync::Arc;

pub(crate) mod embedding;
mod session;
pub use embedding::{Embedding, HostAction};
pub use session::{EditCommand, EditContext, EditorSession, SaveRequest, SessionUpdate};

/// A named starting document offered by the selected editor.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveTemplate {
    pub name: String,
    pub label: String,
}

pub const MAX_DOCUMENT: usize = 8 * 1024 * 1024;
const MAX_HISTORY: usize = 32 * 1024 * 1024;

/// Capabilities supplied by the embedder, independently of package view state.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct EditorContext {
    pub read_only: bool,
    // An absent session is Luau nil, not mlua's truthy null userdata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<EditContext>,
    pub embedding: Embedding,
}

#[derive(Clone)]
struct Revision {
    bytes: Arc<[u8]>,
    state: BTreeMap<String, String>,
}

/// A script-owned document with host-owned transactions. No script values
/// escape an operation; revisions contain only owned bytes and view state.
#[derive(Clone)]
pub struct Document {
    profile: Profile,
    context: EditorContext,
    current: Revision,
    saved: Arc<[u8]>,
    undo: Vec<Arc<[u8]>>,
    redo: Vec<Arc<[u8]>>,
    view: Node,
    diagnostics: Vec<String>,
}

impl Document {
    pub fn open(profile: Profile, input: &[u8]) -> Result<Self> {
        Self::open_with_context(profile, input, EditorContext::default())
    }

    pub fn open_with_context(profile: Profile, input: &[u8], context: EditorContext) -> Result<Self> {
        context.embedding.validate()?;
        if input.len() > MAX_DOCUMENT {
            return Err(invalid("document exceeds 8 MiB"));
        }
        let bytes: Arc<[u8]> = Runtime::new(&profile)?.transform("decode", input)?.into();
        if bytes.len() > MAX_DOCUMENT {
            return Err(invalid("decoded document exceeds 8 MiB"));
        }
        let state = BTreeMap::new();
        let (view, diagnostics) = Self::inspect(&profile, &bytes, &state, &context)?;
        Ok(Self {
            profile,
            context,
            saved: bytes.clone(),
            current: Revision { bytes, state },
            undo: Vec::new(),
            redo: Vec::new(),
            view,
            diagnostics,
        })
    }

    fn inspect(
        profile: &Profile,
        bytes: &[u8],
        state: &BTreeMap<String, String>,
        context: &EditorContext,
    ) -> Result<(Node, Vec<String>)> {
        // Separate environments make validation and rendering observational: mutations
        // in either callback cannot influence the other or the stored model.
        let diagnostics = Runtime::new(profile)?.validate(bytes)?;
        let view = Runtime::new(profile)?.view(bytes, state, Self::context_with_validation(context, &diagnostics))?;
        Ok((view, diagnostics))
    }

    fn context_with_validation(context: &EditorContext, diagnostics: &[String]) -> EditorContext {
        let mut effective = context.clone();
        for (id, action) in &mut effective.embedding.actions {
            action.enabled = context.action_enabled(id);
        }
        let mut context = effective;
        if let Some(session) = &mut context.session {
            session.can_save = session.editable && session.editing && !session.saving && diagnostics.is_empty();
        }
        context
    }

    pub fn title(&self) -> &str {
        self.profile.display_name()
    }
    pub fn view(&self) -> &Node {
        &self.view
    }
    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }
    pub fn is_dirty(&self) -> bool {
        self.current.bytes != self.saved
    }
    pub fn is_read_only(&self) -> bool {
        self.context.read_only
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn profile_digest(&self) -> [u8; 32] {
        self.profile.digest()
    }

    pub fn locale(&self) -> &unic_langid::LanguageIdentifier {
        self.profile.locale()
    }

    /// Refresh translated UI and diagnostics without decoding again or
    /// changing document bytes, drafts, saved state, or undo/redo history.
    /// Formatting/script failures leave the previous locale and view intact.
    pub fn set_locale(&mut self, locale: &str) -> Result<()> {
        let profile = self.profile.clone().with_locale(locale)?;
        if profile.locale() == self.profile.locale() {
            return Ok(());
        }
        let (view, diagnostics) = Self::inspect(&profile, &self.current.bytes, &self.current.state, &self.context)?;
        self.profile = profile;
        self.view = view;
        self.diagnostics = diagnostics;
        Ok(())
    }

    /// Atomically refresh host action capabilities and control placement. A
    /// script failure preserves the old capabilities, view, document and history.
    pub fn set_embedding(&mut self, embedding: Embedding) -> Result<()> {
        embedding.validate()?;
        let mut context = self.context.clone();
        context.embedding = embedding;
        let (view, diagnostics) = Self::inspect(&self.profile, &self.current.bytes, &self.current.state, &context)?;
        self.context = context;
        self.view = view;
        self.diagnostics = diagnostics;
        Ok(())
    }

    /// Effects are returned only after the whole update commits. The caller
    /// executes them once; undo, redo and view refresh never replay effects.
    pub fn dispatch(&mut self, action: Action) -> Result<Vec<crate::Effect>> {
        if !self.view.accepts(&action) {
            return Err(invalid("action is not offered by the current view"));
        }
        let crate::runtime::Update { bytes, state, effects } = Runtime::new(&self.profile)?.update(
            &self.current.bytes,
            &self.current.state,
            &action,
            Self::context_with_validation(&self.context, &self.diagnostics),
        )?;
        if self.context.read_only && bytes.as_slice() != self.current.bytes.as_ref() {
            return Err(invalid("read-only document cannot be modified"));
        }
        if self.context.session.is_none()
            && effects
                .iter()
                .any(|effect| matches!(effect, crate::Effect::Edit { .. }))
        {
            return Err(invalid("edit commands require an editor session"));
        }
        self.context.validate_effects(&effects)?;
        let unchanged = bytes.as_slice() == self.current.bytes.as_ref();
        // Validation only depends on bytes and the fixed profile. Pointer,
        // filter and scroll updates cannot change its answer. A no-op update
        // can return effects without rebuilding an identical view either.
        if unchanged && state == self.current.state {
            return Ok(effects);
        }
        let (view, diagnostics) = if unchanged {
            let view = Runtime::new(&self.profile)?.view(
                &bytes,
                &state,
                Self::context_with_validation(&self.context, &self.diagnostics),
            )?;
            (view, self.diagnostics.clone())
        } else {
            Self::inspect(&self.profile, &bytes, &state, &self.context)?
        };
        let bytes = if unchanged {
            self.current.bytes.clone()
        } else {
            bytes.into()
        };
        let revision = Revision { bytes, state };
        if revision.bytes != self.current.bytes {
            self.undo.push(self.current.bytes.clone());
            self.redo.clear();
            while self.undo.len() > 64 || self.undo.iter().map(|bytes| bytes.len()).sum::<usize>() > MAX_HISTORY {
                self.undo.remove(0);
            }
        }
        self.current = revision;
        self.view = view;
        self.diagnostics = diagnostics;
        Ok(effects)
    }

    pub fn undo(&mut self) -> Result<()> {
        if let Some(revision) = self.undo.last() {
            // View state may contain unapplied inputs from before a commit.
            // Reset it so those drafts cannot hide the restored document.
            let state = BTreeMap::new();
            let (view, diagnostics) = Self::inspect(&self.profile, revision, &state, &self.context)?;
            self.redo
                .push(std::mem::replace(&mut self.current.bytes, self.undo.pop().unwrap()));
            self.current.state = state;
            self.view = view;
            self.diagnostics = diagnostics;
        }
        Ok(())
    }

    pub fn redo(&mut self) -> Result<()> {
        if let Some(revision) = self.redo.last() {
            let state = BTreeMap::new();
            let (view, diagnostics) = Self::inspect(&self.profile, revision, &state, &self.context)?;
            self.undo
                .push(std::mem::replace(&mut self.current.bytes, self.redo.pop().unwrap()));
            self.current.state = state;
            self.view = view;
            self.diagnostics = diagnostics;
        }
        Ok(())
    }

    /// The host writes these bytes. Only call `mark_saved` after that succeeds.
    pub fn encode(&self) -> Result<Vec<u8>> {
        if !self.diagnostics.is_empty() {
            return Err(invalid(self.diagnostics.join("\n")));
        }
        let (output, decoded) = self.prepare_snapshot()?;
        let errors = Runtime::new(&self.profile)?.validate(&decoded)?;
        if !errors.is_empty() {
            return Err(invalid(errors.join("\n")));
        }
        Ok(output)
    }

    /// Snapshot for a session without committing to disk. Build diagnostics
    /// remain advisory here; the package's encoder repairs format checksums.
    pub fn snapshot(&self) -> Result<Vec<u8>> {
        self.prepare_snapshot().map(|(output, _)| output)
    }

    fn prepare_snapshot(&self) -> Result<(Vec<u8>, Vec<u8>)> {
        let output = Runtime::new(&self.profile)?.transform("encode", &self.current.bytes)?;
        if output.len() > MAX_DOCUMENT {
            return Err(invalid("encoded document exceeds 8 MiB"));
        }
        // A broken encoder cannot silently write an unreadable save.
        let decoded = Runtime::new(&self.profile)?.transform("decode", &output)?;
        if decoded.len() > MAX_DOCUMENT {
            return Err(invalid("decoded document exceeds 8 MiB"));
        }
        Ok((output, decoded))
    }

    pub fn mark_saved(&mut self) {
        self.saved = self.current.bytes.clone();
    }
}
