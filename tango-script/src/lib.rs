//! Typed, sandboxed game packages and extensions. Game knowledge belongs in Luau.

mod archive;
mod bps;
mod checker;
mod decode;
mod editor;
mod effects;
mod encoding;
mod gamemode;
mod i18n;
mod inputs;
mod lz77;
mod modules;
mod package;
mod runtime;
mod telemetry;
pub mod ui;

pub use checker::Checker;
pub use editor::{
    Document, EditCommand, EditContext, EditorContext, EditorSession, Embedding, HostAction, SaveRequest, SaveTemplate,
    SessionUpdate, MAX_DOCUMENT,
};
pub use effects::Effect;
pub use gamemode::{GameModeProgram, LegacyReplay, PreparedGameMode, ReplayImport};
pub use inputs::{Inputs, MAX_INPUTS, MAX_INPUT_BYTES};
pub use package::{
    ExportKind, Manifest, ModuleExport, Package, PackageRef, Profile, MAX_PACKAGE_BYTES, MAX_PACKAGE_FILES,
};
pub use runtime::MAX_BUFFER;
pub use telemetry::{
    TelemetryContext, TelemetryEvent, TelemetryFrame, TelemetrySample, TelemetrySampler, TelemetryValue,
    TelemetryViewContext,
};
pub use ui::{Action, Choice, Event, Node};

pub const SDK: &str = include_str!("../sdk/tango.d.luau");

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Typecheck(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    TomlDecode(#[from] toml::de::Error),
    #[error(transparent)]
    Script(#[from] mlua::Error),
    #[error(transparent)]
    Archive(#[from] zip::result::ZipError),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Format retained package failures for the current UI language without
    /// executing scripts again. Plain errors retain their original text.
    pub fn localized(&self, locale: &unic_langid::LanguageIdentifier) -> String {
        match self {
            Self::Script(error) => i18n::localized(error, locale).to_string(),
            _ => self.to_string(),
        }
    }
}

pub(crate) fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}
