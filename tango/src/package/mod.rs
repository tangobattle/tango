//! Package capabilities and their integration with the Tango host.

pub(crate) mod editor;
mod error;
pub(crate) mod gamemode;
pub(crate) mod replay;
pub(crate) mod telemetry;

mod catalog;
pub(crate) use catalog::Catalog;
pub(crate) mod selection;

mod choice;
pub(crate) use choice::Choice;
