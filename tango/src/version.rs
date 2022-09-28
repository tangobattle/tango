const RAW_VERSION: &str = git_version::git_version!(args = ["--tags", "--always"]);

lazy_static! {
    static ref VERSION: semver::Version = RAW_VERSION[1..].parse().unwrap();
}

pub fn current() -> semver::Version {
    VERSION.clone()
}
