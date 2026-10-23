#!/bin/sh
# Uninstall the daily adblocker refresh schedule.
#   sh scripts/uninstall-schedule.sh
#
# Removes the LaunchAgent and the installed refresh script. Leaves the
# sudoers entry in place (remove it separately with sudo if desired) and
# does NOT touch /etc/hosts or the cached blocklist.
set -e

HOME_DIR="$HOME"
LAUNCH_DIR="$HOME_DIR/Library/LaunchAgents"
APP_DIR="$HOME_DIR/Library/Application Support/adblocker"
LABEL="local.adblocker.update"

echo "Uninstalling adblocker daily refresh..."

launchctl unload "$LAUNCH_DIR/local.adblocker.update.plist" 2>/dev/null || true
rm -f "$LAUNCH_DIR/local.adblocker.update.plist"
rm -f "$APP_DIR/refresh-and-apply.sh"

echo "Removed LaunchAgent and refresh script."
echo "To remove the passwordless-sudoers entry as well, run:"
echo "  sudo rm /etc/sudoers.d/adblocker"
echo "Your /etc/hosts block and cached blocklist are left untouched."