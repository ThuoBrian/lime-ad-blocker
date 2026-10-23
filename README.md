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

## Scheduled refresh (optional)

To keep the blocklist fresh automatically, install a daily launchd job that
runs `adblocker update` and then `adblocker apply` at 04:17 each day:

```bash
sh scripts/install-schedule.sh
```

This copies the refresh script into
`~/Library/Application Support/adblocker/`, generates and loads the
LaunchAgent, and prints a one-time `sudo` command to create a passwordless
sudoers entry restricted to **only** `adblocker apply` and `adblocker disable`
(so the unattended job can write `/etc/hosts` without your password). Run
that printed command, then test it:

```bash
launchctl start local.adblocker.update      # run once now
tail -20 ~/Library/Application\ Support/adblocker/refresh.log
```

Notes:

- The LaunchAgent runs in your user session, so it fires on wake/login if the
  Mac was asleep at 04:17.
- The sudoers entry grants passwordless root to that one binary only. The
  `apply` path is constrained to writing `0.0.0.0 <domain>` lines between
  the markers, so the realistic worst case is "block domains of an
  attacker's choosing," not arbitrary execution. Don't let another process
  replace `~/.cargo/bin/adblocker`.
- Uninstall with `sh scripts/uninstall-schedule.sh` (and `sudo rm
  /etc/sudoers.d/adblocker` to remove the sudoers entry). Your `/etc/hosts`
  block and cache are left in place.

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
most social platforms serve ads from their own domains. It also cannot block
ads whose hostnames are generated at runtime by JavaScript, or popunders
opened with `window.open`. Use `adblocker` **alongside** a browser content
blocker such as [uBlock Origin](https://github.com/gorhill/uBlock); it is a
complement, not a replacement.

`adblocker` is the broad, whole-machine layer (every app, ~71k known
ad/tracker domains). uBlock Origin is the in-browser layer that can also
block scripts, URL paths, and popups. You need both because a hosts file
operates below the browser and cannot see same-domain or JavaScript-injected
ads.

### Setting up uBlock Origin for the cases `adblocker` can't handle

1. **Install uBlock Origin** in your browser from
   [github.com/gorhill/uBlock](https://github.com/gorhill/uBlock) (or your
   browser's add-on store). Use uBlock *Origin*, not the unrelated "uBlock"
   extension.

2. **Enable the popups and annoyances filter lists.** Open the uBlock Origin
   dashboard (click its icon → the gears / ⚙️ icon) → **Filter lists** →
   **Annoyances**, and turn on **Popups** and **AdGuard Annoyances**. Click
   **Apply changes**. This generically catches `window.open` popunders and
   on-site nuisances that no hosts file can see.

3. **Block the ad-injection script on a specific site.** When a site serves
   both content and ads from the same domain (so blocking the domain would
   break the content), block just the ad script's path instead. In the
   dashboard → **My filters**, add a line like:
   ```
   ||example.com/assets/ads.js$script
   ```
   This is a *path-level* block — it kills the ad script without touching the
   content on the same host. A hosts file cannot do this; only a browser
   extension can.

4. **Catch what slips through.** If a popunder still opens, uBlock Origin
   logs the blocked or attempted URL in its dashboard → **Logger**. If the
   popunder lives on a *separate* ad domain, you can add it either to
   uBlock's **My filters** or to `adblocker`:
   ```bash
   adblocker block that-ad-domain.example
   just update && sudo just apply
   ```
   If it lives on the *same* domain as the content, keep the block in
   uBlock Origin (step 3), not in `adblocker`.

### A concrete example

Streaming embed sites (e.g. `vsembed.ru`) are the exact case a hosts file
cannot touch: the video and the ads are served from the same host, the ad
script (`/assets/sbx.js`) is bundled and same-origin, and the popunder URLs
are built at runtime with `window.open` from the current date — so there is
no fixed ad domain to sinkhole. Adding the site to `adblocker` would break
the video. The fix lives entirely in uBlock Origin: block the script path
(`||vsembed.ru/assets/sbx.js$script`) and enable the Popups filter list.

## Credits

Blocklist: [StevenBlack/hosts](https://github.com/StevenBlack/hosts) — MIT
License. This project merges, validates, and applies that list; it does not
redistribute the list itself.

## License

MIT