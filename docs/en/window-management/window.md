# Window: move and control windows

<script setup>
import ModeVideo from '../../.vitepress/components/ModeVideo'
</script>

Move, resize, centre, cycle window states and control audio.

<ModeVideo file="window.mp4" title="Window: move and control windows" description="Move, resize, centre, cycle window states and control audio." />

## How to use it

Entering Window activates and locks the window under the pointer. Moving the pointer elsewhere does not automatically change the target. Type a displayed window number to activate that window and move the pointer to its centre.

| Key | Action in Window |
| --- | --- |
| `H / J / K / L` | Move left / down / up / right; hold for continuous adjustment |
| `S` | Toggle movement and resizing around the centre |
| `C` | Centre the window |
| `D` | Move to the next display |
| `F` / `Shift+F` | Maximize / restore; minimize / restore |
| Number | Select a window by number |
| `Tab / Shift+Tab` | Next / previous window; prefer members of the current tab group |
| `X` | Request that the window close; the app may ask you to save |
| `Z / Shift+Z` | Undo / redo window adjustments |
| `Shift+C` | Restore positions, sizes, and window states from the start of this window session |

By default, numbers cover non-minimized windows on all displays. Closing a window does not renumber the others; re-entering clears gaps. When both 1 and 12 exist, typing 1 briefly waits for more digits (250 ms by default).

Use `screens = "current"` to show only the current display. See [Window configuration](/en/reference/configuration#window-configuration) for more options.

::: tip Use the pointer while arranging windows
Hold `Primary` for Normal movement, scrolling, and clicks. Release it to resume the same window. Each window mode has its own bindings; remapping Normal does not remap them.
:::

## Move with grid targeting

Want to move a window farther in fewer keystrokes? Press `G` for Grid. Window binds `Primary+F` to Recursive Grid, but the default temporary layer gives Normal's full chord precedence and opens UI Hint. Add `primary+f` to `temporary_mode_passthrough_keys` to retain Window's binding. As you select cells, the window center follows the target point within the display's work area.

Backtracking, restarting, and display switching use the targeting mode's usual controls. Finishing returns to Window; `keep` lets you continue selecting. One `Z` undoes the targeting session.

::: info Remap grid entries in Window
These entries belong to `[window.bindings]`. Remapping Normal does not change them. Bare `F` still maximizes/restores. Add the entries yourself if you already use a custom Window binding table.
:::

## Audio controls

These default combinations work in Window, Quick, and Editor. Hold `V` and press the partner key; add `Shift` for system controls.

| Scope | Volume down / up | Toggle mute | Previous / next output device |
| --- | --- | --- | --- |
| Application owning the target window | `V+J / V+K` | `V+M` | `V+H / V+L` |
| System | `Shift+V+J / Shift+V+K` | `Shift+V+M` | `Shift+V+H / Shift+V+L` |

Volume changes by 1% per step. Raising application volume does not automatically unmute it. Multiple windows from the same app may share an audio session.

- **Windows:** application volume needs an available audio session; start playback if none is found. Whether a device change moves an existing stream immediately depends on the application.
- **macOS:** system audio works on macOS 14+; independent application audio needs macOS 14.2+ and System Audio Recording permission. Local processing adds audio latency. Some output devices do not support software volume control.

[Window management overview](/en/window-management/) · [Full configuration reference](/en/reference/configuration)

## Select multiple windows

1. Tap `Ctrl` to start selecting. The current window is selected first.
2. Type `123` for windows 1, 2, and 3. Separate multi-digit numbers with spaces, as in `123 12 23`. Repeat a number to deselect it.
3. Press `Ctrl` or `Enter` to confirm, or move/resize directly to adjust them together. `E` arranges only these windows; `T` combines them into a tab group.

Selected windows show a border and `✓`. Use Backspace to fix input or `Esc` to undo this input edit and return to the original window.

::: tip Clear the selection
`Ctrl+X` clears multi-selection. Entering Quick, Restore, or exiting also clears it and first returns to the original window. On macOS, `Ctrl` means Control.
:::

Merge these entries into your existing tables to customize the keys:

```toml
[window.bindings]
ctrl = "window_multi_select"
"ctrl+x" = "window_multi_clear"

[window.multi_select.bindings]
"ctrl enter" = "window_multi_confirm"
"primary+h" = "arrow_left"
"primary+l" = "arrow_right"
```
