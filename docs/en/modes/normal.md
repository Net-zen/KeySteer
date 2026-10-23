# Normal mode

<script setup>
import ModeVideo from '../../.vitepress/components/ModeVideo'
import KeyLayout from '../../.vitepress/components/KeyLayout'
</script>

Use Normal for everyday movement, clicking, scrolling, and dragging. Press `Primary+E` to enter and `Esc` or `Q` to return to Idle.

`Primary` means left Alt on Windows and Command on macOS by default. Normal uses configured keys; other keys still reach the current application.

<ModeVideo file="normal.mp4" title="Normal mode demonstration" description="Keyboard pointer movement, speed modifiers, scrolling, and common click operations." />

## Default keys

<KeyLayout
  layout="q w e r t y u i o p/Caps a s d f g h j k l ; '/Shift z x c v b n m , . Slash RShift/Ctrl Primary Alt Space"
  move="h j k l"
  click="; ' RShift"
  speed="Caps Shift v b"
  scroll="m ,"
  state="n"
  navigation="t y u i"
  mode="e f g q Primary"
  label="Normal default key groups"
  hint="Colours identify action types; blank keys are unassigned."
/>

| Key | Action |
| --- | --- |
| `H / J / K / L` | Move the pointer left / down / up / right |
| `Caps Lock` / `Left Shift` | Precision / slow movement |
| `V` or `B` | Fast movement |
| `M` / `,` | Scroll down / up |
| `;` / `'` / `Right Shift` | Left / right / middle click |
| `N` + a click key | Hold a mouse button for dragging; tap `N` alone to release |
| `T / Y / I / U` | Home / End / Page Up / Page Down |
| `G` / `F` / `Primary+F` | Grid / Recursive Grid / UI Hint |
| `Primary+S` | Move the pointer to the next display |
| `Primary+D` | Move the window under the pointer to the next display |
| `Q` / `Esc` | Return to Idle |

::: tip No need to memorize everything
Press `?` to show the available keys, then press it again to hide them. Capital letters are for readability; hold Shift only when a shortcut says `Shift+`.
:::

## Blind grid positioning

Once you know the grid positions, type cell keys to jump, then use `H/J/K/L` for small adjustments. You stay in Normal, with no grid drawn on screen.

::: info Optional, off by default
This example uses four areas: `1` top left, `2` top right, `3` bottom left, and `4` bottom right. Type `2 → 3` to jump into the top-right area, then its bottom-left quarter.
:::

Add this configuration to try it. First check that `1/2/3/4`, `Tab`, `Backspace`, and `Space` have no other bindings. These keys are free in the shipped defaults.

```toml
[normal.targeting]
method = "grid"
grid_cols = 2
grid_rows = 2
keys = "1234"
max_depth = 3
reset_on = ["move", "click"]
```

| Action | What happens |
| --- | --- |
| Keep typing cell keys | Narrow the area and jump to each smaller cell's center |
| `Tab` / `Backspace` | Go back one level, leaving the pointer where it is |
| `Space` | Start selecting from the whole screen again, without moving the pointer |
| Reach the last level | Stay there; you can still adjust, click, or go back |

With this configuration, fine movement with `H/J/K/L` or a KeySteer click makes the next selection start from the whole screen. Use `reset_on = []` to keep the area. Physical mouse actions and scrolling do not start over. `Esc` and click keys keep their usual actions.

