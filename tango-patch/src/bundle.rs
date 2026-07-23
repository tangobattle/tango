//! Writing a `.tangopatch` (feature `bundle`).
//!
//! [`Builder`] assembles a package in memory; [`read_dir`] fills one from
//! a source tree laid out exactly like the finished package, which is
//! what `tango-patch pack` does:
//!
//! ```text
//! bn6_allstars-1.1.0/
//! ├── manifest.toml
//! ├── README.md
//! ├── roms/BR5E_00.bps
//! ├── roms/BR6E_00.bps
//! └── saves/BR6E_00.sav
//! ```
//!
//! Output is byte-for-byte deterministic — fixed entry order, fixed
//! timestamps — because the patch repo commits these files, and a rebuild
//! that produced different bytes would show up as a spurious diff every
//! time.

use crate::layout::{RomTarget, DEFAULT_TEMPLATE, MANIFEST_PATH, README_PATH, ROMS_DIR, SAVES_DIR};
use crate::manifest::Manifest;
use crate::Error;
use std::collections::BTreeMap;
use std::io::{Seek, Write};
use std::path::Path;

pub struct Builder {
    manifest: Manifest,
    readme: Option<String>,
    roms: BTreeMap<RomTarget, Vec<u8>>,
    /// Keyed by `(target, template name)`; the default template's name is
    /// [`DEFAULT_TEMPLATE`], which sorts first.
    saves: BTreeMap<(RomTarget, String), Vec<u8>>,
}

/// What [`Builder::write_file`] produced — everything an index needs
/// about a freshly built package without re-opening it.
#[derive(Debug, Clone)]
pub struct Built {
    pub path: std::path::PathBuf,
    pub size: u64,
    pub sha256: String,
}

impl Builder {
    pub fn new(manifest: Manifest) -> Self {
        Builder {
            manifest,
            readme: None,
            roms: BTreeMap::new(),
            saves: BTreeMap::new(),
        }
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn set_readme(&mut self, readme: impl Into<String>) -> &mut Self {
        self.readme = Some(readme.into());
        self
    }

    pub fn add_rom(&mut self, target: RomTarget, bps: Vec<u8>) -> &mut Self {
        self.roms.insert(target, bps);
        self
    }

    pub fn add_save_template(&mut self, target: RomTarget, template: &str, save: Vec<u8>) -> Result<&mut Self, Error> {
        if template != DEFAULT_TEMPLATE {
            crate::validate_name(template).map_err(|e| Error::Invalid(format!("save template: {e}")))?;
        }
        self.saves.insert((target, template.to_owned()), save);
        Ok(self)
    }

    pub fn targets(&self) -> impl ExactSizeIterator<Item = RomTarget> + '_ {
        self.roms.keys().copied()
    }

    /// Everything that must hold before a package is worth shipping.
    /// Called by [`Self::write`], so a written package is a valid one.
    pub fn validate(&self) -> Result<(), Error> {
        if self.roms.is_empty() {
            return Err(Error::Invalid(format!(
                "no {ROMS_DIR}/*.bps — a patch must patch at least one rom"
            )));
        }
        // A template whose rom isn't in the package would be unreachable:
        // targets come from the .bps entries.
        for (target, template) in self.saves.keys() {
            if !self.roms.contains_key(target) {
                return Err(Error::Invalid(format!(
                    "save template {} has no matching {}",
                    target.save_path(template),
                    target.rom_path()
                )));
            }
        }
        Ok(())
    }

    /// Serialize into `writer`. Entries are emitted in a fixed order with
    /// fixed timestamps, so the same inputs always yield the same bytes.
    pub fn write<W: Write + Seek>(&self, writer: W) -> Result<(), Error> {
        self.validate()?;

        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(zip::DateTime::default())
            .unix_permissions(0o644);
        let mut zip = zip::ZipWriter::new(writer);

        // Manifest first so a reader hits it in the first block.
        zip.start_file(MANIFEST_PATH, options)?;
        zip.write_all(self.manifest.to_toml()?.as_bytes())?;

        if let Some(readme) = &self.readme {
            zip.start_file(README_PATH, options)?;
            zip.write_all(readme.as_bytes())?;
        }
        for (target, bps) in &self.roms {
            zip.start_file(target.rom_path(), options)?;
            zip.write_all(bps)?;
        }
        for ((target, template), save) in &self.saves {
            zip.start_file(target.save_path(template), options)?;
            zip.write_all(save)?;
        }

        zip.finish()?;
        Ok(())
    }

