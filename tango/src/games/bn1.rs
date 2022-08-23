mod hooks;
mod munger;
mod offsets;

use crate::games;

pub struct BN1;
impl games::Game for BN1 {
    fn family_name(&self) -> &str {
        "bn1"
    }

    fn version_name(&self) -> Option<&str> {
        None
    }

    fn hooks(&self, revision: u8) -> Option<Box<dyn games::Hooks + Send + Sync + 'static>> {
        if revision == 0 {
            Some(hooks::Hooks::new(offsets::AREE_00))
        } else {
            None
        }
    }
}

pub struct EXE1;
impl games::Game for EXE1 {
    fn family_name(&self) -> &str {
        "exe1"
    }

    fn version_name(&self) -> Option<&str> {
        None
    }

    fn hooks(&self, revision: u8) -> Option<Box<dyn games::Hooks + Send + Sync + 'static>> {
        if revision == 0 {
            Some(hooks::Hooks::new(offsets::AREJ_00))
        } else {
            None
        }
    }
}
