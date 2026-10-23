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
apply:
    sudo adblocker apply

# Remove the ad-blocker block from /etc/hosts (needs sudo)
disable:
    sudo adblocker disable

# Show current status
status:
    adblocker status

# Run the test suite
test:
    cargo test

# Format check + clippy with warnings as errors
lint:
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings