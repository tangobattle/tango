use super::*;
use crate::http::ReqwestHttp;
use crate::storage::StdStorage;
use tango_patch::{bundle::Builder, Compatibility};

/// The scans and fetches under test take the seam traits; natively
/// those are just the filesystem and reqwest.
const FS: &StdStorage = &StdStorage;

fn net() -> ReqwestHttp {
    ReqwestHttp::new()
}

/// Enumerate then scan, the way the frontend's rescan does.
async fn scanned(root: &Path) -> Catalog {
    let listing = FS.list(&scan_roots(root)).await;
    scan(FS, root, &listing).unwrap()
}

/// A scratch data directory that cleans up on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("tango-patch-scan-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn manifest(name: &str, version: &str, netplay: &str) -> tango_patch::Manifest {
    tango_patch::Manifest::parse(&format!(
        r#"
format = 2
name = "{name}"
version = "{version}"
title = "Test {name}"
authors = ["Someone <someone@example.com>"]
netplay = "{netplay}"

[rom_overrides.BR6E_00]
legal_chip_ranges = [[1, 17], [19, 20]]
"#
    ))
    .unwrap()
}

/// Install a package patching BR6E_00 (BN6 Falzar), which every
/// gamesupport-enabled build knows.
fn install(root: &Path, name: &str, version: &str, netplay: &str) {
    let mut builder = Builder::new(manifest(name, version, netplay));
    builder.set_readme("# hello");
    builder.add_rom("BR6E_00".parse().unwrap(), b"not really a bps".to_vec());
    builder.write_file(root).unwrap();
}

/// An index offering `name` at `version` without it being installed.
fn index_with(root: &Path, entries: &[(&str, &str, &str)]) {
    let mut index = tango_patch::Index::default();
    for (name, version, netplay) in entries {
        index.patches.entry((*name).to_owned()).or_default().insert(
            version.parse().unwrap(),
            tango_patch::index::Entry {
                title: format!("Test {name}"),
                authors: vec!["Someone <someone@example.com>".into()],
                license: None,
                source: None,
                netplay: netplay.parse().unwrap(),
                games: vec!["BR6E_00".parse().unwrap()],
                path: format!("{name}/{name}-{version}.tangopatch"),
                size: 1234,
                sha256: "0".repeat(64),
                readme: None,
            },
        );
    }
    std::fs::write(index_path(root), index.to_json().unwrap()).unwrap();
}

#[cfg(feature = "gamesupport-bn6")]
fn bn6_falzar() -> GameRef {
    crate::game::find_by_rom_info(b"BR6E", 0).expect("gamesupport-bn6 must be enabled for this test")
}

#[cfg(feature = "gamesupport-bn6")]
fn bn6_gregar() -> GameRef {
    crate::game::find_by_rom_info(b"BR5E", 0).expect("gamesupport-bn6 must be enabled for this test")
}

fn v(s: &str) -> semver::Version {
    s.parse().unwrap()
}

#[cfg(feature = "gamesupport-bn6")]
#[tokio::test]
async fn scans_installed_packages() {
    let dir = TempDir::new();
    install(&dir.0, "bn6_test", "1.0.0", "group:testing");

    let catalog = scanned(&dir.0).await;
    assert_eq!(catalog.installed.len(), 1);
    assert_eq!(catalog.title("bn6_test"), Some("Test bn6_test"));
    // mailparse reduces the address to its display name.
    // Kept raw; the display reduction happens at render time so
    // that installing a patch can't change how its authors read.
    assert_eq!(
        catalog.installed["bn6_test"].authors,
        vec!["Someone <someone@example.com>"]
    );
    assert_eq!(display_authors(&catalog.installed["bn6_test"].authors), vec!["Someone"]);

    let version = catalog.version("bn6_test", &v("1.0.0")).unwrap();
    assert_eq!(version.netplay, Compatibility::Group("testing".into()));
    assert_eq!(
        version.rom_overrides[&"BR6E_00".parse().unwrap()]
            .legal_chip_ranges
            .as_deref()
            .unwrap(),
        [[1, 17], [19, 20]]
    );
    assert_eq!(
        version.rom_overrides_for(bn6_falzar()).legal_chip_ranges,
        Some(vec![[1, 17], [19, 20]])
    );
    assert!(version.rom_overrides_for(bn6_gregar()).is_empty());
    assert_eq!(version.readme.as_deref(), Some("# hello"));
    assert!(version.supported_games.contains(&bn6_falzar()));
    assert!(catalog.is_installed("bn6_test", &v("1.0.0")));
}

