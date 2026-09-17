//! In-memory storage for portable boundary tests.
use crate::storage::{Entry, ListFuture, Listing, ReadSeek, Storage};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
};
#[derive(Default)]
pub struct Memory(pub Mutex<HashMap<PathBuf, Vec<u8>>>);
impl Storage for Memory {
    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        self.0
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .ok_or(std::io::ErrorKind::NotFound.into())
    }
    fn open(&self, path: &Path) -> std::io::Result<Box<dyn ReadSeek>> {
        Ok(Box::new(std::io::Cursor::new(self.read(path)?)))
    }
    fn write(&self, path: &Path, data: &[u8]) -> std::io::Result<()> {
        self.0.lock().unwrap().insert(path.into(), data.into());
        Ok(())
    }
    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        self.0.lock().unwrap().remove(path);
        Ok(())
    }
    fn create_dir_all(&self, _: &Path) -> std::io::Result<()> {
        Ok(())
    }
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        let mut files = self.0.lock().unwrap();
        let data = files.remove(from).ok_or(std::io::ErrorKind::NotFound)?;
        files.insert(to.into(), data);
        Ok(())
    }
    fn is_file(&self, path: &Path) -> bool {
        self.0.lock().unwrap().contains_key(path)
    }
    fn list<'a>(&'a self, roots: &'a [PathBuf]) -> ListFuture<'a> {
        Box::pin(async move {
            Listing::new(
                self.0
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|(p, _)| roots.iter().any(|root| p.starts_with(root)))
                    .map(|(path, data)| Entry {
                        path: path.clone(),
                        len: data.len() as u64,
                        modified: None,
                    })
                    .collect(),
            )
        })
    }
}
