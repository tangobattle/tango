//! Building a repo index out of packages, and reading back what the
//! client needs from it.

#![cfg(all(feature = "bundle", feature = "package", feature = "index"))]

mod common;

use common::{package, target, TempDir};
use tango_patch::{Compatibility, Index, Tag};

fn repo() -> (TempDir, Index) {
    let dir = TempDir::new();
    for (name, version, netplay) in [
        ("bn6_allstars", "1.0.0", "group:allstars"),
        ("bn6_allstars", "1.1.0", "group:allstars"),
        ("bn6_soundmod", "2.0.0", "vanilla"),
        ("bn6_shift", "0.1.0", "isolated"),
    ] {
        package(name, version, netplay)
            .write_file(&dir.path().join(name))
            .unwrap();
    }
    let index = Index::build(dir.path(), true).unwrap();
    (dir, index)
}

#[test]
fn builds_from_a_tree_of_packages() {
    let (_dir, index) = repo();
    assert_eq!(index.len(), 4);
    assert_eq!(index.patches.len(), 3);
    assert_eq!(index.patches["bn6_allstars"].len(), 2);

    let (version, entry) = index.latest("bn6_allstars").unwrap();
    assert_eq!(version.to_string(), "1.1.0");
    assert_eq!(entry.title, "Test bn6_allstars");
    assert_eq!(entry.license.as_deref(), Some("MIT"));
    assert_eq!(entry.games, vec![target("BR6E_00")]);
    assert_eq!(entry.netplay, Compatibility::Group("allstars".into()));
    assert_eq!(entry.path, "bn6_allstars/bn6_allstars-1.1.0.tangopatch");
    assert_eq!(entry.sha256.len(), 64);
    assert!(entry.size > 0);
}

#[test]
fn the_hash_matches_the_package_on_disk() {
    let (dir, index) = repo();
    let (_, entry) = index.latest("bn6_allstars").unwrap();
    let raw = std::fs::read(dir.path().join(&entry.path)).unwrap();
    assert_eq!(entry.size, raw.len() as u64);
    assert_eq!(entry.sha256, tango_patch::bundle::sha256_hex(&raw));
}

#[test]
fn readmes_are_extracted_for_browsing_before_download() {
    let (dir, index) = repo();
    let (_, entry) = index.latest("bn6_soundmod").unwrap();
    let readme = entry.readme.as_deref().unwrap();
    assert_eq!(readme, "bn6_soundmod/bn6_soundmod-2.0.0.README.md");
    assert_eq!(std::fs::read_to_string(dir.path().join(readme)).unwrap(), "# readme");
}

#[test]
fn readmes_are_not_extracted_unless_asked_for() {
    let (dir, _) = repo();
    let index = Index::build(dir.path(), false).unwrap();
    assert!(index.latest("bn6_soundmod").unwrap().1.readme.is_none());
}

#[test]
fn round_trips_through_json() {
    let (_dir, index) = repo();
    assert_eq!(Index::parse(&index.to_json().unwrap()).unwrap(), index);
}

#[test]
fn the_index_alone_resolves_netplay_tags() {
    // The point of the index: a client can check compatibility with a
    // peer's patch without ever downloading it.
    let (_dir, index) = repo();

    let version = "1.1.0".parse().unwrap();
    let entry = index.get("bn6_allstars", &version).unwrap();
    assert_eq!(
        entry.tag("bn6", "bn6_allstars", &version),
        Tag::Group {
            family: "bn6".into(),
            group: "allstars".into()
        }
    );
    // ... and against the other version of the same patch, which shares
    // the group.
    let older = "1.0.0".parse().unwrap();
    assert_eq!(
        index
            .get("bn6_allstars", &older)
            .unwrap()
            .tag("bn6", "bn6_allstars", &older),
        entry.tag("bn6", "bn6_allstars", &version)
    );

    let version = "2.0.0".parse().unwrap();
    let entry = index.get("bn6_soundmod", &version).unwrap();
    assert_eq!(entry.tag("bn6", "bn6_soundmod", &version), Tag::vanilla("bn6"));

    let version = "0.1.0".parse().unwrap();
    let entry = index.get("bn6_shift", &version).unwrap();
    assert_eq!(
        entry.tag("bn6", "bn6_shift", &version),
        Tag::Exact {
            family: "bn6".into(),
            patch: "bn6_shift".into(),
            version
        }
    );
}

#[test]
fn a_misnamed_package_is_rejected() {
    let dir = TempDir::new();
    let built = package("bn6_allstars", "1.0.0", "isolated")
        .write_file(dir.path())
        .unwrap();
    std::fs::rename(&built.path, dir.path().join("wrong.tangopatch")).unwrap();
    let err = Index::build(dir.path(), false).unwrap_err().to_string();
    assert!(err.contains("should be named"), "{err}");
}

#[test]
fn an_empty_tree_indexes_to_nothing() {
    let dir = TempDir::new();
    assert!(Index::build(dir.path(), false).unwrap().is_empty());
}
