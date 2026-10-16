# Tabs: group windows

<script setup>
import ModeVideo from '../../.vitepress/components/ModeVideo'
</script>

Press T to group the current selection, or group by application when nothing is multi-selected.

<ModeVideo file="tabs.mp4" title="Tabs: group windows" description="Press T to group the current selection, or group by application when nothing is multi-selected." />

## How to use it

Press `T` from Window. Grouping follows the current selection:

| Before entry | What happens |
| --- | --- |
| Windows are multi-selected | Confirm input and group only the selection; members of existing groups bring their whole groups |
| No multi-selection | Group compatible, ungrouped windows by app on each display; retain existing groups |

`Z` undoes this arrangement in one step. Unselected windows stay unchanged.

You can also combine windows from different applications:

1. Choose the first displayed window number, for example `1`, then press Space to end the number.
2. Type the second window number, for example `2`, then Space. They form a group immediately.
3. Add more numbers to append members. Press `T` to finish this group and prepare another.
4. Press `Q` to return to Window. Groups remain after leaving the mode; click a tab to switch members.

The numbers `1` and `2` are examples: use the numbers on your screen. Space makes the boundary explicit so windows 1 and 2 are not interpreted as window 12.

| Key | Action in Tabs |
| --- | --- |
| `Tab / Shift+Tab` or `J / K` | Next / previous tab |
| `H / L` | Reorder the active tab left / right |
| `~`, then a group number | Select a whole group, such as `~1` |
| `D` | Remove the active member from its group |
| `X` | Dissolve the group without closing applications |
| `Z / Shift+Z` | Undo / redo grouping and ordering |
| `Ctrl+S` | Save a tab template |

Only the current tab is visible. Quick and Editor treat a group as one target. Minimizing its current member minimizes the group; dissolving it or quitting KeySteer normally restores members hidden by KeySteer.

::: tip Groups remain after leaving the mode
Groups last until you dissolve them or quit KeySteer. Press `Ctrl+S` to save a template for next time.
:::

On Windows, you can also drag tabs after leaving the mode: reorder them, move a window to another strip, or drag `~group number` to merge a whole group. Scroll the strip when there are many tabs. macOS uses system minimize/restore operations to switch members, so system animations may appear. Native fullscreen and incompatible windows report that grouping is unsupported.

For saving instructions and a demo, see [Save an arrangement](/en/window-management/restore#save).

[Window management overview](/en/window-management/) · [Full configuration reference](/en/reference/configuration)
