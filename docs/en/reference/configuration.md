# Configuration

KeySteer works without a configuration file. To change keys, speeds, or colors, edit a TOML file. You can [download the complete defaults](/generated/keysteer.default.toml).

::: tip Want a live preview?
Try [Configuration & Simulator](/en/editor/) first. Use this page to look up fields and syntax, and [Modes and actions](/en/reference/modes-and-actions) for action names and arguments.
:::

## Configuration file location

| Platform | Data directory |
| --- | --- |
| Windows | Beside the executable |
| macOS `.app` | `~/Library/Application Support/KeySteer/` |

Save a file named `keysteer.<name>.toml` here, such as `keysteer.user.toml`. The app prefers user configuration, then `keysteer.default.toml`. If neither exists, it uses built-in defaults.

You can also specify a file. A bare filename resolves in the data directory; a relative path starting with `./` resolves from the working directory.

```bash
keysteer --config keysteer.user.toml
keysteer --config ./profiles/keysteer.work.toml
keysteer --check --config keysteer.user.toml
```

| Command | Purpose |
| --- | --- |
| `keysteer --check -c keysteer.user.toml` | Validate configuration without starting control |
| `keysteer --dump-config` | Print the complete effective configuration |
| `keysteer --doctor` | Check keyboard, displays, permissions, and entry keys |
| `keysteer --help` | Show command-line options and default shortcuts |

## Minimal configuration

This example shows how to leave Normal with Space.

::: warning A custom binding table replaces the defaults
Omitted ordinary fields keep their defaults, but tables such as `[normal.bindings]` are replaced as a whole. Start from the complete defaults and keep the other keys you need. User configuration and the default file are also not merged field by field.
:::

```toml
[normal.bindings]
# Use Space to return to Idle from Normal.
space = "idle"
```

After saving, choose **Reload Configuration** from the status menu. The app returns to Idle; use your entry key to enter Normal again.

For tap-to-toggle speeds, add these lines to your existing `[normal.bindings]`:

```toml
"1" = "precision_toggle"
"2" = "slow_toggle"
"3" = "fast_toggle"
```

Tap the same key to return to normal speed, or another key to change speed. Set the multipliers in `[pointer]`. Grid and other label modes still prioritize digits as label input.

Use `left_shift = "slow"` for hold-to-slow, or `shift = "slow_toggle"` for tap-to-toggle. A separate `right_shift` binding takes priority for right Shift.

## Configuration structure

| Section | What it changes |
| --- | --- |
| `[general]` | Excluded apps |
| `[key_aliases]` | Custom key names and platform modifiers |
| `[hotkeys]` | Keys that enter a mode from Idle |
| `[normal]`, `[grid]`, `[recursive_grid]`, `[bisect]`, `[ui_hint]` | Pointer and targeting mode keys and behavior |
| `[window]`, `[window_quick]`, `[window_editor]`, `[window_restore]`, `[window_tab]` | Each window mode's keys and behavior |
| `[pointer]`, `[scroll]` | Pointer speed, scrolling distance and speed |
| `[theme]`, `[mode_indicator]` | Colors and mode badges |
| `[key_help]` | Key help panels |
| `[[app_configs]]` | App-specific key exceptions |
| `[plugin_modes]` | Plugin settings and keys |
| `[debug]` | Diagnostic logs |

## Keys and aliases

### Chords with a shared prefix

By default, `Primary+S` switches the pointer's display, and `Primary+D` moves a window. To share a prefix instead:

```toml
[normal.bindings]
"primary+s" = "screen next"
"primary+s+d" = "move_window next"
```

- Hold `Primary+S`, then press `D` to move a window.
- Release the shorter chord without `D` to switch the pointer's display.

`move_window previous` selects the previous display; `move_window 2` selects a numbered one. No wait setting is needed. Changing modes or reloading configuration cancels a pending shorter action.

::: tip Give continuous actions their own keys
A shared prefix makes the shorter chord wait for release. Separate bindings work better for movement, held clicks, and key locking.
:::

### Key syntax

```toml
[normal]
long_press_toggle_ms = 500
auto_release_ms = 0

[normal.bindings]
h = "move_left"
"primary+shift+s" = "send primary+shift+s"
"v b" = "fast"
```

| Syntax | Meaning |
| --- | --- |
| `h` | One key |
| `primary+shift+s` | Keys held together as a chord |
| `"v b"` | V and B independently perform the same action; spaces do not mean a sequence |
| `"mod+c enter"` | Chords and single keys can share a group; either the system copy shortcut or Enter performs the action |
| `ctrl` / `alt` / `shift` | Either side can trigger the binding |
| `left_shift` / `right_shift` | Only the specified side |

