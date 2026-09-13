//! Upstream Luau type checking over the same immutable module graph as require.
mod bridge;
pub(crate) use bridge::initialize;

use crate::{Error, Result};

pub struct Checker;
impl Checker {
    pub fn new() -> Self {
        Self
    }

    pub(crate) fn check(&self, graph: &crate::modules::Graph) -> Result<()> {
        bridge::check(graph)
    }

    pub(crate) fn syntax(&self, source: &str) -> Result<()> {
        initialize();
        mlua::chunk::Compiler::new()
            .compile(source)
            .map(|_| ())
            .map_err(|error| Error::Typecheck(error.to_string()))
    }
}
