pub enum Command {}

pub type Parser = crate::msg::Parser<Command>;

pub fn parser(charset: &[String]) -> Parser {
    crate::msg::Parser::builder()
        .add_stop_rule(b"\xe7")
        .add_charset_rules(charset, 0xe5)
        .add_text_rule(b"\xe8", "\n")
        .build()
}