    pub fn to_vec(&self) -> Result<Vec<u8>, Error> {
        let mut buf = std::io::Cursor::new(Vec::new());
        self.write(&mut buf)?;
        Ok(buf.into_inner())
    }

    /// Write `<dir>/<name>-<version>.tangopatch`, creating `dir` if
    /// needed. The file name is fixed by the manifest so a package's URL
    /// is derivable from its identity.
    pub fn write_file(&self, dir: &Path) -> Result<Built, Error> {
        let raw = self.to_vec()?;
        std::fs::create_dir_all(dir)?;
        let path = dir.join(self.manifest.file_name());
        std::fs::write(&path, &raw).map_err(|e| Error::from(e).at(&path))?;
        Ok(Built {
            path,
            size: raw.len() as u64,
            sha256: crate::sha256_hex(&raw),
        })
    }
}

/// Read a source tree into a [`Builder`]. The layout is the package's own
/// (see the module docs); a plain `README` is accepted alongside
/// `README.md`, since that's what patch authors have been writing.
pub fn read_dir(src: &Path) -> Result<Builder, Error> {
    let manifest_path = src.join(MANIFEST_PATH);
    let raw = std::fs::read_to_string(&manifest_path).map_err(|e| Error::from(e).at(&manifest_path))?;
    let manifest = Manifest::parse(&raw).map_err(|e| e.at(&manifest_path))?;
    let mut builder = Builder::new(manifest);

    for candidate in [README_PATH, "README"] {
        let path = src.join(candidate);
        if path.is_file() {
            builder.set_readme(String::from_utf8_lossy(&std::fs::read(&path)?).into_owned());
            break;
        }
    }

    for (path, name) in list_dir(&src.join(ROMS_DIR))? {
        let Some(stem) = name.strip_suffix(".bps") else {
            return Err(Error::Invalid(format!("{ROMS_DIR}/{name}: not a .bps")));
        };
        let target: RomTarget = stem.parse().map_err(|e: Error| e.at(&path))?;
        builder.add_rom(target, std::fs::read(&path)?);
    }

    for (path, name) in list_dir(&src.join(SAVES_DIR))? {
        let Some(stem) = name.strip_suffix(".sav") else {
            return Err(Error::Invalid(format!("{SAVES_DIR}/{name}: not a .sav")));
        };
        let (target, template) = crate::layout::split_template_stem(stem).map_err(|e| e.at(&path))?;
        builder
            .add_save_template(target, &template, std::fs::read(&path)?)
            .map_err(|e| e.at(&path))?;
    }

    builder.validate().map_err(|e| e.at(src))?;
    Ok(builder)
}

/// Files directly inside `dir`, sorted; an absent directory is empty.
/// Subdirectories and dotfiles are skipped rather than silently packed.
fn list_dir(dir: &Path) -> Result<Vec<(std::path::PathBuf, String)>, Error> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(Error::from(e).at(dir)),
    };
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || !entry.path().is_file() {
            continue;
        }
        out.push((entry.path(), name));
    }
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn builder() -> Builder {
        Builder::new(
            Manifest::parse(
                r#"
format = 1
name = "test_patch"
version = "1.2.3"
title = "Test Patch"
"#,
            )
            .unwrap(),
        )
    }

    fn target(s: &str) -> RomTarget {
        s.parse().unwrap()
    }

    #[test]
    fn output_is_deterministic() {
        let build = || {
            let mut b = builder();
            b.set_readme("# hello");
            b.add_rom(target("BR6E_00"), b"bps".to_vec());
            b.to_vec().unwrap()
        };
        assert_eq!(build(), build());
    }

    #[test]
    fn a_patch_with_no_roms_is_rejected() {
        let mut b = builder();
        b.set_readme("# nothing here");
        assert!(b.to_vec().is_err());
    }

    #[test]
    fn a_dangling_save_template_is_rejected() {
        let mut b = builder();
        b.add_rom(target("BR6E_00"), b"bps".to_vec());
        b.add_save_template(target("BR5E_00"), DEFAULT_TEMPLATE, b"save".to_vec())
            .unwrap();
        let err = b.to_vec().unwrap_err().to_string();
        assert!(err.contains("BR5E_00"), "{err}");
    }

    #[test]
    fn a_bad_template_name_is_rejected() {
        assert!(builder()
            .add_save_template(target("BR6E_00"), "two words", b"save".to_vec())
            .is_err());
    }
}
