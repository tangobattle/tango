use crate::battle;

struct InnerFacade {
    match_: std::sync::Arc<tokio::sync::Mutex<Option<std::sync::Arc<battle::Match>>>>,
    cancellation_token: tokio_util::sync::CancellationToken,
}

#[derive(Clone)]
pub struct Facade(std::rc::Rc<std::cell::RefCell<InnerFacade>>);

impl Facade {
    pub fn new(
        match_: std::sync::Arc<tokio::sync::Mutex<Option<std::sync::Arc<battle::Match>>>>,
        cancellation_token: tokio_util::sync::CancellationToken,
    ) -> Self {
        Self(std::rc::Rc::new(std::cell::RefCell::new(InnerFacade {
            match_,
            cancellation_token,
        })))
    }
    pub async fn match_(&self) -> Option<std::sync::Arc<battle::Match>> {
        self.0.borrow().match_.lock().await.clone()
    }

    pub async fn abort_match(&self) {
        *self.0.borrow().match_.lock().await = None;
        self.0.borrow().cancellation_token.cancel();
    }

    pub async fn end_match(&self) {
        std::process::exit(0);
    }
}