To use an existing Grid or Recursive Grid layout, see [Blind positioning configuration](/en/reference/configuration#normal-blind-positioning). Remove `[normal.targeting]` to turn it off; an empty table enables the default grid.

## Pointer speed

Hold `Caps Lock` for precise movement, left `Shift` to slow down, or `V` / `B` to speed up. Release the key to return to the previous speed. These keys also affect scrolling.

::: tip Prefer tapping to holding?
Add these three lines to your existing `[normal.bindings]`. Tap to switch speed, then tap the same key again to return to normal speed. The current speed appears below the mode name.

```toml
f5 = "precision_toggle"
f6 = "slow_toggle"
f7 = "fast_toggle"
```
:::

To change overall speed, open **Normal → Behavior** in [Configuration & Simulator](/en/editor/). When editing TOML, speed and multiplier settings go in `[pointer]`; see [Full speed settings](/en/reference/configuration#pointer-scrolling-and-theme).

## Dragging and holding

To drag a window or file, hold the left mouse button while leaving your fingers free to move:

1. Move the pointer onto a window title bar or the file to drag.
2. Hold `;` for about half a second, then release it. The left mouse button stays down.
3. Use `H/J/K/L` to drag to the destination.
4. Tap `N` alone to release the button and finish.

::: tip You can also use N with a click key
Hold `N`, tap `;`, then release `N` to keep the left button down. Pressing the click key before `N` works too. Use `'` or right `Shift` for the right or middle button.
:::

`N` can also keep other keys held, including several at once. Tapping `N` alone, switching back to Normal, or returning to Idle releases all keys and mouse buttons held this way.

To change the hold time, adjust:

```toml
[normal]
long_press_toggle_ms = 500 # Half a second; 0 disables long-press holding.
auto_release_ms = 0       # Manual release by default; try a delay such as 300 ms.
```

For example, with `auto_release_ms = 300`, hold a click key to keep the mouse button down, then drag while holding `Ctrl`. When movement stops for about 0.3 seconds, the mouse button releases; you still release Ctrl yourself. The modifier must pass through to the application. Buttons held through `N`, `press`, or `toggle` still need manual release. Both settings accept `0–60000` milliseconds.

## Custom shortcuts

Open [Configuration & Simulator](/en/editor/), choose a key, and pick an action to try a binding. To edit TOML directly, merge these lines into your existing `[normal.bindings]`:

```toml
[normal.bindings]
"primary+space" = "left_click"
"primary+g" = "grid"
q = "idle"
```

This uses `Primary+Space` to click and `Primary+G` to open Grid. After saving, choose **Reload Configuration** from the tray or status menu.

::: warning Edit the existing key table
A new `[normal.bindings]` table replaces the whole default key table. Keep your movement, click, and exit keys too. Start from the [complete default configuration](/generated/keysteer.default.toml).
:::

In Grid and UI Hint, matching keys select labels first. Hold `Primary` to temporarily use Normal's pointer controls.

Unbound keys reach the application by default, so tools such as AHK and Quicker can still use them. If `H` moves the pointer but `Alt+H` has no binding, `Alt+H` still reaches the application. To also block other keys, set `passthrough_unbound_keys = false` in `[normal]`; the default is `true`.

## Temporary text input

To type while using Normal:

1. Press `\` to start typing.
2. Type normally; application shortcuts such as `Ctrl/Cmd+A/C/V` work too.
3. Press `Enter`, `\`, or `Esc` to return to Normal.

::: info Enter only returns to Normal by default
It does not insert a newline or submit to the application. Use an example below to change `[text_input.bindings]` for multiline text, IME candidates, or submitting before returning. Holding `Q` while typing does not open Quick Switch.
:::

::: code-group
```toml [Default: Enter returns]
[text_input.bindings]
'enter \ esc' = "normal"
```

```toml [Keep Enter in the app]
[text_input.bindings]
'\ esc' = "normal"
```

```toml [Submit, then return]
[text_input.bindings]
'\ esc' = "normal"
enter = ["send enter", "normal"]
```
:::

To temporarily move the pointer while typing, use right Ctrl: hold it, move with `H/J/K/L`, then release it to resume typing.

```toml
[text_input]
temporary_mode = "normal"
temporary_mode_keys = ["right_ctrl"]
```

Finishing temporary control also ends continuous movement and held keys. Choose a key that does not take over your editing shortcuts. With `inherits = ["normal"]`, letters such as HJKL perform pointer actions, which gets in the way of typing.

### Edit text from the home row (optional) {#optional-home-row-editing-keys}

You can enable these chords to move your fingers less. They are off by default so your application's shortcuts stay available.

| Chord | Action |
| --- | --- |
| `Primary+H/J/K/L` | Left / down / up / right arrow |
| `Primary+U/I/O` | Backspace / Delete / Insert |
| `Primary+T/Y` | Home / End |

Pick the lines you need from the full example and add them to your existing `[text_input.bindings]`. Ctrl and Shift variants can extend selections. If Primary is already Ctrl, skip Ctrl+Primary combinations. Explicit full chords take priority over temporary pointer controls, and holding a chord repeats it as usual in the application.

::: details Full editing-key example (copy only the bindings you need)

```toml
# Optional Text Input editing keys; choose modifiers distinct from Primary.
# Explicit bindings replace the default table, so keep the return keys.
[text_input.bindings]
'enter \ esc' = "normal"
"primary+h" = "arrow_left"
"primary+j" = "arrow_down"
"primary+k" = "arrow_up"
"primary+l" = "arrow_right"
"primary+u" = "backspace"
"primary+i" = "delete"
"primary+o" = "insert"
"primary+t" = "home"
"primary+y" = "end"
"shift+primary+h" = "send shift+arrow_left"
"shift+primary+j" = "send shift+arrow_down"
"shift+primary+k" = "send shift+arrow_up"
"shift+primary+l" = "send shift+arrow_right"
"shift+primary+u" = "send shift+backspace"
"shift+primary+i" = "send shift+delete"
"shift+primary+o" = "send shift+insert"
"shift+primary+t" = "send shift+home"
"shift+primary+y" = "send shift+end"
"ctrl+primary+h" = "send ctrl+arrow_left"
"ctrl+primary+j" = "send ctrl+arrow_down"
"ctrl+primary+k" = "send ctrl+arrow_up"
"ctrl+primary+l" = "send ctrl+arrow_right"
"ctrl+primary+u" = "send ctrl+backspace"
"ctrl+primary+i" = "send ctrl+delete"
"ctrl+primary+o" = "send ctrl+insert"
"ctrl+primary+t" = "send ctrl+home"
"ctrl+primary+y" = "send ctrl+end"
"ctrl+shift+primary+h" = "send ctrl+shift+arrow_left"
"ctrl+shift+primary+j" = "send ctrl+shift+arrow_down"
"ctrl+shift+primary+k" = "send ctrl+shift+arrow_up"
"ctrl+shift+primary+l" = "send ctrl+shift+arrow_right"
"ctrl+shift+primary+u" = "send ctrl+shift+backspace"
"ctrl+shift+primary+i" = "send ctrl+shift+delete"
"ctrl+shift+primary+o" = "send ctrl+shift+insert"
"ctrl+shift+primary+t" = "send ctrl+shift+home"
"ctrl+shift+primary+y" = "send ctrl+shift+end"
```

:::

## Mode badge style and position

The “Normal” label beside the pointer reminds you which mode is active. You can rename it, change its color, size, and position, or hide it.

In [Configuration & Simulator](/en/editor/), open **Global settings → Mode badge** to edit it with a live preview. Drag the orange anchor to position it, or use arrow keys for small adjustments. Hold Shift to move by 10 at a time.

To edit TOML directly, merge settings into the existing tables with the same names:

```toml
[mode_indicator.modes.normal]
enabled = true   # false hides the badge.
text = "Normal" # Use any name you like.

[mode_indicator.ui]
indicator_offset = [-12, 18] # 12 left and 18 down.
font_size = 13
background_color = "#0A1338FF"
text_color = "#E8EEFFFF"
border_radius = 6
padding_x = 8
padding_y = 4

[mode_indicator.modes.text_input]
enabled = false # Hidden while typing by default; true shows it.
```

`indicator_offset = [X, Y]` places the label's top-right corner. Positive X moves right, negative left; positive Y moves down, negative up. Near a display edge, the label moves back into view. Both values accept integers from `-32768` to `32767`.

Global styling affects every mode. To change only Normal, use `[mode_indicator.modes.normal.ui]`. Remove an individual setting or reset it in the editor to use the global value again. Text Input can use its own `.ui` table too.

Older `position`, `indicator_x_offset`, and `indicator_y_offset` fields are retired; use `indicator_offset = [X, Y]` instead.
