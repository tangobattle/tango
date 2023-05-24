pub const EREADER_NAME_COMMAND: &[u8] = b"\xff\x00";
#[repr(packed, C)]
#[derive(bytemuck::AnyBitPattern, Clone, Copy)]
pub struct EreaderNameCommand {
    pub index: u8,
}
const _: () = assert!(std::mem::size_of::<EreaderNameCommand>() == 0x1);

pub const EREADER_DESCRIPTION_COMMAND: &[u8] = b"\xff\x01";
#[repr(packed, C)]
#[derive(bytemuck::AnyBitPattern, Clone, Copy)]
pub struct EreaderDescriptionCommand {
    pub index: u8,
}
const _: () = assert!(std::mem::size_of::<EreaderDescriptionCommand>() == 0x1);

pub fn parser(charset: &[String]) -> crate::msg::Parser {
    crate::msg::Parser::builder()
        .with_ignore_unknown(true)
        .add_eof_rule(b"\xe5")
        .add_charset_rules(charset, 0xe4)
        .add_command_rule(EREADER_NAME_COMMAND, std::mem::size_of::<EreaderNameCommand>())
        .add_command_rule(
            EREADER_DESCRIPTION_COMMAND,
            std::mem::size_of::<EreaderDescriptionCommand>(),
        )
        .add_text_rule(b"\xe8", "\n")
        .add_command_rule(b"\xe6", 1)
        .add_command_rule(b"\xe7\x01", 0)
        .add_command_rule(b"\xe7\x02", 0)
        .add_command_rule(b"\xe7\x03", 0)
        .add_command_rule(b"\xed\x00", 2)
        .add_command_rule(b"\xf0\x00", 1)
        .add_command_rule(b"\xfc\x06", 0)
        .build()
}
