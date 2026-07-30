//! The DS build keeps the GBA games' text-archive encoding wholesale —
//! the chip names decode with BN5's own charsets — so this is BN5's
//! parser with every command downgraded to a skip: nothing this crate
//! renders keeps a command (no e-reader text exists on the DS cart,
//! and the print-var in chip descriptions only pads layout).

pub enum Command {}

pub type Parser = tango_gamesupport_common::dataview::msg::Parser<Command>;

pub fn parser(charset: &[&str]) -> Parser {
    tango_gamesupport_common::dataview::msg::Parser::builder()
        .add_stop_rule(b"\xe6")
        .add_charset_rules(charset, 0xe4)
        .add_text_rule(b"\xe9", "\n")
        .add_skip_rule(b"\xe7", 1)
        .add_skip_rule(b"\xe8\x01", 0)
        .add_skip_rule(b"\xe8\x02", 0)
        .add_skip_rule(b"\xe8\x03", 0)
        .add_skip_rule(b"\xe8\x04", 2)
        .add_skip_rule(b"\xe8\x05", 2)
        .add_skip_rule(b"\xe8\x06", 2)
        .add_skip_rule(b"\xee\x00", 2)
        .add_skip_rule(b"\xf1\x00", 1)
        .add_skip_rule(b"\xfa\x03", 2)
        .build()
}
