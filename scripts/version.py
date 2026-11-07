#!/usr/bin/env python3
"""Calmcursor version tool.

The single source of truth is `version` in the root Cargo.toml
([workspace.package]). The desktop app, CLI and libraries all inherit it, and
Tauri reads it for the installers. This script keeps the few other places in
step and pulls release notes out of CHANGELOG.md.

  scripts/version.py                 print the current version
  scripts/version.py check [TAG]     verify every file agrees (and TAG == v<version>)
  scripts/version.py bump X.Y.Z      set a new version everywhere and open its changelog section
  scripts/version.py notes [X.Y.Z]   print the changelog section for a version

Build numbers are not stored in the repository: CI passes the GitHub Actions
run number to the build (CALMCURSOR_BUILD_NUMBER), and it appears in the app's
About screen, `calmcursor-cli --version`, the macOS CFBundleVersion and the
fourth field of the Windows MSI version.
"""

from __future__ import annotations

import datetime as _dt
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CARGO = ROOT / "Cargo.toml"
PACKAGE = ROOT / "app" / "package.json"
LOCK = ROOT / "app" / "package-lock.json"
CHANGELOG = ROOT / "CHANGELOG.md"
REPO = "https://github.com/lobojoel2003/Calmcursor"
SEMVER = re.compile(r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?$")


def cargo_version() -> str:
    text = CARGO.read_text(encoding="utf-8")
    m = re.search(r"\[workspace\.package\][^\[]*?^version\s*=\s*\"([^\"]+)\"", text, re.S | re.M)
    if not m:
        sys.exit("error: no version in [workspace.package] of Cargo.toml")
    return m.group(1)


def internal_dep_versions() -> dict[str, str]:
    text = CARGO.read_text(encoding="utf-8")
    return dict(re.findall(r"^(calmcursor-[a-z]+)\s*=\s*\{[^}]*version\s*=\s*\"([^\"]+)\"", text, re.M))


def changelog_section(version: str) -> str | None:
    text = CHANGELOG.read_text(encoding="utf-8")
    m = re.search(
        rf"^## \[{re.escape(version)}\][^\n]*\n(.*?)(?=^## \[|^\[[^\]]+\]:|\Z)", text, re.S | re.M
    )
    return m.group(1).strip() if m else None


def check(tag: str | None) -> int:
    version = cargo_version()
    problems: list[str] = []
    if not SEMVER.match(version):
        problems.append(f"Cargo.toml version {version!r} is not semantic versioning (X.Y.Z)")
    for name, v in internal_dep_versions().items():
        if v != version:
            problems.append(f"Cargo.toml [workspace.dependencies] {name} = {v}, expected {version}")
    pkg = json.loads(PACKAGE.read_text(encoding="utf-8"))
    if pkg.get("version") != version:
        problems.append(f"app/package.json version = {pkg.get('version')}, expected {version}")
    if LOCK.exists():
        lock = json.loads(LOCK.read_text(encoding="utf-8"))
        for where, v in (("version", lock.get("version")), ('packages[""].version', lock.get("packages", {}).get("", {}).get("version"))):
            if v != version:
                problems.append(f"app/package-lock.json {where} = {v}, expected {version}")
    if changelog_section(version) is None:
        problems.append(f"CHANGELOG.md has no section '## [{version}]'")
    if tag is not None:
        tag = tag.removeprefix("refs/tags/")
        if tag != f"v{version}":
            problems.append(f"tag {tag!r} does not match version (expected 'v{version}')")
    if problems:
        print("Version check failed:", *problems, sep="\n  - ")
        return 1
    print(f"Version {version} is consistent" + (f" with tag {tag}" if tag else "") + ".")
    return 0


def bump(new: str) -> int:
    if not SEMVER.match(new):
        sys.exit(f"error: {new!r} is not a semantic version (X.Y.Z)")
    old = cargo_version()
    text = CARGO.read_text(encoding="utf-8")
    text = re.sub(
        r"(\[workspace\.package\][^\[]*?^version\s*=\s*)\"[^\"]+\"",
        rf'\g<1>"{new}"',
        text,
        count=1,
        flags=re.S | re.M,
    )
    text = re.sub(
        r"^(calmcursor-[a-z]+\s*=\s*\{[^}]*version\s*=\s*)\"[^\"]+\"", rf'\g<1>"{new}"', text, flags=re.M
    )
    CARGO.write_text(text, encoding="utf-8")

    for path in (PACKAGE, LOCK):
        if not path.exists():
            continue
        data = json.loads(path.read_text(encoding="utf-8"))
        data["version"] = new
        if "packages" in data and "" in data["packages"]:
            data["packages"][""]["version"] = new
        path.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")

    log = CHANGELOG.read_text(encoding="utf-8")
    if f"## [{new}]" not in log:
        today = _dt.date.today().isoformat()
        log = log.replace("## [Unreleased]", f"## [Unreleased]\n\n## [{new}] - {today}", 1)
        log = re.sub(
            r"^\[Unreleased\]: .*$",
            f"[Unreleased]: {REPO}/compare/v{new}...HEAD\n[{new}]: {REPO}/compare/v{old}...v{new}",
            log,
            count=1,
            flags=re.M,
        )
        CHANGELOG.write_text(log, encoding="utf-8")

    try:
        subprocess.run(["cargo", "update", "--workspace", "--quiet"], cwd=ROOT, check=True)
    except (OSError, subprocess.CalledProcessError):
        print("note: run `cargo update --workspace` to refresh Cargo.lock")
    print(f"Bumped {old} -> {new}. Next:")
    print(f"  1. Move the changes under '## [Unreleased]' in CHANGELOG.md into '## [{new}]'.")
    print(f"  2. git commit -am 'Release {new}'")
    print(f"  3. git tag -a v{new} -m 'Calmcursor {new}' && git push origin main v{new}")
    return 0


def main(argv: list[str]) -> int:
    if not argv:
        print(cargo_version())
        return 0
    cmd, *rest = argv
    if cmd == "check":
        return check(rest[0] if rest else None)
    if cmd == "bump" and len(rest) == 1:
        return bump(rest[0])
    if cmd == "notes":
        version = rest[0].removeprefix("v") if rest else cargo_version()
        section = changelog_section(version)
        if section is None:
            sys.exit(f"error: CHANGELOG.md has no section for {version}")
        print(section)
        return 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
