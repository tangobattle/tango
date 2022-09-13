pub trait ReadWrite
where
    Self: std::io::Read + std::io::Write,
{
}

impl<T> ReadWrite for T where T: std::io::Read + std::io::Write {}

#[cfg(unix)]
pub fn open() -> std::io::Result<Box<dyn ReadWrite>> {
    let tmpdir = if let Some(tmpdir) = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"]
        .iter()
        .flat_map(|key| std::env::var_os(key))
        .next()
    {
        std::path::PathBuf::from(tmpdir)
    } else {
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "no temp dir"));
    };

    (0..10)
        .flat_map(|i| std::os::unix::net::UnixStream::connect(&tmpdir.join(format!("discord-ipc-{}", i))).ok())
        .next()
        .map(|s| Box::new(s) as Box<dyn ReadWrite>)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "could not connect"))
}

#[cfg(windows)]
pub fn open() -> std::io::Result<Box<dyn ReadWrite>> {
    use std::os::windows::fs::OpenOptionsExt;
    (0..10)
        .flat_map(|i| {
            std::fs::OpenOptions::new()
                .access_mode(0x3)
                .open(format!(r"\\?\pipe\discord-ipc-{}", i))
                .ok()
        })
        .next()
        .map(|s| Box::new(s) as Box<dyn ReadWrite>)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "could not connect"))
}
