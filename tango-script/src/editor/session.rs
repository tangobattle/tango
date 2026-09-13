//! Whole-document edit sessions, independent of UI, game formats and storage.
use super::*;
use crate::Effect;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditCommand {
    Begin,
    Cancel,
    Save,
}

#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub struct EditContext {
    pub editable: bool,
    pub editing: bool,
    pub saving: bool,
    pub(crate) can_save: bool,
}

/// An opaque receipt for a particular write. Clones refer to that same write,
/// never to another session or a newer save attempt.
#[derive(Clone)]
pub struct SaveRequest {
    token: Arc<()>,
    bytes: Arc<[u8]>,
}
impl std::fmt::Debug for SaveRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SaveRequest")
            .field("byte_len", &self.bytes.len())
            .finish_non_exhaustive()
    }
}
impl SaveRequest {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[derive(Default)]
pub struct SessionUpdate {
    /// Execute once, after the document transition has succeeded.
    pub effects: Vec<Effect>,
    /// Write these bytes, then acknowledge this exact receipt with finish_save.
    pub save: Option<SaveRequest>,
}

struct PendingSave {
    request: SaveRequest,
    success: Document,
    failure: Document,
}

pub struct EditorSession {
    document: Document,
    pending: Option<PendingSave>,
}

impl EditorSession {
    pub fn open(profile: Profile, input: &[u8], editable: bool) -> Result<Self> {
        Self::open_embedded(profile, input, editable, Embedding::default())
    }

    pub fn open_embedded(profile: Profile, input: &[u8], editable: bool, embedding: Embedding) -> Result<Self> {
        Ok(Self {
            document: Document::open_with_context(
                profile,
                input,
                EditorContext {
                    read_only: true,
                    embedding,
                    session: Some(EditContext {
                        editable,
                        ..Default::default()
                    }),
                },
            )?,
            pending: None,
        })
    }

    pub fn document(&self) -> &Document {
        &self.document
    }
    pub fn is_editing(&self) -> bool {
        self.document.context.session.unwrap().editing
    }
    pub fn is_saving(&self) -> bool {
        self.pending.is_some()
    }

    fn idle(&self) -> Result<()> {
        if self.pending.is_some() {
            return Err(invalid("a save is awaiting completion"));
        }
        Ok(())
    }

    pub fn dispatch(&mut self, action: Action) -> Result<SessionUpdate> {
        self.idle()?;
        // Bytes, history and raster payloads are Arc-backed. A candidate owns
        // only its state/tree copies until every callback and effect succeeds.
        let mut next = self.document.clone();
        let effects = next.dispatch(action)?;
        self.apply(next, effects)
    }

    /// For embedding controls outside the script's view tree. The same host
    /// capability checks apply as for a package-returned edit command.
    pub fn edit(&mut self, command: EditCommand) -> Result<SessionUpdate> {
        self.idle()?;
        self.apply(self.document.clone(), vec![Effect::Edit { command }])
    }

    fn apply(&mut self, mut next: Document, effects: Vec<Effect>) -> Result<SessionUpdate> {
        let mut command = None;
        let mut output = SessionUpdate::default();
        let mut pending = None;
        for effect in effects {
            match effect {
                Effect::Edit { command: requested } => {
                    if command.replace(requested).is_some() {
                        return Err(invalid("an update may request only one edit transition"));
                    }
                }
                effect => output.effects.push(effect),
            }
        }
        let context = next.context.session.unwrap();
        match command {
            Some(EditCommand::Begin) => {
                if !context.editable || context.editing {
                    return Err(invalid("cannot begin an edit session"));
                }
                Self::transition(&mut next, true)?;
            }
            Some(EditCommand::Cancel) => {
                if !context.editing {
                    return Err(invalid("no edit session to cancel"));
                }
                next.current.bytes = next.saved.clone();
                Self::transition(&mut next, false)?;
            }
            Some(EditCommand::Save) => {
                if !context.editing || !context.editable {
                    return Err(invalid("no edit session to save"));
                }
                let bytes: Arc<[u8]> = next.encode()?.into();
                let mut success = next.clone();
                // Adopt the exact model represented by the written bytes,
                // including any package-owned encoder normalization.
                success.current.bytes = Runtime::new(&success.profile)?.transform("decode", &bytes)?.into();
                Self::transition(&mut success, false)?;
                success.mark_saved();
                let failure = next.clone();
                next.context.session.as_mut().unwrap().saving = true;
                Self::refresh(&mut next)?;
                let request = SaveRequest {
                    token: Arc::new(()),
                    bytes,
                };
                pending = Some(PendingSave {
                    request: request.clone(),
                    success,
                    failure,
                });
                output.save = Some(request);
            }
            None => {}
        }
        // Recheck against the resulting mode too: a combined Begin/Invoke must
        // not launch an action that is unavailable while editing or saving.
        next.context.validate_effects(&output.effects)?;
        self.document = next;
        self.pending = pending;
        Ok(output)
    }

