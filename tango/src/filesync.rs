use sha2::Digest;
use tokio::io::AsyncReadExt;

pub type Entries = std::collections::HashMap<std::ffi::OsString, Entry>;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum Entry {
    Directory(Entries),
    File(#[serde(with = "serde_hex::SerHex::<serde_hex::StrictPfx>")] [u8; 32]),
}

#[async_recursion::async_recursion]
async fn sync_entry<F>(
    root: &std::path::Path,
    path: &std::path::Path,
    entry: &Entry,
    fetch_cb: &(impl Fn(&std::path::Path, &std::path::Path) -> F + Send + Sync),
) -> std::io::Result<()>
where
    F: std::future::Future<Output = std::io::Result<()>> + Send,
{
    let real_path = root.join(path);

    match entry {
        Entry::Directory(entries) => {
            match tokio::fs::metadata(&real_path).await {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    tokio::fs::create_dir(&real_path).await?;
                }
                Err(e) => {
                    return Err(e);
                }
            }

            for (filename, child) in entries.iter() {
                sync_entry(root, &path.join(filename), child, fetch_cb).await?;
            }
        }
        Entry::File(hash) => {
            let needs_fetch = match tokio::fs::metadata(&real_path).await {
                Ok(_) => {
                    let mut f = tokio::fs::File::open(&real_path).await?;
                    let mut hasher = sha2::Sha256::new();
                    let mut buf = [0u8; 8196];
                    loop {
                        let n = f.read(&mut buf).await?;
                        if n == 0 {
                            break;
                        }
                        hasher.update(&buf[..n]);
                    }

                    &hasher.finalize()[..] != &hash[..]
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
                Err(e) => {
                    return Err(e);
                }
            };

            if needs_fetch {
                fetch_cb(root, path).await?;
            }
        }
    }

    Ok(())
}

pub async fn sync<F>(
    root: &std::path::Path,
    entries: &Vec<Entry>,
    fetch_cb: &(impl Fn(&std::path::Path, &std::path::Path) -> F + Send + Sync),
) -> std::io::Result<()>
where
    F: std::future::Future<Output = std::io::Result<()>> + Send,
{
    for entry in entries.iter() {
        sync_entry(root, std::path::Path::new("."), entry, fetch_cb).await?;
    }
    Ok(())
}
