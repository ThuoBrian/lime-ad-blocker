set windows-shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-Command"]

default:
    @just --list

# Build the binary
build:
    cargo build

# Install the binary to ~/.cargo/bin
install:
    cargo install --path .

# Download and cache the blocklist (runs as your normal user)
update:
    adblocker update

# Write the cached blocklist into /etc/hosts (needs sudo)
[unix]
apply:
    sudo adblocker apply

# Write the cached blocklist into the hosts file (run from an Administrator terminal)
[windows]
apply:
    adblocker apply

# Remove the ad-blocker block from /etc/hosts (needs sudo)
[unix]
disable:
    sudo adblocker disable

# Remove the ad-blocker block from the hosts file (run from an Administrator terminal)
[windows]
disable:
    adblocker disable

# Turn blocking off for a while, e.g. `just pause 15m` (needs sudo)
[unix]
pause duration:
    sudo adblocker pause {{duration}}

# Turn blocking off for a while, e.g. `just pause 15m` (run from an Administrator terminal)
[windows]
pause duration:
    adblocker pause {{duration}}

# Undo the last change to the hosts file (needs sudo)
[unix]
restore:
    sudo adblocker restore

# Undo the last change to the hosts file (run from an Administrator terminal)
[windows]
restore:
    adblocker restore

# Show current status
status:
    adblocker status

# Explain whether and why a domain is blocked
check domain:
    adblocker check {{domain}}

# Install the daily refresh (launchd on macOS)
[unix]
schedule:
    sh scripts/install-schedule.sh

# Install the daily refresh (Task Scheduler; run from an Administrator terminal)
[windows]
schedule:
    powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/windows/install-schedule.ps1

# Run the test suite
test:
    cargo test

# Format check + clippy with warnings as errors
lint:
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
