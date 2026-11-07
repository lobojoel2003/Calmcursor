//! Embeds build metadata (build number, git commit, build date) so every
//! binary can report exactly which build it is.
//!
//! CI sets `CALMCURSOR_BUILD_NUMBER` (the GitHub Actions run number) and
//! `CALMCURSOR_GIT_SHA`. Local builds fall back to `git` and "dev".

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=CALMCURSOR_BUILD_NUMBER");
    println!("cargo:rerun-if-env-changed=CALMCURSOR_GIT_SHA");
    println!("cargo:rerun-if-env-changed=CALMCURSOR_BUILD_DATE");
    println!("cargo:rerun-if-env-changed=CALMCURSOR_CHANNEL");

    let build_number = env_or("CALMCURSOR_BUILD_NUMBER", || "dev".to_string());
    let git_sha = env_or("CALMCURSOR_GIT_SHA", || {
        git(&["rev-parse", "--short=8", "HEAD"]).unwrap_or_else(|| "unknown".into())
    });
    let short_sha: String = git_sha.chars().take(8).collect();
    let build_date = env_or("CALMCURSOR_BUILD_DATE", || {
        git(&["log", "-1", "--format=%cs"]).unwrap_or_else(|| "unknown".into())
    });
    let channel = env_or("CALMCURSOR_CHANNEL", || "local".to_string());
    let target = std::env::var("TARGET").unwrap_or_default();

    println!("cargo:rustc-env=CALMCURSOR_BUILD_NUMBER={build_number}");
    println!("cargo:rustc-env=CALMCURSOR_GIT_SHA={short_sha}");
    println!("cargo:rustc-env=CALMCURSOR_BUILD_DATE={build_date}");
    println!("cargo:rustc-env=CALMCURSOR_CHANNEL={channel}");
    println!("cargo:rustc-env=CALMCURSOR_TARGET={target}");
}

fn env_or(key: &str, fallback: impl FnOnce() -> String) -> String {
    match std::env::var(key) {
        Ok(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => fallback(),
    }
}

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!s.is_empty()).then_some(s)
}
