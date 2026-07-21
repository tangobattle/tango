//! Browser bootstrap and platform glue: the wasm entry point, plus the
//! gesture-gated boot, OPFS import, and save-export helpers the UI
//! screens call into. The component tree itself lives in `crate::ui`.

use dioxus::prelude::*;
use wasm_bindgen::JsCast;

use crate::library::{self, SAVE_EXTENSIONS};
use crate::runtime::Runtime;
use crate::storage::{self, Storage};

const WORKLET_JS: Asset = asset!("/assets/audio-worklet.js");

/// The C shim's clock (mgba's `gettimeofday` for savestate stamps).
#[no_mangle]
pub extern "C" fn tango_web_now_unix_ms() -> f64 {
    js_sys::Date::now()
}

pub fn main() {
    install_panic_hook();
    let _ = console_log::init_with_level(log::Level::Info);
    mgba::log::install_default_logger();
    install_watchdog();
    install_service_worker();
    dioxus::launch(crate::ui::App);
}

/// Register the offline-shell service worker (../../sw.js). The file
/// sits at the site root, not in the asset bundle: its URL sets the
/// registration scope, GitHub Pages can't send Service-Worker-Allowed
/// headers to widen one, and dx flattens every bundled asset into
/// /assets/ — so CI copies it beside index.html instead. Debug builds
/// skip registration: dx serve doesn't serve the file, and a
/// cache-first shell fights hot reload anyway. Fire-and-forget:
/// losing it (insecure context, private browsing) only loses offline
/// support.
fn install_service_worker() {
    if cfg!(debug_assertions) {
        return;
    }
    let Some(window) = web_sys::window() else { return };
    let promise = window.navigator().service_worker().register("/sw.js");
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(e) = wasm_bindgen_futures::JsFuture::from(promise).await {
            log::warn!("service worker registration failed: {e:?}");
        }
    });
}

/// The console panic hook, plus a durable copy: a panic on wasm never
/// unwinds, so a mid-pump panic leaves the runtime's RefCell borrowed
/// forever and the session freezes with a healthy event loop — easy to
/// mistake for a hang and easy to lose the console for. Persist the
/// last panic (message + location + when) into
/// `localStorage["tango-web-panic"]` so it survives the reload and can be
/// read post-mortem.
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        console_error_panic_hook::hook(info);
        let record = format!(
            "{{\"at\":\"{}\",\"panic\":{}}}",
            String::from(js_sys::Date::new_0().to_iso_string()),
            js_sys::JSON::stringify(&info.to_string().into())
                .map(String::from)
                .unwrap_or_else(|_| "\"?\"".into())
        );
        if let Some(storage) = local_storage() {
            let _ = storage.set_item(PANIC_KEY, &record);
        }
    }));
}

/// The watchdog's persisted keys. `tango-web-heartbeat` is the newest
/// stamp; `-prev` is the previous page load's final stamp (a wedge's
/// time of death); `tango-web-stalls` is JSONL, newest last.
const HEARTBEAT_KEY: &str = "tango-web-heartbeat";
const HEARTBEAT_PREV_KEY: &str = "tango-web-heartbeat-prev";
const STALLS_KEY: &str = "tango-web-stalls";
const PANIC_KEY: &str = "tango-web-panic";

/// Gaps between watchdog firings longer than this are recorded as
/// stalls. Hidden-tab timer throttling stretches the interval
/// legitimately (up to a minute under intensive throttling), so every
/// record carries a `hidden` flag to discount those.
const STALL_MS: f64 = 2_500.0;
/// Stall records kept, newest last.
const MAX_STALL_RECORDS: usize = 40;

