# Contributing to Calmcursor

Thank you for helping. Bug reports from people who use Calmcursor every day are
as valuable as code, so please [open an issue](https://github.com/lobojoel2003/Calmcursor/issues/new/choose)
whenever something feels wrong.

## Project layout

| Path | What it is |
|---|---|
| `crates/calmcursor-core` | Platform-free Rust: the One Euro filter, the steadying engine, click lock, tremor analysis, calibration, settings. Fully unit-tested. |
| `crates/calmcursor-platform` | The OS hooks (Windows, macOS, Linux) and the tick thread that drives the engine. |
| `crates/calmcursor-cli` | `calmcursor-cli`, the headless tool. |
| `app/` | The desktop app: Tauri v2 (`app/src-tauri`) with a TypeScript frontend (`app/src`). |
| `packaging/linux` | udev rule and install scripts shipped in the `.deb` and `.rpm`. |
| `branding/` | The logo source (SVG). Icons in `app/src-tauri/icons` are generated from it. |
| `scripts/version.py` | Version checks, bumps and release notes. |
| `docs/` | Architecture notes, release process and README images. |

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how the pieces fit together.

## Getting set up

You need Rust (stable) and Node.js 20 or newer.

On Linux, install the WebKitGTK development packages first:

```bash
sudo apt-get install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev
```

Then:

```bash
cd app
npm install
npm run tauri dev
```

No mouse to test with, or want to try changes safely? Run with simulated input,
which never touches your real pointer:

```bash
CALMCURSOR_BACKEND=simulated npm run tauri dev
```

## Before you open a pull request

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd app && npm run typecheck)
```

CI runs the same checks on Windows, macOS and Linux.

## Design principles

- **Safety first.** If anything goes wrong, the pointer must go back to
  normal. Never leave a device grabbed or a hook installed after an error.
- **Built for shaky hands.** Every control is at least 48 px tall, nothing
  needs a double-click or a precise drag, and text is plain and friendly.
- **Measure, don't guess.** Changes to filtering or calibration should come
  with a test in `calmcursor-core` that shows the effect, using the
  simulator in `calmcursor_core::simulate`.
- **Private by default.** No network access, no telemetry.

## Releasing

See [docs/RELEASING.md](docs/RELEASING.md).

By contributing you agree that your contributions are licensed under the
GNU GPL v3 or later, like the rest of the project.
