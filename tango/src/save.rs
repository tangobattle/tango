use byteorder::ByteOrder;

use crate::game;

pub fn scan_saves(
    path: &std::path::Path,
) -> std::collections::HashMap<&'static (dyn game::Game + Send + Sync), Vec<std::path::PathBuf>> {
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
                Ok(_) => {
                    log::info!("{}: {:?}", path.display(), game.family_and_variant());
                    let save_paths = paths.entry(*game).or_insert_with(|| vec![]);
                    save_paths.push(path.to_path_buf());
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
        saves.sort_by_key(|path| {
            let components = path
                .components()
                .map(|c| c.as_os_str().to_os_string())
                .collect::<Vec<_>>();
            (components.len(), components)
        });
    }

    paths
}

pub trait Save {}

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
