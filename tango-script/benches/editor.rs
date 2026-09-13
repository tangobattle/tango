//! Replay package-authored interaction traces against local inputs, without writing saves.
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

use serde::Deserialize;
use tango_script::{Action, EditorSession, Inputs, Package, PackageRef, Profile};

#[derive(Deserialize)]
struct Case {
    name: String,
    #[serde(default)]
    setup: Vec<Action>,
    actions: Vec<Action>,
}

fn load(root: &Path) -> Package {
    fn collect(root: &Path, path: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                collect(root, &entry.path(), files);
            } else if entry.file_type().unwrap().is_file() {
                let name = entry
                    .path()
                    .strip_prefix(root)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .replace('\\', "/");
                files.insert(name, std::fs::read(entry.path()).unwrap());
            }
        }
    }
    let mut files = BTreeMap::new();
    collect(root, root, &mut files);
    Package::load(files).unwrap()
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).filter(|arg| arg != "--bench").collect();
    assert!(
        args.len() >= 5,
        "usage: editor PACKAGES PACKAGE SAVE ROM TRACE [ITERATIONS]"
    );
    let packages: Vec<_> = std::fs::read_dir(&args[0])
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.join("package.toml").is_file())
        .map(|path| load(&path))
        .collect();
    let package = packages
        .iter()
        .find(|package| package.manifest().name == args[1])
        .unwrap();
    let profile = Profile::resolve(
        &packages,
        &PackageRef {
            name: args[1].clone(),
            version: package.manifest().version.clone(),
        },
    )
    .unwrap()
    .with_inputs(Inputs::new([("rom".into(), std::fs::read(&args[3]).unwrap())].into()).unwrap());
    let sram = std::fs::read(&args[2]).unwrap();
    let cases: Vec<Case> = serde_json::from_slice(&std::fs::read(&args[4]).unwrap()).unwrap();
    let iterations = args.get(5).map(|n| n.parse::<usize>().unwrap()).unwrap_or(20);
    assert!(iterations > 0);
    for case in cases {
        assert!(!case.actions.is_empty());
        let start = Instant::now();
        let mut session = EditorSession::open(profile.clone(), &sram, true).unwrap();
        let opened = start.elapsed();
        for action in case.setup {
            assert!(session.dispatch(action).unwrap().save.is_none());
        }
        let mut samples = Vec::<Duration>::new();
        for _ in 0..iterations {
            for action in &case.actions {
                let start = Instant::now();
                assert!(session.dispatch(action.clone()).unwrap().save.is_none());
                samples.push(start.elapsed());
            }
        }
        samples.sort();
        let millis = |duration: Duration| duration.as_secs_f64() * 1000.0;
        println!(
            "{}: open {:.2} ms; {} actions; median {:.2} ms; p95 {:.2} ms",
            case.name,
            millis(opened),
            samples.len(),
            millis(samples[samples.len() / 2]),
            millis(samples[(samples.len() - 1) * 95 / 100])
        );
    }
}
