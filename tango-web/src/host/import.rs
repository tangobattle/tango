//! The shared import pass: picked/dropped files routed into storage by
//! kind (ROMs normalized into `roms/`, saves content-validated into the
//! flat `saves/` directory). Only [`read_file`] differs per target —
//! the browser reads the backing `File` via its own `arrayBuffer()`
//! promise, native reads through Dioxus's file engine.

use crate::library::{self, SAVE_EXTENSIONS};
use crate::storage::{self, Storage};

/// Read a picked file's bytes via the File's own `arrayBuffer()`.
/// Dioxus's `FileData::read_bytes` drives a FileReader without hooking
/// `onerror`, so an unreadable file — iOS pickers produce these for
/// not-yet-downloaded iCloud items — hangs the import forever instead
/// of failing; the promise path rejects properly.
#[cfg(target_arch = "wasm32")]
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

#[cfg(not(target_arch = "wasm32"))]
async fn read_file(file: &dioxus::html::FileData) -> anyhow::Result<Vec<u8>> {
    Ok(file
        .read_bytes()
        .await
        .map_err(|e| anyhow::anyhow!("{e:?}"))?
        .to_vec())
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

/// Import picked files into storage, routed by extension: ROMs into
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