    fn refresh(document: &mut Document) -> Result<()> {
        let (view, diagnostics) = Document::inspect(
            &document.profile,
            &document.current.bytes,
            &document.current.state,
            &document.context,
        )?;
        document.view = view;
        document.diagnostics = diagnostics;
        Ok(())
    }

    fn transition(document: &mut Document, editing: bool) -> Result<()> {
        document.context.read_only = !editing;
        document.context.session.as_mut().unwrap().editing = editing;
        document.current.state = Runtime::new(&document.profile)?.reset_view(&document.current.state)?;
        Self::refresh(document)?;
        document.undo.clear();
        document.redo.clear();
        Ok(())
    }

    /// Called after storage completes. Both views were prepared before the
    /// write: acknowledging success cannot now fail because of package code.
    pub fn finish_save(&mut self, request: &SaveRequest, written: bool) -> Result<()> {
        let Some(pending) = &self.pending else {
            return Err(invalid("no pending save"));
        };
        if !Arc::ptr_eq(&request.token, &pending.request.token) {
            return Err(invalid("save receipt belongs to a different write"));
        }
        let pending = self.pending.take().unwrap();
        self.document = if written { pending.success } else { pending.failure };
        Ok(())
    }

    pub fn set_locale(&mut self, locale: &str) -> Result<()> {
        self.idle()?;
        self.document.set_locale(locale)
    }

    pub fn set_embedding(&mut self, embedding: Embedding) -> Result<()> {
        self.idle()?;
        self.document.set_embedding(embedding)
    }

    /// Refresh an embedded panel's locale, controls and edit permission as one
    /// transaction. Repeating the same configuration is free of script calls,
    /// including during a pending write. Revocation preserves an open draft;
    /// the host may re-enable it or cancel, but cannot mutate or save it.
    pub fn configure(&mut self, locale: &str, editable: bool, embedding: Embedding) -> Result<()> {
        embedding.validate()?;
        let locale = crate::i18n::locale(locale)?;
        if &locale == self.document.profile.locale()
            && self.document.context.session.unwrap().editable == editable
            && self.document.context.embedding == embedding
        {
            return Ok(());
        }
        self.idle()?;
        let mut next = self.document.clone();
        next.profile = next.profile.with_locale(&locale.to_string())?;
        next.context.embedding = embedding;
        let session = next.context.session.as_mut().unwrap();
        session.editable = editable;
        next.context.read_only = !editable || !session.editing;
        Self::refresh(&mut next)?;
        self.document = next;
        Ok(())
    }

    pub fn undo(&mut self) -> Result<()> {
        self.idle()?;
        if !self.is_editing() || self.document.is_read_only() {
            return Err(invalid("no edit session to undo"));
        }
        self.document.undo()
    }
    pub fn redo(&mut self) -> Result<()> {
        self.idle()?;
        if !self.is_editing() || self.document.is_read_only() {
            return Err(invalid("no edit session to redo"));
        }
        self.document.redo()
    }
}
