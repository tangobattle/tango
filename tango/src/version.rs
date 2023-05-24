lazy_static! {
    static ref VERSION: semver::Version = env!("CARGO_PKG_VERSION").parse().unwrap();
}

pub fn current() -> semver::Version {
    VERSION.clone()
}
