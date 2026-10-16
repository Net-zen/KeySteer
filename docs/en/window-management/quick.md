# Quick: fast split layouts

<script setup>
import ModeVideo from '../../.vitepress/components/ModeVideo'
</script>

Enter with A, then choose placement and ratios with direction keys.

<ModeVideo file="quick.mp4" title="Quick: fast split layouts" description="Enter with A, then choose placement and ratios with direction keys." />

## How to use it

1. Place the pointer over your browser, press `Alt+W`, release the entry keys, then press `A` for Quick.
2. Press `H` to place the browser on the left half.
3. Type your editor's window number, then press `L` to place it on the right half.
4. Press `Q` to return to Window, then `Q` again to start working.

Quick adjusts horizontal and vertical placement separately. The first direction starts nearest half-screen; repeat it to shrink, reverse it to expand. For example, `H` then `K` places the window in the upper-left area.

Default proportions are `1/4, 1/3, 1/2, 2/3, 3/4`, plus full-screen.

::: tip A misplaced window is easy to fix
`Z` undoes the last step, `Shift+Z` redoes it. Leaving with `Q` keeps the new positions.
:::

## Configure the scale

```toml
[window_quick]
split_ratios = ["1/4", "1/3", "1/2", "2/3", "3/4"]
```

[Window management overview](/en/window-management/) · [Full configuration reference](/en/reference/configuration)
