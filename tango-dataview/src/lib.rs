pub mod auto_battle_data;
pub mod game;
pub mod msg;
pub mod navicust;
pub mod rom;
pub mod save;

#[cfg(target_endian = "big")]
compile_error!("Big endian architectures are not currently supported");
