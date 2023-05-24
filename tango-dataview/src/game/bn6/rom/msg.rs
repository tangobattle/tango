pub const PRINT_VAR_COMMAND: &[u8] = b"\xfa\x03";
#[repr(packed, C)]
#[derive(bytemuck::AnyBitPattern, Clone, Copy, c2rust_bitfields::BitfieldStruct)]
pub struct PrintVarCommand {
    #[bitfield(name = "min_length", ty = "u8", bits = "2..=5")]
    #[bitfield(name = "pad_zeros", ty = "bool", bits = "6..=6")]
    #[bitfield(name = "pad_left", ty = "bool", bits = "7..=7")]
    pub params: [u8; 1],
    pub buffer: u8,
}
const _: () = assert!(std::mem::size_of::<PrintVarCommand>() == 0x2);

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
        .add_eof_rule(b"\xe6")
        .add_charset_rules(charset, 0xe4)
        .add_text_rule(b"\xe9", "\n")
        .add_command_rule(PRINT_VAR_COMMAND, std::mem::size_of::<PrintVarCommand>())
        .add_command_rule(
            EREADER_DESCRIPTION_COMMAND,
            std::mem::size_of::<EreaderDescriptionCommand>(),
        )
        .add_command_rule(b"\xe7", 1)
        .add_command_rule(b"\xe8\x01", 0)
        .add_command_rule(b"\xe8\x02", 0)
        .add_command_rule(b"\xe8\x03", 0)
        .add_command_rule(b"\xe8\x04", 2)
        .add_command_rule(b"\xe8\x05", 2)
        .add_command_rule(b"\xe8\x06", 2)
        .add_command_rule(b"\xee\x00", 2)
        .add_command_rule(b"\xf1\x00", 1)
        .build()
}
