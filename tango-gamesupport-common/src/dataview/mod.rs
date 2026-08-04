//! The save/ROM dataview substrate — formerly the public
//! `tango-dataview` crate. The traits every game crate's `dataview`
//! module implements (`save::Save`, `rom::Assets`) plus the shared
//! derived views (navicust composition, auto-battle data, msg
//! decoding). Always compiled — no UI dependencies — so headless game
//! builds get save parsing without this crate's `ui` feature.
//!
//! The public boundary crosses here through the opaque handles below:
//! `Game::parse_save` / `Game::load_rom_assets` hand the app
//! `tango_gamesupport::BoxedSave` / `BoxedAssets` envelopes minted by
//! [`wrap_save`] / [`wrap_assets`]; this crate is the only place that
//! looks back inside them.

pub mod auto_battle_data;
pub mod msg;
pub mod navicust;
pub mod nds;
pub mod rom;
pub mod save;

#[cfg(target_endian = "big")]
compile_error!("Big endian architectures are not currently supported");

/// The concrete type behind every `tango_gamesupport::BoxedSave`.
pub struct SaveHandle(pub Box<dyn save::Save + Send + Sync>);

impl tango_gamesupport::SaveData for SaveHandle {
    fn to_sram_dump(&self) -> Vec<u8> {
        self.0.to_sram_dump()
    }

    fn rebuild_checksum(&mut self) {
        self.0.rebuild_checksum();
    }

    fn clone_box(&self) -> tango_gamesupport::BoxedSave {
        Box::new(SaveHandle(self.0.clone_box()))
    }
}

/// Seal a parsed save into the public opaque envelope — for the game
/// crates' `parse_save_fn` and template registrations.
pub fn wrap_save(save: Box<dyn save::Save + Send + Sync>) -> tango_gamesupport::BoxedSave {
    Box::new(SaveHandle(save))
}

/// Recover the parsed save from the public envelope. Every `BoxedSave`
/// is minted by [`wrap_save`], so a mismatch is a wiring bug worth a
/// loud panic.
pub fn unwrap_save(save: tango_gamesupport::BoxedSave) -> Box<dyn save::Save + Send + Sync> {
    (save as Box<dyn std::any::Any>)
        .downcast::<SaveHandle>()
        .expect("BoxedSave must hold this crate's SaveHandle")
        .0
}

/// The concrete type behind every `tango_gamesupport::BoxedAssets`.
pub struct AssetsHandle(pub Box<dyn rom::Assets + Send + Sync>);

impl tango_gamesupport::AssetsData for AssetsHandle {}

pub fn wrap_assets(assets: Box<dyn rom::Assets + Send + Sync>) -> tango_gamesupport::BoxedAssets {
    Box::new(AssetsHandle(assets))
}

pub fn unwrap_assets(assets: tango_gamesupport::BoxedAssets) -> Box<dyn rom::Assets + Send + Sync> {
    (assets as Box<dyn std::any::Any>)
        .downcast::<AssetsHandle>()
        .expect("BoxedAssets must hold this crate's AssetsHandle")
        .0
}

// `?` in the game crates' parse fns lands dataview parse errors in the
// public error type. Boxed rather than a public variant of its own:
// the shape is private, the app only displays it.
impl From<save::Error> for tango_gamesupport::Error {
    fn from(e: save::Error) -> Self {
        tango_gamesupport::Error::Save(Box::new(e))
    }
}
