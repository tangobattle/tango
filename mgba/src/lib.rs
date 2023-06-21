pub mod arm_core;
pub mod blip;
pub mod core;
pub mod gba;
pub mod input;
mod log;
pub mod state;
pub mod sync;
pub mod thread;
pub mod timing;
pub mod trapper;
pub mod vfile;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("call to {0} failed")]
    CallFailed(&'static str),
}
