//! The remake keeps the GBA game's text encoding wholesale: OSS's chip
//! names and descriptions are BN1's own bytes, decoded by BN1's own
//! charset (which is how the archives were found — a byte search for
//! the GBA entries). So this is BN1's parser verbatim.

pub enum Command {}

pub type Parser = tango_gamesupport_common_dataview::msg::Parser<Command>;

pub fn parser(charset: &[&str]) -> Parser {
    tango_gamesupport_common_dataview::msg::Parser::builder()
        .add_stop_rule(b"\xe7")
        .add_charset_rules(charset, 0xe5)
        .add_text_rule(b"\xe8", "\n")
        .build()
}
