#!/bin/sh
# Calmcursor post-remove: drop the udev rule from the running system.
set -e
if command -v udevadm >/dev/null 2>&1; then
    udevadm control --reload-rules 2>/dev/null || true
fi
exit 0
