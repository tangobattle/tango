pub enum Command {}

pub type Parser = crate::msg::Parser<Command>;

pub fn parser(charset: &[&str]) -> Parser {
    crate::msg::Parser::builder()
        .add_stop_rule(b"\xe7")
        .add_charset_rules(charset, 0xe5)
        .add_text_rule(b"\xe8", "\n")
        .add_skip_rule(b"\xea\x00", 2)
        .add_skip_rule(b"\xea\xff", 2)
        .add_skip_rule(b"\xeb", 0)
        .add_skip_rule(b"\xec\x00", 1)
        .add_skip_rule(b"\xee\x02", 2)
        .add_skip_rule(b"\xf1\x02", 0)
        .add_skip_rule(b"\xf1\x03", 0)
        .build()
}
