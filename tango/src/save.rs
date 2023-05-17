use byteorder::ByteOrder;

use crate::{game, scanner};

#[derive(Clone)]
pub struct ScannedSave {
    pub path: std::path::PathBuf,
    pub save: Box<dyn Save + Send + Sync>,
}

pub fn scan_saves(
    path: &std::path::Path,
) -> std::collections::HashMap<&'static (dyn game::Game + Send + Sync), Vec<ScannedSave>> {
    let mut paths = std::collections::HashMap::new();

    for entry in walkdir::WalkDir::new(path) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                log::error!("failed to read entry: {:?}", e);
                continue;
            }
        };

        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        let buf = match std::fs::read(path) {
            Ok(buf) => buf,
            Err(e) => {
                log::warn!("{}: {}", path.display(), e);
                continue;
            }
        };

        let mut ok = false;
        let mut errors = vec![];
        for game in game::GAMES.iter() {
            match game.parse_save(&buf) {
                Ok(save) => {
                    log::info!("{}: {:?}", path.display(), game.family_and_variant());
                    let saves = paths.entry(*game).or_insert_with(|| vec![]);
                    saves.push(ScannedSave {
                        path: path.to_path_buf(),
                        save,
                    });
                    ok = true;
                }
                Err(e) => {
                    errors.push((*game, e));
                }
            }
        }

        if !ok {
            log::warn!(
                "{}:\n{}",
                path.display(),
                errors
                    .iter()
                    .map(|(k, v)| format!("{:?}: {}", k.family_and_variant(), v))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }
    }

    for (_, saves) in paths.iter_mut() {
        saves.sort_by_key(|s| {
            let components = s
                .path
                .components()
                .map(|c| c.as_os_str().to_os_string())
                .collect::<Vec<_>>();
            (-(components.len() as isize), components)
        });
    }

    paths
}

pub trait SaveClone {
    fn clone_box(&self) -> Box<dyn Save + Sync + Send>;
}

impl<T> SaveClone for T
where
    T: 'static + Save + Sync + Send + Clone,
{
    fn clone_box(&self) -> Box<dyn Save + Sync + Send> {
        Box::new(self.clone())
    }
}

pub enum PatchCardsView<'a> {
    PatchCard4s(Box<dyn PatchCard4sView<'a> + 'a>),
    PatchCard56s(Box<dyn PatchCard56sView<'a> + 'a>),
}

pub enum PatchCardsViewMut<'a> {
    PatchCard4s(Box<dyn PatchCard4sViewMut<'a> + 'a>),
    PatchCard56s(Box<dyn PatchCard56sViewMut<'a> + 'a>),
}

pub trait Save
where
    Self: SaveClone,
{
    fn to_vec(&self) -> Vec<u8>;
    fn as_raw_wram(&self) -> &[u8];

    fn view_chips(&self) -> Option<Box<dyn ChipsView + '_>> {
        None
    }

    fn view_chips_mut(&mut self) -> Option<Box<dyn ChipsViewMut + '_>> {
        None
    }

    fn view_patch_cards(&self) -> Option<PatchCardsView> {
        None
    }

    fn view_patch_cards_mut(&mut self) -> Option<PatchCardsViewMut> {
        None
    }

    fn view_navicust(&self) -> Option<Box<dyn NavicustView + '_>> {
        None
    }

    fn view_navicust_mut(&mut self) -> Option<Box<dyn NavicustViewMut + '_>> {
        None
    }

    fn view_auto_battle_data(&self) -> Option<Box<dyn AutoBattleDataView + '_>> {
        None
    }

    fn view_navi(&self) -> Option<Box<dyn NaviView + '_>> {
        None
    }
}

impl Clone for Box<dyn Save + Send + Sync> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

pub fn mask_save(buf: &mut [u8], mask_offset: usize) {
    let mask = byteorder::LittleEndian::read_u32(&buf[mask_offset..mask_offset + 4]);
    for b in buf.iter_mut() {
        *b = *b ^ (mask as u8);
    }
    byteorder::LittleEndian::write_u32(&mut buf[mask_offset..mask_offset + 4], mask);
}

