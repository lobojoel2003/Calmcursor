#!/bin/sh
# Calmcursor post-install: load uinput and apply the udev rule now, so
# steadying works without a reboot (a log out and in may still be needed).
set -e
if command -v modprobe >/dev/null 2>&1; then
    modprobe uinput 2>/dev/null || true
fi
if command -v udevadm >/dev/null 2>&1; then
    udevadm control --reload-rules 2>/dev/null || true
    udevadm trigger --subsystem-match=misc --sysname-match=uinput 2>/dev/null || true
    udevadm trigger --subsystem-match=input 2>/dev/null || true
fi
exit 0
