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

    fn hooks(&self) -> Box<dyn games::Hooks + Send + Sync + 'static> {
        Box::new(hooks::AREE_00.clone())
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

    fn hooks(&self) -> Box<dyn games::Hooks + Send + Sync + 'static> {
        Box::new(hooks::AREJ_00.clone())
    }
}
