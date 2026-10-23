#!/bin/sh
# Daily refresh: download + cache the blocklist, then apply it to /etc/hosts.
# `apply` elevates via a passwordless sudoers entry for the adblocker binary
# only (see scripts/install-schedule.sh). Invoked by the
# local.adblocker.update LaunchAgent.
set -e

# Resolve the invoking user's home even when launchd runs us with a minimal
# environment. SUDO_USER is set when elevated; otherwise fall back to $HOME.
USER_HOME="$HOME"
if [ -n "$SUDO_USER" ] && [ "$SUDO_USER" != "$USER" ]; then
    USER_HOME="$(eval echo ~"$SUDO_USER")"
fi

export PATH="$USER_HOME/.cargo/bin:/usr/local/bin:/usr/bin:/bin"

APP_DIR="$USER_HOME/Library/Application Support/adblocker"
LOG="$APP_DIR/refresh.log"
ADBLOCKER="$USER_HOME/.cargo/bin/adblocker"

mkdir -p "$APP_DIR"
{
    echo "=== $(date '+%Y-%m-%d %H:%M:%S') ==="
    "$ADBLOCKER" update
    sudo -n "$ADBLOCKER" apply
} >> "$LOG" 2>&1