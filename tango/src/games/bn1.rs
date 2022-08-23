mod hooks;
mod munger;
mod offsets;

use crate::games;

pub struct BN1;
impl games::Game for BN1 {
    fn family_name(&self) -> &str {
        "bn1"
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("en-US")
    }

    fn version_name(&self) -> Option<&str> {
        None
    }

    fn expected_crc32(&self) -> u32 {
        0x1d347971
    }

    fn hooks(&self) -> Box<dyn games::Hooks + Send + Sync + 'static> {
        Box::new(hooks::Hooks {
            offsets: &offsets::AREE_00,
        })
    }
}

pub struct EXE1;
impl games::Game for EXE1 {
    fn family_name(&self) -> &str {
        "exe1"
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn version_name(&self) -> Option<&str> {
        None
    }

    fn expected_crc32(&self) -> u32 {
        0xd9516e50
    }

    fn hooks(&self) -> Box<dyn games::Hooks + Send + Sync + 'static> {
        Box::new(hooks::Hooks {
            offsets: &offsets::AREJ_00,
        })
    }
}