#[tokio::test]
async fn an_empty_or_absent_patches_dir_is_not_an_error() {
    let dir = TempDir::new();
    assert!(scanned(&dir.0).await.installed.is_empty());
    std::fs::create_dir_all(&dir.0).unwrap();
    assert!(scanned(&dir.0).await.installed.is_empty());
}

#[cfg(feature = "gamesupport-bn6")]
#[tokio::test]
async fn the_catalog_merges_installed_and_offered() {
    let dir = TempDir::new();
    install(&dir.0, "bn6_test", "1.0.0", "group:testing");
    index_with(
        &dir.0,
        &[
            // A newer version of something we have, plus something
            // we don't have at all.
            ("bn6_test", "2.0.0", "group:testing"),
            ("bn6_other", "1.0.0", "vanilla"),
        ],
    );

    let catalog = scanned(&dir.0).await;
    assert_eq!(
        catalog.names().into_iter().collect::<Vec<_>>(),
        vec!["bn6_other", "bn6_test"]
    );

    let versions = catalog.versions("bn6_test");
    assert_eq!(versions.len(), 2);
    assert!(versions[&v("1.0.0")].is_installed());
    assert!(!versions[&v("2.0.0")].is_installed());
    assert_eq!(versions[&v("2.0.0")].size(), Some(1234));

    // Not installed, but still browsable and resolvable.
    assert_eq!(catalog.title("bn6_other"), Some("Test bn6_other"));
    assert!(catalog
        .supported_games("bn6_other", &v("1.0.0"))
        .contains(&bn6_falzar()));
    assert!(!catalog.is_installed("bn6_other", &v("1.0.0")));
}

#[cfg(feature = "gamesupport-bn6")]
#[tokio::test]
async fn tags_resolve_from_the_index_before_anything_is_downloaded() {
    let dir = TempDir::new();
    index_with(
        &dir.0,
        &[("bn6_cosmetic", "1.0.0", "vanilla"), ("bn6_mod", "1.0.0", "isolated")],
    );
    let catalog = scanned(&dir.0).await;
    let game = bn6_falzar();

    // This is what lets the lobby judge a peer's patch it has never
    // seen: a cosmetic patch plays the unpatched game...
    assert_eq!(
        catalog.tag(game, Some(("bn6_cosmetic", &v("1.0.0")))),
        catalog.tag(game, None)
    );
    // ... and a gameplay patch does not.
    assert_ne!(
        catalog.tag(game, Some(("bn6_mod", &v("1.0.0")))),
        catalog.tag(game, None)
    );
    // A patch nobody has heard of can't be vouched for either way.
    assert_eq!(catalog.tag(game, Some(("bn6_unknown", &v("1.0.0")))), None);
}

#[tokio::test]
async fn the_index_and_the_package_agree_on_author_form() {
    let dir = TempDir::new();
    install(&dir.0, "bn6_test", "1.0.0", "isolated");
    index_with(&dir.0, &[("bn6_test", "1.0.0", "isolated")]);
    let catalog = scanned(&dir.0).await;

    let installed = &catalog.installed["bn6_test"].authors;
    let indexed = &catalog.entry("bn6_test", &v("1.0.0")).unwrap().authors;
    // Both sides hold the same raw form, so the single reduction the
    // UI runs can't make a patch look like it changed hands the
    // moment it finishes downloading.
    assert_eq!(installed, indexed);
    assert_eq!(display_authors(installed), display_authors(indexed));
    assert_eq!(display_authors(installed), vec!["Someone"]);
}

#[cfg(feature = "gamesupport-bn6")]
#[tokio::test]
async fn an_installed_package_outranks_the_index() {
    // The installed package is the thing that would actually run, so
    // a stale or wrong index entry must not decide compatibility.
    let dir = TempDir::new();
    install(&dir.0, "bn6_test", "1.0.0", "isolated");
    index_with(&dir.0, &[("bn6_test", "1.0.0", "vanilla")]);

    let catalog = scanned(&dir.0).await;
    assert_eq!(
        catalog.compatibility("bn6_test", &v("1.0.0")),
        Some(&Compatibility::Isolated)
    );
    assert_ne!(
        catalog.tag(bn6_falzar(), Some(("bn6_test", &v("1.0.0")))),
        catalog.tag(bn6_falzar(), None)
    );
}

#[cfg(feature = "gamesupport-bn6")]
#[tokio::test]
async fn newest_version_spans_installed_and_offered() {
    let dir = TempDir::new();
    install(&dir.0, "bn6_test", "1.0.0", "isolated");
    index_with(&dir.0, &[("bn6_test", "1.2.0", "isolated")]);
    let catalog = scanned(&dir.0).await;
    assert_eq!(catalog.newest_version("bn6_test", None), Some(v("1.2.0")));
    assert_eq!(catalog.newest_version("bn6_test", Some(bn6_falzar())), Some(v("1.2.0")));
    assert_eq!(catalog.newest_version("nonexistent", None), None);
}

