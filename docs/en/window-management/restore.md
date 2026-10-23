# Restore: saved layouts and tab templates

<script setup>
import ModeVideo from '../../.vitepress/components/ModeVideo'
</script>

Save an arrangement, then fill it with the windows you need today.

## Save an arrangement {#save}

<ModeVideo file="save.mp4" title="Saving layouts and tab groups" description="Optional notes and automatic names. This clip demonstrates saving; follow the steps below to restore." />

1. Press `Ctrl+S` in Editor or Tabs.
2. Enter an optional note, such as “Coding”, up to 80 characters.
3. Press Enter. An empty note uses an automatic Layout N or Tabs N name.

## Restore an arrangement {#restore}

Press `R` from Window to open the shared preset list, then type a preset number. Use `PageUp / PageDown` to browse six entries per page.

| Template type | What restoring does |
| --- | --- |
| Layout | Fills saved regions with current windows; on success, enters Editor by default for further adjustment |
| Tabs | Enters Tabs so you can choose the required windows in order; applies when all are selected, and can be cancelled before then |

::: info Restore arrangements with current windows
Layouts fill regions in recent activity order. Extra windows stay put; missing windows leave empty regions. Choose tab-template members again. Templates do not launch apps or reopen documents.
:::

To delete a preset, press `X` in Restore to switch to deletion, type its number, check the displayed name, and press `Enter`. Press `X` again to return to restoration. Deleting a template does not affect an existing tab group.

Layouts and tab templates share `workspace.ksw`: beside the executable on Windows portable builds, or in `~/Library/Application Support/KeySteer/` for the macOS app. Use the [Configuration & simulator](/en/editor/) to import, practise, and export. The website only changes sample windows and browser storage. Replace the file in the application's data directory with your export, then reopen Restore; back up the original file before replacing it if needed.

[Window management overview](/en/window-management/) · [Full configuration reference](/en/reference/configuration)
