//! The art a session's emulator surface sits on: the game's background
//! from a Battle Network Legacy Collection install, when there is one.
//! Loaded once per game by a session launch, never while drawing.

/// Texture handle for `game`'s background art. Pulls the TGA out of the
/// appropriate BNLC volume's shared `exe.dat` and caches the decoded
/// handle per game. `None` whenever Steam / BNLC / the target entry
/// can't be read — the view drops the background widget instead of
/// degrading to a placeholder.
pub(super) fn load(game: &'static crate::library::game::Game) -> Option<iced::widget::image::Handle> {
    use std::collections::HashMap;
    use std::sync::LazyLock;
    static CACHE: LazyLock<std::sync::Mutex<HashMap<usize, Option<iced::widget::image::Handle>>>> =
        LazyLock::new(Default::default);
    let key = game as *const _ as usize;
    if let Some(cached) = CACHE.lock().unwrap().get(&key).cloned() {
        return cached;
    }
    // No BNLC release to borrow art from — the pane falls back to no
    // background, which it already does when BNLC is not installed.
    let bg = game.background?;
    let path = format!("exe/data/bg/{}", bg.tga);
    let handle = crate::library::bnlc::get(bg.volume)
        .and_then(|b| b.read_shared_file(&path))
        .and_then(|bytes| {
            // TGA has no magic prefix, so the image crate's
            // auto-detect refuses to guess it. Pass the format
            // explicitly — every shared-archive background is TGA.
            image::load_from_memory_with_format(&bytes, image::ImageFormat::Tga)
                .inspect_err(|e| log::warn!("bnlc bg {:?}/{}: decode: {e}", bg.volume, bg.tga))
                .ok()
        })
        .map(|img| {
            let rgba = img.into_rgba8();
            let (w, h) = rgba.dimensions();
            iced::widget::image::Handle::from_rgba(w, h, rgba.into_raw())
        });
    CACHE.lock().unwrap().insert(key, handle.clone());
    handle
}
