//! [`tango_library::http::Http`], in a browser: one `fetch`, read off
//! the response stream so the size cap and the progress callback both
//! mean something.
//!
//! # No `If-None-Match`
//!
//! The trait offers a conditional GET, and the native backend sends the
//! validator itself. Here we deliberately don't. `If-None-Match` is not
//! a CORS-safelisted request header, so setting it turns every index
//! poll into a preflight `OPTIONS` against the patch repo — which is a
//! CDN that has no reason to answer one. The browser's own HTTP cache
//! already revalidates on the repo's `Cache-Control`/`ETag` without our
//! help, so the caller's `NotModified` fast path simply never fires and
//! the index is re-parsed instead of skipped. That costs a few
//! milliseconds on a fetch this app does once at startup, and it is why
//! the index isn't polled on a timer here the way the desktop polls it.

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use tango_library::http::{Error, Fetch, GetFuture, Http, Request};

pub struct BrowserHttp;

fn transport(what: &str, e: impl std::fmt::Debug) -> Error {
    Error::Transport(format!("{what}: {e:?}"))
}

impl Http for BrowserHttp {
    fn get<'a>(&'a self, request: Request<'a>) -> GetFuture<'a> {
        Box::pin(async move {
            let window = web_sys::window().ok_or_else(|| Error::Transport("no window".into()))?;
            let response: web_sys::Response = JsFuture::from(window.fetch_with_str(request.url))
                .await
                .map_err(|e| transport("fetch", e))?
                .unchecked_into();

            if response.status() == 304 {
                return Ok(Fetch::NotModified);
            }
            if !response.ok() {
                return Err(Error::Status(response.status()));
            }

            // Only readable cross-origin when the repo lists it in
            // `Access-Control-Expose-Headers`; `None` just means the
            // caller stores no validator, which costs nothing given the
            // module note above.
            let etag = response.headers().get("etag").ok().flatten();

            // `Content-Length` is likewise not always exposed, so the
            // caller's expected size is the better total to report.
            let total = response
                .headers()
                .get("content-length")
                .ok()
                .flatten()
                .and_then(|v| v.parse::<u64>().ok())
                .or(request.max_len)
                .unwrap_or(0);

            let Some(stream) = response.body() else {
                // A body-less 200 (HEAD-like, or an opaque response we
                // can't read). Treat it as empty and let the caller's
                // hash check reject it.
                return Ok(Fetch::Body { etag, body: Vec::new() });
            };
            let reader: web_sys::ReadableStreamDefaultReader = stream
                .get_reader()
                .dyn_into()
                .map_err(|e| transport("getReader", e))?;

            let mut body: Vec<u8> = Vec::with_capacity(total as usize);
            if let Some(progress) = request.on_progress {
                if !progress.call(0, total) {
                    let _ = reader.cancel();
                    return Ok(Fetch::Cancelled);
                }
            }

            loop {
                let result = JsFuture::from(reader.read()).await.map_err(|e| transport("read", e))?;
                if js_sys::Reflect::get(&result, &JsValue::from_str("done"))
                    .map(|d| d.is_truthy())
                    .unwrap_or(true)
                {
                    break;
                }
                let chunk = js_sys::Reflect::get(&result, &JsValue::from_str("value"))
                    .map_err(|e| transport("chunk", e))?
                    .unchecked_into::<js_sys::Uint8Array>();

                // Abort on overrun rather than after: an oversized body
                // would otherwise be buffered in full before any hash
                // check could reject it.
                if let Some(limit) = request.max_len {
                    if body.len() as u64 + chunk.length() as u64 > limit {
                        let _ = reader.cancel();
                        return Err(Error::TooLarge { limit });
                    }
                }
                let start = body.len();
                body.resize(start + chunk.length() as usize, 0);
                chunk.copy_to(&mut body[start..]);

                if let Some(progress) = request.on_progress {
                    if !progress.call(body.len() as u64, total) {
                        let _ = reader.cancel();
                        return Ok(Fetch::Cancelled);
                    }
                }
            }

            Ok(Fetch::Body { etag, body })
        })
    }
}
