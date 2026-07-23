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

// Ceilings on how much any single archive entry may decompress to. A
// `.tangopatch` is untrusted input — a scanned file could have been
// hand-crafted, and a package fetched from a repo is only hash-checked
// against what that same repo published — so a "zip bomb" (a few KB that
// inflates to gigabytes, or a header that merely *claims* to) must not be
// able to exhaust memory when a package is opened or read. These bounds
// sit far above any real payload: a GBA save is at most 128 KiB, a
// manifest a handful of lines, a BPS patch is bounded by the ROM it
// patches (largest supported ROM is 32 MiB).
const MAX_MANIFEST: u64 = 256 * 1024;
const MAX_README: u64 = 4 * 1024 * 1024;
const MAX_SAVE: u64 = 1024 * 1024;
const MAX_BPS: u64 = 64 * 1024 * 1024;

/// Decompress an archive entry into memory, refusing to buffer more than
/// `limit` bytes even if the entry claims — or delivers — more.
///
/// Both the up-front reservation and the actual read are bounded, so
/// neither a lying central-directory `uncompressed_size` nor a genuine
/// decompression bomb can force a large allocation. `declared` is the
/// entry's header size, trusted only to *shrink* the initial reservation.
fn read_capped(entry: impl Read, declared: u64, limit: u64, what: &str) -> Result<Vec<u8>, Error> {
    let mut buf = Vec::with_capacity(declared.min(limit) as usize);
    // `take(limit + 1)` reads at most one byte past the limit, so an
    // over-limit entry is detected without ever buffering it whole.
    entry.take(limit + 1).read_to_end(&mut buf)?;
    if buf.len() as u64 > limit {
        return Err(Error::Invalid(format!(
            "{what} is larger than the {limit}-byte limit (possible zip bomb)"
        )));
    }
    Ok(buf)
}

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
            let entry = archive
                .by_name(MANIFEST_PATH)
                .map_err(|_| Error::Invalid(format!("no {MANIFEST_PATH}")))?;
            let declared = entry.size();
            let raw = read_capped(entry, declared, MAX_MANIFEST, MANIFEST_PATH)?;
            let raw = String::from_utf8(raw).map_err(|_| Error::Invalid(format!("{MANIFEST_PATH} is not UTF-8")))?;
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
        self.read_entry(&target.rom_path(), MAX_BPS)
    }

    /// A save template's raw SRAM dump.
    pub fn save_template(&mut self, target: RomTarget, template: &str) -> Result<Vec<u8>, Error> {
        self.read_entry(&target.save_path(template), MAX_SAVE)
    }

    /// The patch's README, as markdown.
    pub fn readme(&mut self) -> Result<Option<String>, Error> {
        if !self.has_readme {
            return Ok(None);
        }
        let raw = self.read_entry(README_PATH, MAX_README)?;
        Ok(Some(String::from_utf8_lossy(&raw).into_owned()))
    }

    fn read_entry(&mut self, path: &str, limit: u64) -> Result<Vec<u8>, Error> {
        let entry = self.archive.by_name(path)?;
        let declared = entry.size();
        read_capped(entry, declared, limit, path)
    }
}
