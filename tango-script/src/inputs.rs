use std::collections::BTreeMap;
use std::sync::Arc;

use sha3::{Digest, Sha3_256};

use crate::{invalid, Result};

pub const MAX_INPUT_BYTES: usize = 512 * 1024 * 1024;
pub const MAX_INPUTS: usize = 16;

/// Immutable, explicitly supplied data, separate from the package's files and
/// mutable document. Slot names are capabilities, never filesystem paths.
#[derive(Clone, Default)]
pub struct Inputs {
    files: Arc<BTreeMap<String, Arc<[u8]>>>,
}

impl Inputs {
    pub fn new(files: BTreeMap<String, Vec<u8>>) -> Result<Self> {
        if files.len() > MAX_INPUTS {
            return Err(invalid("profile exceeds 16 input buffers"));
        }
        let mut total = 0usize;
        for (name, bytes) in &files {
            if !crate::package::valid_name(name) {
                return Err(invalid("input names must be single names, not paths"));
            }
            total = total
                .checked_add(bytes.len())
                .ok_or_else(|| invalid("input buffers too large"))?;
            if total > MAX_INPUT_BYTES {
                return Err(invalid("input buffers exceed 512 MiB"));
            }
        }
        Ok(Self {
            files: Arc::new(files.into_iter().map(|(name, bytes)| (name, bytes.into())).collect()),
        })
    }

    pub(crate) fn get(&self, name: &str) -> Option<&[u8]> {
        self.files.get(name).map(AsRef::as_ref)
    }

    pub(crate) fn digest(&self) -> [u8; 32] {
        let mut hash = Sha3_256::new();
        hash.update(b"tango-inputs-v1");
        for (name, bytes) in self.files.iter() {
            hash.update((name.len() as u64).to_le_bytes());
            hash.update(name.as_bytes());
            hash.update((bytes.len() as u64).to_le_bytes());
            hash.update(bytes);
        }
        hash.finalize().into()
    }
}
