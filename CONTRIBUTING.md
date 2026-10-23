# Contributing to Lime

Thanks for helping out. Lime edits a system file with admin rights, so the bar
for changes is "obviously safe", but the codebase is small and the tests make
it easy to work on without touching your real hosts file.

## Ways to help

- **Test it on your machine.** CI covers macOS, Windows, and Linux against
  temporary files, but the admin commands (`apply`, `disable`, `pause`,
  `restore`, `uninstall --yes`) and the daily-refresh installers still need
  more real-world runs. Reports like "worked on Ubuntu 24.04" or "Defender
  flagged it on Windows 11" are useful.
- **Report bugs.** Open an issue with your OS and version, the command you
  ran, what you expected, and what happened. Include the output of
  `adblocker status` and, if it's about one domain, `adblocker check <domain>`.
- **Suggest protected domains.** If blocking a domain breaks something
  important (updates, sign-in, activation), open an issue with the domain and
  what broke.
- **Pick up an open issue**, or improve the docs.

For anything bigger than a bug fix, open an issue first so we can agree on the
approach before you spend time on it.

## Security issues

Please don't report security problems in a public issue. Use the repository's
**Security** tab → **Report a vulnerability** instead.

## Setting up

You need [Rust](https://www.rust-lang.org/) stable, 1.88 or newer.
[`just`](https://github.com/casey/just) is optional but saves typing.

```bash
git clone https://github.com/ThuoBrian/lime-ad-blocker.git
cd lime-ad-blocker
cargo test
```

CI always uses the latest stable Rust, and new Clippy releases sometimes add
lints. Run `rustup update stable` before you start, so your local checks match
CI.

Read [ARCHITECTURE.md](ARCHITECTURE.md) before changing code. It's short and
explains the module layout, the privilege split, and why things are the way
they are.

## Before you open a pull request

Run the same checks CI runs:

```bash
just lint    # cargo fmt --check + cargo clippy --all-targets -- -D warnings
just test    # cargo test
```

Clippy runs with the `pedantic` group and warnings count as errors, so fix
what it reports rather than silencing it. If an `allow` really is the right
call, put it on the smallest item possible and say why.

## How the code is organized for testing

- **Tests never touch the real hosts file, your app folder, or the network.**
  Integration tests use the `temp_paths` helper in `tests/integration.rs`,
  which builds a `Paths` pointing at a temp folder. `run_update_with` takes a
  fake fetcher, so `update` can be tested offline.
- **Commands that write the hosts file come in two layers.** `run_<name>`
  checks for admin rights and flushes DNS. The `<name>_no_flush` function
  underneath does the actual work, and that's the one tests call.
- **Platform-specific code** goes in `#[cfg(...)]` variants next to each
  other, not in a separate module. Tests that only make sense on one platform
  get the same `#[cfg(...)]`.

To add a command, follow the steps in
[ARCHITECTURE.md](ARCHITECTURE.md#how-to-add-a-new-command).

## Trying your change for real

To run the binary without touching your own config, point it at a scratch
folder:

```bash
ADBLOCKER_DIR=/tmp/lime-test cargo run -- update
ADBLOCKER_DIR=/tmp/lime-test cargo run -- check doubleclick.net
```

On Windows PowerShell, set it with `$env:ADBLOCKER_DIR = "$env:TEMP\lime-test"`.

Testing `apply` or `disable` means editing your real hosts file. Lime backs
it up before every change, and `adblocker restore` undoes the last one. Still,
keep a copy of your own before you start:

```bash
sudo cp /etc/hosts ~/hosts.before-lime-testing
```

On Windows, copy `C:\Windows\System32\drivers\etc\hosts` somewhere safe from
an Administrator terminal.

## What we're careful about

- **The hosts file write path.** Anything that changes what `apply`, `disable`,
  `restore`, or `pause` write needs tests showing the content outside the
  markers is preserved byte for byte, including CRLF line endings.
- **Validation and protected domains.** Changes to `normalize_domain`,
  `parse_hosts_text`, or `PROTECTED_DOMAINS` affect what can end up in a system
  file. Explain the reason in the PR, and for protected domains say what
  breaks without the entry.
- **Dependencies.** Lime has very few on purpose. A new crate needs a reason
  the standard library can't cover. `unsafe` is forbidden in `Cargo.toml`.
- **Privilege.** Downloading and parsing stay unprivileged. Don't move work
  into the admin-only commands unless it needs to be there.

## Commits and pull requests

- Keep each PR to one change. A bug fix and a refactor are two PRs.
- Write commit messages as a short imperative summary (under about 70
  characters), a blank line, then what changed and why. `git log` has plenty
  of examples.
- Update [README.md](README.md) when user-facing behavior changes, and
  [ARCHITECTURE.md](ARCHITECTURE.md) when the design does.
- Fill in the pull request template, especially which platforms you tested
  on.

By contributing, you agree that your contributions are licensed under the
[MIT License](LICENSE).