pub fn compute_save_raw_checksum(buf: &[u8], checksum_offset: usize) -> u32 {
    buf.iter().map(|v| *v as u32).sum::<u32>()
        - buf[checksum_offset..checksum_offset + 4]
            .iter()
            .map(|v| *v as u32)
            .sum::<u32>()
}

#[derive(Clone, Debug, std::hash::Hash, Eq, PartialEq)]
pub struct Chip {
    pub id: usize,
    pub code: char,
}

pub trait ChipsView<'a> {
    fn num_folders(&self) -> usize;
    fn equipped_folder_index(&self) -> usize;
    fn regular_chip_is_in_place(&self) -> bool;
    fn chips_have_mb(&self) -> bool {
        true
    }
    fn regular_chip_index(&self, folder_index: usize) -> Option<usize>;
    fn tag_chip_indexes(&self, folder_index: usize) -> Option<[usize; 2]>;
    fn chip(&self, folder_index: usize, chip_index: usize) -> Option<Chip>;
}

pub trait ChipsViewMut<'a> {
    fn set_equipped_folder(&mut self, folder_index: usize) -> bool {
        let _ = folder_index;
        false
    }
    fn set_chip(&mut self, folder_index: usize, chip_index: usize, chip: Chip) -> bool;
    fn set_tag_chip_indexes(&mut self, folder_index: usize, chip_indexes: Option<[usize; 2]>) -> bool {
        let _ = folder_index;
        let _ = chip_indexes;
        false
    }
    fn set_regular_chip_index(&mut self, folder_index: usize, chip_index: usize) -> bool {
        let _ = folder_index;
        let _ = chip_index;
        false
    }
    fn can_set_regular_chip(&self) -> bool {
        false
    }
    fn can_set_tag_chips(&self) -> bool {
        false
    }
}

#[derive(Clone, Debug, std::hash::Hash, Eq, PartialEq)]
pub struct PatchCard {
    pub id: usize,
    pub enabled: bool,
}

pub trait PatchCard56sView<'a> {
    fn count(&self) -> usize;
    fn patch_card(&self, slot: usize) -> Option<PatchCard>;
}

pub trait PatchCard56sViewMut<'a> {
    fn set_count(&mut self, count: usize);
    fn set_patch_card(&mut self, slot: usize, patch_card: PatchCard) -> bool;
}

pub trait PatchCard4sView<'a> {
    fn patch_card(&self, slot: usize) -> Option<PatchCard>;
}

pub trait PatchCard4sViewMut<'a> {
    fn set_patch_card(&mut self, slot: usize, patch_card: Option<PatchCard>);
}

pub trait NaviView<'a> {
    fn navi(&self) -> usize;
}

#[derive(Clone, Debug, std::hash::Hash, Eq, PartialEq)]
pub struct NavicustPart {
    pub id: usize,
    pub variant: usize,
    pub col: u8,
    pub row: u8,
    pub rot: u8,
    pub compressed: bool,
}

pub trait NavicustView<'a> {
    fn count(&self) -> usize {
        25
    }
    fn style(&self) -> Option<usize> {
        None
    }
    fn num_styles(&self) -> usize {
        0
    }
    fn width(&self) -> usize;
    fn height(&self) -> usize;
    fn command_line(&self) -> usize;
    fn has_out_of_bounds(&self) -> bool;
    fn navicust_part(&self, i: usize) -> Option<NavicustPart>;
}

pub trait NavicustViewMut<'a> {
    fn can_set_style(&self) -> bool {
        false
    }

    fn set_style(&self, _style: usize) -> bool {
        false
    }

    fn set_navicust_part(&self, _i: usize, _part: NavicustPart) -> bool;
}

pub trait AutoBattleDataView<'a> {
    fn chip_use_count(&self, id: usize) -> Option<u16>;
    fn secondary_chip_use_count(&self, id: usize) -> Option<u16>;
}

pub type Scanner =
    scanner::Scanner<std::collections::HashMap<&'static (dyn game::Game + Send + Sync), Vec<ScannedSave>>>;
