//! What the writer produces, the reader reads back.

#![cfg(all(feature = "bundle", feature = "package"))]

mod common;

use common::{manifest, target, TempDir};
use tango_patch::layout::DEFAULT_TEMPLATE;
use tango_patch::{bundle, Compatibility, Error, Package};

fn full() -> bundle::Builder {
    let mut b = bundle::Builder::new(manifest("test_patch", "1.2.3", "group:testing"));
    b.set_readme("# hello");
    b.add_rom(target("BR6E_00"), b"bps-6".to_vec());
    b.add_rom(target("BR5E_00"), b"bps-5".to_vec());
    b.add_save_template(target("BR6E_00"), DEFAULT_TEMPLATE, b"save".to_vec())
        .unwrap();
    b.add_save_template(target("BR6E_00"), "gregar", b"save-gregar".to_vec())
        .unwrap();
    b
}

#[test]
fn a_built_package_reads_back_intact() {
    let mut pkg = Package::read(std::io::Cursor::new(full().to_vec().unwrap())).unwrap();

    assert_eq!(pkg.manifest().name, "test_patch");
    assert_eq!(pkg.manifest().version.to_string(), "1.2.3");
    assert_eq!(pkg.manifest().netplay, Compatibility::Group("testing".into()));
    assert_eq!(pkg.manifest().license.as_deref(), Some("MIT"));

    // Contents come from the archive, not the manifest.
    assert_eq!(
        pkg.targets().collect::<Vec<_>>(),
        vec![target("BR5E_00"), target("BR6E_00")]
    );
    assert!(pkg.supports(target("BR6E_00")));
    assert!(!pkg.supports(target("B4WE_00")));

    assert_eq!(pkg.bps(target("BR6E_00")).unwrap(), b"bps-6");
    assert_eq!(pkg.bps(target("BR5E_00")).unwrap(), b"bps-5");
    assert_eq!(pkg.readme().unwrap().as_deref(), Some("# hello"));

    assert_eq!(
        pkg.save_templates(target("BR6E_00")).collect::<Vec<_>>(),
        vec![DEFAULT_TEMPLATE, "gregar"]
    );
    assert_eq!(pkg.save_templates(target("BR5E_00")).count(), 0);
    assert_eq!(pkg.save_template(target("BR6E_00"), DEFAULT_TEMPLATE).unwrap(), b"save");
    assert_eq!(pkg.save_template(target("BR6E_00"), "gregar").unwrap(), b"save-gregar");
}

#[test]
fn asking_for_an_unpatched_rom_is_a_clear_error() {
    let mut pkg = Package::read(std::io::Cursor::new(full().to_vec().unwrap())).unwrap();
    assert!(matches!(pkg.bps(target("B4WE_00")), Err(Error::NoSuchTarget(_))));
}

#[test]
fn a_package_is_named_after_its_contents() {
    let dir = TempDir::new();
    let built = full().write_file(dir.path()).unwrap();
    assert_eq!(
        built.path.file_name().unwrap().to_string_lossy(),
        "test_patch-1.2.3.tangopatch"
    );
    assert_eq!(built.sha256.len(), 64);
    assert_eq!(built.size, std::fs::metadata(&built.path).unwrap().len());
    assert!(Package::open(&built.path).is_ok());
}

#[test]
fn a_source_directory_packs_into_the_same_package() {
    let dir = TempDir::new();
    let src = dir.path().join("src");
    std::fs::create_dir_all(src.join("roms")).unwrap();
    std::fs::create_dir_all(src.join("saves")).unwrap();
    std::fs::write(src.join("manifest.toml"), full().manifest().to_toml().unwrap()).unwrap();
    // A plain `README` is accepted; it lands in the package as README.md.
    std::fs::write(src.join("README"), "# hello").unwrap();
    std::fs::write(src.join("roms/BR6E_00.bps"), b"bps-6").unwrap();
    std::fs::write(src.join("roms/BR5E_00.bps"), b"bps-5").unwrap();
    std::fs::write(src.join("saves/BR6E_00.sav"), b"save").unwrap();
    std::fs::write(src.join("saves/BR6E_00.gregar.sav"), b"save-gregar").unwrap();

    assert_eq!(
        bundle::read_dir(&src).unwrap().to_vec().unwrap(),
        full().to_vec().unwrap()
    );
}

#[test]
fn garbage_is_not_a_package() {
    assert!(Package::read(std::io::Cursor::new(b"not a zip".to_vec())).is_err());
    let truncated = full().to_vec().unwrap();
    assert!(Package::read(std::io::Cursor::new(truncated[..truncated.len() / 2].to_vec())).is_err());
}
