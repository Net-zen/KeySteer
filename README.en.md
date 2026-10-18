# KeySteer

<p align="center">
  <img src="assets/brand/keysteer-wordmark.webp" alt="KeySteer" width="760">
</p>

<p align="center"><strong>From clicks to split screens, put your workspace on your keyboard.</strong></p>

<h3 align="center">
  <a href="https://github.com/dccif/KeySteer/blob/main/README.md">简体中文</a> · <strong>English</strong>
</h3>

KeySteer brings native keyboard control to Windows and macOS. Move the pointer, type labels to target controls, split and tile windows, and save layouts for reuse.

[Download](https://github.com/dccif/KeySteer/releases/latest) · [Documentation](https://dccif.github.io/KeySteer/en/guide/getting-started) · [Try the simulator](https://dccif.github.io/KeySteer/en/editor/)

**Contents** · [Install](#installation) · [Get started](#get-started) · [Features & demos](#features) · [Configure](#configuration) · [Develop](#development-and-packaging) · [License](#license-and-copyright)

## Installation

Download the ZIP matching your operating system and CPU architecture from [GitHub Releases](https://github.com/dccif/KeySteer/releases/latest):

| System | After extracting |
| --- | --- |
| Windows 10/11 · x64, ARM64 | Run `KeySteer.exe` |
| macOS 14+ · Apple Silicon, Intel | Move `KeySteer.app` to `/Applications` and open it |

On macOS, grant keyboard and mouse control permission before use. UI Hint visual recognition also needs screen recording permission. For authorization or trouble opening the app, see [macOS setup](https://dccif.github.io/KeySteer/en/guide/macos).

<details>
<summary>Install and upgrade with Scoop or Homebrew</summary>

Exit KeySteer before upgrading.

### Windows: Scoop

```powershell
scoop bucket add keysteer https://github.com/dccif/KeySteer.git
scoop install keysteer/keysteer

# Upgrade
scoop update
scoop update keysteer
```

The x64 package uses AVX2 by default; choose the compatible build from Releases for an older CPU. Scoop upgrades retain user configurations and `workspace.ksw` layouts. When migrating from a manual installation, copy these files into the Scoop installation directory.

### macOS: Homebrew

```bash
brew tap dccif/keysteer https://github.com/dccif/KeySteer.git
brew install --cask dccif/keysteer/keysteer

# Upgrade
brew update
brew upgrade --cask dccif/keysteer/keysteer
```

Selects the Apple Silicon or Intel package automatically and retains user configuration during upgrades. Complete the permission setup above after installing.

</details>

## Get started

No configuration file is needed. Once launched, KeySteer waits in the Windows tray or macOS status bar while you type as usual.

### Make your first click

1. Press **left `Alt+E`** on Windows / **`Command+E`** on macOS to enter Normal, then release the entry keys.
2. Hold `H/J/K/L` to move left / down / up / right.
3. Press `;` for a left click.
4. Press `Esc` to return to idle and resume typing.

### Move a window next

1. Place the pointer over a window, press **`Alt+W`** on Windows / **`Option+W`** on macOS, then release the entry keys.
2. Use `H/J/K/L` to move the window.
3. Press `Z` to undo an adjustment, or `Q` to finish and keep the result.

In Normal or Window, press **`?` (Shift+/)** to show available keys; press it again to hide them. Uppercase letters are for readability; hold Shift only when `Shift+` is written.

In the documentation, `Primary` means left `Alt` on Windows and `Command` on macOS by default. Change it in `[key_aliases]` if needed.

For more practice, see [Getting started](https://dccif.github.io/KeySteer/en/guide/getting-started). Resizing, centring, multiple displays, and audio controls are covered in the [window management guide](https://dccif.github.io/KeySteer/en/window-management/).

<details>
<summary>Mouse key reference</summary>

### Default keys

Use these in Normal.

| Key | Action |
| --- | --- |
| `Caps Lock` / left `Shift` / `V` or `B` | Hold for precision / slow / fast movement; also affects scrolling |
| `M` / `,` | Scroll down / up |
| `'` / right `Shift` | Right / middle click |
| Hold `N`, then tap `;` | Latch the left button to drag; tap `N` alone to release |
| `Primary+S` | Move the pointer to the next display |
| `Q` or `Esc` | Return to idle |

See [Normal mode](https://dccif.github.io/KeySteer/en/modes/normal) for all keys and dragging options.

</details>

## Features

Learn these as you need them. Use targeting keys from Normal and `A/E/T/R` from Window.

| What you want to do | Key | Guide |
| --- | --- | --- |
| Reach a screen region with two keys | `G` | [Grid](https://dccif.github.io/KeySteer/en/modes/grid) |
| Subdivide regions to reach a small target | `F` | [Recursive Grid](https://dccif.github.io/KeySteer/en/modes/recursive-grid) |
| Type labels to select controls; search text or pinyin initials, copy information, or pick colors | `Primary+F` | [UI Hint](https://dccif.github.io/KeySteer/en/modes/ui-hint) |
| Put a window on half a screen, then adjust its ratio | `A` | [Quick](https://dccif.github.io/KeySteer/en/window-management/quick) |
| Tile several windows immediately, then adjust regions | `E` | [Editor](https://dccif.github.io/KeySteer/en/window-management/editor) |
| Collect windows into persistent tab groups | `T` | [Tabs](https://dccif.github.io/KeySteer/en/window-management/tabs) |
| Restore saved layouts and groups with current windows | `R` | [Restore](https://dccif.github.io/KeySteer/en/window-management/restore) |

In Editor or Tabs, press `Ctrl+S`, enter a note or leave it empty, then press `Enter` to save a preset. From a Window submode, press `Q` to return to Window, then again to return to idle.

### Video demonstrations

<details>
<summary>Pointer and interface targeting: Normal, Grid, Recursive Grid, UI Hint</summary>

### Normal

https://github.com/user-attachments/assets/10c990c4-903c-49fb-b8d7-5441430d3496

### Grid

https://github.com/user-attachments/assets/4ecb749e-d770-43c1-907a-e55a4144a9ca

### Recursive Grid

https://github.com/user-attachments/assets/cb399755-5cde-40a0-ba64-d00c7e581cc6

### UI Hint

https://github.com/user-attachments/assets/71efcae3-eb11-46d0-aba4-0a5df5e9c80c

### UI Hint search: initials, copying, and color picking

https://github.com/user-attachments/assets/84d0872d-81f6-4e42-aa7f-767061cef646

</details>

<details>
<summary>Window management: moving, splits, tiling, tabs, and saving</summary>

### Window: control your workspace

https://github.com/user-attachments/assets/a1f38691-f6cf-45bf-b264-03a1a70cd104

### Quick: press A for split layouts

https://github.com/user-attachments/assets/e9a278cd-a05a-4931-80cc-d2f832142296

### Editor: press E to tile and fine-tune

https://github.com/user-attachments/assets/e21b68e3-f970-4e47-b5a2-fec80720b43b

### Tabs: group windows by application

https://github.com/user-attachments/assets/2c1b894d-7402-4b57-9fd1-da03f5bd81a9

### Save: keep layouts and tab groups

https://github.com/user-attachments/assets/9965dbb6-ed80-4071-b636-563563b83b7d

</details>

<details>
<summary>Key help and visual configuration</summary>

### key_help: keep available actions in view

https://github.com/user-attachments/assets/9c28c826-d465-4d47-b671-7a275c282a76

### Configuration & Simulator: edit on a full keyboard

https://github.com/user-attachments/assets/fbb5f750-fcda-4d3f-92da-1f7667387927

</details>

## Configuration

To change keys, speed, or colors, open **Configuration & Simulator...** from the tray or status menu. This loads your current configuration in the [web editor](https://dccif.github.io/KeySteer/en/editor/) (network access required).

Export it as `keysteer.user.toml`, place it in the data directory, then choose **Reload Configuration** from the menu:

- Windows: beside `KeySteer.exe`.
- macOS `.app`: `~/Library/Application Support/KeySteer/`.

For manual editing, start from the [complete default configuration](https://dccif.github.io/KeySteer/generated/keysteer.default.toml). A custom `bindings` table replaces that mode's entire default key table; keep the movement, exit, and help keys you need.

See the [configuration reference](https://dccif.github.io/KeySteer/en/reference/configuration) for loading rules, command-line diagnostics, and troubleshooting. Action syntax is in [Modes and actions](https://dccif.github.io/KeySteer/en/reference/modes-and-actions).

## Development and packaging

<details>
<summary>Run from source and contribute</summary>

Use the Rust version in [`rust-toolchain.toml`](https://github.com/dccif/KeySteer/blob/main/rust-toolchain.toml). The documentation site uses Node.js 24.

```bash
cargo run
```

See the [development workflow](https://dccif.github.io/KeySteer/en/development/workflow) for tests and documentation commands, and the [packaging guide](https://dccif.github.io/KeySteer/en/development/workflow#platform-packaging) for release archives.

### Performance

Measurement methods and input/rendering constraints are covered in [Safety and performance](https://dccif.github.io/KeySteer/en/development/safety-and-performance).

</details>

## License and copyright

Copyright © 2026 dccif. KeySteer is released under the **GNU General Public License v3.0 or later (GPL-3.0-or-later)**; see [LICENSE](https://github.com/dccif/KeySteer/blob/main/LICENSE) for the full text.
