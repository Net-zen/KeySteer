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

CI calls the platform packaging scripts. The native workflow publishes four
platform ZIPs; the cross-build workflow publishes six, including compatible
and AVX-512 Windows x64 builds. Both compose release notes with
`tools/compose-release-notes.sh`. Optional macOS signing secrets are documented
in `.github/workflows/build.yml` and `macos/README-cross.md`.

The workflows have short display names and separate responsibilities:

| Name | File | Responsibility |
| --- | --- | --- |
| Start | `start.yml` | Tag push or manual entry; dispatch Cross on the default branch |
| Cross | `cross-build.yml` | Resolve the selected commit and coordinate the cross-build pipeline |
| Windows | `windows.yml` | Reusable Linux-to-Windows build matrix |
| macOS | `macos.yml` | Reusable Linux-to-macOS build matrix |
| Release | `release.yml` | Validate the archives and publish the GitHub Release |
| Packages | `package-managers.yml` | Update the Homebrew tap and Scoop bucket |
| Pages | `pages.yml` | Build and deploy documentation, then clean the successful Start run |
| Native | `build.yml` | Optional native builds using the same Release workflow |

Use Start when manually selecting another branch or tag; direct Cross runs must
select the default branch. Source checkout remains pinned to the chosen commit.
New tags must include this workflow layout. Historical tags retain the trigger
definitions stored in their own commits.
Reusable workflows keep the caller's default-branch context and existing runner
matrices. A complete tag release uses 11 runner jobs instead of the previous 12:
Start, source resolution, four Windows builds, two macOS builds, Release,
Packages, and Pages. Cleanup is a final step on the Pages runner.

Release preserves existing tag objects and verifies their commit before changing
release assets; it creates a tag only when absent. Cross may replace an existing
Release; Native still rejects duplicate Releases. After successful builds,
publication, manifest synchronization and Pages deployment, cleanup removes only
the corresponding successful Start run. Failures and cancellations retain it;
the version tag and Cross run always remain. Cleanup uses `actions: write` and
checks the entry's workflow path, run ID, commit and completed status.

Validate the release and cleanup policies locally with:

```bash
python -B -m unittest discover -s .github/scripts -p 'test_*.py'
```

## Homebrew tap and Scoop bucket

This repository also hosts `Casks/keysteer.rb` and `bucket/keysteer.json`.
Use the explicit Git URL when registering it; no separate repository or token
is required. Installation and upgrade commands are in the project READMEs.

After either publisher succeeds, `package-managers.yml` reads the latest stable
GitHub Release, downloads ZIPs, verifies their SHA256 digests, and commits both
manifests to the default branch with `GITHUB_TOKEN`. Pre-releases do not replace
the stable package version. The job can also be run manually to synchronize an
existing release. Branch rules must allow the bot's metadata commit; otherwise
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
