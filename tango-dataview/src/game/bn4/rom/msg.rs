pub enum Command {
    EreaderNameCommand(EreaderNameCommand),
    EreaderDescriptionCommand(EreaderDescriptionCommand),
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

pub fn parser(charset: &[String]) -> Parser {
    crate::msg::Parser::builder()
        .add_stop_rule(b"\xe5")
        .add_charset_rules(charset, 0xe4)
        .add_command_rule::<EreaderNameCommand>(b"\xff\x00")
        .add_command_rule::<EreaderDescriptionCommand>(b"\xff\x01")
        .add_text_rule(b"\xe8", "\n")
        .add_skip_rule(b"\xe6", 1)
        .add_skip_rule(b"\xe7\x01", 0)
        .add_skip_rule(b"\xe7\x02", 0)
        .add_skip_rule(b"\xe7\x03", 0)
        .add_skip_rule(b"\xed\x00", 2)
        .add_skip_rule(b"\xf0\x00", 1)
        .add_skip_rule(b"\xfc\x06", 0)
        .build()
}
