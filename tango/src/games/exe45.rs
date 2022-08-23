mod hooks;

use crate::games;

#[derive(Clone)]
pub struct EXE45;
impl games::Game for EXE45 {
    fn family(&self) -> &str {
        "exe45"
    }

    fn language(&self) -> unic_langid::LanguageIdentifier {
        unic_langid::langid!("ja-JP")
    }

    fn variant(&self) -> u32 {
        0
    }

    fn expected_crc32(&self) -> u32 {
        0xa646601b
    }

    fn hooks(&self) -> Box<dyn games::Hooks + Send + Sync + 'static> {
        Box::new(hooks::BR4J_00.clone())
    }
}
