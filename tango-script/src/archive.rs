//! Deterministic packages contain the complete immutable file tree.
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Write};

use crate::{invalid, Package, Result, MAX_PACKAGE_BYTES, MAX_PACKAGE_FILES};

impl Package {
    pub fn from_archive(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_PACKAGE_BYTES {
            return Err(invalid("package archive exceeds 64 MiB"));
        }
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
        if archive.len() > MAX_PACKAGE_FILES {
            return Err(invalid("too many package archive entries"));
        }
        let mut names = BTreeSet::new();
        let mut directories = BTreeSet::new();
        let mut files = BTreeMap::new();
        let mut remaining = MAX_PACKAGE_BYTES;
        for i in 0..archive.len() {
            let entry = archive.by_index(i)?;
            let is_directory = entry.is_dir();
            let name = if is_directory {
                entry.name().strip_suffix('/').expect("directory name")
            } else {
                entry.name()
            }
            .to_owned();
            if !crate::package::valid_path(&name) || !names.insert(name.clone()) {
                return Err(invalid("invalid or duplicate archive entry"));
            }
            if let Some(mode) = entry.unix_mode().map(|mode| mode & 0o170000) {
                if mode != 0 && mode != (if is_directory { 0o040000 } else { 0o100000 }) {
                    return Err(invalid(
                        "package archives can contain only regular files and directories",
                    ));
                }
            }
            if is_directory {
                if entry.size() != 0 {
                    return Err(invalid("package directory contains data"));
                }
                directories.insert(name);
                continue;
            }
            let limit = remaining.min(if name == "package.toml" {
                64 * 1024
            } else {
                MAX_PACKAGE_BYTES
            });
            if entry.size() > limit as u64 {
                return Err(invalid("expanded package exceeds byte limit"));
            }
            let mut bytes = Vec::new();
            entry.take(limit as u64 + 1).read_to_end(&mut bytes)?;
            if bytes.len() > limit {
                return Err(invalid("expanded package exceeds byte limit"));
            }
            remaining -= bytes.len();
            files.insert(name, bytes);
        }
        for directory in directories {
            for (slash, _) in directory.match_indices('/') {
                if files.contains_key(&directory[..slash]) {
                    return Err(invalid("package path is both a file and a directory"));
                }
            }
        }
        Self::load(files)
    }

    pub fn to_archive(&self) -> Result<Vec<u8>> {
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(zip::DateTime::default());
        for (path, bytes) in self.files.iter() {
            archive.start_file(path, options)?;
            archive.write_all(bytes)?;
        }
        let bytes = archive.finish()?.into_inner();
        if bytes.len() > MAX_PACKAGE_BYTES {
            return Err(invalid("package archive exceeds 64 MiB"));
        }
        Ok(bytes)
    }
}