#[tokio::test]
async fn a_corrupt_package_is_skipped_not_fatal() {
    let dir = TempDir::new();
    install(&dir.0, "bn6_test", "1.0.0", "isolated");
    std::fs::write(
        dir.0.join(format!("junk-1.0.0.{}", tango_patch::EXTENSION)),
        b"not a zip",
    )
    .unwrap();
    // A file someone dropped in by hand shouldn't cost them the
    // rest of their patches.
    let catalog = scanned(&dir.0).await;
    assert_eq!(catalog.installed.len(), 1);
    assert!(catalog.is_installed("bn6_test", &v("1.0.0")));
}

/// The smallest HTTP server that can stand in for a patch repo:
/// serves files under a root, with the ETag handling `fetch_index`
/// depends on. Testing against a real socket is the point — URL
/// joining, conditional requests, and streamed bodies are exactly
/// what a hand-rolled fake would paper over.
async fn serve(root: PathBuf) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let root = root.clone();
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = vec![0u8; 8192];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]).into_owned();
                let path = request
                    .lines()
                    .next()
                    .and_then(|l| l.split_whitespace().nth(1))
                    .unwrap_or("/")
                    .trim_start_matches('/')
                    .to_owned();
                let if_none_match = request
                    .lines()
                    .find_map(|l| l.strip_prefix("if-none-match: ").or(l.strip_prefix("If-None-Match: ")))
                    .map(|s| s.trim().to_owned());

                let response = match std::fs::read(root.join(&path)) {
                    Ok(body) => {
                        let etag = format!("\"{}\"", tango_patch::sha256_hex(&body));
                        if if_none_match.as_deref() == Some(etag.as_str()) {
                            b"HTTP/1.1 304 Not Modified\r\nContent-Length: 0\r\n\r\n".to_vec()
                        } else {
                            let mut out = format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nETag: {etag}\r\n\r\n",
                                body.len()
                            )
                            .into_bytes();
                            out.extend_from_slice(&body);
                            out
                        }
                    }
                    Err(_) => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec(),
                };
                let _ = socket.write_all(&response).await;
                let _ = socket.flush().await;
            });
        }
    });
    format!("http://{addr}")
}

/// Build a repo of packages plus its index, the way the bundler's
/// `pack` + `index` would.
fn build_repo(root: &Path, packages: &[(&str, &str, &str)]) {
    for (name, version, netplay) in packages {
        let mut builder = Builder::new(manifest(name, version, netplay));
        builder.set_readme(format!("# {name} {version}"));
        builder.add_rom(
            "BR6E_00".parse().unwrap(),
            format!("bps for {name} {version}").into_bytes(),
        );
        builder.write_file(&root.join(name)).unwrap();
    }
    let index = tango_patch::Index::build(root, true).unwrap();
    std::fs::write(root.join(tango_patch::index::FILE_NAME), index.to_json().unwrap()).unwrap();
}

#[tokio::test]
async fn fetches_the_index_then_installs_only_what_is_asked_for() {
    let repo = TempDir::new();
    build_repo(
        &repo.0,
        &[("bn6_one", "1.0.0", "vanilla"), ("bn6_two", "2.0.0", "group:pair")],
    );
    let url = serve(repo.0.clone()).await;
    let data = TempDir::new();

    // First fetch pulls the index; the second is a 304.
    assert!(
        fetch_index(&net(), FS, &url, &data.0).await.unwrap(),
        "first fetch should store"
    );
    assert!(
        !fetch_index(&net(), FS, &url, &data.0).await.unwrap(),
        "second fetch should 304"
    );

    // The whole repo is browsable, with nothing downloaded.
    let catalog = scanned(&data.0).await;
    assert!(catalog.installed.is_empty());
    assert_eq!(
        catalog.names().into_iter().collect::<Vec<_>>(),
        vec!["bn6_one", "bn6_two"]
    );
    assert_eq!(catalog.title("bn6_two"), Some("Test bn6_two"));
    assert_eq!(
        std::fs::read_dir(&data.0)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == tango_patch::EXTENSION))
            .count(),
        0,
        "nothing should have been downloaded"
    );

    // Install one of them.
    let version = v("2.0.0");
    let entry = catalog.entry("bn6_two", &version).unwrap().clone();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen_w = seen.clone();
    download(&net(), FS, &url, &data.0, "bn6_two", &version, &entry, move |p| {
        seen_w.lock().unwrap().push(p.downloaded);
        true
    })
    .await
    .unwrap();
    assert!(
        seen.lock().unwrap().last().copied() == Some(entry.size),
        "progress should end at the full size: {:?}",
        seen.lock().unwrap()
    );

    let catalog = scanned(&data.0).await;
    assert!(catalog.is_installed("bn6_two", &version));
    assert!(!catalog.is_installed("bn6_one", &v("1.0.0")), "only the asked-for one");
    assert_eq!(
        catalog.version("bn6_two", &version).unwrap().readme.as_deref(),
        Some("# bn6_two 2.0.0")
    );
    assert_eq!(
        catalog.compatibility("bn6_two", &version),
        Some(&Compatibility::Group("pair".into()))
    );
}