/// The in-app wedge watchdog: a 1s interval on the event loop that
/// (a) stamps a heartbeat — wall time, monotonic time, the runtime's
/// `tangoWebFrontier`/`tangoWebSlices`/`tangoWebSession` probes — into
/// localStorage, so a hard wedge (main thread stuck inside wasm, no
/// panic) leaves its time of death and last-known state behind; and
/// (b) records any gap over [`STALL_MS`] between firings — the event
/// loop was blocked that long, the signature of a recovering grind.
/// At startup the previous life's tail (panic, stalls, final heartbeat)
/// is logged, so a wedge's post-mortem survives the reload that clears
/// the console. (The devtools-injected watchdog from the first wedge
/// hunt died with every reload; this one is part of the app.)
fn install_watchdog() {
    let Some(storage) = local_storage() else {
        return;
    };
    if let Ok(Some(p)) = storage.get_item(PANIC_KEY) {
        log::warn!("previous life panicked: {p} (localStorage[{PANIC_KEY:?}])");
    }
    if let Ok(Some(hb)) = storage.get_item(HEARTBEAT_KEY) {
        log::info!("previous life's final heartbeat: {hb}");
        let _ = storage.set_item(HEARTBEAT_PREV_KEY, &hb);
    }
    if let Ok(Some(stalls)) = storage.get_item(STALLS_KEY) {
        let n = stalls.lines().count();
        if n > 0 {
            log::warn!(
                "{n} recorded main-thread stall(s), newest {} — \
                 localStorage[{STALLS_KEY:?}], removeItem to clear",
                stalls.lines().last().unwrap_or_default()
            );
        }
    }

    let last = std::cell::Cell::new(performance_now());
    gloo_timers::callback::Interval::new(1_000, move || {
        let Some(storage) = local_storage() else {
            return;
        };
        let now = performance_now();
        let gap = now - last.replace(now);
        let frontier = js_number_global("tangoWebFrontier").unwrap_or(-1.0);
        let slices = js_number_global("tangoWebSlices").unwrap_or(-1.0);
        let session = js_string_global("tangoWebSession").unwrap_or_else(|| "?".into());
        let at = String::from(js_sys::Date::new_0().to_iso_string());
        let _ = storage.set_item(
            HEARTBEAT_KEY,
            &format!(
                "{{\"at\":\"{at}\",\"mono\":{now:.0},\"frontier\":{frontier},\
                 \"slices\":{slices},\"session\":\"{session}\"}}"
            ),
        );
        if gap > STALL_MS {
            let hidden = web_sys::window()
                .and_then(|w| w.document())
                .map(|d| d.hidden())
                .unwrap_or(false);
            let mut lines: Vec<String> = storage
                .get_item(STALLS_KEY)
                .ok()
                .flatten()
                .map(|s| s.lines().map(str::to_owned).collect())
                .unwrap_or_default();
            lines.push(format!(
                "{{\"at\":\"{at}\",\"gap_ms\":{gap:.0},\"hidden\":{hidden},\
                 \"frontier\":{frontier},\"session\":\"{session}\"}}"
            ));
            let start = lines.len().saturating_sub(MAX_STALL_RECORDS);
            let _ = storage.set_item(STALLS_KEY, &lines[start..].join("\n"));
        }
    })
    .forget();
}

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window().and_then(|w| w.local_storage().ok().flatten())
}

fn performance_now() -> f64 {
    web_sys::window().unwrap().performance().unwrap().now()
}

/// A numeric debug probe the runtime pump publishes on `globalThis`.
fn js_number_global(name: &str) -> Option<f64> {
    js_sys::Reflect::get(&js_sys::global(), &name.into())
        .ok()
        .and_then(|v| v.as_f64())
}

/// A string debug probe the runtime pump publishes on `globalThis`.
fn js_string_global(name: &str) -> Option<String> {
    js_sys::Reflect::get(&js_sys::global(), &name.into())
        .ok()
        .and_then(|v| v.as_string())
}

/// Ensure the audio sink exists (must run within a user gesture), then
/// boot the detected game. A missing sink degrades to silence rather
/// than failing the boot.
pub async fn boot(
    runtime: std::rc::Rc<std::cell::RefCell<Runtime>>,
    game: library::GameRef,
    rom: Vec<u8>,
    save: Option<Vec<u8>>,
    save_file: Option<String>,
) -> anyhow::Result<()> {
    if !runtime.borrow().has_audio() {
        match crate::platform::audio::web::WebAudio::create(&WORKLET_JS.to_string(), || {
            crate::runtime::pump_from_audio_report();
        })
        .await
        {
            Ok(audio) => runtime.borrow_mut().set_audio(audio),
            Err(e) => log::error!("audio unavailable: {e:?}"),
        }
    }
    runtime.borrow_mut().start_local(game, rom, save, save_file)
}

/// Whether this is iOS/iPadOS WebKit. iPadOS 13+ masquerades as macOS,
/// so the touch-point count disambiguates.
pub fn is_ios() -> bool {
    let Some(nav) = web_sys::window().map(|w| w.navigator()) else {
        return false;
    };
    let ua = nav.user_agent().unwrap_or_default();
    ["iPhone", "iPad", "iPod"].iter().any(|d| ua.contains(d))
        || (ua.contains("Macintosh") && nav.max_touch_points() > 1)
}

/// Clear a file input after handling its pick, so picking the very same
/// file again fires `change` again (an unchanged value doesn't, which
/// reads as a dead importer on retries and re-imports).
pub fn reset_file_input(evt: &dioxus::events::FormEvent) {
    use dioxus::web::WebEventExt;
    if let Some(input) = evt
        .try_as_web_event()
        .and_then(|e| e.target())
        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
    {
        input.set_value("");
    }
}

