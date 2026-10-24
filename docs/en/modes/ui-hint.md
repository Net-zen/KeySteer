# UI Hint mode

<script setup>
import ModeVideo from '../../.vitepress/components/ModeVideo'
</script>

Small button, distant pointer? UI Hint puts short labels on buttons, links, menus, and inputs. Type a label to move straight to its target.

<ModeVideo file="uihint.mp4" title="UI Hint mode demonstration" description="Scanning interface elements, filtering with typed labels, and targeting a control." />

Enter from Normal with `Primary+F`.

::: tip Point, then click
By default, selecting a label moves the pointer and returns to Normal. Press `;` to left-click when ready.
:::

## Default controls

| Key | Action |
| --- | --- |
| Label characters | Narrow the results; a complete match locates the target |
| `Shift` | Cycle overlapping labels |
| `Primary+R` | Recognize the current interface again |
| Hold `Primary` | Temporarily use Normal movement, scrolling, and clicks |
| `Primary+Q` / `Esc` | Return to Normal |

## Scanning strategies

Start with the default, `hybrid`. Labels appear in batches; you can type as soon as your target appears.

| Strategy | When to use it |
| --- | --- |
| `hybrid` | Default: combines control information, text recognition (OCR), and outlines |
| `axtree` | Standard buttons, menus, and controls with accessibility information |
| `vision` | Custom interfaces; combines text and outline recognition |
| `contour` | Outlines only; results have no text names to search |

By default, scanning starts with the **window under the pointer**, then the active window if none is found, then the pointer's display. Set `scan_scope = "active"` to prefer the active window, `"screen"` to scan the pointer's whole display, or `"all"` to scan all displays. Display scopes include visible UI across applications and skip content completely covered by other windows. Empty results do not cause a change of scope.

Changing the target window or display starts a new scan; moving between displays within `all` preserves existing labels. All displays share one numbering space, so complete labels do not repeat across displays. Ordinary pointer movement does not keep rescanning. Press `Primary+R` when you need fresh labels. Result storage grows dynamically without truncation at 10,000 targets; more visible content still takes additional time and memory to recognize.

::: info Two macOS permissions
Keyboard control needs **Device Control and Data Access** (macOS 27; formerly **Accessibility**). Visual recognition also needs **Screen & System Audio Recording** (formerly **Screen Recording**). See [macOS permissions](/en/guide/macos).
:::

## Search and result previews

<ModeVideo
  file="uihint-search.mp4"
  title="UI Hint search, copying, and color picking"
  description="22-second bilingual demo, subtitles only: pinyin initials, result cycling, copying information, point adjustment, and color picking."
/>

### Find a target

1. Press `/` and type text, Chinese pinyin initials, or a label, such as `Copy`, `fz`, or `text_field`.
2. The panel previews the best match. Press `Tab` for the next result.
3. Press `Enter`, `/`, or `Primary+Q` to move to the previewed point and stay in UI Hint. Press `Esc` to cancel.

Cycling previews does not move the pointer. Editing the query shows the new best match. Use `@la` or `la@` to search labels only. Separate targets with spaces to select several, for example `@la @ka`.

::: tip The current preview turns green
A green label marks the target being previewed or adjusted. No single label is highlighted while several points remain at their average center. Change the style under **UI Hint → Appearance → Current label highlight** in the web editor.
:::

### Copy text, coordinates, or color

| Key | Action |
| --- | --- |
| `Ctrl+1 / 2 / 3 / 4` | Copy text, accessibility information, coordinates, or color |
| Tap `Ctrl` | Switch between query input and point adjustment |
| `H/J/K/L` while adjusting | Fine-tune the sampling point |
| `Ctrl+Shift+4` | Cycle HEX, RGB, and HSL color formats |

Copying information successfully confirms the point and closes search. Empty content or a failed copy keeps search open. These Ctrl shortcuts are the same on Windows and macOS.

### Change search order and keys

Results rank by match quality first: **exact match → prefix → substring**. Below, `search_match_priority` only breaks ties between categories. Include each once: `pinyin` for initials, `text` for text and control types, and `label` for label codes.

```toml
[ui_hint]
search_match_priority = ["pinyin", "text", "label"]

[ui_hint.search_bindings]
ctrl = "point_toggle"
tab = "point_next" # Can be changed to f9, alt+f9, etc.
"ctrl+shift+4" = "color_next"
```

In `search_bindings`, `point_toggle` toggles adjustment, `point_next` cycles results or selected points, and `color_next` changes the color format. A custom table replaces the default table; the example keeps all three default actions.

Input editing uses `[ui_hint.search_edit_keys]` separately, for example `"enter / primary+q" = "accept"`. Spaces on the left separate alternative shortcuts. Omitted editing actions keep their default keys.

See [Search and information copying](/en/reference/modes-and-actions#uihint-search-and-information-copying) for multi-selection, copying, and style options.

## Common configuration

```toml
[ui_hint]
strategy = "hybrid"
hint_characters = "asdfghjkl"
scan_timeout_ms = 2500
scan_retry_count = 1
scan_retry_delay_ms = 200
visible_check_enabled = false
placement = "bottom"
label_x_offset = 0
label_y_offset = -8
clickable_roles = ["button", "link", "checkbox", "text_field", "menu_item"]

[ui_hint.lifecycle]
after_finish = "normal"
after_click = "normal"
```

| Setting | Purpose |
| --- | --- |
| `hint_characters` | Characters used to generate labels |
| `scan_timeout_ms` | Maximum time per scan, in milliseconds |
| `scan_retry_count` / `scan_retry_delay_ms` | Retry count and delay when no labels are found |
| `clickable_roles` | Filter accessibility control types; does not filter OCR or outlines |
| `placement` / `label_x_offset` / `label_y_offset` | Label position and offset relative to its target |
| `after_finish` / `after_click` | Where to go after locating or clicking |

For incomplete results on a large page, increase the timeout and retry count. Changing windows scans immediately without consuming an empty-result retry.

## Visual style

You can also preview and adjust these settings in [Configuration & Simulator](/en/editor/).

```toml
[ui_hint.ui]
font_size = 17
padding_x = -1
padding_y = -1
border_width = 1

[ui_hint.boundary_highlight]
enabled = false
border_width = 1

[ui_hint.search_input_ui]
position_mode = "screen"
position = ["95%", "50%", "5%", "50%"]
width = 320
```

`ui` controls label size and borders; `boundary_highlight` outlines target areas; `search_input_ui` controls the search box. See [Search panel positioning](/en/reference/configuration#search-panel-positioning) for position settings.

## Visual-recognition guidance

| Situation | What to try |
| --- | --- |
| Custom interface lacks control information | Use `strategy = "vision"` |
| Too many standard-control labels | Narrow `clickable_roles` |
| Windows text recognition is poor | Check the installed Windows OCR languages; KeySteer uses the system OCR engine |
| No visual labels or colors on macOS | Check screen-recording permission, then restart KeySteer |
