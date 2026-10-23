#!/bin/sh
# Daily refresh: download + cache the blocklist, then apply it to /etc/hosts.
# `apply` elevates via a passwordless sudoers entry for the adblocker binary
# only (see scripts/install-schedule.sh). Invoked by the
# local.adblocker.update LaunchAgent, which passes the binary's path as $1
# (defaulting to ~/.cargo/bin/adblocker).
set -e

# Resolve the invoking user's home even when launchd runs us with a minimal
# environment. SUDO_USER is set when elevated; otherwise fall back to $HOME.
USER_HOME="$HOME"
if [ -n "$SUDO_USER" ] && [ "$SUDO_USER" != "$USER" ]; then
    # Ask Directory Services rather than `eval` an environment value.
    USER_HOME="$(dscl . -read "/Users/$SUDO_USER" NFSHomeDirectory | sed 's/^NFSHomeDirectory: //')"
fi

export PATH="$USER_HOME/.cargo/bin:/usr/local/bin:/usr/bin:/bin"

APP_DIR="$USER_HOME/Library/Application Support/adblocker"
LOG="$APP_DIR/refresh.log"
ADBLOCKER="${1:-$USER_HOME/.cargo/bin/adblocker}"

mkdir -p "$APP_DIR"

# Keep the log to its most recent lines (a few months of daily runs) however
# this run ends.
trim_log() {
    tail -n 1000 "$LOG" > "$LOG.tmp" && mv "$LOG.tmp" "$LOG"
}
trap trim_log EXIT

{
    echo "=== $(date '+%Y-%m-%d %H:%M:%S') ==="
    "$ADBLOCKER" update
    sudo -n "$ADBLOCKER" apply
} >> "$LOG" 2>&1