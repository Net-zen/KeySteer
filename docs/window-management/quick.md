# Quick：快捷分屏

<script setup>
import ModeVideo from '../.vitepress/components/ModeVideo'
</script>

在 Window 按 A 进入，方向键安排窗口位置与分屏比例。

<ModeVideo file="quick.mp4" title="Quick：快捷分屏" description="在 Window 按 A 进入，方向键安排窗口位置与分屏比例。" />

## 操作步骤与默认按键

1. 鼠标放到浏览器上，按 `Alt+W`，松开入口键后按 `A` 进入 Quick。
2. 按 `H`，把浏览器放到左半屏。
3. 输入编辑器的窗口编号，再按 `L`，把它放到右半屏。
4. 按 `Q` 返回 Window，再按 `Q` 开始工作。

Quick 的横向、纵向分别调整：首次从最接近半屏的比例开始，同方向继续按会缩小，反方向会扩大。例如先 `H` 再 `K`，可放到左上区域。

默认比例为 `1/4、1/3、1/2、2/3、3/4`，另含满屏。

::: tip 放错位置也没关系
`Z` 撤销上一步，`Shift+Z` 重做；`Q` 结束会保留已调整的位置。
:::

## 自定义比例刻度

```toml
[window_quick]
split_ratios = ["1/4", "1/3", "1/2", "2/3", "3/4"]
```

[返回窗口管理总览](/window-management/) · [完整配置参考](/reference/configuration)
