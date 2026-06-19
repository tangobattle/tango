use crate::hooks::Trap;

pub(super) fn traps(hooks: &super::Hooks) -> Vec<Trap> {
    crate::game::shared::start_screen_common_traps!(hooks)
}