`primary` is an alias: left Alt on Windows and Command on macOS by default. To use Ctrl on Windows, set `Primary = "ctrl"` under `[key_aliases.windows]`.

Common names: `a-z`, `0-9`, `space`, `enter`, `esc`, `tab`, `delete`, `backspace`, `up/down/left/right`, `home/end`, `page_up/page_down`, `f1-f20`, and `numpad_0-numpad_9`.

### Mouse buttons

On Windows and macOS, all five buttons below can appear on either side of a binding: as a trigger on the left or a click action on the right.

| Name | Usual button | Aliases |
| --- | --- | --- |
| `mouse_left` | Left | — |
| `mouse_right` | Right | — |
| `mouse_middle` | Middle (wheel press) | — |
| `mouse_x1` | Back | `xbutton1`, `mouse4` |
| `mouse_x2` | Forward | `xbutton2`, `mouse5` |

Merge the lines you need into existing sections; do not duplicate table names:

```toml
[hotkeys]
mouse_x1 = "normal"

[normal.bindings]
mouse_x1 = "key_help"
mouse_x2 = "grid"
"ctrl+mouse_x2" = "ui_hint"
```

All mouse buttons support chords, inheritance, app exceptions, and held scrolling. Unbound buttons, or those set to `none`, keep their native behavior. A matched binding hands that button press to KeySteer.

Map mouse buttons to keys, keys to mouse buttons, or one mouse button to another:

```toml
[normal.bindings]
x = "mouse_left"       # X clicks the left button
mouse_left = "x"       # Left button sends X
mouse_middle = "mouse_right" # Middle button clicks the right button
t = "mouse_x1"
y = "mouse_x2"
```

All five buttons support `press`, `release`, `toggle`, and normal click long-press locking; for example, `press mouse_left`, `release mouse_left`, and `toggle mouse_middle`. As actions, `mouse_left`, `mouse_right`, and `mouse_middle` are equivalent to `left_click`, `right_click`, and `middle_click`. The receiving app decides whether side buttons go Back/Forward. If a mouse driver remaps a button to a keyboard key, bind the key the driver actually sends.

### Custom aliases

Aliases let one configuration work across keyboards and platforms:

```toml
[key_aliases]
Hyper = "right_ctrl"

[key_aliases.windows]
Primary = "left_alt"

[key_aliases.macos]
Primary = "left_cmd"
```

Top-level aliases apply everywhere; platform sections can specify different keys. Names are case-insensitive. Values must be single keys, not chords. `Primary` and `primary` mean the same alias.

### Bindings, arrays, and inheritance

An action can be a string or an array:

```toml
[normal.bindings]
h = "move_left"
x = ["press shift", "left_click", "release shift"]
"primary+shift+b" = ["exec say start", "wait 300", "exec say done"]
```

Keys are resolved from **the mode's own bindings → parent modes listed in `inherits`, in order**. Matching `app_configs` then override the result.

```toml
[grid]
inherits = ["hotkeys", "normal"]

[grid.bindings]
q = "none" # Disable q inherited from Normal.
```

`none` and `__disabled__` explicitly disable a key, including inherited ones. Plugin suggestions do not restore shortcuts that a user deletes or remaps.

::: tip Keep an entry key
Keep at least one `[hotkeys]` entry. Otherwise the app runs, but you cannot enter an operating mode from Idle.
:::

### Action sequences

Arrays run left to right. Use them for “hold a modifier → click → release” or for launching scripts:

```toml
[normal.bindings]
x = ["press shift", "left_click", "release shift"]
"primary+shift+b" = ["exec say start", "wait 300", "exec say done"]
```

`wait` pauses only that sequence; other input still works. Empty arrays are invalid. See [Modes and actions](/en/reference/modes-and-actions) for actions and `exec` arguments.

## Normal and targeting modes

### Normal

Normal handles movement, clicks, scrolling, and entering other modes. This is a partial example:

```toml
[normal]
long_press_toggle_ms = 500
auto_release_ms = 0

[normal.bindings]
h = "move_left"
j = "move_down"
k = "move_up"
l = "move_right"
";" = "left_click"
g = "grid"
f = "ui_hint"
"primary+g" = "recursive_grid"
"primary+f" = "bisect"
"primary+s" = "screen next"
```

