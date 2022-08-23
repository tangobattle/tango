#[macro_use]
extern crate lazy_static;

mod common;

mod hq2x;
pub use hq2x::calculate as hq2x;

mod hq3x;
pub use hq3x::calculate as hq3x;

mod hq4x;
pub use hq4x::calculate as hq4x;
