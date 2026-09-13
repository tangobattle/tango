use std::collections::BTreeMap;
use std::sync::Mutex;

use iced::widget::image;
use tango_script::ui::Raster;

#[derive(Default)]
pub(crate) struct Images {
    cache: Mutex<Cache>,
}

#[derive(Default)]
struct Cache {
    handles: BTreeMap<[u8; 32], image::Handle>,
    bytes: usize,
}

impl Images {
    pub(crate) fn get(&self, raster: &Raster) -> image::Handle {
        let mut cache = self.cache.lock().unwrap();
        if let Some(handle) = cache.handles.get(&raster.digest) {
            return handle.clone();
        }
        // Bound resources retained across document updates. One validated tree
        // fits this budget; older, unused images can always be re-uploaded.
        if cache.bytes + raster.rgba.len() > 64 * 1024 * 1024 || cache.handles.len() >= 16384 {
            *cache = Cache::default();
        }
        let handle = image::Handle::from_rgba(
            raster.width,
            raster.height,
            iced::advanced::graphics::core::Bytes::from_owner(raster.rgba.clone()),
        );
        cache.bytes += raster.rgba.len();
        cache.handles.insert(raster.digest, handle.clone());
        handle
    }

    pub(crate) fn cached(&self, key: &[u8; 32]) -> Option<image::Handle> {
        self.cache.lock().unwrap().handles.get(key).cloned()
    }

    pub(crate) fn insert(&self, key: [u8; 32], width: u32, height: u32, rgba: Vec<u8>) -> image::Handle {
        let bytes = rgba.len();
        let handle = image::Handle::from_rgba(width, height, rgba);
        let mut cache = self.cache.lock().unwrap();
        if cache.bytes + bytes > 64 * 1024 * 1024 || cache.handles.len() >= 16384 {
            *cache = Cache::default();
        }
        cache.bytes += bytes;
        cache.handles.insert(key, handle.clone());
        handle
    }
}
