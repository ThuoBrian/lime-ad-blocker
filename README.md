# adblocker

A simple command-line ad blocker for macOS that manages `/etc/hosts`.
Downloads the [StevenBlack unified hosts](https://github.com/StevenBlack/hosts)
blocklist, validates it, and writes it into `/etc/hosts` between markers so ad
and tracker domains resolve to `0.0.0.0`. This blocks ads across **every**
browser and app on your Mac, not just one.

No GUI, no background services — a single Rust binary driven by a
[Justfile](https://github.com/casey/just).

## Requirements

- macOS (tested on Sequoia 15.7)
- [Rust](https://www.rust-lang.org/) (stable, via rustup)
- [`just`](https://github.com/casey/just) — install with `brew install just`

## Install

```bash
git clone <this-repo> adblocker
cd adblocker
just install
```

`just install` runs `cargo install --path .`, putting the `adblocker` binary
in `~/.cargo/bin` (make sure that's on your `PATH`).

## Usage

The four main commands:

```bash
# Download, validate, and cache the blocklist. Runs as your normal user.
just update

# Write the cached blocklist into /etc/hosts. Needs sudo.
just apply

# Remove the ad-blocker block from /etc/hosts. Needs sudo.
just disable

# Show whether blocking is on, the domain count, and your sources.
just status
```

You can also call the binary directly:

```bash
adblocker update
sudo adblocker apply
sudo adblocker disable
adblocker status
```

### Fine-tuning

```bash
# Never block a domain (e.g. one you rely on).
adblocker allow some-site.example.com

# Block a domain that isn't in the default list.
adblocker block extra-tracker.example.com
```

These edit `~/Library/Application Support/adblocker/config.toml`. Run
`just update` then `just apply` for the change to take effect.

## Verify it works

After `just apply`:

```bash
# The block should be between the markers.
grep -c "0.0.0.0" /etc/hosts

# A blocked domain should resolve to 0.0.0.0.
dscacheutil -q host -a name some-ad-domain.example.com

# A protected domain (Apple update) should still resolve correctly.
dig swscan.apple.com
```

## Undo everything

```bash
# Remove the ad-blocker block, leaving your original /etc/hosts entries intact.
just disable
```

If anything goes wrong, the original `/etc/hosts` is backed up before the
first change:

```bash
sudo cp /etc/hosts.adblocker-backup /etc/hosts
sudo dscacheutil -flushcache
sudo killall -HUP mDNSResponder
```

## Configuration

Copy [`config.example.toml`](config.example.toml) to
`~/Library/Application Support/adblocker/config.toml` (or let `adblocker`
create it for you on first run) to change the blocklist `sources`, add an
`allowlist`, or add `custom_blocks`.

## Development

```bash
just test   # cargo test
just lint   # cargo fmt --check + cargo clippy -D warnings
just build
```

See [ARCHITECTURE.md](ARCHITECTURE.md) for how the tool is built.

## Limitations

A hosts file blocks whole domains, so it **cannot** block ads served from the
same domain as the content you want — YouTube, many streaming services, and
most social platforms serve ads from their own domains. Use `adblocker`
**alongside** a browser content blocker such as
[uBlock Origin](https://github.com/gorhill/uBlock); it is a complement, not a
replacement.

## Credits

Blocklist: [StevenBlack/hosts](https://github.com/StevenBlack/hosts) — MIT
License. This project merges, validates, and applies that list; it does not
redistribute the list itself.

## License

MIT