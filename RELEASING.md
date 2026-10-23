# Releasing

Releases are built by [`.github/workflows/release.yml`](.github/workflows/release.yml)
when a `v*` tag is pushed. It runs the tests, builds binaries for macOS
(Apple Silicon and Intel), Windows (x64), and Linux (x64), and publishes a
GitHub Release with an archive and a `.sha256` file for each.

## Cut a release

1. Bump `version` in `Cargo.toml` and run `cargo build` so `Cargo.lock`
   follows.
2. Commit, then tag and push. The tag must be `v` plus the Cargo version; the
   workflow refuses a mismatch.

   ```bash
   git tag v0.2.0
   git push origin master v0.2.0
   ```

3. Wait for the Release workflow to finish and check the release page.

## Update the package managers

These are optional and each is a separate repository or PR, so nothing here
is published automatically.

- **Homebrew** — create a tap repo named `homebrew-tap` once, copy
  [`packaging/homebrew/adblocker.rb`](packaging/homebrew/adblocker.rb) into
  its `Formula/` folder, and on each release set `version` and the two
  `sha256` values from the release's `.sha256` files. Users then run
  `brew install ThuoBrian/tap/adblocker`.
- **Scoop** — create a bucket repo once and copy
  [`packaging/scoop/adblocker.json`](packaging/scoop/adblocker.json) into it.
  Its `autoupdate` block lets Scoop's `checkver` tooling bump the version and
  hash on later releases. Users run `scoop bucket add lime <bucket-url>` then
  `scoop install adblocker`.
- **winget** — submit to `microsoft/winget-pkgs` with
  [`wingetcreate`](https://github.com/microsoft/winget-create):
  `wingetcreate new <windows-zip-url>` for the first release, then
  `wingetcreate update` for later ones.

## Code signing (not set up yet)

Unsigned binaries work, but an unsigned `.exe` that edits the hosts file is
likely to trigger SmartScreen warnings and Defender heuristics, and macOS
Gatekeeper quarantines unsigned downloads (Homebrew installs avoid this).

- **Windows**: buy an OV/EV code-signing certificate (or use Azure Trusted
  Signing), store it as repository secrets, and add a `signtool sign` step
  after the Windows build in the release workflow.
- **macOS**: join the Apple Developer Program, then sign with `codesign` and
  notarize with `xcrun notarytool` in the macOS build steps.
