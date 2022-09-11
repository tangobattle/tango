use sha2::Digest;
use tokio::io::AsyncReadExt;

pub type Entries = std::collections::HashMap<String, Entry>;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum Entry {
    Directory(Entries),
    File(#[serde(with = "serde_hex::SerHex::<serde_hex::StrictPfx>")] [u8; 32]),
}

#[async_recursion::async_recursion]
async fn sync_entry(
    root: &std::path::Path,
    path: &std::path::Path,
    entry: &Entry,
    fetch_cb: &(impl Fn(&std::path::Path) -> futures::future::BoxFuture<std::io::Result<()>> + Send + Sync),
    sem: std::sync::Arc<tokio::sync::Semaphore>,
) -> std::io::Result<()> {
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

            futures::future::join_all(entries.iter().map(|(filename, child)| {
                let filename = filename.clone();
                let sem = sem.clone();
                async { Ok::<_, std::io::Error>(sync_entry(root, &path.join(filename), child, fetch_cb, sem).await?) }
            }))
            .await
            .into_iter()
            .collect::<Result<_, _>>()?;
        }
        Entry::File(hash) => {
            let _permit = sem
                .acquire()
                .await
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
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
                fetch_cb(path).await?;
            }
        }
    }

    Ok(())
}

pub async fn sync(
    root: &std::path::Path,
    entries: &Entries,
    fetch_cb: impl Fn(&std::path::Path) -> futures::future::BoxFuture<std::io::Result<()>> + Send + Sync,
    concurrency_level: usize,
) -> std::io::Result<()> {
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(concurrency_level));

    futures::future::join_all(entries.iter().map(|(filename, child)| {
        let sem = sem.clone();
        let path = std::path::PathBuf::from(filename.clone());
        let fetch_cb = &fetch_cb;
        async move { Ok::<_, std::io::Error>(sync_entry(root, &path, child, fetch_cb, sem).await?) }
    }))
    .await
    .into_iter()
    .collect::<Result<_, _>>()?;
    Ok(())
}
