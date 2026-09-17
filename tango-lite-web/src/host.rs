//! Browser composition root. UI components receive these handles through context.
#[derive(Clone)]
pub struct Host {
    pub library: crate::library::Handle,
    pub engine: crate::engine::Handle,
    pub link: crate::link::Handle,
}
impl Default for Host {
    fn default() -> Self {
        let library = crate::library::Handle::default();
        let engine = crate::engine::Handle::new(library.clone());
        let link = crate::link::Handle::new(library.clone(), engine.clone());
        Self { library, engine, link }
    }
}
pub type Context = dioxus::prelude::Signal<Host>;
