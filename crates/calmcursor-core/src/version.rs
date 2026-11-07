//! Build identification: semantic version, CI build number and git commit.

use serde::{Deserialize, Serialize};

/// The semantic version from `Cargo.toml` (for example `0.1.0`).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The CI build number (GitHub Actions run number), or `dev` for local builds.
pub const BUILD_NUMBER: &str = env!("CALMCURSOR_BUILD_NUMBER");
/// Short git commit hash the build came from.
pub const GIT_SHA: &str = env!("CALMCURSOR_GIT_SHA");
/// Commit date (YYYY-MM-DD) of the build.
pub const BUILD_DATE: &str = env!("CALMCURSOR_BUILD_DATE");
/// `release`, `ci` or `local`.
pub const CHANNEL: &str = env!("CALMCURSOR_CHANNEL");
/// The Rust target triple.
pub const TARGET: &str = env!("CALMCURSOR_TARGET");

/// All build details in one serialisable value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildInfo {
    pub version: String,
    pub build_number: String,
    pub git_sha: String,
    pub build_date: String,
    pub channel: String,
    pub target: String,
}

impl BuildInfo {
    pub fn current() -> Self {
        Self {
            version: VERSION.into(),
            build_number: BUILD_NUMBER.into(),
            git_sha: GIT_SHA.into(),
            build_date: BUILD_DATE.into(),
            channel: CHANNEL.into(),
            target: TARGET.into(),
        }
    }

    /// For example `0.1.0 (build 42, 1a2b3c4d, 2026-11-07)`.
    pub fn long_version(&self) -> String {
        format!(
            "{} (build {}, {}, {})",
            self.version, self.build_number, self.git_sha, self.build_date
        )
    }
}

/// Shorthand for `BuildInfo::current().long_version()`.
pub fn long_version() -> String {
    BuildInfo::current().long_version()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_semver() {
        let parts: Vec<&str> = VERSION.split('.').collect();
        assert_eq!(parts.len(), 3);
        assert!(parts.iter().all(|p| p.parse::<u64>().is_ok()));
    }

    #[test]
    fn long_version_contains_everything() {
        let v = long_version();
        assert!(v.starts_with(VERSION));
        assert!(v.contains(BUILD_NUMBER));
        assert!(v.contains(GIT_SHA));
    }
}
