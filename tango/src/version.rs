lazy_static! {
    static ref VERSION: semver::Version = env!("CARGO_PKG_VERSION").parse().unwrap();
}

pub fn vcs_info() -> &'static str {
    git_version::git_version!()
}

pub fn current() -> semver::Version {
    VERSION.clone()
}
