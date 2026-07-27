//! The save view lives in `tango-gamesupport-ui`; each game family's
//! editor lives in its own UI crate in the `gamesupport` submodule.
//! What stays here is the one thing only the app can own: the
//! per-family [`SaveUi`] registry, because the `gamesupport-*` feature
//! gates are this crate's.

pub use tango_gamesupport_ui::save_ui::SaveUi;
pub use tango_gamesupport_ui::save_view::*;

use crate::library::rom::GameRef;

/// The save-editor UI for `game`'s family. Families compiled in without
/// their UI crate fall back to a bare folder list (never expected in a
/// shipped build — every `gamesupport-<game>` feature turns both on).
pub fn save_ui_for(game: GameRef) -> &'static dyn SaveUi {
    let (family, _) = game.family_and_variant();
    match family {
        #[cfg(feature = "gamesupport-bcc")]
        "bcc" | "exebcgp" => &tango_gamesupport_bcc_ui::SAVE_UI,
        #[cfg(feature = "gamesupport-bn1")]
        "bn1" | "exe1" => &tango_gamesupport_bn1_ui::SAVE_UI,
        #[cfg(feature = "gamesupport-bn2")]
        "bn2" | "exe2" => &tango_gamesupport_bn2_ui::SAVE_UI,
        #[cfg(feature = "gamesupport-bn3")]
        "bn3" | "exe3" => &tango_gamesupport_bn3_ui::SAVE_UI,
        #[cfg(feature = "gamesupport-bn4")]
        "bn4" | "exe4" => &tango_gamesupport_bn4_ui::SAVE_UI,
        #[cfg(feature = "gamesupport-bn5")]
        "bn5" | "exe5" => &tango_gamesupport_bn5_ui::SAVE_UI,
        #[cfg(feature = "gamesupport-bn6")]
        "bn6" | "exe6" => &tango_gamesupport_bn6_ui::SAVE_UI,
        #[cfg(feature = "gamesupport-exe45")]
        "exe45" => &tango_gamesupport_exe45_ui::SAVE_UI,
        _ => &tango_gamesupport_ui::save_ui::FALLBACK_SAVE_UI,
    }
}