/// Cancelling mid-stream stops the download AND leaves nothing on
/// disk — not the package, and not the temporary it was streaming
/// into. That tidying is the whole reason cancellation is
/// cooperative rather than just dropping the future.
#[tokio::test]
async fn a_cancelled_download_leaves_nothing_behind() {
    let repo = TempDir::new();
    build_repo(&repo.0, &[("bn6_one", "1.0.0", "vanilla")]);
    let url = serve(repo.0.clone()).await;
    let data = TempDir::new();
    fetch_index(&net(), FS, &url, &data.0).await.unwrap();

    let catalog = scanned(&data.0).await;
    let version = v("1.0.0");
    let entry = catalog.entry("bn6_one", &version).unwrap().clone();

    let outcome = download(&net(), FS, &url, &data.0, "bn6_one", &version, &entry, |_| false)
        .await
        .unwrap();
    assert!(matches!(outcome, Outcome::Cancelled), "{outcome:?}");
    assert!(!package_path(&data.0, "bn6_one", &version).exists());
    let leftovers: Vec<_> = std::fs::read_dir(&data.0)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".tmp") || n.ends_with(".part"))
        .collect();
    assert!(leftovers.is_empty(), "left a partial file: {leftovers:?}");
    assert!(!scanned(&data.0).await.is_installed("bn6_one", &version));
}

#[tokio::test]
async fn a_package_that_is_not_what_the_index_promised_is_rejected() {
    let repo = TempDir::new();
    build_repo(&repo.0, &[("bn6_one", "1.0.0", "vanilla")]);
    let url = serve(repo.0.clone()).await;
    let data = TempDir::new();
    fetch_index(&net(), FS, &url, &data.0).await.unwrap();

    let catalog = scanned(&data.0).await;
    let version = v("1.0.0");
    let mut entry = catalog.entry("bn6_one", &version).unwrap().clone();
    entry.sha256 = "0".repeat(64);

    let err = download(&net(), FS, &url, &data.0, "bn6_one", &version, &entry, |_| true)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("hash mismatch"), "{err}");
    // Nothing half-written left behind: not the package, and not
    // the temporary it streamed into.
    assert!(!package_path(&data.0, "bn6_one", &version).exists());
    // Only the index and its validator; no package, and no
    // half-written temporary.
    let mut left: Vec<String> = std::fs::read_dir(&data.0)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left, vec!["index.etag", "index.json"], "no leftovers");
    assert!(scanned(&data.0).await.installed.is_empty());
}

#[tokio::test]
async fn a_failed_fetch_leaves_the_cached_index_usable() {
    let repo = TempDir::new();
    build_repo(&repo.0, &[("bn6_one", "1.0.0", "vanilla")]);
    let url = serve(repo.0.clone()).await;
    let data = TempDir::new();
    fetch_index(&net(), FS, &url, &data.0).await.unwrap();

    // Repo goes away (or serves garbage) — we keep browsing what we
    // last saw, which is what makes the app work offline.
    std::fs::write(repo.0.join(tango_patch::index::FILE_NAME), "not json at all").unwrap();
    assert!(fetch_index(&net(), FS, &url, &data.0).await.is_err());
    let catalog = scanned(&data.0).await;
    assert_eq!(catalog.names().len(), 1);
    assert!(catalog.entry("bn6_one", &v("1.0.0")).is_some());
}

#[tokio::test]
async fn a_corrupt_index_leaves_the_installed_patches_alone() {
    let dir = TempDir::new();
    install(&dir.0, "bn6_test", "1.0.0", "isolated");
    std::fs::write(index_path(&dir.0), "{ this is not json").unwrap();
    let catalog = scanned(&dir.0).await;
    assert!(catalog.index.is_empty());
    assert!(catalog.is_installed("bn6_test", &v("1.0.0")));
}
