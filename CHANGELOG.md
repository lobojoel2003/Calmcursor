# Changelog

All notable changes to Calmcursor are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
Calmcursor uses [Semantic Versioning](https://semver.org/). Every build also
carries a build number (the CI run number) and the git commit it came from;
both appear in the app's About screen and in `calmcursor-cli --version`.

## [Unreleased]

## [0.1.0] - 2026-11-07

The first release.

### Added

- **Steadying on Windows, macOS and Linux.** An adaptive One Euro filter,
  written in Rust, smooths out hand tremor while keeping deliberate moves
  responsive.
  - Windows: a low-level mouse hook (`WH_MOUSE_LL`). No admin rights needed.
  - macOS: a Quartz event tap. Needs Accessibility permission.
  - Linux: an evdev grab plus a uinput virtual pointer. Works on X11 and every
    Wayland compositor. Ships a udev rule in the `.deb` and `.rpm`.
- **30-second calibration game** with three rounds (Rest, Follow, Tap). It
  measures the person's tremor rhythm and size, tests a grid of filter
  settings against their own movement, and recommends the least-laggy one
  that removes most of the shake.
- **Live trace** of the raw path next to the steadied one, with a running
  "shake removed" meter and a demo mode with a simulated tremor.
- **Click lock**: holds the pointer still for a moment after a button press,
  so clicks don't slip or turn into accidental drags.
- **Double-press filter**: optionally ignores a second press that comes too
  soon after a release.
- Tray / menu-bar icon, global pause shortcut (default
  `Ctrl+Alt+Shift+S` / `⌘⌥⇧S`), launch at login and start hidden.
- `calmcursor-cli`: run steadying without the window, check permissions, and
  render a before/after chart from a simulated tremor.
- Simulated input backend (`CALMCURSOR_BACKEND=simulated`) for demos and
  development on machines without a mouse.
- Installers for every platform: `.msi` and `.exe` (Windows), universal
  `.dmg` (Apple silicon and Intel), `.deb`, `.rpm` and `.AppImage` (Linux),
  plus CLI archives and `SHA256SUMS.txt`.

[Unreleased]: https://github.com/lobojoel2003/Calmcursor/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/lobojoel2003/Calmcursor/releases/tag/v0.1.0
