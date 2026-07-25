//! What applying a [`Message`](crate::Message) asks the host to do next.
//!
//! The netplay machine used to return `iced::Task<Message>`, which was
//! the only reason this whole subsystem — a connection state machine
//! with no opinion about pixels — belonged to one UI toolkit. It only
//! ever used four shapes of it: nothing, feed a message straight back,
//! drive a future and feed its result back, or several of those at once.
//! [`Effect`] is those four shapes and nothing else.
//!
//! A host runs an effect by draining [`Effect::into_steps`] and doing as
//! each [`Step`] says, feeding every resulting `Message` back into
//! [`State::update`](crate::State::update). This mirrors the
//! `Option<Effect>` reducer shape the tabs already use, so it composes
//! the same way.

use crate::Message;
use tango_library::marker::{BoxFuture, WasmNotSend};

/// Async work the host drives, resolving to the message to feed back.
pub type Work = BoxFuture<'static, Message>;

/// One thing for the host to do.
pub enum Step {
    /// Feed this message back into `update`.
    Emit(Message),
    /// Drive this to completion, then feed its message back into
    /// `update`.
    Spawn(Work),
}

/// Zero or more [`Step`]s, in order.
#[derive(Default)]
pub struct Effect(Vec<Step>);

impl Effect {
    /// Nothing to do.
    pub fn none() -> Self {
        Self(Vec::new())
    }

    /// Feed `msg` straight back into `update`.
    pub fn done(msg: Message) -> Self {
        Self(vec![Step::Emit(msg)])
    }

    /// Drive `future`, mapping its output to the message to feed back.
    pub fn perform<T: WasmNotSend + 'static>(
        future: impl std::future::Future<Output = T> + WasmNotSend + 'static,
        f: impl FnOnce(T) -> Message + WasmNotSend + 'static,
    ) -> Self {
        Self(vec![Step::Spawn(Box::pin(async move { f(future.await) }))])
    }

    /// Run several effects. Order is preserved, and empty effects cost
    /// nothing — which is what lets the handlers keep composing an
    /// unconditional `batch` of maybe-empty parts.
    pub fn batch(effects: impl IntoIterator<Item = Self>) -> Self {
        Self(effects.into_iter().flat_map(|e| e.0).collect())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Take the steps, in order, for the host to run.
    pub fn into_steps(self) -> Vec<Step> {
        self.0
    }
}
