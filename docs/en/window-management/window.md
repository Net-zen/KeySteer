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

By default, candidates are **non-minimized windows on the current display**. Existing numbers stay stable where possible: closing a window does not renumber the others, although re-entering may reclaim gaps. Only a prefix that could form a longer number waits, for 250 ms by default.

Need to click something? Hold `Primary` to temporarily use Normal's pointer movement, scrolling, and click bindings. Release it to resume the same target. Each window mode has independent direction bindings; remapping Normal does not automatically remap these modes.

## Move with grid targeting

While Window is in Move state, Grid and Recursive Grid can run above the same locked window session. Defaults are `G` or `Primary+G` for Grid and `Primary+F` for Recursive Grid; bare `F` still maximizes.

Entrances come from Normal's effective bindings. Compilation borrows unoccupied Grid/Recursive Grid entrances after aliases and application overrides, preserving existing Window bindings, including `none`. The temporary Normal layer keeps its configured entrances as well; no physical G/F keys are hardcoded.

The actual modes retain their configuration, Tab/Backspace navigation, Space reset, cross-display path preservation, and completion behavior. Whenever the picker moves the pointer, the window center follows within the destination work area. Leaving for Normal/Idle resumes Window; `keep` continues targeting. One targeting session creates one window undo step.

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

Press `Ctrl` to enter selection input. Selected windows gain borders immediately. The first run selects individual digits (`123` selects 1, 2 and 3); space-separated runs are complete labels (`123 12 23` adds 12 and 23). A label after a space commits only at the next space or confirmation. Repeating a selected label removes it.

Action keys such as `E`, `S` and `H/J/K/L` commit the pending number and immediately perform the action. Explicit confirmation is optional: press `Ctrl` or `Enter` to confirm only, or `Esc` to cancel this input edit. `Ctrl+X` clears the selection and restores the original window without changing the configured target preference.

Movement, held directions, resizing after `S`, centring, display changes, window states, closing and application audio affect every selected window. `E` opens Editor and tiles only the selection; without multi-selection it retains the usual global layout. A selected tab member represents the whole group. Geometry runs once per group; closing and audio cover all members. Audio runs once per process.

Configure entry and clear in `[window.bindings]` with `ctrl = "window_multi_select"` and `"ctrl+x" = "window_multi_clear"`. Configure confirmation in `[window.multi_select.bindings]` with `"ctrl enter" = "window_multi_confirm"`. Spaces separate alternative keys; `+` combines keys. On macOS, `ctrl` means Control.
