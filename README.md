<div align="center">
  <img src="assets/logo.svg" width="120" alt="Lime: a lime slice crossing out an ad tile" />
</div>

# Lime

A command-line ad blocker for macOS, Windows, and Linux. It blocks ads and
trackers in every browser and app on your computer, not just one, by managing
the system hosts file.

No GUI, no background services. One Rust binary (called `adblocker`), with an
optional [Justfile](https://github.com/casey/just) for shortcuts.

## How it works

Before your computer asks a DNS server where a website lives, it checks the
hosts file. If that file says `0.0.0.0 ads.example.com`, every request for
`ads.example.com` goes nowhere.

Lime downloads the [StevenBlack unified hosts](https://github.com/StevenBlack/hosts)
list (about 71,000 ad and tracker domains), checks every line, and writes the
domains into your hosts file between two markers:

```text
# BEGIN AD-BLOCKER
0.0.0.0 ads.example.com
0.0.0.0 tracker.example.org
...
# END AD-BLOCKER
```

Everything outside the markers is left alone. The work is split in two:
downloading and checking the list runs as your normal user, and only the step
that writes the hosts file needs admin rights.

## Install

**Prebuilt:** binaries for macOS, Windows, and Linux are attached to each
[GitHub Release](https://github.com/ThuoBrian/lime-ad-blocker/releases) (the
first one hasn't been published yet). Download the archive for your platform,
check it against its `.sha256` file, and put `adblocker` (or `adblocker.exe`)
somewhere on your `PATH`.

**From source** (needs [Rust](https://www.rust-lang.org/) 1.88 or newer):

```bash
git clone https://github.com/ThuoBrian/lime-ad-blocker.git
cd lime-ad-blocker
cargo install --path .      # or: just install
```

This puts the binary in `~/.cargo/bin` (`%USERPROFILE%\.cargo\bin` on
Windows). Check it's on your `PATH` with `adblocker --version`.

## Quick start

Some commands change the hosts file and need admin rights. On macOS and Linux,
put `sudo` in front of them. On Windows, run them from an **Administrator**
terminal (right-click Terminal, then *Run as administrator*). If you've turned
on *Sudo for Windows* (Settings → System → For developers), `sudo` works there
too. The examples below use the macOS and Linux form.

**1. Download the blocklist.** Normal terminal, no admin needed:

```bash
adblocker update
```

```text
Updated: 71491 domains from 1 source(s) (6825 rejected).
```

Lime fetches the list over HTTPS, drops any line that isn't a valid
`0.0.0.0 domain` entry, removes anything you've allowlisted or that's
protected (see [What Lime never blocks](#what-lime-never-blocks)), and saves
the result as a cache. Nothing on your system has changed yet.

**2. Turn blocking on.** Needs admin:

```bash
sudo adblocker apply
```

Lime checks the cache again, backs up your current hosts file, writes the
block between the markers, and flushes the DNS cache so it takes effect right
away.

**3. Check it's working.**

```bash
adblocker status
```

You should see `Blocking: on` and about 71,000 domains in the hosts file. To
test a single domain, `adblocker check doubleclick.net` tells you whether it's
blocked and why.

That's it. To keep the list fresh, set up the
[daily refresh](#daily-refresh).

## When a site breaks

Find out why a domain is blocked:

```bash
adblocker check cdn.some-site.example.com
```

It shows which source lists the domain and whether it's on your allowlist, a
custom block, or protected. If it's blocking something you need, allowlist it
and re-apply:

```bash
adblocker allow some-site.example.com
sudo adblocker apply
```

Allowing a domain also allows its subdomains, so `allow example.com` unblocks
`ads.example.com` too. `allow` rebuilds the cache from the last download, so
you don't need another `update`.

To switch blocking off for a while instead, say to finish a checkout:

```bash
sudo adblocker pause 15m
```

Blocking turns back on when the time is up, or as soon as you press Ctrl-C or
close the terminal. Durations use `s`, `m`, or `h`, up to `24h`.

## Managing your lists

```bash
adblocker list                               # sources, allowlist, custom blocks
adblocker block extra-tracker.example.com    # block a domain the sources miss
adblocker unblock extra-tracker.example.com
adblocker unallow some-site.example.com
```

Like `allow`, these rebuild the cache straight away, so follow them with
`sudo adblocker apply`.

## Undo and uninstall

```bash
sudo adblocker disable     # remove Lime's block, keep your other entries
sudo adblocker restore     # undo the last change; run it again to redo
```

Before every change (`apply`, `disable`, `pause`, `restore`), Lime copies the
hosts file to `hosts.adblocker-backup` in the same folder. `restore` swaps that
copy back in. That also gets you out of trouble if something else mangles the
markers, which makes Lime refuse to edit the file.

To remove Lime completely:

```bash
adblocker uninstall               # shows what would be removed
sudo adblocker uninstall --yes    # removes the block, the backup, and Lime's data
```

`uninstall` only deletes files it knows it created, then tells you what's left
to remove yourself: the daily refresh and the binary.

## Keeping the list fresh

`update` keeps you safe from a bad download in two ways:

- If a source can't be downloaded, Lime uses that source's last good copy and
  prints a warning.
- If the new list is less than half the size of the current one, `update`
  refuses it, because that usually means the upstream list is broken. Run
  `adblocker update --force` if the drop is expected, for example after
  removing a source.

### Daily refresh

Each platform has its own way to run `update` then `apply` every day at 04:17.
In each case the log keeps only its last 1,000 lines.

**macOS** uses a LaunchAgent:

```bash
sh scripts/install-schedule.sh
```

The script installs the job and prints a one-time `sudo` command that creates a
sudoers entry allowing only `adblocker apply` and `adblocker disable` to run
without a password. Run that command, then test the job:

```bash
launchctl start local.adblocker.update
tail -20 ~/Library/Application\ Support/adblocker/refresh.log
```

The job runs in your user session, so if the Mac was asleep at 04:17 it runs
on wake. Remove it with `sh scripts/uninstall-schedule.sh` and
`sudo rm /etc/sudoers.d/adblocker`.

**Windows** uses Task Scheduler. From an Administrator PowerShell in the repo:

```powershell
just schedule
# or: powershell -ExecutionPolicy Bypass -File scripts\windows\install-schedule.ps1
```

The `adblocker-update` task runs as you, with highest privileges, while you're
logged on. If the PC was off at 04:17, it runs at the next chance. No password
is stored. Test it:

```powershell
Start-ScheduledTask -TaskName adblocker-update
Get-Content "$env:APPDATA\adblocker\refresh.log" -Tail 20
```

Remove it with `scripts\windows\uninstall-schedule.ps1` (as Administrator).

**Linux** has no installer yet. Add a root cron entry such as:

```cron
17 4 * * * runuser -u YOUR_USER -- /home/YOUR_USER/.cargo/bin/adblocker update && /home/YOUR_USER/.cargo/bin/adblocker apply
```

**What the scheduled job can and can't do.** On macOS and Windows the job
applies a cache that your normal account can write, using admin rights it
doesn't ask for each time. That sounds risky, but `apply` only ever writes
valid `0.0.0.0 <domain>` lines and never touches protected domains. The worst
someone with access to your account could do is get domains blocked that you
didn't choose. Don't let other software replace the `adblocker` binary itself.

## Platform notes

| | macOS | Windows | Linux |
| --- | --- | --- | --- |
| Hosts file | `/etc/hosts` | `C:\Windows\System32\drivers\etc\hosts` | `/etc/hosts` |
| Lime's files | `~/Library/Application Support/adblocker/` | `%APPDATA%\adblocker\` | `~/.config/adblocker/` |
| Admin rights | `sudo` | Administrator terminal | `sudo` |
| DNS flush | `dscacheutil` + `mDNSResponder` | `ipconfig /flushdns` | `resolvectl flush-caches` |

**Windows:**

- Run `apply` as the same Windows account you used for `update`, because the
  cache lives under that account's `%APPDATA%`. If you have to elevate as a
  different account, set `ADBLOCKER_DIR` to the first account's
  `%APPDATA%\adblocker` in the elevated terminal.
- Lime keeps the hosts file's Windows (CRLF) line endings.
- Microsoft Defender flags hosts files that redirect certain Microsoft domains
  (`SettingsModifier:Win32/HostsFileHijack`) and may revert them. Lime never
  blocks Windows Update, activation, Microsoft sign-in, or Defender's update
  domains. If Defender still flags the file, check for a Microsoft domain you
  added with `adblocker block`.

**Linux:**

- Most distros reset `HOME` under `sudo`, so Lime looks up your real home in
  `/etc/passwd` and uses the same files `update` wrote. `$XDG_CONFIG_HOME` is
  respected.
- The DNS flush only runs if systemd-resolved is present. Without it there's
  usually no local cache to flush.

## Configuration

Lime creates `config.toml` in its folder (see the table above) on first run.
[`config.example.toml`](config.example.toml) shows the format. It holds:

- `sources`: blocklist URLs. Each must be `https://` and serve a hosts-format
  file.
- `allowlist`: domains (and their subdomains) never to block. Managed with
  `allow` and `unallow`.
- `custom_blocks`: extra domains to block. Managed with `block` and `unblock`.

Set `ADBLOCKER_DIR` to keep Lime's files somewhere else. Besides the config,
the folder holds `blocklist.cache` and `sources/*.list`, the last good copy of
each source.

### What Lime never blocks

Some domains are protected because blocking them breaks the system: Apple
software update, activation, App Store and iCloud, and Microsoft's Windows
Update, activation, sign-in, and Defender domains. Each entry also covers its
subdomains. The host of every source in your config is protected too, so a
list can't block its own updates. The full list is `PROTECTED_DOMAINS` in
[`src/validate.rs`](src/validate.rs).

## Command reference

| Command | Admin | What it does |
| --- | --- | --- |
| `update [--force]` | | Download, check, and cache the blocklist |
| `apply` | yes | Write the cached blocklist into the hosts file |
| `disable` | yes | Remove Lime's block from the hosts file |
| `pause <duration>` | yes | Turn blocking off for a while, e.g. `15m` |
| `restore` | yes | Undo the last change to the hosts file |
| `status` | | Blocking on/off, domain counts, last update, sources |
| `check <domain>` | | Whether and why a domain is blocked |
| `list` | | Your sources, allowlist, and custom blocks |
| `allow` / `unallow <domain>` | | Add or remove an allowlist entry |
| `block` / `unblock <domain>` | | Add or remove a custom block |
| `uninstall [--yes]` | with `--yes` | Remove the block, the backup, and Lime's data |

With `just` installed, `just update`, `just apply`, `just disable`,
`just status`, `just pause 15m`, `just restore`, and `just check <domain>` do
the same, adding `sudo` on macOS and Linux where needed. Run `just` to see them
all.

## Limitations

A hosts file blocks whole domains, so it can't touch ads served from the same
domain as the content you want. YouTube, most streaming services, and most
social platforms serve ads from their own domains. It also can't block
hostnames that JavaScript generates at runtime, or popunders opened with
`window.open`. Run Lime alongside a browser content blocker such as
[uBlock Origin](https://github.com/gorhill/uBlock); it's a complement, not a
replacement.

Lime is the whole-machine layer: every app, about 71,000 known ad and tracker
domains. uBlock Origin is the in-browser layer that can also block scripts,
URL paths, and popups. You need both because a hosts file works below the
browser and can't see same-domain or JavaScript-injected ads.

A hosts entry also only blocks the exact name. `0.0.0.0 ads.example.com`
doesn't block `x.ads.example.com` unless that name is listed too, because
hosts files have no wildcards.

### Setups that may skip the hosts file

Some software resolves names without asking the operating system, so the
hosts file never comes into play. Depending on version and settings, this
includes some VPN clients, iCloud Private Relay, and browsers set to
"DNS over HTTPS only" or "secure DNS" modes. These haven't been tested one by
one. To check your own setup, run `adblocker check <domain>` on a domain that
should be blocked, then visit it in the browser or app you care about. If it
loads, that software is going around the hosts file.

### Setting up uBlock Origin for the cases Lime can't handle

1. **Install uBlock Origin** in your browser from
   [github.com/gorhill/uBlock](https://github.com/gorhill/uBlock) (or your
   browser's add-on store). Use uBlock *Origin*, not the unrelated "uBlock"
   extension.

2. **Enable the popups and annoyances filter lists.** Open the uBlock Origin
   dashboard (click its icon, then the gears / ⚙️ icon) → **Filter lists** →
   **Annoyances**, and turn on **Popups** and **AdGuard Annoyances**. Click
   **Apply changes**. This catches `window.open` popunders and on-site
   nuisances that no hosts file can see.

3. **Block the ad-injection script on a specific site.** When a site serves
   both content and ads from the same domain (so blocking the domain would
   break the content), block just the ad script's path instead. In the
   dashboard → **My filters**, add a line like:

   ```text
   ||example.com/assets/ads.js$script
   ```

   This is a *path-level* block: it kills the ad script without touching the
   content on the same host. A hosts file can't do this; only a browser
   extension can.

4. **Catch what slips through.** If a popunder still opens, uBlock Origin
   logs the blocked or attempted URL in its dashboard → **Logger**. If the
   popunder lives on a *separate* ad domain, you can add it either to
   uBlock's **My filters** or to Lime:

   ```bash
   adblocker block that-ad-domain.example
   sudo adblocker apply
   ```

   If it lives on the *same* domain as the content, keep the block in
   uBlock Origin (step 3), not in Lime.

### A concrete example

Streaming embed sites (e.g. `vsembed.ru`) are the exact case a hosts file
can't touch: the video and the ads are served from the same host, the ad
script (`/assets/sbx.js`) is bundled and same-origin, and the popunder URLs
are built at runtime with `window.open` from the current date, so there's no
fixed ad domain to sinkhole. Adding the site to Lime would break the video.
The fix lives entirely in uBlock Origin: block the script path
(`||vsembed.ru/assets/sbx.js$script`) and enable the Popups filter list.

## Development

```bash
just test   # cargo test
just lint   # cargo fmt --check + cargo clippy -D warnings
just build
```

CI runs fmt, clippy, and the test suite on macOS, Windows, and Linux for every
push and pull request. See [ARCHITECTURE.md](ARCHITECTURE.md) for how the tool
is built and [RELEASING.md](RELEASING.md) for how releases are cut.

## Credits

Blocklist: [StevenBlack/hosts](https://github.com/StevenBlack/hosts), MIT
License. Lime merges, validates, and applies that list; it doesn't
redistribute the list itself.

## License

MIT. See [LICENSE](LICENSE).
