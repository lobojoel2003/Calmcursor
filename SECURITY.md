# Security policy

Calmcursor sits between your mouse and the rest of the system, so it is
written to fail safe: if it stops or crashes, the operating system releases
its hooks and the pointer behaves normally again.

## What Calmcursor can access

- **Windows**: a low-level mouse hook in the current user session. It sees
  pointer movement and button presses only. It never reads the keyboard.
- **macOS**: a Quartz event tap for mouse events, which needs Accessibility
  permission. It never reads the keyboard.
- **Linux**: exclusive access to mice and trackballs (`/dev/input`), and the
  ability to create one virtual pointer (`/dev/uinput`). Devices that also
  send keyboard keys are deliberately left alone.

Calmcursor makes no network connections. The only file it writes is its
settings file in your configuration folder.

## Supported versions

Security fixes go into the latest release.

## Reporting a problem

Please report security issues privately through
[GitHub security advisories](https://github.com/lobojoel2003/Calmcursor/security/advisories/new)
rather than a public issue. You will get a reply within a week.
