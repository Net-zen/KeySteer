# KeySteer packaging

Do not distribute the bare files from `target/release`. Use the platform script
so users receive the application identity, icon and portable archive expected
by the operating system.

## macOS

KeySteer targets macOS 14 or later so the native ScreenCaptureKit screenshot
API can be used directly without compatibility shims.

Run on a Mac; the host architecture is selected automatically:

```bash
bash packaging/macos/package.sh
```

Pass `aarch64-apple-darwin` or `x86_64-apple-darwin` to build a specific target.
The script builds an optimized binary, creates and signs `KeySteer.app`, embeds
the icon, and produces one versioned ZIP under `dist/<target>/`.

Local builds use an ad-hoc signature. A public release should provide a stable
Developer ID identity so Accessibility and Screen Recording permissions remain
attached to KeySteer across upgrades:

```bash
KEYSTEER_CODESIGN_IDENTITY="Developer ID Application: Example (TEAMID)" \
APPLE_ID="developer@example.com" \
APPLE_TEAM_ID="TEAMID" \
APPLE_APP_PASSWORD="xxxx-xxxx-xxxx-xxxx" \
bash packaging/macos/package.sh
```

`KEYSTEER_NOTARY_PROFILE` may be used instead of the three Apple account
variables when a `notarytool` keychain profile is already configured.

When a user installs `KeySteer.app` manually from the official GitHub Release
and Gatekeeper refuses to open it, move the app to `/Applications` and run:

```bash
sudo xattr -cr /Applications/KeySteer.app
```

This recursively clears the app bundle's extended attributes and must only be
used for an archive verified to come from the official KeySteer release page.

## Windows

Run in PowerShell; the host architecture is selected automatically:

```powershell
.\packaging\windows\package.ps1
```

Pass `x86_64-pc-windows-msvc` or `aarch64-pc-windows-msvc` for a specific
target. The output is a portable ZIP containing the icon-bearing, GUI-subsystem
`KeySteer.exe` under `dist/<target>/`.

### Benchmark the unpacked Windows package

After packaging, benchmark the actual distributed executable (not
`target/release`) with the comparison runner wrapper:

```powershell
.\tools\benchmark-windows-dist.ps1 x86_64-pc-windows-msvc
```

The script defaults to the validated `Alt+E`, then `;` left-click scenario and
writes the runner CSV plus a JSON sidecar under `dist/<target>/benchmarks/`.
Pass `-SetupKey`, `-Key`, or `-Observe` to match another configuration;
`-Executable` supports an archive extracted elsewhere, and `-KeepRunning`
leaves the launched KeySteer instance open. Its recorded `process_create_ms`
measures `CreateProcess` completion only; the fixed `-StartupMs` delay is the
application-settle window, not a readiness metric.

## GitHub releases

CI calls the platform packaging scripts through two workflows:

| Name | File | Responsibility |
| --- | --- | --- |
| Build | `build.yml` | Tag/manual entry, cross or native compilation, Release publication, Homebrew/Scoop synchronization |
| Pages | `pages.yml` | Build and deploy documentation; clean a successful forwarding entry |

Build defaults to `build_mode=cross`; explicitly choose `native` to compile on
Windows 2025 and macOS 26. `platform` accepts windows, macos or all. Publishing
builds every supported platform: four ZIPs for native, six for cross (including
compatible and AVX-512 Windows x64). With publication disabled, only the selected
platforms build. Unselected compilation jobs are skipped without starting runners.
Both modes use `.github/scripts/release.py` and `tools/compose-release-notes.sh`.
Optional macOS signing secrets are documented in `macos/README-cross.md`.

Tag pushes default to cross publication. Tags and manually selected non-default
refs enter the same Build workflow, whose Prepare job dispatches Build once on
the default branch. The default-branch run resolves the selected source commit
and builds directly, retaining the shared cache scope. Deleted tags do not build.
New tags must include this workflow layout; historical tags retain the trigger
definitions stored in their own commits.

A complete cross tag release uses 10 runner jobs: entry, source resolution, four
Windows builds, two macOS builds, publication/manifests and Pages. Manifest
synchronization shares the publication runner; cleanup shares the Pages runner.
The shared script validates archives, composes notes and preserves existing tag
objects. It verifies their commit before changing release assets and creates a
tag only when absent. Cross may replace an existing Release; native rejects
duplicate Releases. Publication runs share one concurrency group, while entry
runs use separate groups to avoid cancelling their actual builds.

After successful compilation, publication, manifest synchronization and Pages
deployment, cleanup removes only the corresponding completed entry run. Since
entries and builds share `build.yml`, cleanup also checks that the entry only
executed Prepare and skipped all remaining jobs. Failures, cancellations and
runs containing actual builds/deployments are retained. Version tags and actual
Build records remain. Cleanup uses `actions: write` and checks the workflow path,
run ID, commit, completed status and job list.

Validate the release and cleanup policies locally with:

```bash
python -B -m unittest discover -s .github/scripts -p 'test_*.py'
```

## Homebrew tap and Scoop bucket

This repository also hosts `Casks/keysteer.rb` and `bucket/keysteer.json`.
Use the explicit Git URL when registering it; no separate repository or token
is required. Installation and upgrade commands are in the project READMEs.

After publication succeeds, the same Build job reads the latest stable
GitHub Release, downloads ZIPs, verifies their SHA256 digests, and commits both
manifests to the default branch with `GITHUB_TOKEN`. Pre-releases do not replace
the stable package version. Use the generator below for manual synchronization.
Branch rules must allow the bot's metadata commit; otherwise
generate and submit the manifests through the normal review process.

For manual generation, save the official release API response as JSON and run:

```bash
python packaging/package-managers/generate.py release.json --assets-dir release-assets
python -m unittest discover -s packaging/package-managers -p 'test_*.py'
```

`--assets-dir` hashes the downloaded archives and checks GitHub's digests when
available. Without it, every selected asset must have a valid API SHA256 digest.
Scoop selects the default AVX2 x64 build with no filename suffix, even when
compatible and AVX-512 packages are present; ARM64 is separate. Exit the installed app before upgrading.

Scoop hooks snapshot user TOML files and `workspace.ksw` to its persist directory
before upgrade/uninstall, then restore them into the new version's directory.
The executable and `keysteer.default.toml` always come from the new package.
Deleted user configs are removed from the next snapshot. `scoop uninstall -p`
purges the retained snapshot; ordinary uninstall allows a later reinstall to
restore it. macOS configuration stays in Application Support across upgrades.
