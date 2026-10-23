## What this changes

<!-- One or two sentences on what changes and why. Link the issue if there is one: "Fixes #123". -->

## Type of change

- [ ] Bug fix
- [ ] New feature or command
- [ ] Change to validation, protected domains, or the hosts-file write path
- [ ] Docs only
- [ ] Build, CI, or packaging

## Checks

- [ ] `just lint` passes (fmt + clippy with `-D warnings`)
- [ ] `just test` passes
- [ ] New behavior has tests, and they don't touch the real hosts file, the network, or my app folder
- [ ] README.md / ARCHITECTURE.md updated if behavior or design changed
- [ ] No new dependencies, or the reason for one is explained below

## Tested on

<!-- CI covers automated tests. Tick where you also ran it by hand. -->

- [ ] macOS (version: )
- [ ] Windows (version: )
- [ ] Linux (distro and version: )

**Admin commands run against a real hosts file** (`apply`, `disable`, `pause`, `restore`, `uninstall --yes`), if this PR touches them:

<!-- Which commands, on which OS, and what happened. Write "not applicable" if the PR doesn't touch them. -->

## Notes for the reviewer

<!-- Anything worth knowing: trade-offs, things you weren't sure about, follow-ups left for later. Delete this section if there's nothing. -->
