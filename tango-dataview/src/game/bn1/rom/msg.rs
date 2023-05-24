pub fn parser(charset: &[String]) -> crate::msg::Parser {
    crate::msg::Parser::builder()
        .with_ignore_unknown(true)
        .add_eof_rule(b"\xe7")
        .add_charset_rules(charset, 0xe5)
        .add_text_rule(b"\xe8", "\n")
        .build()
}
