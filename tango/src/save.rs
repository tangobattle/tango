use byteorder::ByteOrder;

use crate::game;

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

pub enum ModcardsView<'a> {
    Modcards4(Box<dyn Modcards4View<'a> + 'a>),
    Modcards56(Box<dyn Modcards56View<'a> + 'a>),
}

pub enum NaviView<'a> {
    Navi4dot556(Box<dyn Navi4dot556View<'a> + 'a>),
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

    fn view_modcards(&self) -> Option<ModcardsView> {
        None
    }

    fn view_navicust(&self) -> Option<Box<dyn NavicustView + '_>> {
        None
    }

    fn view_dark_ai(&self) -> Option<Box<dyn DarkAIView + '_>> {
        None
    }

    fn view_navi(&self) -> Option<NaviView> {
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
    buf.iter()
        .enumerate()
        .map(|(i, b)| {
            if i < checksum_offset || i >= checksum_offset + 4 {
                *b as u32
            } else {
                0
            }
        })
        .sum::<u32>()
}

#[derive(Clone, Debug, std::hash::Hash, Eq, PartialEq)]
pub struct Chip {
    pub id: usize,
    pub code: usize,
}

pub trait ChipsView<'a> {
    fn chip_codes(&self) -> &'static [u8];
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

#[derive(Clone, Debug, std::hash::Hash, Eq, PartialEq)]
pub struct Modcard {
    pub id: usize,
    pub enabled: bool,
}

pub trait Modcards56View<'a> {
    fn count(&self) -> usize;
    fn modcard(&self, slot: usize) -> Option<Modcard>;
}

pub trait Modcards4View<'a> {
    fn modcard(&self, slot: usize) -> Option<Modcard>;
}

pub trait Navi4dot556View<'a> {
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
    fn width(&self) -> usize;
    fn height(&self) -> usize;
    fn command_line(&self) -> usize;
    fn has_out_of_bounds(&self) -> bool;
    fn navicust_part(&self, i: usize) -> Option<NavicustPart>;
}

pub trait DarkAIView<'a> {
    fn chip_use_count(&self, id: usize) -> Option<u16>;
    fn secondary_chip_use_count(&self, id: usize) -> Option<u16>;
}
