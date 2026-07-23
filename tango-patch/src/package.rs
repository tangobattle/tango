//! Reading a `.tangopatch` (feature `package`).
//!
//! What a package *contains* is never restated in its manifest: the
//! supported ROMs and save templates are read off the archive itself (see
//! [`crate::layout`]), so metadata and payload can't disagree.

use crate::layout::{
    split_template_stem, strip_dir_ext, RomTarget, BPS_EXT, MANIFEST_PATH, README_PATH, ROMS_DIR, SAVES_DIR, SAVE_EXT,
};
use crate::manifest::Manifest;
use crate::Error;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Seek};

/// An opened package. Cheap to construct: only the zip's central
/// directory and the manifest are read up front, so a scanner can open
/// every installed package without touching the patch payloads.
pub struct Package<R> {
    archive: zip::ZipArchive<R>,
    manifest: Manifest,
    contents: BTreeMap<RomTarget, BTreeSet<String>>,
    has_readme: bool,
}

impl Package<std::io::BufReader<std::fs::File>> {
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self, Error> {
        let path = path.as_ref();
        Package::read(std::io::BufReader::new(std::fs::File::open(path)?))
            // Which package failed matters more than which zip offset did.
            .map_err(|e| e.at(path))
    }
}

impl<R: Read + Seek> Package<R> {
    pub fn read(reader: R) -> Result<Self, Error> {
        let mut archive = zip::ZipArchive::new(reader)?;

        let manifest = {
            let mut entry = archive
                .by_name(MANIFEST_PATH)
                .map_err(|_| Error::Invalid(format!("no {MANIFEST_PATH}")))?;
            let mut raw = String::new();
            entry.read_to_string(&mut raw)?;
            Manifest::parse(&raw)?
        };

        // Payloads first, then templates — a template is only reachable
        // through the target its .bps establishes.
        let names: Vec<String> = archive.file_names().map(|s| s.to_owned()).collect();
        let mut contents: BTreeMap<RomTarget, BTreeSet<String>> = BTreeMap::new();
        for name in &names {
            if let Some(stem) = strip_dir_ext(name, ROMS_DIR, BPS_EXT) {
                contents.entry(stem.parse()?).or_default();
            }
        }
        for name in &names {
            let Some(stem) = strip_dir_ext(name, SAVES_DIR, SAVE_EXT) else {
                continue;
            };
            let (target, template) = split_template_stem(stem)?;
            // A template with no matching .bps is unreachable, not fatal:
            // what a session needs is the patch. `bundle` rejects it, so
            // it can't ship in the first place.
            if let Some(templates) = contents.get_mut(&target) {
                templates.insert(template);
            }
        }
        if contents.is_empty() {
            return Err(Error::Invalid(format!("no {ROMS_DIR}/*{BPS_EXT} entries")));
        }

        Ok(Package {
            manifest,
            contents,
            has_readme: names.iter().any(|n| n == README_PATH),
            archive,
        })
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Every ROM build this package patches, in a stable order.
    pub fn targets(&self) -> impl ExactSizeIterator<Item = RomTarget> + '_ {
        self.contents.keys().copied()
    }

    pub fn supports(&self, target: RomTarget) -> bool {
        self.contents.contains_key(&target)
    }

    /// Save template names for `target`. [`crate::layout::DEFAULT_TEMPLATE`]
    /// (the empty string) is the unnamed one. Empty if the package ships
    /// none.
    pub fn save_templates(&self, target: RomTarget) -> impl ExactSizeIterator<Item = &str> {
        static NONE: BTreeSet<String> = BTreeSet::new();
        self.contents.get(&target).unwrap_or(&NONE).iter().map(|s| s.as_str())
    }

    /// The BPS patch for `target`.
    pub fn bps(&mut self, target: RomTarget) -> Result<Vec<u8>, Error> {
        if !self.supports(target) {
            return Err(Error::NoSuchTarget(target));
        }
        self.read_entry(&target.rom_path())
    }

    /// A save template's raw SRAM dump.
    pub fn save_template(&mut self, target: RomTarget, template: &str) -> Result<Vec<u8>, Error> {
        self.read_entry(&target.save_path(template))
    }

    /// The patch's README, as markdown.
    pub fn readme(&mut self) -> Result<Option<String>, Error> {
        if !self.has_readme {
            return Ok(None);
        }
        let raw = self.read_entry(README_PATH)?;
        Ok(Some(String::from_utf8_lossy(&raw).into_owned()))
    }

    fn read_entry(&mut self, path: &str) -> Result<Vec<u8>, Error> {
        let mut entry = self.archive.by_name(path)?;
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf)?;
        Ok(buf)
    }
}