| Setting | Purpose |
| --- | --- |
| `passthrough_unbound_keys = true` | Default: pass unbound keys and chords to the current app |
| `passthrough_unbound_keys = false` | Capture the keyboard and use relaxed chord matching |
| `long_press_toggle_ms` | Hold a click key to keep the mouse button down; default 500 ms, 0 disables long-press locking |
| `auto_release_ms` | Default 0 for manual release; a positive value can release after a qualifying drag stops |

Both time values accept `0–60000` ms. Automatic release applies only to a long-held click with a forwarded physical modifier: movement starts the timer, and further movement resets it. It releases the mouse button, not the modifier. Explicit `press` and `toggle` locks are excluded.

A no-argument `toggle` can lock several partner keys, with either key pressed first. Tap the activation key alone, return to Normal, or enter Idle to release them all. Holding the activation key alone locks that key itself. See [Dragging and holding](/en/modes/normal#dragging-and-holding) for steps.

Grid, Recursive Grid, Bisect, and UI Hint always capture the keyboard. Idle always forwards unmatched input.

### Grid

```toml
[grid]
grid_cols = 5
grid_rows = 4
keys = "12345qwertasdfgzxcvb"
max_depth = 3
cursor_follow_selection = true

[grid.lifecycle]
after_finish = "normal"
after_click = "finish"
```

| Setting | Purpose |
| --- | --- |
| `grid_cols` / `grid_rows` | Columns / rows |
| `keys` | Left to right, top to bottom; character count must equal columns × rows |
| `max_depth` | Number of selections before targeting finishes |
| `cursor_follow_selection` | Whether the pointer follows cell selection |

The initial view previews the first two keys: large labels for the first, small labels for the second. Under `[grid.ui]`, `matched_text_color`, `text_color`, and `matched_border_color` control large labels, small-label base color, and inner boundaries. Previewing does not change the selection depth.

### Recursive Grid

```toml
[recursive_grid]
grid_cols = 3
grid_rows = 3
keys = "qweasdzxc"
max_depth = 10
min_size_width = 1
min_size_height = 1

[recursive_grid.lifecycle]
after_finish = "keep"
after_click = "keep"
```

`max_depth` accepts `1–20`. `min_size_width` / `min_size_height` set the smallest area. The default `font_size = 0` scales text to the cell; a positive value such as 20 limits the maximum size.

Use `layers` for a different grid at a particular depth. Omitted fields inherit the base settings:

```toml
[recursive_grid]
layers = [
  { depth = 0, grid_cols = 2, grid_rows = 2, keys = "crtn" },
]
```

### Bisect

From Normal, `Primary+F` opens [Bisect](/en/modes/bisect). Each h/j/k/l press keeps the left/bottom/top/right half. Only the chosen axis is divided; no label alphabet, grid dimensions, or `layers` are accepted. All other navigation, cursor-follow and lifecycle behavior matches Recursive Grid. Settings are independent.

```toml
[bisect]
max_depth = 20
min_size_width = 2
min_size_height = 2
cursor_follow_selection = true

[bisect.bindings]
h = "bisect_left"
j = "bisect_down"
k = "bisect_up"
l = "bisect_right"

[bisect.lifecycle]
after_finish = "keep"
after_click = "keep"

[bisect.ui]
line_width = 1
```

`max_depth` accepts 1–40 cuts; minimum dimensions must be positive. An axis stops when its next half would be smaller than its minimum, while the other axis remains available. `line_color` and `highlight_color` accept the same RGBA/theme colors as Recursive Grid. `line_width` accepts integers 0–20; 0 hides interior guides while retaining the region outline. Override direction keys through `[bisect.bindings]`; set the old key to `"none"` when removing it. Like grid navigation controls, omitted direction bindings retain their defaults.

### UI Hint

```toml
[ui_hint]
strategy = "hybrid" # axtree, vision, contour, or hybrid
hint_characters = "asdfghjkl"
scan_timeout_ms = 2500
scan_retry_count = 1
scan_retry_delay_ms = 200
visible_check_enabled = false
clickable_roles = ["button", "link", "checkbox", "text_field", "menu_item"]

[ui_hint.lifecycle]
after_finish = "normal"
after_click = "normal"
```

| `strategy` | Recognition sources |
| --- | --- |
| `hybrid` (default) | Accessibility, all available OCR, and outlines |
| `axtree` | Accessibility information |
| `vision` | OCR and outlines; also Vision rectangle detection on macOS |
| `contour` | Outlines, without OCR or accessibility scanning |

Labels appear in batches. Outlines have no text names to search; `clickable_roles` only filters accessibility results.

| Setting | Purpose |
| --- | --- |
| `scan_timeout_ms` | Time limit per scan |
| `scan_retry_count` / `scan_retry_delay_ms` | Retries / delay when a scan completes or times out with no labels |
| `vision.request_timeout_ms` | Request timeout for visual and outline recognition |
| `vision.rectangle_max_candidates` | Outline candidate limit, default 10000 |
| `vision.detect_text` / `vision.detect_rectangles` | Only control `vision` sources; do not disable sources in `hybrid` or standalone `contour` |

Scans remain limited by scope, timeout, and the overall 10000-target cap. Very small targets in high-resolution images may be missed. Windows uses the installed system OCR engine. Visual recognition on macOS needs screen-recording permission.

For search steps, see [UI Hint](/en/modes/ui-hint#search-and-result-previews). `search_match_priority = ["pinyin", "text", "label"]` orders categories with equal match quality. `point_next` in `search_bindings` defaults to Tab.

Use `[ui_hint.search_highlight]` for current-label highlighting. `background_color`, `text_color`, and `border_color` support light/dark themes. `border_width` accepts 0–20, defaults to 1.5, and 0 hides the border. Omitted settings use built-in green.

## Window configuration

For a first exercise, see the [Window guide](/en/modes/window).

The five sections are `[window]`, `[window_quick]`, `[window_editor]`, `[window_restore]`, and `[window_tab]`. Each has its own bindings, inheritance, app exceptions, temporary mode, `ui`, and `lifecycle`. By default they inherit only hotkeys and temporarily use Normal while Primary is held.

| Setting | Where | Default / purpose |
| --- | --- | --- |
| `move_step` / `move_speed` | Window | Movement step 20 / speed 600 |
| `resize_step` / `resize_speed` | Window and Editor, independently | Resize step 20 / speed 500 |
| `split_ratios` | Quick | `["1/4", "1/3", "1/2", "2/3", "3/4"]` |
| `gap` | Quick, Editor, Restore, independently | Window spacing, 0 |
| `number_timeout_ms` | All five | Multi-digit number wait, 250 ms; accepts 100–2000 |
| `border_width` | All five | Window outline width, 3 |
| `lifecycle.after_finish` | Restore | Enter `"window_editor"` after successful restoration |

Steps and spacing use logical pixels; speeds use logical pixels per second. Windows adapts to the target display's DPI. Number styling uses each mode's `ui`; the controls panel uses `[key_help]`.

This example adds direct entry keys and independent settings. Merge it into existing tables:

```toml
[hotkeys]
"alt+w" = "window"
"alt+a" = "window_quick"
"alt+e" = "window_editor"
"alt+r" = "window_restore"

[window_quick]
split_ratios = ["1/5", "2/5", "3/5", "4/5"]
gap = 12.0

[window_editor]
resize_step = 10.0
gap = 12.0

[window_restore.lifecycle]
after_finish = "window_editor"
```

App exceptions belong under sections such as `[[window_editor.app_configs]]`. Share keys explicitly with `inherits`. Actions do not depend on the entry path: Editor's `q = "grid"` always enters Grid.

::: info Migrating old window configuration
Old fields report migration errors and do not rewrite your file. Use these replacements:
:::

| Old form | Current form |
| --- | --- |
| `window_layout` / `window_edit` / `window_saved_layouts` | `window_quick` / `window_editor` / `window_restore` |
| Mode section `[window_delete]` | Merge into `[window_restore]`; `window_delete` is only an action that toggles deletion |
| `window_cancel` / `window_exit` | Write the destination mode, such as `window` or `idle` |
| `window.exit_mode`, `layout_keys`, `double_tap_ms` | Remove |
| `window.split_ratios` | Move to `window_quick` |
| `window.gap` | Move to Quick / Editor / Restore as appropriate |

## Pointer, scrolling, and theme

```toml
[pointer]
initial_speed = 1000.0
max_speed = 2200.0
acceleration = 3000.0
smooth_acceleration = true
tap_distance = 2.5
slow_multiplier = 0.35
precision_multiplier = 0.12
fast_multiplier = 2.0

[scroll]
speed = 500.0
scroll_step = 50
scroll_step_half = 500
scroll_step_full = 1000000

[platform.macos.scroll]
invert_horizontal = false
invert_vertical = true
```

| Setting | What it changes |
| --- | --- |
| `initial_speed` / `max_speed` / `acceleration` | Starting speed, maximum speed, acceleration; pixels/s and pixels/s² |
| `smooth_acceleration` | true softens starting/stopping; false uses linear acceleration |
| `tap_distance` | Distance for a short movement press |
| `slow_multiplier` / `precision_multiplier` / `fast_multiplier` | Slow, precise, and fast multipliers |
| `scroll.speed` / `scroll_step` | Hold speed / tap distance; defaults 500 pixels/s and 50 pixels |
| `scroll_step_half` / `scroll_step_full` | Half-page / full-page distance |
| `invert_horizontal` / `invert_vertical` | macOS horizontal / vertical scroll inversion |

Normal scrolling continues while held and stops on release. `speed = 0` leaves taps only. Half/full-page actions run once per press. Precision, slow, and fast keys also affect scrolling. Speeds do not depend on display refresh rate.

Theme colors use `#RRGGBBAA`; the last two digits are opacity. Light and dark appearances can be configured separately:

```toml
[theme.dark]
surface = "#0A1338FF"
accent = "#6E82D6FF"
accent_alt = "#8FA2F0FF"
on_accent_alt = "#081022FF"
text = "#E8EEFFFF"

[mode_indicator.cursor]
left_pressed_color = "#00FF00FF"
middle_pressed_color = "#FF00FFFF"
right_pressed_color = "#00FFFFFF"
```

`*_pressed_color` controls the held-button circle; its fill uses 20% of the configured opacity. See [Normal](/en/modes/normal#mode-badge-style-and-position) for full mode badge settings.

## Application overrides: `[[app_configs]]`

Disable or replace keys for a particular app:

```toml
[[app_configs]]
bundle_id = "com.apple.Terminal"
bindings = { "primary+shift+e" = "none" }

[[normal.app_configs]]
bundle_id = "Figma"
bindings = { v = "none", "primary+f" = "grid" }
```

Top-level `[[app_configs]]` affects every mode. `[[normal.app_configs]]` only affects Normal. `bundle_id` accepts a macOS bundle id, Windows executable name, or window-title substring.

## Plugin settings

Plugin mode names use `plugin:<id>`:

```toml
[plugin_modes."plugin:screen-selector".settings]
preserve = true

[plugin_modes."plugin:screen-selector"]
inherits = ["hotkeys", "normal"]
```

Screen Selector's `preserve = true` keeps the current Grid/Recursive Grid/Bisect path across displays. With false, targeting starts again on the new display. See [Plugin development](/en/development/plugin-development) to build your own.

## Runtime changes and debugging

Use `set_config` with a dot-separated field path. Successful validation saves the change to the active configuration file:

```toml
[normal.bindings]
"primary+1" = "set_config pointer.max_speed 800"
"primary+2" = "set_config theme.dark.accent \"#FF8800FF\""
```

Invalid changes preserve the last valid configuration. After editing the file manually, reload it from the status menu.

When troubleshooting, enable the log categories you need:

```toml
[debug]
enabled = true
keys = true
actions = true
modes = true
backend = true
pointer = false
motion = false
overlay = true
timers = true
```

Logs are saved as `keysteer.log` in the data directory. Turn debugging off afterward to avoid continually producing large logs.

## Key help

Press `?` to show or hide mode controls without clearing the selection. Keep `"?" = "key_help"` in custom binding tables. The question mark matches the character produced by your keyboard layout.

| Setting | Modes | Default |
| --- | --- | --- |
| `mouse_key_help` | Normal, Grid, Recursive Grid, Bisect, UI Hint | false |
| `window_key_help` | Window, Quick, Editor, Restore, Tabs | true |

For example, hide the window panel initially too:

```toml
[key_help]
mouse_key_help = false
window_key_help = false
```

Manual visibility persists within the mode group. Leaving for Idle and re-entering restores the default. Hiding help does not hide window numbers or borders. The old `[key_help].enabled` field is now `mouse_key_help`.

`[key_help]` supports `font_family`, `font_size`, `background_color`, `text_color`, `border_color`, `border_width`, `border_radius`, `padding_x`, and `padding_y`. Omitted fonts and colors follow the mode badge and theme. Colors support light/dark tables. Choose **Edit key help style** in the [simulator](/en/simulator) for a preview.

Window number size uses `[window.ui].font_size`, default 28. Quick's ratio scale accepts fractions and decimals. Use a string such as `"0.30"` to preserve trailing zeros. The display preview shows the current width and height proportions.

### Window card guide lines

When a number card is moved out of the way, a guide line points to its window. Hiding lines does not disable collision avoidance.

```toml
[window.card]
guide_line_enabled = true
guide_line_width = 3.0 # 0–32; 0 also hides lines
# Inherits the card border color. The last two digits control alpha.
# Light/dark themed colors are also supported.
# guide_line_color = "#6E82D680"
# Optional overrides for the built-in mint-green selection style:
# selected_background_color = { light = "#E8F6F0FF", dark = "#284D44FF" }
# selected_border_color = { light = "#60B49CFF", dark = "#85CDB8FF" }
# selected_border_width = 1.5 # 0–20; 0 hides the selected-card border
```

Line width accepts 0–32, with 0 hiding lines. Selected border width accepts 0–20, with 0 hiding the border. Colors accept `#RRGGBBAA` or light/dark tables.

Preview normal and selected cards under **Window management → Shared appearance**. Exports include changed fields; reset them to restore the built-in green style.

## Mode usage and quick switching

Hold `Q` alone to enter the `quick_switch` mode after 350 ms by default. It stays open after release. Then type labels to filter and select, using `wertyuiop` by default. It uses the ordinary mode lifecycle and bindings, and can also be entered through a regular mode binding.

```toml
[mode_usage]
save_after_entries = 100

[quick_switch]
enabled = true
key = "q"
hint_characters = "wertyuiop"
hold_ms = 350
blacklist = ["idle"]
position = "mouse" # screen / window / mouse

[quick_switch.bindings]
esc = "cancel_selection"
q = "cancel_selection"
backspace = "previous_level"

[quick_switch.ui]
font_size = 28
border_width = 1
border_radius = 6
padding_x = 6
padding_y = 8
# background_color = "#EEF2FFFF"
# text_color = "#193781FF"
# border_color = "#506DD0FF"
```

| Setting | Purpose |
| --- | --- |
| `save_after_entries` | Save after this many new mode entries; must be greater than 0 |
| `enabled` / `key` / `hold_ms` | Enable, activation key, and hold delay |
| `hint_characters` | Ordered label alphabet: at least two distinct characters, excluding the trigger key; `123456789` is also supported |
| `blacklist` | Hide modes from this panel; original shortcuts still work |
| `position` | `screen` display center, `window` foreground-window center, `mouse` below the pointer badge |
| `quick_switch.bindings` | Ordinary bindings: `cancel_selection` returns and `previous_level` deletes one prefix character |
| `quick_switch.ui` | Fonts, size, colors, border, corners, and padding |

Modes rank by entry count, with ties sorted by mode id and no nine-item limit. Labels reuse UI Hint’s normal prefix-free algorithm: up to nine modes use single characters, then additional modes use longer labels. For example, ten modes get `w e r t y u i o pw pe`. Typing a prefix filters the list; a complete label selects immediately. By default, Backspace removes a character; Escape or a fresh Q press returns to the preceding mode. Releasing the trigger keeps the panel open. Labels stay fixed throughout filtering. The picker excludes itself and the preceding mode. Long lists wrap into columns based on screen height. If no foreground window is available, `window` falls back to display center. The panel stays within the current display.

An explicit `[quick_switch.bindings]` table replaces the defaults: include only the `esc` line for Escape, only the `q` line for Q, or both for either key. Keep the `backspace` line if needed. Binding keys must not conflict with label characters.

Typed label prefixes use the same yellow highlight as UI Hint by default. Override `quick_switch.ui.matched_text_color` with a shared or light/dark color. The `Quick Switch` mode badge uses the shared indicator style and theme accent.

::: info Q still performs its original action first
Pressing Q immediately runs its binding. After entering Quick Switch, that action stops repeating; release ends the original gesture normally and keeps the panel open. A hold can still open the panel after its action enters Idle. Q is not captured when starting in Idle, paused, in excluded apps, or during text/note entry.
:::

Only actual mode changes add counts; keeping or restarting the same mode does not. Counts are stored in `workspace.ksw` and also saved with preset changes and normal shutdown. Forced termination or power loss can lose recent unsaved counts.

Import a workspace in the web editor to inspect counts and edit the panel. Exports retain statistics. A v2 workspace with statistics requires an app version that supports that format.

## Search panel positioning

Both panels use a reference object and four percentage insets:

```toml
[ui_hint.search_input_ui]
position_mode = "screen"
position = ["95%", "50%", "5%", "50%"]

[ui_hint.search_info_ui]
position_mode = "search_input"
position = ["44%", "50%", "56%", "50%"]
```

| `position_mode` | Reference |
| --- | --- |
| `screen` | Display containing the pointer |
| `window` | Recognized window, or the current display if unavailable |
| `search_input` | Search box; only for the information panel, so it follows the input |

`position` lists **top, right, bottom, left insets**, each from `0%–100%`. Opposite sides must total at most 100%. Screen/window placement uses the selected area's center. Width, fonts, and padding determine panel size separately.

| Information panel relative to search box | `position` |
| --- | --- |
| Above with a small gap (default) | `["44%", "50%", "56%", "50%"]` |
| Directly above, no gap | `["50%", "50%", "50%", "50%"]` |
| Below with the same gap | `["56%", "50%", "44%", "50%"]` |

The input center defaults to 95% of display height. Click, drag a range, or adjust sliders on the web appearance page to preview both panels and their spacing. Old `x_offset` / `y_offset` values no longer apply; use `position` instead.

## Temporary input layers

All modes share `inherits`, `temporary_mode`, `temporary_mode_keys`, and `temporary_mode_passthrough_keys`, including Normal, Quick Switch, Text Input, Text Select, UI Hint, grids, windows, and plugins. Omitting `temporary_mode_keys` from a user configuration or setting it to `[]` disables the layer at load time, so input skips temporary matching. The shipped UI Hint configuration explicitly sets `temporary_mode_keys = ["primary"]`: hold Primary to borrow Normal. Text Select disables the layer by default and omits these optional fields.

Activation keys are consumed only while the temporary layer is active; ordinary routing is unchanged otherwise. Matching proceeds through:

1. Explicit full chords in the current mode's own bindings, excluding inheritance.
2. Full chords in the temporary mode after removing the activation keys that actually matched.
3. Full chords in the current mode after removing those activation keys.
4. Other temporary-mode matches using the same remaining keys.
5. Other current-mode matches or label input using the same remaining keys.

The last four steps include each layer's inherited bindings, and `none` remains terminal. Remaining full chords take priority, with the temporary layer winning ties. Consumed activation keys are never restored: `Primary+H` in temporary Normal uses `h = "move_left"`, not Normal's `primary+h`. Other modifiers remain available. Grid's own explicit `Primary+Q` still returns to Normal. To retain a full shortcut such as `Primary+S` for switching displays, explicitly configure it in the current mode's `bindings`.

To send Q directly to the current mode, declare it as a passthrough key:

```toml
[grid]
temporary_mode = "normal"
temporary_mode_keys = ["primary"]
temporary_mode_passthrough_keys = ["q"]

[grid.bindings]
q = "normal"
```

Passthrough keys skip only the temporary mode; they do not send keys to the OS. The default is `[]`. Chords, aliases, and side-specific modifiers are supported in targeting modes, window modes, and plugins.

Windows and text editors use the same precedence. While a temporary layer is active, all keys pass through the shared route before editing, including movement, scrolling, and mode changes. Releasing its trigger stops borrowed held actions and retains the original mode and selection. Use passthrough keys to retain current-mode controls. UI Hint and Text Select reserve their overlap-cycling modifier for cycling.

## Normal blind positioning

Normal can also accept grid keys directly, but this is off by default. See [Normal blind positioning](../modes/normal.md#blind-grid-positioning) for setup and a safe example.

| `[normal.targeting]` field | Purpose |
| --- | --- |
| `method` | `grid` (default) or `recursive_grid`; inherits that grid's layout |
| `reset_on` | Default `["move", "click"]`; reset after movement or a KeySteer click; accepts a subset or `[]` |
| `grid_cols` / `grid_rows` / `keys` / `max_depth` | Override the layout only in Normal, without changing standalone grid modes |
| `min_size_width` / `min_size_height` / `layers` | `recursive_grid` only; explicit layers replace the whole list, `[]` clears it |

Omitted fields inherit from the chosen grid. UI styling is not accepted here. Key conflicts are reported on loading. Remove the section to disable the feature. See `keysteer.default.toml` for the fully commented example.

## Text selection configuration

`text_select` shares UIHint OCR options and appearance but has an independent `scan_scope`, defaulting to `all`. Choose `window`, `active`, `screen`, or `all` under **Text Select → Behavior** in the web editor. Configure its label alphabet and controls separately:

```toml
[text_select]
enabled = true
scan_scope = "all"
hint_characters = "asdfghjklqwertyuiopzvbnm"

[text_select.lifecycle]
after_copy = "normal"

[text_select.bindings]
"alt+c" = "text_refine" # Option+C on macOS; Alt+C on Windows
"alt+backspace" = "previous_level"
"ctrl+backspace" = "first_level"
tab = "text_cycle_page"

"ctrl+a" = "text_select_all"
backspace = "text_clear_selection"
"ctrl+x" = "text_clear_input"
"mod+c enter" = "text_copy"

"primary+q" = "normal"
esc = "cancel_selection"
"?" = "key_help"
```

Under **Text Select → Keys**, use the shared key mapper to assign **Copy text** (`text_copy`) or **Clear input** (`text_clear_input`). Defaults are `mod+c enter` and `ctrl+x`, respectively. Ctrl+C (Command+C on macOS) and Enter always copy checked panel text, regardless of query content or input highlighting. Ctrl+X clears the entire input without changing the clipboard or level; confirmed word/character checks stay unchanged. Ctrl+A selects input when nonempty or checks all items when empty. These controls can all be remapped.

Every `[text_select.ui]` field has a built-in default, so the entire section can be omitted. Write only the fields you want to customize, or edit them under **Text Select → Appearance** in the web configuration editor. Drag the outline’s bottom-right handle to adjust padding, or select pointer placement and click or drag its hotspot. Horizontal/vertical sliders and arrow keys provide fine adjustment; Shift takes larger steps and Esc cancels a drag.

`panel_width` sets this panel’s width (800 by default, range 320–2400), constrained to the available screen/window. A separator and spacing distinguish the selection count from compact cards. The footer shows the current/total pages and the configured cycle key; Tab advances and wraps from the last page to the first. Backspace with empty input clears checked items; Alt+Backspace returns to the parent.

Word cards wrap according to each word’s length; a long word does not widen other cards. Characters wrap automatically too. `word_min_width` and `char_min_width` set their minimum widths (48 and 40 by default, range 24–240), both editable in **Text Select → Appearance**. Smaller values fit more columns; larger values add room. On short screens oversized cards are capped and long text is elided; copied text stays complete.

`pointer_offset = [x, y]` offsets the pointer from the original region’s bottom-right corner; the default `[12, 12]` moves right/down. The outline fields control only first-level regions: optional themed `border_color`, `border_width` from 0–20 (zero hides it), and a single `padding = [x, y]` array expanding both horizontal/vertical sides by 0–100, default `[2, 2]`. Word and character levels start fully checked and use fixed decimal numbers; `hint_characters` controls regions only. Text and pinyin search all items across every page in the current level, filtering choices without changing checks. Single numbers toggle checks; `1-20` and `1-3 4-5` select inclusive ranges in input order. Unambiguous numbers and range endpoints resolve immediately; otherwise a space, copy or refine action confirms them.

`text_select.lifecycle.after_copy` runs only after successfully copying checked text and defaults to `normal`. Use `keep` to retain the selection, `restart` to scan again, `return` to restore the previous mode, or another built-in/configured plugin mode. Click actions and `finish` are not supported. Failed copies preserve the selection. Entry shows the input and original-position hints; the compact text preview appears after input and stays visible during refinement.

Older configurations without `[text_select]` or `[text_input]` inherit the mode and its default controls. Missing Normal entry keys (`c` / `\`) are supplied only when unoccupied and the mode has not already been remapped. Explicit `none`, occupied keys (including custom chords and Normal targeting), and `text_select.enabled = false` are respected.

Normal binds `c = "text_select"` by default. Inside Text Select, ordinary `c` is input, `Alt+C` refines (Option+C on macOS, independent of Primary), Enter copies the OCR selection, and `Primary+Q` returns to Normal without copying. Ctrl+C (Command+C on macOS) always copies checked OCR text. Ctrl+X clears the entire query; input Copy/Cut editing shortcuts are not active in this mode. In existing configurations, replace the old refinement binding with `alt+c = "text_refine"`. Entry and refinement bindings are independent. An explicit Text Select binding table replaces its defaults. Space remains input; Backspace edits nonempty input and clears checked items when empty; Alt+Backspace returns to the parent. UIHint's `search_edit_keys` supplies cursor movement and paste, with Text Select controls taking priority. Bare letters assigned to controls are excluded from region labels.

Formerly fixed grid, UIHint, screen-selection and window navigation keys are now configurable. Their defaults are unchanged; sparse older configurations retain those controls, and explicit `none` disables them. See [Configurable selection controls](/en/reference/modes-and-actions#configurable-selection-controls).
