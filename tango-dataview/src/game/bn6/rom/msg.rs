pub enum Command {
    PrintVarCommand(PrintVarCommand),
    EreaderNameCommand(EreaderNameCommand),
    EreaderDescriptionCommand(EreaderDescriptionCommand),
}

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
impl crate::msg::CommandBody<Command> for PrintVarCommand {
    fn into_wrapped(self) -> Command {
        Command::PrintVarCommand(self)
    }
}

#[repr(packed, C)]
#[derive(bytemuck::AnyBitPattern, Clone, Copy)]
pub struct EreaderNameCommand {
    pub index: u8,
}
const _: () = assert!(std::mem::size_of::<EreaderNameCommand>() == 0x1);
impl crate::msg::CommandBody<Command> for EreaderNameCommand {
    fn into_wrapped(self) -> Command {
        Command::EreaderNameCommand(self)
    }
}

#[repr(packed, C)]
#[derive(bytemuck::AnyBitPattern, Clone, Copy)]
pub struct EreaderDescriptionCommand {
    pub index: u8,
}
const _: () = assert!(std::mem::size_of::<EreaderDescriptionCommand>() == 0x1);
impl crate::msg::CommandBody<Command> for EreaderDescriptionCommand {
    fn into_wrapped(self) -> Command {
        Command::EreaderDescriptionCommand(self)
    }
}

pub type Parser = crate::msg::Parser<Command>;

pub fn parser(charset: &[&str]) -> Parser {
    crate::msg::Parser::builder()
        .add_stop_rule(b"\xe6")
        .add_charset_rules(charset, 0xe4)
        .add_text_rule(b"\xe9", "\n")
        .add_command_rule::<PrintVarCommand>(b"\xfa\x03")
        .add_command_rule::<EreaderDescriptionCommand>(b"\xff\x01")
        .add_skip_rule(b"\xe7", 1)
        .add_skip_rule(b"\xe8\x01", 0)
        .add_skip_rule(b"\xe8\x02", 0)
        .add_skip_rule(b"\xe8\x03", 0)
        .add_skip_rule(b"\xe8\x04", 2)
        .add_skip_rule(b"\xe8\x05", 2)
        .add_skip_rule(b"\xe8\x06", 2)
        .add_skip_rule(b"\xee\x00", 2)
        .add_skip_rule(b"\xf1\x00", 1)
        .build()
}