/// Read a picked file's bytes via the File's own `arrayBuffer()`.
/// Dioxus's `FileData::read_bytes` drives a FileReader without hooking
/// `onerror`, so an unreadable file — iOS pickers produce these for
/// not-yet-downloaded iCloud items — hangs the import forever instead
/// of failing; the promise path rejects properly.
async fn read_file(file: &dioxus::html::FileData) -> anyhow::Result<Vec<u8>> {
    use dioxus::web::WebFileExt;
    let web_file = file
        .get_web_file()
        .ok_or_else(|| anyhow::anyhow!("no backing File"))?;
    let buf = wasm_bindgen_futures::JsFuture::from(web_file.array_buffer())
        .await
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    Ok(js_sys::Uint8Array::new(&buf).to_vec())
}

/// What an import pass did: files landed per kind and files skipped.
#[derive(Default, Clone)]
pub struct ImportCounts {
    pub roms: u32,
    pub saves: u32,
    pub skipped: u32,
    /// The (last) imported ROM's game and save's file name — only
    /// meaningful to callers when the matching count is exactly 1
    /// (a lone arrival gets auto-selected).
    pub rom_game: Option<library::GameRef>,
    pub save_name: Option<String>,
}

/// Import picked files into OPFS, routed by extension: ROMs into
/// `roms/` (normalized names), saves into the flat `saves/` directory
/// (which game a save belongs to is content-detected at listing time,
/// like the desktop scanner).
pub async fn import_files(storage: &Storage, files: Vec<dioxus::html::FileData>) -> ImportCounts {
    let mut counts = ImportCounts::default();
    for file in files {
        let name = file.name();
        let bytes = match read_file(&file).await {
            Ok(b) => b,
            Err(e) => {
                log::error!("couldn't read {name}: {e:?}");
                counts.skipped += 1;
                continue;
            }
        };
        if library::has_extension(&name, library::ROM_EXTENSIONS) {
            let info = match library::rom_info(&name, &bytes) {
                Ok(info) => info,
                Err(e) => {
                    log::warn!("not importing {name}: {e}");
                    counts.skipped += 1;
                    continue;
                }
            };
            // The stored name is normalized to the cartridge, not the
            // picked file, so re-importing the same ROM overwrites
            // itself instead of piling up copies.
            let stored = library::normalized_file_name(&info);
            match storage::write(storage.roms(), &stored, &bytes).await {
                Ok(()) => {
                    counts.roms += 1;
                    counts.rom_game = Some(info.game);
                }
                Err(e) => {
                    log::error!("couldn't import {name}: {e}");
                    counts.skipped += 1;
                }
            }
        } else if library::has_extension(&name, SAVE_EXTENSIONS) {
            // GBA flash tops out at 128 KiB; leave headroom for
            // emulator save footers.
            if bytes.len() > 512 * 1024 {
                log::warn!("not importing {name}: save file too large");
                counts.skipped += 1;
                continue;
            }
            // A save that no registered game can load is junk — refuse
            // it now rather than showing a row no game ever lists.
            if library::save_compatible_games(&bytes).is_empty() {
                log::warn!("not importing {name}: no supported game can load it");
                counts.skipped += 1;
                continue;
            }
            match storage::write(storage.saves(), &name, &bytes).await {
                Ok(()) => {
                    counts.saves += 1;
                    counts.save_name = Some(name.clone());
                }
                Err(e) => {
                    log::error!("couldn't import {name}: {e}");
                    counts.skipped += 1;
                }
            }
        } else if let Ok(info) = library::rom_info(&name, &bytes) {
            // Unknown extension but the content identifies as a clean
            // dump: still a ROM. iOS's picker is fond of handing files
            // over with mangled names.
            let stored = library::normalized_file_name(&info);
            match storage::write(storage.roms(), &stored, &bytes).await {
                Ok(()) => {
                    counts.roms += 1;
                    counts.rom_game = Some(info.game);
                }
                Err(e) => {
                    log::error!("couldn't import {name}: {e}");
                    counts.skipped += 1;
                }
            }
        } else {
            log::warn!("not importing {name}: unrecognized extension");
            counts.skipped += 1;
        }
    }
    counts
}

/// Offer a byte blob as a download (save/replay export).
#[allow(dead_code)] // save export UI
pub fn download_bytes(name: &str, bytes: &[u8]) {
    let array = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes).buffer());
    let Ok(blob) = web_sys::Blob::new_with_buffer_source_sequence(&array) else {
        return;
    };
    let Ok(url) = web_sys::Url::create_object_url_with_blob(&blob) else {
        return;
    };
    let document = web_sys::window().unwrap().document().unwrap();
    if let Ok(a) = document.create_element("a") {
        let a: web_sys::HtmlAnchorElement = a.unchecked_into();
        a.set_href(&url);
        a.set_download(name);
        a.click();
    }
    let _ = web_sys::Url::revoke_object_url(&url);
}
