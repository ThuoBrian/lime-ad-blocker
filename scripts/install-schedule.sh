#!/bin/sh
# Install the daily adblocker refresh schedule (launchd) for the current user.
#
#   sh scripts/install-schedule.sh
#
# What it does:
#   1. Copies refresh-and-apply.sh into ~/Library/Application Support/adblocker/
#   2. Generates ~/Library/LaunchAgents/local.adblocker.update.plist with your
#      real home path, scheduled daily at 04:17.
#   3. Loads the LaunchAgent.
#   4. Prints the passwordless-sudoers command to run yourself (it needs your
#      password, so this script does not run it).
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
HOME_DIR="$HOME"
# Wherever it was installed (cargo, Homebrew, or a release download on PATH).
ADBLOCKER="$(command -v adblocker || echo "$HOME_DIR/.cargo/bin/adblocker")"
APP_DIR="$HOME_DIR/Library/Application Support/adblocker"
LAUNCH_DIR="$HOME_DIR/Library/LaunchAgents"
LABEL="local.adblocker.update"

if [ ! -x "$ADBLOCKER" ]; then
    echo "error: adblocker not found on PATH or in ~/.cargo/bin. Run 'just install' first." >&2
    exit 1
fi

echo "Installing adblocker daily refresh for $USER ($HOME_DIR)..."

mkdir -p "$APP_DIR" "$LAUNCH_DIR"

# 1. Refresh script.
cp "$SCRIPT_DIR/refresh-and-apply.sh" "$APP_DIR/refresh-and-apply.sh"
chmod +x "$APP_DIR/refresh-and-apply.sh"

# 2. Generate the plist from the template.
sed -e "s|__HOME__|$HOME_DIR|g" -e "s|__ADBLOCKER__|$ADBLOCKER|g" \
    "$SCRIPT_DIR/local.adblocker.update.plist" \
    > "$LAUNCH_DIR/local.adblocker.update.plist"

# 3. (Re)load the LaunchAgent.
launchctl unload "$LAUNCH_DIR/local.adblocker.update.plist" 2>/dev/null || true
launchctl load "$LAUNCH_DIR/local.adblocker.update.plist"

if launchctl list | grep -q "$LABEL"; then
    echo "Scheduled: $LABEL runs daily at 04:17. Log: $APP_DIR/refresh.log"
else
    echo "error: LaunchAgent did not load." >&2
    exit 1
fi

# 4. Sudoers step — needs your password, so print it for you to run.
USER_NAME="$(id -un)"
cat <<EOF

To let the scheduled job apply changes to /etc/hosts without a password,
run this once (it grants passwordless sudo to ONLY adblocker apply/disable):

  sudo sh -c 'echo "$USER_NAME ALL=(root) NOPASSWD: $ADBLOCKER apply, $ADBLOCKER disable" > /etc/sudoers.d/adblocker && chmod 0440 /etc/sudoers.d/adblocker && visudo -cf /etc/sudoers.d/adblocker'

To test the schedule now:   launchctl start $LABEL
To uninstall:               sh scripts/uninstall-schedule.sh
EOF