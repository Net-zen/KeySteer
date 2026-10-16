# Configuration & Simulator (Beta)

Try changes in the browser before using them on your desktop. Adjust shortcuts, pointer speed, and mode styles, or practise layouts and tab groups with sample windows.

Choose **Configuration & Simulator...** from the app's status menu to import the active configuration and existing workspace automatically. You can also open it below and import files yourself.

<p class="ks-open-simulator"><a href="../simulator" target="_blank" rel="noopener">Open Configuration & Simulator ↗</a></p>

::: info Preview locally, then apply your files
Data stays in your browser and is not uploaded. Website actions change the preview. The desktop app uses your changes after you download the configuration or workspace file and place it in its data directory.
:::

## Recommended workflow

1. Import your TOML file or start from defaults.
2. Change keys and styles while watching the preview.
3. Download `keysteer.<name>.toml` into [KeySteer's data directory](/en/reference/configuration#configuration-file-location).
4. Choose **Reload Configuration** from the status menu.

::: tip If configuration validation fails
Run `keysteer --check -c <file>` for the reason. The simulator previews behavior; the desktop app remains authoritative for complex actions, external commands, and platform permissions.
:::

### Edit layouts and tab templates

Opening from the app menu also imports presets from an existing `workspace.ksw`. Or choose **Import workspace file**. Select a layout number to edit regions; `Ctrl+S` edits its note. **Save and download workspace** saves current edits and preserves other presets.

Place the downloaded `workspace.ksw` in the data directory, then use `Alt+W → R` to read it again. No restart or TOML reload is needed.

See [Configuration](/en/reference/configuration) and [Modes and actions](/en/reference/modes-and-actions) for the full syntax. The simulator opens in a separate page and also works well on a second display.

## Visual window card editing

Select Window, Quick, Editor, Restore, or Tabs, then adjust the card editor:

| What to change | How |
| --- | --- |
| Background, number, app name, title, border | Choose colors and opacity |
| Fonts, sizes, weight, corners, spacing | Edit values alongside the live preview |
| Card positions | Click or drag an anchor, draw a range, adjust inset sliders, or select a preset |

Changes update the preview and exported TOML. Each setting can be reset individually. During actual use, cards still avoid other markers.
