<div align="center">
  <img src="assets/logo.svg" width="120" alt="Lime: a lime slice crossing out an ad tile" />
</div>

# Lime

A command-line ad blocker for macOS, Windows, and Linux that manages the
system hosts file. It downloads the
[StevenBlack unified hosts](https://github.com/StevenBlack/hosts) blocklist,
validates it, and writes it into the hosts file between markers so ad and
tracker domains resolve to `0.0.0.0`. That blocks ads in every browser and
app on your computer, not just one.

No GUI, no background services. One Rust binary (called `adblocker`), driven
by a [Justfile](https://github.com/casey/just).

## Requirements

- macOS (tested on Sequoia 15.7), Windows 10/11, or Linux
- To build from source: [Rust](https://www.rust-lang.org/) 1.88+ (stable, via
  rustup)
- Optional: [`just`](https://github.com/casey/just) for the shortcuts below
  (`brew install just` on macOS, `winget install Casey.Just` on Windows)

## Install

**Prebuilt:** download the archive for your platform from
[Releases](https://github.com/ThuoBrian/lime-ad-blocker/releases), check it
against its `.sha256` file, and put `adblocker` (or `adblocker.exe`) somewhere
on your `PATH`.

**From source:**

```bash
git clone https://github.com/ThuoBrian/lime-ad-blocker.git
cd lime-ad-blocker
just install
```

`just install` runs `cargo install --path .`, putting the `adblocker` binary
in `~/.cargo/bin` (`%USERPROFILE%\.cargo\bin` on Windows; make sure that's on
your `PATH`).

The sections below use macOS commands. On Windows, see
[Using Lime on Windows](#using-lime-on-windows); on Linux, see
[Using Lime on Linux](#using-lime-on-linux).

## Usage

The four main commands:

```bash
# Download, validate, and cache the blocklist. Runs as your normal user.
just update

# Write the cached blocklist into /etc/hosts. Needs sudo.
just apply

# Remove the ad-blocker block from /etc/hosts. Needs sudo.
just disable

# Show whether blocking is on, domain counts, last update, and your sources.
just status
```

You can also call the binary directly:

```bash
adblocker update
sudo adblocker apply
sudo adblocker disable
adblocker status
```

If a source can't be downloaded, `update` uses that source's last good copy
and prints a warning. If the new list is less than half the size of the
current one, `update` refuses it, because that usually means the upstream
list is broken. Run `adblocker update --force` if the drop is expected (for
example, you removed a source).

### When a site breaks

```bash
# Why is this domain blocked (or not)? Shows which source lists it and
# whether it's allowlisted, a custom block, or protected.
adblocker check cdn.some-site.example.com

# Never block it, or any of its subdomains.
adblocker allow some-site.example.com
sudo adblocker apply

# Or switch blocking off for a while. It turns itself back on when the time
# is up, or when you press Ctrl-C.
sudo adblocker pause 15m
```

`allow` and `block` rebuild the cache from the last download, so you only
need `apply` afterwards, not another `update`. Allowing a domain also allows
its subdomains: `allow example.com` unblocks `ads.example.com` too.

### Managing your lists

```bash
adblocker list                               # sources, allowlist, custom blocks
adblocker block extra-tracker.example.com    # block a domain the sources miss
adblocker unblock extra-tracker.example.com
adblocker unallow some-site.example.com
```

These edit `config.toml` in your app directory (see
[Configuration](#configuration)).

## Scheduled refresh (optional)

To keep the blocklist fresh automatically, install a daily launchd job that
runs `adblocker update` and then `adblocker apply` at 04:17 each day:

```bash
sh scripts/install-schedule.sh
```

This copies the refresh script into
`~/Library/Application Support/adblocker/`, generates and loads the
LaunchAgent, and prints a one-time `sudo` command to create a passwordless
sudoers entry restricted to only `adblocker apply` and `adblocker disable`
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
- The cache that `apply` reads is in your user folder, so anything running
  as you can change it. `apply` still only writes valid `0.0.0.0 <domain>`
  lines and never protected domains, so the worst case is again "domains
  blocked that you didn't choose."
- The refresh log keeps only its last 1,000 lines.
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

## Undo and uninstall

```bash
# Remove the ad-blocker block, leaving your other /etc/hosts entries intact.
just disable
```

Before every change (`apply`, `disable`, `pause`, `restore`), Lime copies the
hosts file to `/etc/hosts.adblocker-backup`. If a change goes wrong, or
something else mangles the markers, undo the last change:

```bash
sudo adblocker restore      # run it again to redo
```

To remove Lime completely:

```bash
adblocker uninstall               # shows what would be removed
sudo adblocker uninstall --yes    # removes the block, the backup, and Lime's data
```

`uninstall` only deletes files it knows it created. It then tells you what's
left to remove yourself: the daily refresh and the binary.

## Using Lime on Windows

Lime works the same way on Windows. The differences:

| | macOS | Windows |
| --- | --- | --- |
| Hosts file | `/etc/hosts` | `C:\Windows\System32\drivers\etc\hosts` |
| Config and cache | `~/Library/Application Support/adblocker/` | `%APPDATA%\adblocker\` |
| Elevation for `apply`, `disable`, `pause`, `restore` | `sudo` | An Administrator terminal |
| DNS flush | `dscacheutil` + `mDNSResponder` | `ipconfig /flushdns` |
| Daily refresh | launchd | Task Scheduler |

Run `update`, `status`, `check`, `list`, `allow` and `block` in a normal
terminal. Run `apply`, `disable`, `pause` and `restore` from an
**Administrator** terminal (right-click Terminal → *Run as administrator*). If you've turned on *Sudo for Windows* (Settings → System
→ For developers), `sudo adblocker apply` works from a normal terminal too.

```powershell
adblocker update           # normal terminal
adblocker apply            # Administrator terminal
adblocker status
```

Run `apply` and `disable` as the same Windows account you used for `update`.
The cache lives under that account's `%APPDATA%`, so if you elevate with a
different admin account, `apply` won't find it. If you have to elevate as a
different account, set `ADBLOCKER_DIR` to the first account's
`%APPDATA%\adblocker` in the elevated terminal.

Lime keeps the hosts file's existing Windows (CRLF) line endings.

### Daily refresh

From an Administrator PowerShell in the repo:

```powershell
just schedule
# or: powershell -ExecutionPolicy Bypass -File scripts\windows\install-schedule.ps1
```

This registers an `adblocker-update` task that runs `adblocker update` and then
`adblocker apply` daily at 04:17. It runs as you, with highest privileges,
while you're logged on. If the PC was off at 04:17, it runs at the next chance.
No password is stored and no sudoers-style setup is needed. As on macOS, the
elevated task applies a cache that your normal account can write. `apply`
only ever writes valid, non-protected `0.0.0.0` lines, so the worst case is
"domains blocked that you didn't choose." Test it and read the log (it keeps
the last 1,000 lines):

```powershell
Start-ScheduledTask -TaskName adblocker-update
Get-Content "$env:APPDATA\adblocker\refresh.log" -Tail 20
```

Uninstall with `scripts\windows\uninstall-schedule.ps1` (as Administrator).

### Verify and undo on Windows

```powershell
Resolve-DnsName some-ad-domain.example.com   # should return 0.0.0.0
adblocker disable                            # Administrator terminal
adblocker restore                            # Administrator: undo the last change
```

The rolling backup is
`C:\Windows\System32\drivers\etc\hosts.adblocker-backup`.

### Microsoft Defender

Defender watches the hosts file. It flags hosts files that redirect certain
Microsoft domains as `SettingsModifier:Win32/HostsFileHijack` and may quarantine
or revert them. Lime never blocks Windows Update, activation, Microsoft sign-in
or Defender's own update domains (see `PROTECTED_DOMAINS` in
`src/validate.rs`). If Defender still flags the file, it's usually because of a
Microsoft domain you added with `adblocker block`. Remove that entry, or allow
the detection in Windows Security only if you're sure.

## Using Lime on Linux

The commands are the same as on macOS:

```bash
adblocker update
sudo adblocker apply
```

- **Files:** config and cache live in `~/.config/adblocker/` (or
  `$XDG_CONFIG_HOME/adblocker/`). Most distros reset `HOME` under `sudo`, so
  when run with sudo, Lime looks up your real home in `/etc/passwd` and uses
  the same files `update` wrote.
- **DNS:** Lime flushes the cache with `resolvectl flush-caches` when
  systemd-resolved is present. Without it there's usually no local cache to
  flush.
- **Daily refresh:** there's no installer yet. Add a root cron entry such as:

  ```cron
  17 4 * * * runuser -u YOUR_USER -- /home/YOUR_USER/.cargo/bin/adblocker update && /home/YOUR_USER/.cargo/bin/adblocker apply
  ```

## Configuration

Copy [`config.example.toml`](config.example.toml) to `config.toml` in your app
directory, or let `adblocker` create it for you on first run:

| OS | App directory |
| --- | --- |
| macOS | `~/Library/Application Support/adblocker/` |
| Windows | `%APPDATA%\adblocker\` |
| Linux | `~/.config/adblocker/` |

Set `ADBLOCKER_DIR` to use a different directory. Use `config.toml` to change
the blocklist `sources`, add an `allowlist`, or add `custom_blocks`. Sources
must be `https://` URLs. The host of every source is never blocked, so a list
can't block its own updates. The app directory also holds `blocklist.cache`
and `sources/*.list`, the last good copy of each source.

## Development

```bash
just test   # cargo test
just lint   # cargo fmt --check + cargo clippy -D warnings
just build
```

CI runs fmt, clippy and the test suite on macOS, Windows and Linux for every
push and pull request. See [ARCHITECTURE.md](ARCHITECTURE.md) for how the tool
is built and [RELEASING.md](RELEASING.md) for how releases are cut.

## Limitations

A hosts file blocks whole domains, so it can't touch ads served from the same
domain as the content you want. YouTube, most streaming services, and most
social platforms serve ads from their own domains. It also can't block
hostnames that JavaScript generates at runtime, or popunders opened with
`window.open`. Run Lime alongside a browser content blocker such as
[uBlock Origin](https://github.com/gorhill/uBlock); it's a complement, not a
replacement.

Lime is the whole-machine layer: every app, ~71k known ad/tracker domains.
uBlock Origin is the in-browser layer that can also block scripts, URL paths,
and popups. You need both because a hosts file operates below the browser and
can't see same-domain or JavaScript-injected ads.

### Setups that may skip the hosts file

Some software resolves names without asking the operating system, so the
hosts file never comes into play. Depending on version and settings, this
includes some VPN clients, iCloud Private Relay, and browsers set to
"DNS over HTTPS only" or "secure DNS" modes. These haven't been tested one by
one. To check your own setup, run `adblocker check <domain>` on a domain that
should be blocked, then visit it in the browser or app you care about. If it
loads, that software is going around the hosts file.

A hosts entry also only blocks the exact name. `0.0.0.0 ads.example.com` does
not block `x.ads.example.com` unless that name is listed too, because hosts
files have no wildcards.

### Setting up uBlock Origin for the cases Lime can't handle

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
   This is a *path-level* block: it kills the ad script without touching the
   content on the same host. A hosts file cannot do this; only a browser
   extension can.

4. **Catch what slips through.** If a popunder still opens, uBlock Origin
   logs the blocked or attempted URL in its dashboard → **Logger**. If the
   popunder lives on a *separate* ad domain, you can add it either to
   uBlock's **My filters** or to Lime:
   ```bash
   adblocker block that-ad-domain.example
   just update && sudo just apply
   ```
   If it lives on the *same* domain as the content, keep the block in
   uBlock Origin (step 3), not in Lime.

### A concrete example

Streaming embed sites (e.g. `vsembed.ru`) are the exact case a hosts file
cannot touch: the video and the ads are served from the same host, the ad
script (`/assets/sbx.js`) is bundled and same-origin, and the popunder URLs
are built at runtime with `window.open` from the current date, so there is
no fixed ad domain to sinkhole. Adding the site to Lime would break the
video. The fix lives entirely in uBlock Origin: block the script path
(`||vsembed.ru/assets/sbx.js$script`) and enable the Popups filter list.

## Credits

Blocklist: [StevenBlack/hosts](https://github.com/StevenBlack/hosts), MIT
License. Lime merges, validates, and applies that list; it does not
redistribute the list itself.

## License

MIT