//! The network seam: one conditional, size-capped, cancellable GET.
//!
//! Only the patch repo is fetched from here — the index (small, polled
//! on an ETag) and packages (large, hash-verified). A browser frontend
//! implements this over `fetch`; the native one over reqwest, behind the
//! `native` feature.

use crate::marker::{BoxFuture, WasmNotSend, WasmNotSync};

/// Future returned by [`Http::get`]. `Send` off wasm, where `fetch`
/// futures aren't.
pub type GetFuture<'a> = BoxFuture<'a, Result<Fetch, Error>>;

/// Called with `(downloaded, total)` as the body arrives; returning
/// false cancels the transfer. `total` is the server's content length,
/// or the caller's expected size when the server didn't say.
pub type ProgressFn<'a> = dyn ProgressCallback + 'a;

/// Blanket-implemented over closures so [`ProgressFn`] can be one `dyn`
/// type on both targets: a bare `dyn Fn + Send + Sync` would need its
/// own `#[cfg]` pair, since the marker traits can't be named in a trait
/// object.
pub trait ProgressCallback: WasmNotSend + WasmNotSync {
    fn call(&self, downloaded: u64, total: u64) -> bool;
}

impl<F: Fn(u64, u64) -> bool + WasmNotSend + WasmNotSync> ProgressCallback for F {
    fn call(&self, downloaded: u64, total: u64) -> bool {
        self(downloaded, total)
    }
}

pub struct Request<'a> {
    pub url: &'a str,
    /// Sent as `If-None-Match`, making the request conditional.
    pub if_none_match: Option<&'a str>,
    /// Hard cap on the body. A server sending more than this is
    /// misbehaving, and buffering it unbounded would let it exhaust
    /// memory before any post-transfer hash check runs — so the
    /// implementation must abort as soon as it overruns.
    pub max_len: Option<u64>,
    pub on_progress: Option<&'a ProgressFn<'a>>,
}

impl<'a> Request<'a> {
    pub fn new(url: &'a str) -> Self {
        Self {
            url,
            if_none_match: None,
            max_len: None,
            on_progress: None,
        }
    }
}

/// How a GET ended, short of an error.
#[derive(Debug)]
pub enum Fetch {
    /// The conditional request matched: nothing was transferred.
    NotModified,
    /// `on_progress` asked to stop.
    Cancelled,
    Body {
        etag: Option<String>,
        body: Vec<u8>,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Transport(String),
    #[error("http status {0}")]
    Status(u16),
    #[error("response body exceeds the {limit} bytes expected")]
    TooLarge { limit: u64 },
    #[error("timed out")]
    Timeout,
}

pub trait Http: WasmNotSend + WasmNotSync + 'static {
    fn get<'a>(&'a self, request: Request<'a>) -> GetFuture<'a>;
}

/// How long a single transfer, and each chunk within it, may stall.
pub const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
mod reqwest_impl {
    use super::{Error, Fetch, GetFuture, Http, Request, TIMEOUT};
    use futures::StreamExt;

    /// reqwest-backed [`Http`]. Holds the client so connections pool
    /// across the index poll and any package downloads.
    pub struct ReqwestHttp {
        client: reqwest::Client,
    }

    impl ReqwestHttp {
        pub fn new() -> Self {
            Self {
                client: reqwest::Client::new(),
            }
        }
    }

    impl Default for ReqwestHttp {
        fn default() -> Self {
            Self::new()
        }
    }

    fn transport(e: impl std::fmt::Display) -> Error {
        Error::Transport(e.to_string())
    }

    impl Http for ReqwestHttp {
        fn get<'a>(&'a self, request: Request<'a>) -> GetFuture<'a> {
            Box::pin(async move {
                let mut req = self
                    .client
                    .get(request.url)
                    .header("User-Agent", "tango")
                    .timeout(TIMEOUT);
                if let Some(etag) = request.if_none_match {
                    req = req.header(reqwest::header::IF_NONE_MATCH, etag);
                }

                let response = req.send().await.map_err(transport)?;
                if response.status() == reqwest::StatusCode::NOT_MODIFIED {
                    return Ok(Fetch::NotModified);
                }
                if !response.status().is_success() {
                    return Err(Error::Status(response.status().as_u16()));
                }

                let etag = response
                    .headers()
                    .get(reqwest::header::ETAG)
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_owned());

                let total = response.content_length().or(request.max_len).unwrap_or(0);
                let mut body = Vec::with_capacity(total as usize);
                let mut stream = response.bytes_stream();

                if let Some(progress) = request.on_progress {
                    if !progress.call(0, total) {
                        return Ok(Fetch::Cancelled);
                    }
                }

                while let Some(chunk) = tokio::time::timeout(TIMEOUT, stream.next())
                    .await
                    .map_err(|_| Error::Timeout)?
                {
                    let chunk = chunk.map_err(transport)?;
                    if let Some(limit) = request.max_len {
                        if body.len() as u64 + chunk.len() as u64 > limit {
                            return Err(Error::TooLarge { limit });
                        }
                    }
                    body.extend_from_slice(&chunk);
                    if let Some(progress) = request.on_progress {
                        if !progress.call(body.len() as u64, total) {
                            return Ok(Fetch::Cancelled);
                        }
                    }
                }

                Ok(Fetch::Body { etag, body })
            })
        }
    }
}

#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub use reqwest_impl::ReqwestHttp;
