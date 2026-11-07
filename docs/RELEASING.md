# Versions, build numbers and releases

## Version numbers

Calmcursor follows [Semantic Versioning](https://semver.org/): `MAJOR.MINOR.PATCH`.

- **PATCH** (0.1.0 → 0.1.1): bug fixes only.
- **MINOR** (0.1.0 → 0.2.0): new features; settings files stay compatible.
- **MAJOR**: changes that need people to do something differently.

Before 1.0, minor versions may still change behaviour noticeably.

There is one source of truth: `version` under `[workspace.package]` in the
root `Cargo.toml`. Every crate inherits it, and Tauri reads it for the
installers. `app/package.json` is kept in step by the version script, and CI
fails if anything disagrees.

## Build numbers

Every CI build is numbered with the GitHub Actions run number and stamped with
its git commit and date. You can see them:

- in the app, under **About → This build**;
- with `calmcursor-cli --version`;
- on macOS as the bundle's `CFBundleVersion`;
- on Windows as the fourth field of the MSI version (for example `0.1.0.42`);
- at the bottom of each GitHub release's notes.

Local builds say `build dev`.

## Tags

Each release has an annotated git tag `vX.Y.Z` on the commit it was built from.
Pushing such a tag is what triggers the release workflow.

## Making a release

1. Make sure `main` is green in CI.
2. Bump the version (updates `Cargo.toml`, `app/package.json`,
   `app/package-lock.json`, `Cargo.lock` and adds a changelog section):

   ```bash
   python3 scripts/version.py bump 0.2.0
   ```

3. Move the entries under `## [Unreleased]` in `CHANGELOG.md` into the new
   section, and check it reads well. It becomes the release notes.
4. Commit, tag and push:

   ```bash
   git commit -am "Release 0.2.0"
   git tag -a v0.2.0 -m "Calmcursor 0.2.0"
   git push origin main v0.2.0
   ```

5. The [Release workflow](../.github/workflows/release.yml) then:
   - checks the tag matches the version everywhere;
   - builds the installers: `.msi` and `.exe` (Windows x64), a universal
     `.dmg` (Apple silicon and Intel), `.deb`, `.rpm` and `.AppImage` (Linux
     x64);
   - builds the command-line tool for all three systems;
   - writes `SHA256SUMS.txt`;
   - publishes the GitHub release with notes from the changelog.

To test the whole pipeline without publishing, run the Release workflow by
hand from the Actions tab and leave **Publish** unticked. The files appear as
workflow artifacts.

A version with a suffix, such as `0.2.0-beta.1`, is published as a
pre-release.

## Code signing

The installers are not signed with paid certificates yet:

- **macOS** builds are ad-hoc signed, which Apple silicon requires. The first
  time, right-click the app and choose **Open**. Because ad-hoc signatures
  change with every build, macOS asks for Accessibility permission again
  after each update.
- **Windows** may show a SmartScreen notice. Choose **More info → Run anyway**.

Adding Apple Developer ID and Windows code-signing certificates as repository
secrets, and wiring them into `release.yml`, would remove both prompts.
