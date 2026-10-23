# macOS

KeySteer supports macOS 14 and later. After installing, grant keyboard and pointer control permission (formerly Accessibility), then try your first click in [Getting started](/en/guide/getting-started).

## Install and upgrade with Homebrew

```bash
brew tap dccif/keysteer https://github.com/dccif/KeySteer.git
brew install --cask dccif/keysteer/keysteer
```

The cask selects the Apple Silicon or Intel package. Exit KeySteer before upgrading:

```bash
brew update
brew upgrade --cask dccif/keysteer/keysteer
```

User configuration stays in the data directory below across upgrades. Complete the permission steps below after installing.

## Install from a GitHub Release

Download the ZIP for your architecture from the [official GitHub Releases](https://github.com/dccif/KeySteer/releases/latest), extract it, and move `KeySteer.app` to `/Applications`. If Gatekeeper prevents it from opening, first confirm the download source, then run:

```bash
sudo xattr -cr /Applications/KeySteer.app
```

## First-time permissions

1. Open `/Applications/KeySteer.app`.
2. Go to **System Settings → Privacy & Security**, then open the permission for your macOS version:

   - **macOS 27:** Device Control and Data Access.
   - **macOS 14–26:** Accessibility.

3. Allow KeySteer to control your computer.
4. Restart KeySteer and test it with `Command+E`.

If you cannot find the permission, search System Settings for “Device Control” or “Accessibility” and open the app permission list. If KeySteer is missing, click `+` and add `/Applications/KeySteer.app`. Older app messages mentioning “Accessibility permission” refer to the renamed permission on macOS 27.

Visual recognition in `UI Hint` also needs **Screen & System Audio Recording** permission (called **Screen Recording** on older versions), on the same privacy page. Grid and Recursive Grid do not read screen contents, but still need the keyboard and pointer control permission above.

::: tip Missing visual labels?
Check Screen & System Audio Recording permission, then restart KeySteer.
:::

## File locations

The packaged app uses:

```text
~/Library/Application Support/KeySteer/
```

It contains:

- `keysteer.<name>.toml`: configuration files.
- `keysteer.log`: runtime log.
- `keysteer.log.1`, `.2`, `.3`: rotated logs.

## Scroll direction

When you use natural scrolling, vertical inversion is enabled by default. Adjust it in the configuration if needed:

```toml
[platform.macos.scroll]
invert_horizontal = false
invert_vertical = true
```

## Permission problems

If the top status icon appears but keys do nothing:

1. Check that **Device Control and Data Access** (or **Accessibility** on older versions) authorises `KeySteer.app`.
2. Quit and reopen KeySteer.
3. Run `keysteer --doctor` to check whether the keyboard is available.
4. If it persists, inspect `keysteer.log` in the data directory.

::: tip Keys stop working after an upgrade
Remove the old KeySteer entry from the permission list, add `/Applications/KeySteer.app` again, then restart the app.
:::

## Top status icon

While KeySteer is running, its status icon appears at the top right of the screen. Use it to pause control, reload configuration, check for updates, or quit. This menu is where you manage the app; there is no separate Dock icon.
